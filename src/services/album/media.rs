//! Decoding Album images and caching their thumbnails.
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use image::{DynamicImage, RgbaImage};
use thiserror::Error;

use crate::{
    domain::album::Capture,
    services::image::{self as decoding, fit},
};

/// Bump when the thumbnail format or size changes; the folder is cleared.
// 5: lossless JXL thumbnails.
// 6: captures far from 16:9 (DS layouts) keep their whole picture.
pub const ALBUM_CACHE_VERSION: &str = "6";
/// Enough pixels for a selected grid tile at 4K, including its lift. Photos
/// use the grid's centre crop; small originals retain their native pixels.
pub const THUMBNAIL_SIZE: (u32, u32) = (960, 540);

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
}

/// Decode a photo for display, scaled down (never up) to fit `max`.
pub fn decode_photo(path: &Path, max: (u32, u32)) -> Result<RgbaImage, MediaError> {
    let image = decoding::decode_file(path).map_err(|error| MediaError::Decode {
        path: path.to_owned(),
        message: error.to_string(),
    })?;
    Ok(fit(image, max))
}

pub struct Thumbnail {
    pub image: RgbaImage,
}

/// Thumbnails keyed by file path, size and modified
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

    pub fn thumbnail(&self, capture: &Capture) -> Result<Thumbnail, MediaError> {
        let cached = self.dir.as_deref().zip(cache_key(capture.path()));
        if let Some((dir, key)) = &cached
            && let Some(thumbnail) = read_cached(dir, key)
        {
            return Ok(thumbnail);
        }
        let thumbnail = make_thumbnail(capture)?;
        if let Some((dir, key)) = &cached {
            // Best effort: a full disk only costs regenerating next time.
            let _ = write_cached(dir, key, &thumbnail);
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

fn make_thumbnail(capture: &Capture) -> Result<Thumbnail, MediaError> {
    if let Some(preview) = capture.preview()
        && let Ok(image) = decoding::decode_file(preview)
        && image.width() >= THUMBNAIL_SIZE.0
        && image.height() >= THUMBNAIL_SIZE.1
    {
        return Ok(Thumbnail {
            image: photo_thumbnail(image),
        });
    }
    Ok(Thumbnail {
        image: photo_thumbnail(decoding::decode_file(capture.path()).map_err(|error| {
            MediaError::Decode {
                path: capture.path().to_owned(),
                message: error.to_string(),
            }
        })?),
    })
}

fn photo_thumbnail(image: DynamicImage) -> RgbaImage {
    let (width, height) = (image.width(), image.height());
    let (target_width, target_height) = THUMBNAIL_SIZE;
    if width <= target_width && height <= target_height {
        // In particular, never resample native emulator screenshots. Slint
        // crops them to the tile and enlarges their authored pixels directly.
        return image.into_rgba8();
    }

    // Far from the tile's 16:9 (a DS screenshot, side by side or stacked),
    // the grid shows the whole capture letterboxed: keep all of it.
    if !near_tile_shape(width, height) {
        return fit(image, THUMBNAIL_SIZE);
    }
    // The grid uses image-fit: cover. Fitting the *whole* portrait/square into
    // a wide box first discards most of its useful resolution, only to enlarge
    // and crop it again in Slint. Apply that same centre crop before reducing.
    let (crop_width, crop_height) = if u64::from(width) * u64::from(target_height)
        > u64::from(height) * u64::from(target_width)
    {
        ((height * target_width / target_height).max(1), height)
    } else {
        (width, (width * target_height / target_width).max(1))
    };
    fit(
        image.crop_imm(
            (width - crop_width) / 2,
            (height - crop_height) / 2,
            crop_width,
            crop_height,
        ),
        THUMBNAIL_SIZE,
    )
}

/// Within 20% of the grid tile's 16:9 (GBA's 3:2 is): shown cropped to fill
/// the tile, as before. The Album page uses the same test (`album.slint`).
fn near_tile_shape(width: u32, height: u32) -> bool {
    let aspect = f64::from(width) / f64::from(height.max(1));
    (aspect / (16.0 / 9.0) - 1.0).abs() <= 0.2
}

fn read_cached(dir: &Path, key: &str) -> Option<Thumbnail> {
    let image = decoding::decode_file(&dir.join(format!("{key}.jxl")))
        .ok()?
        .into_rgba8();
    Some(Thumbnail { image })
}

fn write_cached(dir: &Path, key: &str, thumbnail: &Thumbnail) -> std::io::Result<()> {
    write_atomically(&dir.join(format!("{key}.jxl")), |file| {
        crate::platform::jpeg_xl::write_rgba(
            file,
            thumbnail.image.as_raw(),
            thumbnail.image.width(),
            thumbnail.image.height(),
        )
        .map_err(std::io::Error::other)
    })
}

fn write_atomically(
    path: &Path,
    write: impl FnOnce(&mut fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    let mut file = fs::File::create(&temporary)?;
    write(&mut file)?;
    drop(file);
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

    use crate::domain::album::{CaptureOrigin, MediaKind};

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
    fn cached_pixel_art_retains_exact_colours_and_transparency() {
        let dir = temp_dir("pixel-art");
        let path = dir.join("pixels.png");
        let image = RgbaImage::from_fn(32, 16, |x, y| {
            image::Rgba(if (x + y) % 2 == 0 {
                [255, 0, 255, 255]
            } else {
                [0, 255, 0, 64]
            })
        });
        image.save(&path).unwrap();
        let cache_dir = dir.join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        let cache = ThumbnailCache::new(Some(cache_dir.clone()));
        let capture = photo(path);
        assert_eq!(cache.thumbnail(&capture).unwrap().image, image);
        let cached = read_cached(&cache_dir, &cache_key(capture.path()).unwrap())
            .expect("lossless on-disk thumbnail");
        assert_eq!(cached.image, image);
        assert!(
            cache_dir
                .join(format!("{}.jxl", cache_key(capture.path()).unwrap()))
                .is_file()
        );
        assert_eq!(cache.thumbnail(&capture).unwrap().image, image);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn large_photo_thumbnails_keep_detail_in_the_grids_centre_crop() {
        let dir = temp_dir("crop-detail");
        let cache_dir = dir.join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        let cache = ThumbnailCache::new(Some(cache_dir));
        // Shapes near the tile's 16:9 keep the grid's centre crop.
        for (width, height) in [(2000, 1000), (1600, 1000)] {
            let path = dir.join(format!("{width}x{height}.png"));
            let original = RgbaImage::from_fn(width, height, |x, y| {
                // Edges that must be cropped out, and fine central detail.
                image::Rgba(if x < width / 4 || x >= width * 3 / 4 {
                    [255, 0, 0, 255]
                } else if y < height / 4 || y >= height * 3 / 4 {
                    [0, 0, 255, 255]
                } else if (x / 8) % 2 == 0 {
                    [0, 255, 0, 255]
                } else {
                    [0, 0, 0, 255]
                })
            });
            original.save(&path).unwrap();
            let capture = photo(path.clone());
            let thumbnail = cache.thumbnail(&capture).unwrap();
            assert_eq!(thumbnail.image.dimensions(), THUMBNAIL_SIZE);
            let centre = thumbnail.image.get_pixel(480, 270).0;
            assert_eq!(centre[0], 0);
            assert_eq!(centre[2], 0);
            assert_eq!(cache.thumbnail(&capture).unwrap().image, thumbnail.image);
            // A full-screen photo still has its original composition; the
            // thumbnail's crop must never overwrite or affect the original.
            assert_eq!(decode_photo(&path, (4096, 4096)).unwrap(), original);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn narrow_sources_and_native_gba_thumbnails_are_never_enlarged() {
        let gba = RgbaImage::from_fn(240, 160, |x, y| {
            image::Rgba([x as u8, y as u8, 0, (x % 255) as u8])
        });
        assert_eq!(photo_thumbnail(DynamicImage::ImageRgba8(gba.clone())), gba);
        // Far from 16:9, the whole capture is kept, fitted.
        let portrait = DynamicImage::ImageRgba8(RgbaImage::new(320, 1920));
        assert_eq!(photo_thumbnail(portrait).dimensions(), (90, 540));
        let square = DynamicImage::ImageRgba8(RgbaImage::new(1920, 1920));
        assert_eq!(photo_thumbnail(square).dimensions(), (540, 540));
        // A 4× DS screenshot side by side (2048×768) stays whole.
        let side = DynamicImage::ImageRgba8(RgbaImage::new(2048, 768));
        assert_eq!(photo_thumbnail(side).dimensions(), (960, 360));
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

        let first = cache.thumbnail(&capture).unwrap();
        assert_eq!(first.image.dimensions(), THUMBNAIL_SIZE);
        assert_eq!(fs::read_dir(&cache_dir).unwrap().count(), 1);
        let again = cache.thumbnail(&capture).unwrap();
        assert_eq!(again.image.dimensions(), THUMBNAIL_SIZE);

        // A different file at the same path gets a new key.
        RgbaImage::new(800, 800).save(&shot).unwrap();
        let changed = cache.thumbnail(&capture).unwrap();
        assert_eq!(changed.image.dimensions(), (540, 540));

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
        let short = dir.join("short.png");
        RgbaImage::from_pixel(1200, 100, image::Rgba([0, 255, 0, 255]))
            .save(&short)
            .unwrap();
        let cache = ThumbnailCache::new(None);
        for preview in [tiny, short] {
            let capture = photo(shot.clone()).with_preview(Some(preview));
            let thumbnail = cache.thumbnail(&capture).unwrap();
            assert_eq!(thumbnail.image.get_pixel(5, 5).0, [255, 0, 0, 255]);
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
