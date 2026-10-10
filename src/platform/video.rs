//! Video through GStreamer, which the Flatpak runtime ships (with its codec
//! extensions). Implements `services::album::VideoBackend`.
//!
//! Frames are converted and scaled by GStreamer to RGBA that fits the
//! requested box, then handed to the sink straight from GStreamer's buffer.
//! The appsink keeps one buffer and drops late ones, and the sink can refuse
//! a frame before it is copied, so a busy UI thread costs skipped frames,
//! never a growing queue. Hardware decoders are used when GStreamer ranks
//! them first.
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use gstreamer::{self as gst, prelude::*};
use gstreamer_app as gst_app;
use gstreamer_video::{self as gst_video, prelude::*};
use image::RgbaImage;

use crate::services::album::{
    MediaError, VideoBackend, VideoFrame, VideoPlayback, VideoSink, media::VideoProbe,
};

const PREROLL_TIMEOUT: Duration = Duration::from_secs(5);
/// The poster frame: a second in, or a tenth of a short clip, past fades.
const POSTER_OFFSET: Duration = Duration::from_secs(1);

pub struct GstVideo;

impl GstVideo {
    /// Fails when GStreamer or its playback plugins are missing; the Album
    /// then lists videos without playing them.
    pub fn new() -> Result<Self, String> {
        gst::init().map_err(|error| error.to_string())?;
        for element in ["playbin", "videoconvertscale", "appsink"] {
            if gst::ElementFactory::find(element).is_none() {
                return Err(format!("GStreamer element {element} is missing"));
            }
        }
        Ok(Self)
    }
}

/// Plugins a typical playback loads: containers, common decoders, the
/// converter and the audio output. Loading is a one-off dlopen.
const WARM_UP_ELEMENTS: &[&str] = &[
    "playbin",
    "decodebin3",
    "uridecodebin3",
    "qtdemux",
    "matroskademux",
    "avdec_h264",
    "avdec_h265",
    "vp9dec",
    "vp8dec",
    "dav1ddec",
    "avdec_aac",
    "opusdec",
    "vorbisdec",
    "videoconvertscale",
    "audioconvert",
    "audioresample",
    "pulsesink",
    "autoaudiosink",
];

impl VideoBackend for GstVideo {
    fn warm_up(&self) {
        let spawned = std::thread::Builder::new()
            .name("video-warm-up".into())
            .spawn(|| {
                for name in WARM_UP_ELEMENTS {
                    if let Some(factory) = gst::ElementFactory::find(name) {
                        // Missing optional plugins are fine; playback picks others.
                        let _ = factory.load();
                    }
                }
            });
        if let Err(error) = spawned {
            tracing::debug!(%error, "video warm-up did not start");
        }
    }

    fn probe(&self, path: &Path, max: (u32, u32)) -> Result<VideoProbe, MediaError> {
        let failed = |message: String| MediaError::Video {
            path: path.to_owned(),
            message,
        };
        let pipeline = Pipeline::new(path, max, false).map_err(failed)?;
        let result = (|| {
            pipeline.preroll()?;
            let duration = pipeline.duration();
            let offset = duration.map_or(POSTER_OFFSET, |d| POSTER_OFFSET.min(d / 10));
            if !offset.is_zero() {
                pipeline.seek(offset, gst::SeekFlags::KEY_UNIT)?;
                pipeline.wait_async_done()?;
            }
            let sample = pipeline
                .appsink
                .try_pull_preroll(Some(clock(PREROLL_TIMEOUT)))
                .ok_or_else(|| "no frame".to_owned())?;
            Ok(VideoProbe {
                poster: sample_to_image(&sample)?,
                duration,
            })
        })();
        pipeline.stop();
        result.map_err(failed)
    }

    fn play(
        &self,
        path: &Path,
        max: (u32, u32),
        sink: Arc<dyn VideoSink>,
    ) -> Result<Box<dyn VideoPlayback>, MediaError> {
        let pipeline = Pipeline::new(path, max, true).map_err(|message| MediaError::Video {
            path: path.to_owned(),
            message,
        })?;
        let frames = Arc::clone(&sink);
        let deliver = move |appsink: &gst_app::AppSink, preroll: bool| {
            let sample = if preroll {
                appsink.pull_preroll()
            } else {
                appsink.pull_sample()
            }
            .map_err(|_| gst::FlowError::Eos)?;
            if frames.wants_frame() {
                deliver_sample(&sample, frames.as_ref(), !preroll);
            }
            Ok(gst::FlowSuccess::Ok)
        };
        let preroll = deliver.clone();
        pipeline.appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                // A paused seek shows its frame through preroll.
                .new_preroll(move |appsink| preroll(appsink, true))
                .new_sample(move |appsink| deliver(appsink, false))
                .build(),
        );
        // No GLib main loop: end-of-stream and errors go straight to the
        // sink from GStreamer's threads. Nothing else reads this bus.
        if let Some(bus) = pipeline.playbin.bus() {
            let events = Arc::clone(&sink);
            bus.set_sync_handler(move |_, message| {
                match message.view() {
                    gst::MessageView::Eos(_) => events.ended(),
                    gst::MessageView::Error(error) => events.failed(error.error().to_string()),
                    _ => {}
                }
                gst::BusSyncReply::Drop
            });
        }
        pipeline
            .playbin
            .set_state(gst::State::Paused)
            .map_err(|error| MediaError::Video {
                path: path.to_owned(),
                message: error.to_string(),
            })?;
        Ok(Box::new(GstPlayback { pipeline }))
    }
}

struct Pipeline {
    playbin: gst::Element,
    appsink: gst_app::AppSink,
    path: PathBuf,
}

impl Pipeline {
    fn new(path: &Path, (width, height): (u32, u32), audio: bool) -> Result<Self, String> {
        let uri = gst::glib::filename_to_uri(path, None).map_err(|error| error.to_string())?;
        let caps = gst_video::VideoCapsBuilder::new()
            .format(gst_video::VideoFormat::Rgba)
            .width_range(1..=clamp_dimension(width))
            .height_range(1..=clamp_dimension(height))
            .pixel_aspect_ratio(gst::Fraction::new(1, 1))
            .build();
        let appsink = gst_app::AppSink::builder()
            .caps(&caps)
            .max_buffers(1)
            .drop(true)
            .build();
        let convert = gst::ElementFactory::make("videoconvertscale")
            .build()
            .map_err(|error| error.to_string())?;
        // Scaling keeps the display aspect ratio (videoconvertscale fixates
        // the size to fit the caps box without distorting).
        let bin = gst::Bin::new();
        bin.add_many([&convert, appsink.upcast_ref()])
            .map_err(|error| error.to_string())?;
        convert.link(&appsink).map_err(|error| error.to_string())?;
        let target = convert
            .static_pad("sink")
            .ok_or_else(|| "videoconvertscale has no sink pad".to_owned())?;
        let ghost = gst::GhostPad::with_target(&target).map_err(|error| error.to_string())?;
        bin.add_pad(&ghost).map_err(|error| error.to_string())?;

        let playbin = gst::ElementFactory::make("playbin")
            .property("uri", uri.as_str())
            .property("video-sink", &bin)
            .build()
            .map_err(|error| error.to_string())?;
        // Posters need video only; playback also plays sound. Never
        // subtitles.
        playbin.set_property_from_str(
            "flags",
            if audio {
                "video+audio+soft-volume"
            } else {
                "video"
            },
        );
        Ok(Self {
            playbin,
            appsink,
            path: path.to_owned(),
        })
    }

    fn preroll(&self) -> Result<(), String> {
        self.playbin
            .set_state(gst::State::Paused)
            .map_err(|error| error.to_string())?;
        self.wait_async_done()
    }

    fn wait_async_done(&self) -> Result<(), String> {
        let bus = self.playbin.bus().ok_or_else(|| "no bus".to_owned())?;
        match bus.timed_pop_filtered(
            clock(PREROLL_TIMEOUT),
            &[gst::MessageType::AsyncDone, gst::MessageType::Error],
        ) {
            Some(message) => match message.view() {
                gst::MessageView::Error(error) => Err(error.error().to_string()),
                _ => Ok(()),
            },
            None => Err(format!("{} did not start in time", self.path.display())),
        }
    }

    fn duration(&self) -> Option<Duration> {
        self.playbin
            .query_duration::<gst::ClockTime>()
            .map(|time| Duration::from_nanos(time.nseconds()))
    }

    fn seek(&self, position: Duration, accuracy: gst::SeekFlags) -> Result<(), String> {
        self.playbin
            .seek_simple(gst::SeekFlags::FLUSH | accuracy, clock(position))
            .map_err(|error| error.to_string())
    }

    fn stop(&self) {
        let _ = self.playbin.set_state(gst::State::Null);
    }
}

struct GstPlayback {
    pipeline: Pipeline,
}

impl VideoPlayback for GstPlayback {
    fn set_playing(&self, playing: bool) {
        let state = if playing {
            gst::State::Playing
        } else {
            gst::State::Paused
        };
        if let Err(error) = self.pipeline.playbin.set_state(state) {
            tracing::warn!(%error, "video state change failed");
        }
    }

    fn seek(&self, position: Duration) {
        if let Err(error) = self.pipeline.seek(position, gst::SeekFlags::ACCURATE) {
            tracing::warn!(%error, "video seek failed");
        }
    }

    fn position(&self) -> Option<Duration> {
        self.pipeline
            .playbin
            .query_position::<gst::ClockTime>()
            .map(|time| Duration::from_nanos(time.nseconds()))
    }

    fn duration(&self) -> Option<Duration> {
        self.pipeline.duration()
    }
}

impl Drop for GstPlayback {
    fn drop(&mut self) {
        if let Some(bus) = self.pipeline.playbin.bus() {
            bus.unset_sync_handler();
        }
        self.pipeline.stop();
    }
}

fn clock(duration: Duration) -> gst::ClockTime {
    gst::ClockTime::from_nseconds(u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX))
}

fn clamp_dimension(value: u32) -> i32 {
    i32::try_from(value.max(1)).unwrap_or(i32::MAX)
}

fn deliver_sample(sample: &gst::Sample, sink: &dyn VideoSink, live: bool) {
    let Some(frame) = readable_frame(sample) else {
        return;
    };
    let (Ok(data), Some(stride)) = (frame.plane_data(0), frame.plane_stride().first()) else {
        return;
    };
    sink.frame(VideoFrame {
        width: frame.width(),
        height: frame.height(),
        stride: usize::try_from(*stride).unwrap_or(0),
        data,
        position: stream_time(sample),
        frame_duration: sample
            .buffer()
            .and_then(|buffer| buffer.duration())
            .map(|duration| Duration::from_nanos(duration.nseconds())),
        live,
    });
}

/// The sample's stream time: its timestamp within the video, as the
/// timeline shows it (unaffected by seeks' segment bookkeeping).
fn stream_time(sample: &gst::Sample) -> Option<Duration> {
    let pts = sample.buffer()?.pts()?;
    let segment = sample.segment()?.downcast_ref::<gst::ClockTime>()?;
    let time = segment.to_stream_time(pts)?;
    Some(Duration::from_nanos(time.nseconds()))
}

fn readable_frame(sample: &gst::Sample) -> Option<gst_video::VideoFrameRef<&gst::BufferRef>> {
    let info = gst_video::VideoInfo::from_caps(sample.caps()?).ok()?;
    gst_video::VideoFrameRef::from_buffer_ref_readable(sample.buffer()?, &info).ok()
}

fn sample_to_image(sample: &gst::Sample) -> Result<RgbaImage, String> {
    let frame = readable_frame(sample).ok_or_else(|| "unreadable frame".to_owned())?;
    let data = frame.plane_data(0).map_err(|error| error.to_string())?;
    let stride = frame
        .plane_stride()
        .first()
        .and_then(|stride| usize::try_from(*stride).ok())
        .ok_or_else(|| "no stride".to_owned())?;
    let (width, height) = (frame.width(), frame.height());
    let row = width as usize * 4;
    let mut pixels = Vec::with_capacity(row * height as usize);
    for line in data.chunks(stride).take(height as usize) {
        pixels.extend_from_slice(line.get(..row).ok_or_else(|| "short row".to_owned())?);
    }
    RgbaImage::from_raw(width, height, pixels).ok_or_else(|| "frame size mismatch".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };

    /// Encode a short test clip with whatever encoder the system has.
    fn test_clip(dir: &Path) -> Option<PathBuf> {
        gst::init().ok()?;
        let path = dir.join("clip.webm");
        let description = format!(
            "videotestsrc num-buffers=90 ! video/x-raw,width=1280,height=720,framerate=30/1 \
             ! vp8enc deadline=1 ! webmmux ! filesink location={}",
            path.display()
        );
        let pipeline = gst::parse::launch(&description).ok()?;
        pipeline.set_state(gst::State::Playing).ok()?;
        let bus = pipeline.bus()?;
        let done = bus.timed_pop_filtered(
            gst::ClockTime::from_seconds(30),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        )?;
        pipeline.set_state(gst::State::Null).ok()?;
        matches!(done.view(), gst::MessageView::Eos(_)).then_some(path)
    }

    /// One folder per test: tests run in parallel and each encodes a clip.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("horizon-video-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn probe_scales_to_fit_and_reports_the_duration() {
        let Ok(backend) = GstVideo::new() else {
            eprintln!("GStreamer unavailable; skipping");
            return;
        };
        let dir = temp_dir("probe");
        let Some(clip) = test_clip(&dir) else {
            eprintln!("no VP8 encoder; skipping");
            return;
        };
        let probe = backend.probe(&clip, (480, 480)).expect("probe");
        assert_eq!(probe.poster.dimensions(), (480, 270), "aspect ratio kept");
        let duration = probe.duration.expect("duration");
        assert!(
            (2900..=3100).contains(&duration.as_millis()),
            "{duration:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[derive(Default)]
    struct Recorder {
        frames: Mutex<Vec<(u32, u32)>>,
        positions: Mutex<Vec<Duration>>,
        ended: AtomicBool,
    }
    impl VideoSink for Recorder {
        fn wants_frame(&self) -> bool {
            true
        }
        fn frame(&self, frame: VideoFrame<'_>) {
            assert!(frame.stride >= frame.width as usize * 4);
            if let Some(position) = frame.position {
                self.positions.lock().unwrap().push(position);
            }
            assert!(frame.data.len() >= frame.stride * (frame.height as usize - 1));
            self.frames
                .lock()
                .unwrap()
                .push((frame.width, frame.height));
        }
        fn ended(&self) {
            self.ended.store(true, Ordering::SeqCst);
        }
        fn failed(&self, message: String) {
            panic!("playback failed: {message}");
        }
    }

    #[test]
    fn playback_delivers_scaled_frames_until_the_end() {
        let Ok(backend) = GstVideo::new() else {
            return;
        };
        let dir = temp_dir("playback");
        let Some(clip) = test_clip(&dir) else {
            return;
        };
        let recorder = Arc::new(Recorder::default());
        let playback = backend
            .play(&clip, (640, 640), recorder.clone())
            .expect("play");
        playback.set_playing(true);
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while !recorder.ended.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(recorder.ended.load(Ordering::SeqCst), "reached the end");
        let frames = recorder.frames.lock().unwrap();
        assert!(frames.len() > 10, "{} frames", frames.len());
        assert!(frames.iter().all(|size| *size == (640, 360)));
        // Every frame carries its place in the video, in order, from the start
        // to the end of the 3-second clip.
        let positions = recorder.positions.lock().unwrap();
        assert_eq!(positions.len(), frames.len());
        assert!(positions.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(positions.first().unwrap().as_millis() < 100);
        assert!(positions.last().unwrap().as_millis() > 2800);
        drop(playback);
        let _ = std::fs::remove_dir_all(dir);
    }
}
