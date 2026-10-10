//! Decoding captures into pixels: photo display images, cached thumbnails,
//! and the video backend contract (implemented by `platform::video`).
use std::{
    fs,
    io::BufWriter,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use image::{DynamicImage, RgbaImage, codecs::jpeg::JpegEncoder};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::album::{Capture, MediaKind};

/// Bump when the thumbnail format or size changes; the folder is cleared.
pub const ALBUM_CACHE_VERSION: &str = "1";
/// Thumbnails fit inside this box. Large enough for a grid tile on a 4K
/// screen; a few tens of KiB each as JPEG.
pub const THUMBNAIL_SIZE: (u32, u32) = (480, 270);
const THUMBNAIL_QUALITY: u8 = 85;

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not decode {path}: {message}")]
    Decode { path: PathBuf, message: String },
    #[error("video playback is not available: {0}")]
    VideoUnavailable(String),
    #[error("video {path} failed: {message}")]
    Video { path: PathBuf, message: String },
}

/// One decoded frame, borrowed from the decoder's buffer. Rows are `stride`
/// bytes apart; pixels are RGBA.
pub struct VideoFrame<'a> {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub data: &'a [u8],
    /// Where this frame is in the video (its stream time). The timeline
    /// follows the frames on screen rather than polling the pipeline.
    pub position: Option<Duration>,
    /// How long this frame stays on screen (from its timestamp), when known.
    pub frame_duration: Option<Duration>,
    /// From running playback. False for the still frame shown while paused
    /// or before playback has started, so the timeline doesn't run on.
    pub live: bool,
}

/// Receives playback output on the decoder's own threads.
pub trait VideoSink: Send + Sync {
    /// False while the previous frame is still waiting to be shown; the
    /// decoder then drops this one without copying it.
    fn wants_frame(&self) -> bool;
    fn frame(&self, frame: VideoFrame<'_>);
    fn ended(&self);
    fn failed(&self, message: String);
}

/// A playing (or paused) video. Dropping it stops playback and frees the
/// decoder. Used from the UI thread only.
pub trait VideoPlayback {
    fn set_playing(&self, playing: bool);
    fn seek(&self, position: Duration);
    fn position(&self) -> Option<Duration>;
    fn duration(&self) -> Option<Duration>;
}

pub struct VideoProbe {
    /// A representative frame, fitting the requested box.
    pub poster: RgbaImage,
    pub duration: Option<Duration>,
}

/// The video decoder Horizon runs on (GStreamer, from the platform layer).
/// Frames are scaled by the decoder to fit `max`, so a 4K clip on a 1080p
/// screen never moves 4K frames.
pub trait VideoBackend: Send + Sync {
    /// Load the decoders and audio output ahead of the first playback, off
    /// the UI thread, so the first video starts as fast as later ones.
    fn warm_up(&self) {}
    fn probe(&self, path: &Path, max: (u32, u32)) -> Result<VideoProbe, MediaError>;
    fn play(
        &self,
        path: &Path,
        max: (u32, u32),
        sink: Arc<dyn VideoSink>,
    ) -> Result<Box<dyn VideoPlayback>, MediaError>;
}

/// A backend for builds or systems without video support: videos list and
/// show as unavailable.
pub struct NoVideo(pub String);

impl VideoBackend for NoVideo {
    fn probe(&self, _path: &Path, _max: (u32, u32)) -> Result<VideoProbe, MediaError> {
        Err(MediaError::VideoUnavailable(self.0.clone()))
    }

    fn play(
        &self,
        _path: &Path,
        _max: (u32, u32),
        _sink: Arc<dyn VideoSink>,
    ) -> Result<Box<dyn VideoPlayback>, MediaError> {
        Err(MediaError::VideoUnavailable(self.0.clone()))
    }
}

/// Decode a photo for display, scaled down (never up) to fit `max`.
pub fn decode_photo(path: &Path, max: (u32, u32)) -> Result<RgbaImage, MediaError> {
    let image = image::open(path).map_err(|error| MediaError::Decode {
        path: path.to_owned(),
        message: error.to_string(),
    })?;
    Ok(fit(image, max))
}

fn fit(image: DynamicImage, (width, height): (u32, u32)) -> RgbaImage {
    if image.width() > width || image.height() > height {
        image.thumbnail(width, height).into_rgba8()
    } else {
        image.into_rgba8()
    }
}

pub struct Thumbnail {
    pub image: RgbaImage,
    pub duration: Option<Duration>,
}

#[derive(Serialize, Deserialize)]
struct VideoInfo {
    duration_ms: Option<u64>,
}

/// Thumbnails (and video durations) keyed by file path, size and modified
/// time, so an edited or replaced file gets a new one. `None` works without
/// a cache, regenerating each time.
#[derive(Debug, Clone)]
pub struct ThumbnailCache {
    dir: Option<PathBuf>,
}

impl ThumbnailCache {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self { dir }
    }

    pub fn thumbnail(
        &self,
        capture: &Capture,
        video: &dyn VideoBackend,
    ) -> Result<Thumbnail, MediaError> {
        let cached = self.dir.as_deref().zip(cache_key(capture.path()));
        if let Some((dir, key)) = &cached
            && let Some(thumbnail) = read_cached(dir, key, capture.kind())
        {
            return Ok(thumbnail);
        }
        let thumbnail = make_thumbnail(capture, video)?;
        if let Some((dir, key)) = &cached {
            // Best effort: a full disk only costs regenerating next time.
            let _ = write_cached(dir, key, &thumbnail, capture.kind());
        }
        Ok(thumbnail)
    }

    /// Delete thumbnails of captures that no longer exist.
    pub fn prune(&self, captures: &[Capture]) {
        let Some(dir) = &self.dir else {
            return;
        };
        let keep: std::collections::HashSet<String> = captures
            .iter()
            .filter_map(|capture| cache_key(capture.path()))
            .collect();
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.starts_with('.') {
                continue; // the cache's .version marker
            }
            let key = name.split('.').next().unwrap_or(name);
            if !keep.contains(key) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

fn make_thumbnail(capture: &Capture, video: &dyn VideoBackend) -> Result<Thumbnail, MediaError> {
    match capture.kind() {
        MediaKind::Photo => {
            // The source's own small copy, when it is big enough.
            if let Some(preview) = capture.preview()
                && let Ok(image) = image::open(preview)
                && image.width() >= THUMBNAIL_SIZE.0
            {
                return Ok(Thumbnail {
                    image: fit(image, THUMBNAIL_SIZE),
                    duration: None,
                });
            }
            Ok(Thumbnail {
                image: decode_photo(capture.path(), THUMBNAIL_SIZE)?,
                duration: None,
            })
        }
        MediaKind::Video => {
            let probe = video.probe(capture.path(), THUMBNAIL_SIZE)?;
            Ok(Thumbnail {
                image: fit(DynamicImage::ImageRgba8(probe.poster), THUMBNAIL_SIZE),
                duration: probe.duration,
            })
        }
    }
}

fn read_cached(dir: &Path, key: &str, kind: MediaKind) -> Option<Thumbnail> {
    let image = image::open(dir.join(format!("{key}.jpg")))
        .ok()?
        .into_rgba8();
    let duration = match kind {
        MediaKind::Photo => None,
        MediaKind::Video => {
            let info: VideoInfo =
                serde_json::from_slice(&fs::read(dir.join(format!("{key}.json"))).ok()?).ok()?;
            info.duration_ms.map(Duration::from_millis)
        }
    };
    Some(Thumbnail { image, duration })
}

fn write_cached(
    dir: &Path,
    key: &str,
    thumbnail: &Thumbnail,
    kind: MediaKind,
) -> std::io::Result<()> {
    if kind == MediaKind::Video {
        let info = VideoInfo {
            duration_ms: thumbnail
                .duration
                .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
        };
        write_atomically(&dir.join(format!("{key}.json")), |file| {
            serde_json::to_writer(file, &info).map_err(std::io::Error::other)
        })?;
    }
    // JPEG has no alpha; screenshots are opaque anyway.
    let rgb = DynamicImage::ImageRgba8(thumbnail.image.clone()).into_rgb8();
    write_atomically(&dir.join(format!("{key}.jpg")), |file| {
        JpegEncoder::new_with_quality(file, THUMBNAIL_QUALITY)
            .encode_image(&rgb)
            .map_err(std::io::Error::other)
    })
}

fn write_atomically(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<fs::File>) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    let mut file = BufWriter::new(fs::File::create(&temporary)?);
    write(&mut file)?;
    file.into_inner().map_err(|error| error.into_error())?;
    fs::rename(temporary, path)
}

/// FNV-1a over the path, size and modified time: stable across runs and
/// Rust versions, unlike `DefaultHasher`.
fn cache_key(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    };
    feed(path.as_os_str().as_encoded_bytes());
    feed(&metadata.len().to_le_bytes());
    feed(&modified.to_le_bytes());
    Some(format!("{hash:016x}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::album::CaptureOrigin;

    fn temp_dir(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "horizon-album-media-{name}-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp directory");
        path
    }

    fn photo(path: PathBuf) -> Capture {
        Capture::new(path, MediaKind::Photo, CaptureOrigin::Horizon, None, 0)
    }

    struct FixedVideo;
    impl VideoBackend for FixedVideo {
        fn probe(&self, _path: &Path, _max: (u32, u32)) -> Result<VideoProbe, MediaError> {
            Ok(VideoProbe {
                poster: RgbaImage::from_pixel(1920, 1080, image::Rgba([9, 9, 9, 255])),
                duration: Some(Duration::from_millis(42_500)),
            })
        }
        fn play(
            &self,
            _path: &Path,
            _max: (u32, u32),
            _sink: Arc<dyn VideoSink>,
        ) -> Result<Box<dyn VideoPlayback>, MediaError> {
            Err(MediaError::VideoUnavailable("test".into()))
        }
    }

    #[test]
    fn photos_shrink_to_fit_but_are_never_enlarged() {
        let dir = temp_dir("fit");
        let large = dir.join("large.png");
        RgbaImage::new(3840, 2160).save(&large).unwrap();
        let small = dir.join("small.png");
        RgbaImage::new(100, 50).save(&small).unwrap();

        let shown = decode_photo(&large, (1920, 1200)).unwrap();
        assert_eq!(shown.dimensions(), (1920, 1080));
        assert_eq!(
            decode_photo(&small, (1920, 1080)).unwrap().dimensions(),
            (100, 50)
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn thumbnails_are_cached_and_follow_file_changes() {
        let dir = temp_dir("cache");
        let cache_dir = dir.join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        let cache = ThumbnailCache::new(Some(cache_dir.clone()));
        let shot = dir.join("shot.png");
        RgbaImage::new(1600, 900).save(&shot).unwrap();
        let capture = photo(shot.clone());

        let first = cache.thumbnail(&capture, &FixedVideo).unwrap();
        assert_eq!(first.image.dimensions(), THUMBNAIL_SIZE);
        assert_eq!(fs::read_dir(&cache_dir).unwrap().count(), 1);
        let again = cache.thumbnail(&capture, &FixedVideo).unwrap();
        assert_eq!(again.image.dimensions(), THUMBNAIL_SIZE);

        // A different file at the same path gets a new key.
        RgbaImage::new(800, 800).save(&shot).unwrap();
        let changed = cache.thumbnail(&capture, &FixedVideo).unwrap();
        assert_eq!(changed.image.dimensions(), (270, 270));

        cache.prune(&[capture]);
        assert_eq!(
            fs::read_dir(&cache_dir).unwrap().count(),
            1,
            "old key pruned"
        );
        cache.prune(&[]);
        assert_eq!(fs::read_dir(&cache_dir).unwrap().count(), 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn video_thumbnails_keep_their_duration_through_the_cache() {
        let dir = temp_dir("video");
        let cache_dir = dir.join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        let cache = ThumbnailCache::new(Some(cache_dir));
        let clip = dir.join("clip.mp4");
        fs::write(&clip, b"not decoded by the fake backend").unwrap();
        let capture = Capture::new(clip, MediaKind::Video, CaptureOrigin::Horizon, None, 0);

        let made = cache.thumbnail(&capture, &FixedVideo).unwrap();
        assert_eq!(made.duration, Some(Duration::from_millis(42_500)));
        let cached = cache
            .thumbnail(&capture, &NoVideo("cache must answer".into()))
            .unwrap();
        assert_eq!(cached.duration, Some(Duration::from_millis(42_500)));
        assert_eq!(cached.image.dimensions(), THUMBNAIL_SIZE);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_source_preview_is_used_only_when_it_is_large_enough() {
        let dir = temp_dir("preview");
        let shot = dir.join("shot.png");
        RgbaImage::from_pixel(1920, 1080, image::Rgba([255, 0, 0, 255]))
            .save(&shot)
            .unwrap();
        let tiny = dir.join("tiny.png");
        RgbaImage::from_pixel(200, 112, image::Rgba([0, 255, 0, 255]))
            .save(&tiny)
            .unwrap();
        let cache = ThumbnailCache::new(None);
        let capture = photo(shot).with_preview(Some(tiny));
        let thumbnail = cache.thumbnail(&capture, &FixedVideo).unwrap();
        assert_eq!(thumbnail.image.get_pixel(5, 5).0, [255, 0, 0, 255]);
        fs::remove_dir_all(dir).unwrap();
    }
}
