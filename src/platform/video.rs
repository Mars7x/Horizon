//! Video through FFmpeg (`platform::ffmpeg`) with sound straight to PipeWire
//! (`platform::pipewire`). Implements `services::album::VideoBackend`.
//!
//! Each playing video has two threads:
//! - **Decoder:** opens the file, decodes in file order, keeps about half a
//!   second of sound queued for PipeWire and a few frames ahead.
//! - **Presenter:** shows each frame on time by one clock (`AudioClock`).
//!
//! Pressing play (or seeking, or resuming) holds the picture still until its
//! sound is actually heard: sound reaches the ears after the output's
//! latency (about 250 ms over Bluetooth, a few ms on speakers). Then the
//! picture starts and keeps going. It never starts, stops and starts again,
//! and no sound is skipped. If sound never arrives, the picture starts anyway
//! after `SOUND_WAIT`. A clip without sound, or the tail after its sound
//! ends, runs on the system clock.
//!
//! Frames come out of FFmpeg already scaled to fit the requested box, as
//! packed RGBA. Playback decodes on the GPU through VA-API when the system
//! has it and the GPU takes the codec, otherwise in software. The UI side never waits: opening, seeking and pausing are
//! requests to these threads, and a stalled audio server only silences the
//! sound.
use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicI64, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use image::RgbaImage;

use crate::{
    platform::{
        ffmpeg::{Media, Unit},
        pipewire::{AudioStream, Role},
    },
    services::{
        album::{
            MediaError, VideoBackend, VideoFrame, VideoPlayback, VideoSink, media::VideoProbe,
        },
        worker::spawn,
    },
};

/// The poster for a clip that opens on black: a second in, or a tenth of a
/// short clip, past the fade.
const POSTER_OFFSET: Duration = Duration::from_secs(1);
/// A frame this dark on average (of 255) counts as black.
const DARK_LEVEL: u64 = 16;
/// Sound is decoded to this, and PipeWire converts it for the device.
const AUDIO_RATE: u32 = 48_000;
const AUDIO_CHANNELS: u32 = 2;
/// Frames PipeWire asks for per cycle for video sound (about 21 ms).
const AUDIO_LATENCY_FRAMES: u32 = 1024;
/// How much sound the decoder keeps queued ahead.
const AUDIO_AHEAD: Duration = Duration::from_millis(500);
/// Decoded frames kept ahead of the screen, and the most ever held.
const FRAMES_AHEAD: usize = 3;
const FRAMES_MAX: usize = 12;

pub struct FfmpegVideo;

impl FfmpegVideo {
    /// FFmpeg is linked from the runtime, so this can't fail today; kept
    /// fallible so the Album can still fall back to `NoVideo`.
    pub fn new() -> Result<Self, String> {
        Ok(Self)
    }
}

impl VideoBackend for FfmpegVideo {
    fn probe(&self, path: &Path, max: (u32, u32)) -> Result<VideoProbe, MediaError> {
        let failed = |message: String| MediaError::Video {
            path: path.to_owned(),
            message,
        };
        let mut media = Media::open(path, max, false, None).map_err(failed)?;
        let duration = media.info().duration;
        // The first frame, so opening the video shows exactly its thumbnail.
        // A clip that opens on black (a fade-in) takes a frame a little in.
        let first = next_frame(&mut media).map_err(failed)?;
        let poster = match first {
            Some(frame) if !is_dark(&frame) => frame,
            first => {
                let offset = duration.map_or(POSTER_OFFSET, |d| POSTER_OFFSET.min(d / 10));
                media.seek(offset, false).map_err(failed)?;
                next_frame(&mut media)
                    .map_err(failed)?
                    .or(first)
                    .ok_or_else(|| failed("no frame".into()))?
            }
        };
        Ok(VideoProbe { poster, duration })
    }

    fn play(
        &self,
        path: &Path,
        max: (u32, u32),
        sink: Arc<dyn VideoSink>,
    ) -> Result<Box<dyn VideoPlayback>, MediaError> {
        let shared = Arc::new(Shared::default());
        let decoder = {
            let shared = Arc::clone(&shared);
            let sink = Arc::clone(&sink);
            let path = path.to_owned();
            spawn("video-decode", move || {
                decode(&shared, &path, max, sink.as_ref())
            })
        };
        let presenter = decoder && {
            let shared = Arc::clone(&shared);
            spawn("video-present", move || present(&shared, sink.as_ref()))
        };
        if !presenter {
            shared.stop();
            return Err(MediaError::Video {
                path: path.to_owned(),
                message: "video threads did not start".into(),
            });
        }
        Ok(Box::new(Playback { shared }))
    }
}

/// The next video frame from `media`, as an image.
fn next_frame(media: &mut Media) -> Result<Option<RgbaImage>, String> {
    loop {
        match media.decode_next()? {
            Unit::Video {
                pixels,
                width,
                height,
                ..
            } => return Ok(RgbaImage::from_raw(width, height, pixels.to_vec())),
            Unit::Audio { .. } => {}
            Unit::End => return Ok(None),
        }
    }
}

/// True for a black (or nearly black) frame.
fn is_dark(image: &RgbaImage) -> bool {
    let pixels = image.pixels().len() as u64;
    if pixels == 0 {
        return true;
    }
    // Sampled: one pixel in 16 is plenty for an average.
    let (sum, count) = image
        .pixels()
        .step_by(16)
        .fold((0u64, 0u64), |(sum, count), pixel| {
            let [r, g, b, _] = pixel.0;
            (
                sum + (u64::from(r) * 2 + u64::from(g) * 5 + u64::from(b)) / 8,
                count + 1,
            )
        });
    sum / count.max(1) < DARK_LEVEL
}

/// A decoded frame waiting for its moment.
struct Frame {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    position: Option<Duration>,
    duration: Option<Duration>,
}

#[derive(Default)]
struct State {
    playing: bool,
    stop: bool,
    seek: Option<Duration>,
    frames: VecDeque<Frame>,
    /// Spare frame buffers, so steady playback doesn't allocate.
    spare: Vec<Vec<u8>>,
    /// The next frame is shown at once, still (after opening or a seek).
    still: bool,
    decoded_all: bool,
    ended: bool,
    /// The clip has sound going to PipeWire.
    audio: bool,
    /// The sound has ended; the last frames run on the system clock.
    tail: bool,
}

/// Sound queued for PipeWire. Its fill runs on PipeWire's real-time thread
/// and only ever `try_lock`s this.
#[derive(Default)]
struct Sound {
    samples: VecDeque<f32>,
    /// The position of the first queued sample.
    next: Duration,
    playing: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    sound: Arc<Mutex<Sound>>,
    clock: Arc<AudioClock>,
    output: Mutex<Option<AudioStream>>,
    /// Microseconds, or `u64::MAX` while unknown.
    duration_us: AtomicU64,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn stop(&self) {
        self.lock().stop = true;
        self.changed.notify_all();
    }

    fn sound(&self) -> MutexGuard<'_, Sound> {
        self.sound
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Playback's one clock. Held (paused, or before playing), or running on
/// the system clock from where it started, steered by the sound.
///
/// Each buffer PipeWire takes anchors where the sound is: its first sample's
/// position at the time it was taken, less the output latency. The picture
/// follows the system clock, nudged toward the sound by at most `STEER` per
/// reading, so a latency estimate that settles after start-up (Bluetooth
/// reports a smaller one at first) is absorbed smoothly instead of jumping
/// the picture. Only a gap over `RESYNC` jumps.
const STEER: Duration = Duration::from_millis(4);
const RESYNC: Duration = Duration::from_secs(1);
/// The longest the picture waits for its sound before starting anyway.
const SOUND_WAIT: Duration = Duration::from_secs(1);

struct AudioClock {
    epoch: Instant,
    /// Where the clock was held or started (microseconds).
    held_us: AtomicI64,
    /// When it started running (ns since `epoch`); `u64::MAX` while held.
    started_ns: AtomicU64,
    /// Since when it has waited for sound to be heard; `u64::MAX` if not.
    waiting_ns: AtomicU64,
    /// The latest buffer's position (microseconds); -1 before any.
    anchor_us: AtomicI64,
    anchor_ns: AtomicU64,
    span_us: AtomicU64,
    delay_us: AtomicU64,
}

impl Default for AudioClock {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            held_us: AtomicI64::new(0),
            started_ns: AtomicU64::new(u64::MAX),
            waiting_ns: AtomicU64::new(u64::MAX),
            anchor_us: AtomicI64::new(-1),
            anchor_ns: AtomicU64::new(0),
            span_us: AtomicU64::new(0),
            delay_us: AtomicU64::new(0),
        }
    }
}

impl AudioClock {
    fn now_ns(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX - 1)
    }

    /// Run from `position` now (no sound to wait for).
    fn start(&self, position: Duration) {
        self.anchor_us.store(-1, Ordering::Release);
        self.waiting_ns.store(u64::MAX, Ordering::Release);
        self.held_us
            .store(signed_micros(position), Ordering::Release);
        self.started_ns.store(self.now_ns(), Ordering::Release);
    }

    /// Hold at `position` until the sound there is heard, then run.
    fn start_with_sound(&self, position: Duration) {
        self.hold(position);
        self.waiting_ns.store(self.now_ns(), Ordering::Release);
    }

    /// Stop at `position`.
    fn hold(&self, position: Duration) {
        self.anchor_us.store(-1, Ordering::Release);
        self.started_ns.store(u64::MAX, Ordering::Release);
        self.waiting_ns.store(u64::MAX, Ordering::Release);
        self.held_us
            .store(signed_micros(position), Ordering::Release);
    }

    fn anchored(&self) -> bool {
        self.anchor_us.load(Ordering::Acquire) >= 0
    }

    /// The system clock's position since the last start (or the hold).
    fn system(&self) -> Duration {
        let held = self.held_us.load(Ordering::Acquire);
        let started = self.started_ns.load(Ordering::Acquire);
        let running = if started == u64::MAX {
            0
        } else {
            self.now_ns().saturating_sub(started) / 1_000
        };
        Duration::from_micros(u64::try_from(held).unwrap_or(0) + running)
    }

    /// PipeWire took `span` of sound starting at `position`; it is heard
    /// after `delay`.
    fn taken(&self, position: Duration, span: Duration, delay: Duration) {
        self.anchor_ns.store(self.now_ns(), Ordering::Release);
        self.span_us.store(micros(span), Ordering::Release);
        self.delay_us.store(micros(delay), Ordering::Release);
        self.anchor_us
            .store(signed_micros(position), Ordering::Release);
    }

    /// Where the sound being heard is (microseconds), once PipeWire has
    /// taken some. Negative while its first sample is still on its way.
    fn heard_us(&self) -> Option<i64> {
        let anchor = self.anchor_us.load(Ordering::Acquire);
        if anchor < 0 {
            return None;
        }
        let elapsed = self
            .now_ns()
            .saturating_sub(self.anchor_ns.load(Ordering::Acquire))
            / 1_000;
        let elapsed = elapsed.min(self.span_us.load(Ordering::Acquire));
        let delay = self.delay_us.load(Ordering::Acquire);
        Some(anchor + i64::try_from(elapsed).unwrap_or(0) - i64::try_from(delay).unwrap_or(0))
    }

    fn heard(&self) -> Option<Duration> {
        self.heard_us()
            .map(|heard| Duration::from_micros(u64::try_from(heard).unwrap_or(0)))
    }

    fn position(&self) -> Duration {
        let waiting = self.waiting_ns.load(Ordering::Acquire);
        if waiting != u64::MAX {
            let held = self.held_us.load(Ordering::Acquire);
            let heard = self.heard_us().is_some_and(|heard| heard >= held);
            let gave_up = self.now_ns().saturating_sub(waiting) > micros(SOUND_WAIT) * 1_000;
            if !heard && !gave_up {
                return self.system(); // still held
            }
            // The sound is heard: the picture starts from here and runs on.
            self.waiting_ns.store(u64::MAX, Ordering::Release);
            self.started_ns.store(self.now_ns(), Ordering::Release);
        }
        let system = self.system();
        let Some(heard) = self.heard() else {
            return system;
        };
        if self.started_ns.load(Ordering::Acquire) == u64::MAX {
            return system; // held
        }
        let gap = signed_micros(heard) - signed_micros(system);
        if gap.unsigned_abs() > micros(RESYNC) {
            self.held_us.fetch_add(gap, Ordering::AcqRel);
            return heard;
        }
        let steer = (gap / 8).clamp(-signed_micros(STEER), signed_micros(STEER));
        self.held_us.fetch_add(steer, Ordering::AcqRel);
        self.system()
    }
}

fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn signed_micros(duration: Duration) -> i64 {
    i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)
}

/// PipeWire's fill: hand over queued sound, silence when there is none (the
/// clock then holds instead of running ahead).
fn fill(sound: &Mutex<Sound>, clock: &AudioClock, out: &mut [f32], delay: Duration) {
    let Ok(mut sound) = sound.try_lock() else {
        out.fill(0.0);
        return;
    };
    if !sound.playing {
        out.fill(0.0);
        return;
    }
    if sound.samples.is_empty() {
        out.fill(0.0);
        return;
    }
    let count = out.len().min(sound.samples.len());
    let count = count - count % AUDIO_CHANNELS as usize;
    for (slot, sample) in out.iter_mut().zip(sound.samples.drain(..count)) {
        *slot = sample;
    }
    out[count..].fill(0.0);
    let span = frames_duration(count / AUDIO_CHANNELS as usize);
    clock.taken(sound.next, span, delay);
    sound.next += span;
}

fn frames_duration(frames: usize) -> Duration {
    Duration::from_nanos(frames as u64 * 1_000_000_000 / u64::from(AUDIO_RATE))
}

/// The decoder thread.
fn decode(shared: &Shared, path: &Path, max: (u32, u32), sink: &dyn VideoSink) {
    let mut media = match Media::open(path, max, true, Some((AUDIO_RATE, AUDIO_CHANNELS))) {
        Ok(media) => media,
        Err(message) => {
            sink.failed(message);
            shared.stop();
            return;
        }
    };
    let info = media.info();
    tracing::debug!(hardware = info.hardware, "video decoding");
    shared
        .duration_us
        .store(info.duration.map_or(u64::MAX, micros), Ordering::Release);
    if let Some(duration) = info.duration {
        sink.duration(duration);
    }
    let output = if info.has_audio {
        open_output(shared)
    } else {
        None
    };
    let audio = output.is_some();
    *shared
        .output
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = output;
    let playing = {
        let mut state = shared.lock();
        state.audio = audio;
        state.still = true;
        state.playing
    };
    set_sound_playing(shared, playing);
    shared.changed.notify_all();

    loop {
        let mut state = shared.lock();
        if state.stop {
            break;
        }
        if let Some(position) = state.seek.take() {
            state.frames.clear();
            state.decoded_all = false;
            state.ended = false;
            state.tail = false;
            state.still = true;
            let (playing, audio) = (state.playing, state.audio);
            drop(state);
            if let Err(message) = media.seek(position, true) {
                tracing::warn!(%message, "video seek failed");
            }
            {
                let mut sound = shared.sound();
                sound.samples.clear();
                sound.next = position;
            }
            if playing && audio {
                shared.clock.start_with_sound(position);
            } else if playing {
                shared.clock.start(position);
            } else {
                shared.clock.hold(position);
            }
            shared.changed.notify_all();
            continue;
        }
        let sound_ahead = if state.audio {
            frames_duration(shared.sound().samples.len() / AUDIO_CHANNELS as usize)
        } else {
            AUDIO_AHEAD
        };
        let full = state.frames.len() >= FRAMES_MAX
            || (state.frames.len() >= FRAMES_AHEAD && sound_ahead >= AUDIO_AHEAD);
        if state.decoded_all || full {
            drop(wait_for(shared, state, Duration::from_millis(20)));
            continue;
        }
        let spare = state.spare.pop();
        drop(state);

        match media.decode_next() {
            Ok(Unit::Video {
                pixels,
                width,
                height,
                position,
                duration,
            }) => {
                let mut buffer = spare.unwrap_or_default();
                buffer.clear();
                buffer.extend_from_slice(pixels);
                shared.lock().frames.push_back(Frame {
                    pixels: buffer,
                    width,
                    height,
                    position,
                    duration,
                });
                shared.changed.notify_all();
            }
            Ok(Unit::Audio { samples, position }) => {
                if audio {
                    let mut sound = shared.sound();
                    if sound.samples.is_empty()
                        && let Some(position) = position
                    {
                        sound.next = position;
                    }
                    sound.samples.extend(samples.iter().copied());
                }
            }
            Ok(Unit::End) => {
                shared.lock().decoded_all = true;
                shared.changed.notify_all();
            }
            Err(message) => {
                sink.failed(message);
                break;
            }
        }
    }
    // Close the sound here, off the UI thread.
    let output = shared
        .output
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    drop(output);
}

fn open_output(shared: &Shared) -> Option<AudioStream> {
    let sound = Arc::clone(&shared.sound);
    let clock = Arc::clone(&shared.clock);
    match AudioStream::open(
        "Horizon video",
        Role::Movie,
        AUDIO_RATE,
        AUDIO_CHANNELS,
        AUDIO_LATENCY_FRAMES,
        Box::new(move |out: &mut [f32], delay| fill(&sound, &clock, out, delay)),
    ) {
        Ok(stream) => {
            stream.set_active(false);
            Some(stream)
        }
        Err(error) => {
            tracing::warn!(%error, "video sound unavailable; playing silently");
            None
        }
    }
}

fn set_sound_playing(shared: &Shared, playing: bool) {
    shared.sound().playing = playing;
    if let Ok(output) = shared.output.lock()
        && let Some(stream) = output.as_ref()
    {
        stream.set_active(playing);
    }
}

/// The presenter thread.
fn present(shared: &Shared, sink: &dyn VideoSink) {
    let mut state = shared.lock();
    loop {
        if state.stop {
            return;
        }
        if state.still {
            if let Some(frame) = state.frames.pop_front() {
                state.still = false;
                // Time runs from the first picture, not from the press: the
                // file may take a moment to open.
                if state.playing {
                    let from = frame.position.unwrap_or_else(|| shared.clock.position());
                    if state.audio {
                        shared.clock.start_with_sound(from);
                    } else {
                        shared.clock.start(from);
                    }
                }
                shared.changed.notify_all();
                drop(state);
                show(shared, sink, frame, false);
                state = shared.lock();
                continue;
            }
        } else if state.playing {
            // The sound has ended before the picture: finish on the system clock.
            if state.audio
                && !state.tail
                && state.decoded_all
                && shared.clock.anchored()
                && shared.sound().samples.is_empty()
            {
                state.tail = true;
                shared.clock.start(shared.clock.position());
            }
            let due = state.frames.front().map(|frame| frame.position);
            let now = shared.clock.position();
            match due {
                Some(due) => {
                    let due = due.unwrap_or(now);
                    if due > now {
                        let wait = (due - now).min(Duration::from_millis(20));
                        state = wait_for(shared, state, wait);
                        continue;
                    }
                    let Some(frame) = state.frames.pop_front() else {
                        continue;
                    };
                    shared.changed.notify_all();
                    // Running late: skip to the newest frame that is due.
                    let next_due = state
                        .frames
                        .front()
                        .is_some_and(|next| next.position.is_some_and(|position| position <= now));
                    if next_due {
                        state.spare.push(frame.pixels);
                        continue;
                    }
                    drop(state);
                    show(shared, sink, frame, true);
                    state = shared.lock();
                    continue;
                }
                None => {
                    let sound_done = !state.audio || shared.sound().samples.is_empty();
                    if state.decoded_all && sound_done && !state.ended {
                        state.ended = true;
                        drop(state);
                        sink.ended();
                        state = shared.lock();
                        continue;
                    }
                }
            }
        }
        state = wait_for(shared, state, Duration::from_millis(20));
    }
}

fn wait_for<'a>(
    shared: &'a Shared,
    state: MutexGuard<'a, State>,
    wait: Duration,
) -> MutexGuard<'a, State> {
    shared
        .changed
        .wait_timeout(state, wait)
        .map(|(state, _)| state)
        .unwrap_or_else(|poisoned| poisoned.into_inner().0)
}

fn show(shared: &Shared, sink: &dyn VideoSink, frame: Frame, live: bool) {
    if sink.wants_frame() {
        sink.frame(VideoFrame {
            width: frame.width,
            height: frame.height,
            stride: frame.width as usize * 4,
            data: &frame.pixels,
            position: frame.position,
            frame_duration: frame.duration,
            live,
        });
    }
    let mut state = shared.lock();
    if state.spare.len() < FRAMES_AHEAD {
        state.spare.push(frame.pixels);
    }
}

/// The UI's handle. Every call returns at once; dropping it stops both
/// threads and frees the decoder and the sound stream.
struct Playback {
    shared: Arc<Shared>,
}

impl VideoPlayback for Playback {
    fn set_playing(&self, playing: bool) {
        let mut state = self.shared.lock();
        if state.playing != playing {
            let position = self.shared.clock.position();
            if playing && state.audio {
                self.shared.clock.start_with_sound(position);
            } else if playing {
                self.shared.clock.start(position);
            } else {
                self.shared.clock.hold(position);
            }
            state.playing = playing;
        }
        drop(state);
        set_sound_playing(&self.shared, playing);
        self.shared.changed.notify_all();
    }

    fn seek(&self, position: Duration) {
        self.shared.lock().seek = Some(position);
        self.shared.changed.notify_all();
    }

    fn position(&self) -> Option<Duration> {
        Some(self.shared.clock.position())
    }

    fn duration(&self) -> Option<Duration> {
        match self.shared.duration_us.load(Ordering::Acquire) {
            u64::MAX => None,
            micros => Some(Duration::from_micros(micros)),
        }
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.shared.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::PathBuf, sync::atomic::AtomicBool};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn probe_scales_to_fit_and_reports_the_duration() {
        let backend = FfmpegVideo::new().unwrap();
        let probe = backend
            .probe(&fixture("clip-h264-aac.mp4"), (480, 480))
            .expect("probe");
        assert_eq!(probe.poster.dimensions(), (480, 270), "aspect ratio kept");
        let duration = probe.duration.expect("duration");
        assert!(
            (2900..=3100).contains(&duration.as_millis()),
            "{duration:?}"
        );

        let silent = backend
            .probe(&fixture("clip-vp8-silent.webm"), (1920, 1080))
            .expect("probe");
        assert_eq!(silent.poster.dimensions(), (640, 360), "never scaled up");
    }

    #[test]
    fn the_poster_is_the_first_frame_unless_it_is_black() {
        assert!(is_dark(&RgbaImage::new(64, 36)));
        assert!(!is_dark(&RgbaImage::from_pixel(
            64,
            36,
            image::Rgba([90, 90, 90, 255])
        )));
        // The fixture opens on colour bars: its poster is frame 0, the frame
        // playback starts on.
        let backend = FfmpegVideo::new().unwrap();
        let probe = backend
            .probe(&fixture("clip-h264-aac.mp4"), (320, 320))
            .unwrap();
        let mut media =
            Media::open(&fixture("clip-h264-aac.mp4"), (320, 320), false, None).unwrap();
        let first = next_frame(&mut media).unwrap().unwrap();
        assert_eq!(probe.poster, first);
    }

    #[test]
    fn a_missing_file_fails_cleanly() {
        let backend = FfmpegVideo::new().unwrap();
        assert!(
            backend
                .probe(Path::new("/nonexistent/clip.mp4"), (64, 64))
                .is_err()
        );
    }

    #[derive(Default)]
    struct Recorder {
        frames: Mutex<Vec<(u32, u32, bool)>>,
        positions: Mutex<Vec<Duration>>,
        ended: AtomicBool,
    }
    impl VideoSink for Recorder {
        fn wants_frame(&self) -> bool {
            true
        }
        fn frame(&self, frame: VideoFrame<'_>) {
            assert_eq!(frame.stride, frame.width as usize * 4);
            assert_eq!(frame.data.len(), frame.stride * frame.height as usize);
            if let Some(position) = frame.position {
                self.positions.lock().unwrap().push(position);
            }
            self.frames
                .lock()
                .unwrap()
                .push((frame.width, frame.height, frame.live));
        }
        fn ended(&self) {
            self.ended.store(true, Ordering::SeqCst);
        }
        fn failed(&self, message: String) {
            panic!("playback failed: {message}");
        }
    }

    fn play_to_end(clip: &str, max: (u32, u32)) -> Arc<Recorder> {
        let recorder = Arc::new(Recorder::default());
        let playback = FfmpegVideo::new()
            .unwrap()
            .play(&fixture(clip), max, recorder.clone())
            .expect("play");
        playback.set_playing(true);
        let deadline = Instant::now() + Duration::from_secs(15);
        while !recorder.ended.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(recorder.ended.load(Ordering::SeqCst), "reached the end");
        recorder
    }

    #[test]
    fn playback_delivers_scaled_frames_in_order_until_the_end() {
        let recorder = play_to_end("clip-h264-aac.mp4", (640, 640));
        let frames = recorder.frames.lock().unwrap();
        assert!(frames.len() > 60, "{} frames", frames.len());
        assert!(frames.iter().all(|(w, h, _)| (*w, *h) == (640, 360)));
        assert!(!frames[0].2, "the first frame is a still");
        let positions = recorder.positions.lock().unwrap();
        assert!(positions.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(positions.first().unwrap().as_millis() < 50);
        assert!(positions.last().unwrap().as_millis() > 2900);
    }

    #[test]
    fn a_silent_clip_runs_on_the_system_clock() {
        let started = Instant::now();
        let recorder = play_to_end("clip-vp8-silent.webm", (320, 320));
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(1900),
            "real time: {elapsed:?}"
        );
        assert!(recorder.frames.lock().unwrap().len() > 50);
    }

    #[test]
    fn the_clock_holds_runs_and_follows_the_sound() {
        let clock = AudioClock::default();
        assert_eq!(clock.position(), Duration::ZERO);
        clock.hold(Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(clock.position(), Duration::from_secs(2), "held");
        clock.start(Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(30));
        assert!(
            clock.position() >= Duration::from_millis(2030),
            "runs at once"
        );
        // Sound heard 100 ms ahead: the picture is steered toward it a
        // little per reading, never jumped.
        let before = clock.position();
        let heard = before + Duration::from_millis(100);
        clock.taken(
            heard + Duration::from_millis(10),
            Duration::from_millis(500),
            Duration::from_millis(10),
        );
        let mut last = clock.position();
        assert!(
            last - before <= STEER + Duration::from_millis(2),
            "{before:?} -> {last:?}"
        );
        for _ in 0..200 {
            let next = clock.position();
            assert!(next >= last, "never backwards");
            last = next;
        }
        let gap = clock.heard().unwrap().abs_diff(clock.position());
        assert!(gap < Duration::from_millis(5), "converged: {gap:?}");
    }

    #[test]
    fn starting_holds_the_picture_until_its_sound_is_heard() {
        let clock = AudioClock::default();
        clock.start_with_sound(Duration::from_secs(1));
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(clock.position(), Duration::from_secs(1), "held");
        // PipeWire took the sound at 1 s; it is heard 30 ms later.
        clock.taken(
            Duration::from_secs(1),
            Duration::from_millis(100),
            Duration::from_millis(30),
        );
        assert_eq!(clock.position(), Duration::from_secs(1), "still on its way");
        std::thread::sleep(Duration::from_millis(40));
        let started = clock.position();
        assert!(
            started <= Duration::from_millis(1015),
            "starts where it held: {started:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
        let mut last = clock.position();
        assert!(last > started, "then runs");
        for _ in 0..100 {
            let next = clock.position();
            assert!(next >= last, "never backwards");
            last = next;
        }
    }

    #[test]
    fn a_picture_waiting_for_silent_sound_starts_anyway() {
        let clock = AudioClock::default();
        clock.start_with_sound(Duration::ZERO);
        std::thread::sleep(SOUND_WAIT + Duration::from_millis(20));
        let first = clock.position();
        std::thread::sleep(Duration::from_millis(20));
        assert!(clock.position() > first);
    }
}

/// Start-up timing with real sound output (run by hand: it needs PipeWire).
/// `CLIP=<file>` times a real capture instead of the fixture. Each frame's
/// arrival should track its position from the first frame.
#[cfg(test)]
mod startup_timing {
    use super::*;

    struct Log(Mutex<Vec<(Instant, bool, Option<Duration>)>>);
    impl VideoSink for Log {
        fn wants_frame(&self) -> bool {
            true
        }
        fn frame(&self, frame: VideoFrame<'_>) {
            self.0
                .lock()
                .unwrap()
                .push((Instant::now(), frame.live, frame.position));
        }
        fn ended(&self) {}
        fn failed(&self, message: String) {
            eprintln!("failed: {message}");
        }
    }

    #[test]
    #[ignore]
    fn video_startup_timing() {
        let path = std::env::var("CLIP")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/clip-h264-aac.mp4")
            });
        let video = FfmpegVideo::new().unwrap();
        for round in 0..3 {
            let log = Arc::new(Log(Mutex::new(Vec::new())));
            let t0 = Instant::now();
            let playback = video.play(&path, (1280, 720), log.clone()).unwrap();
            playback.set_playing(true);
            let t_play = t0.elapsed();
            std::thread::sleep(Duration::from_millis(1500));
            drop(playback);
            let frames = log.0.lock().unwrap();
            eprintln!("round {round}: play() {t_play:?}, frames {}", frames.len());
            for (i, (at, live, pos)) in frames.iter().take(14).enumerate() {
                eprintln!(
                    "  #{i:02} t={:>7.1}ms live={live} pos={:?}",
                    at.duration_since(t0).as_secs_f64() * 1000.0,
                    pos.map(|p| p.as_millis())
                );
            }
        }
    }
}
