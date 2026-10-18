//! Optional UI feedback sounds, including paired quick-menu cues.
//! This module owns playback; presentation/input adapters forward semantic cues.
//!
//! Sounds play on a native PipeWire stream (`platform::pipewire`) that asks
//! the graph for about 5 ms per cycle. Playing a cue is one atomic store,
//! which PipeWire's next cycle picks up: nothing queues between a press and
//! the sound. A small controller thread opens the stream and keeps it
//! running whenever Horizon is in front (silence costs little, and an output
//! that falls asleep drops the first cue on waking, Bluetooth especially).
//! It stays warm during embedded gameplay in Horizon's own window so quick-menu
//! opening is immediate and closing cues can finish. It pauses when another
//! application has the foreground, and restarts when Horizon is back. After any restart, a cue waits until the output has run
//! for `WARM_UP`, so it is delayed rather than lost.
//! The UI thread never waits on PipeWire: a stalled server only silences
//! the clicks.
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    time::{Duration, Instant},
};

use tracing::warn;

use crate::platform::pipewire::{AudioStream, Role};

const NAVIGATION_WAV: &[u8] = include_bytes!("../ui/assets/horizon-navigation.wav");
const OK_WAV: &[u8] = include_bytes!("../ui/assets/horizon-ok.wav");
const BACK_WAV: &[u8] = include_bytes!("../ui/assets/horizon-back.wav");
const MENU_OPEN_WAV: &[u8] = include_bytes!("../ui/assets/horizon-quick-menu-open.wav");
const MENU_CLOSE_WAV: &[u8] = include_bytes!("../ui/assets/horizon-quick-menu-close.wav");
const SCREENSHOT_WAV: &[u8] = include_bytes!("../ui/assets/horizon-screenshot.wav");
const LAUNCH_WAV: &[u8] = include_bytes!("../ui/assets/horizon-launch.wav");
const CUE_COUNT: usize = 7;

/// Every cue plays at one standard loudness, whatever level its file has:
/// its loudest 30 ms is brought to about -32 dBFS, and its peak never above
/// -16 dBFS, so cues stay quiet and clearly below game audio.
const LOUDNESS_TARGET: f32 = 0.025;
const PEAK_CEILING: f32 = 0.158;
/// Per-cue adjustment after levelling, in `UiSoundCue::index` order.
/// Navigation plays on every focus move, many times a second when a
/// direction is held, so it sits about 7 dB below the decisions.
const CUE_TRIM: [f32; CUE_COUNT] = [0.45, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
const LOUDNESS_WINDOW: usize = 1440;
const CANCEL_CUE: u8 = u8::MAX;

const SAMPLE_RATE: u32 = 48_000;
/// Frames per PipeWire cycle while sounds are live (about 5 ms).
const LATENCY_FRAMES: u32 = 256;
/// After the output (re)starts, cues wait until it has run this long:
/// a waking output, Bluetooth headsets especially, drops its first moments.
const WARM_UP: Duration = Duration::from_millis(300);
/// A pause between PipeWire's calls this long means the output restarted.
const RESTART_GAP: Duration = Duration::from_millis(50);
/// A failed open is retried no sooner than this.
const RETRY_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiSoundCue {
    Navigation,
    Ok,
    Back,
    QuickMenuOpen,
    QuickMenuClose,
    Screenshot,
    /// A game starting, as its card zooms into the player.
    Launch,
}

impl UiSoundCue {
    const fn index(self) -> usize {
        match self {
            Self::Navigation => 0,
            Self::Ok => 1,
            Self::Back => 2,
            Self::QuickMenuOpen => 3,
            Self::QuickMenuClose => 4,
            Self::Screenshot => 5,
            Self::Launch => 6,
        }
    }
}

/// The latest cue: a sequence number (so the same cue can replay) and the
/// cue's index, packed for one atomic store.
#[derive(Default)]
struct Request(AtomicU64);

impl Request {
    fn post(&self, cue: UiSoundCue) {
        self.post_index(cue.index() as u8);
    }
    fn cancel(&self) {
        self.post_index(CANCEL_CUE);
    }
    fn post_index(&self, index: u8) {
        let sequence = (self.0.load(Ordering::Relaxed) >> 8) + 1;
        self.0
            .store(sequence << 8 | u64::from(index), Ordering::Release);
    }
}

enum Command {
    /// A cue was posted: make sure the stream runs.
    Wake,
    /// Horizon gained or lost the foreground.
    Foreground(bool),
}

pub struct UiSounds {
    enabled: bool,
    foreground: bool,
    samples: Arc<[Vec<f32>; CUE_COUNT]>,
    request: Arc<Request>,
    controller: Option<SyncSender<Command>>,
    last_failed_open: Option<Instant>,
}

impl UiSounds {
    pub fn new(enabled: bool) -> Self {
        // A malformed bundled asset is a packaging error, not a user's runtime error.
        let files = [
            NAVIGATION_WAV,
            OK_WAV,
            BACK_WAV,
            MENU_OPEN_WAV,
            MENU_CLOSE_WAV,
            SCREENSHOT_WAV,
            LAUNCH_WAV,
        ];
        let samples = std::array::from_fn(|index| {
            let mut samples = level(
                decode_pcm16_mono_48k(files[index])
                    .expect("bundled Horizon UI WAV must be PCM16 mono 48 kHz")
                    .into_iter()
                    .map(|sample| f32::from(sample) / 32_768.0)
                    .collect(),
            );
            for sample in &mut samples {
                *sample *= CUE_TRIM[index];
            }
            samples
        });
        Self {
            enabled,
            foreground: true,
            samples: Arc::new(samples),
            request: Arc::new(Request::default()),
            controller: None,
            last_failed_open: None,
        }
    }

    pub fn play_navigation(&mut self) {
        self.play(UiSoundCue::Navigation);
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if !enabled {
            // Dropping the sender ends the controller, which closes the stream.
            self.controller = None;
        }
        self.enabled = enabled;
    }

    /// Open the stream before the user navigates. Never blocks.
    pub fn prepare(&mut self) {
        if !self.enabled || !self.foreground || self.controller.is_some() {
            return;
        }
        if self
            .last_failed_open
            .is_some_and(|at| at.elapsed() < RETRY_DELAY)
        {
            return;
        }
        let (sender, commands) = mpsc::sync_channel(8);
        let samples = Arc::clone(&self.samples);
        let request = Arc::clone(&self.request);
        // Snapshot before spawning: a cue posted immediately after prepare()
        // must not be mistaken for an already consumed request by the worker.
        let seen = request.0.load(Ordering::Acquire);
        if crate::services::worker::spawn("ui-sounds", move || {
            control(&samples, &request, &commands, seen)
        }) {
            self.controller = Some(sender);
        } else {
            self.last_failed_open = Some(Instant::now());
        }
    }

    /// Call only after a semantic UI action succeeds; no sound on ignored
    /// input. Never blocks.
    pub fn play(&mut self, cue: UiSoundCue) {
        if !self.enabled || !self.foreground {
            return;
        }
        self.prepare();
        self.request.post(cue);
        self.send(Command::Wake);
    }

    /// Pause only when Horizon loses the foreground, including its embedded player.
    pub fn set_foreground(&mut self, foreground: bool) {
        self.foreground = foreground;
        if !foreground {
            // Cancel both a pending cue and a half-played tail before sleeping.
            // Returning to Horizon must never replay an old Back/menu sound.
            self.request.cancel();
        }
        self.send(Command::Foreground(foreground));
    }

    fn send(&mut self, command: Command) {
        if let Some(controller) = &self.controller {
            match controller.try_send(command) {
                Ok(()) | Err(TrySendError::Full(_)) => {}
                Err(TrySendError::Disconnected(_)) => {
                    // The stream couldn't open; try again later.
                    self.controller = None;
                    self.last_failed_open = Some(Instant::now());
                }
            }
        }
    }
}

/// The controller thread: owns the stream, pauses it when quiet.
fn control(
    samples: &Arc<[Vec<f32>; CUE_COUNT]>,
    request: &Arc<Request>,
    commands: &mpsc::Receiver<Command>,
    mut seen: u64,
) {
    let voice = Arc::clone(samples);
    let cues = Arc::clone(request);
    let mut playing: Option<(usize, usize)> = None;
    let mut last_call: Option<Instant> = None;
    let mut warm = 0usize;
    let warm_up = (WARM_UP.as_millis() as usize) * SAMPLE_RATE as usize / 1000;
    let stream = match AudioStream::open(
        "Horizon UI sounds",
        Role::Game,
        SAMPLE_RATE,
        1,
        LATENCY_FRAMES,
        Box::new(move |out: &mut [f32], _delay| {
            let now = Instant::now();
            if last_call.is_none_or(|last| now.duration_since(last) > RESTART_GAP) {
                warm = 0; // the output (re)started
                playing = None; // Never resume a stale partial cue after a gap.
            }
            last_call = Some(now);
            // A newer cue replaces the one playing (no pile-up on held
            // input). While the output warms up it waits, rather than being
            // lost in the first moments the device drops.
            let latest = cues.0.load(Ordering::Acquire);
            let cue = (latest & 0xff) as usize;
            if latest != seen && (cue >= voice.len() || warm >= warm_up) {
                seen = latest;
                playing = voice.get(cue).map(|_| (cue, 0));
            }
            let mut written = 0;
            if let Some((cue, position)) = playing.as_mut() {
                let sound = &voice[*cue];
                let count = out.len().min(sound.len() - *position);
                out[..count].copy_from_slice(&sound[*position..*position + count]);
                *position += count;
                written = count;
                if *position >= sound.len() {
                    playing = None;
                }
            }
            out[written..].fill(0.0);
            warm = warm.saturating_add(out.len());
        }),
    ) {
        Ok(stream) => stream,
        Err(error) => {
            warn!(%error, "UI sounds unavailable; menus remain usable");
            return;
        }
    };
    // Running whenever Horizon is in front, so the output never falls asleep
    // under the user, including embedded gameplay; paused when Horizon is inactive.
    let mut foreground = true;
    let mut active = true;
    for command in commands {
        let want = match command {
            Command::Wake => foreground,
            Command::Foreground(now) => {
                foreground = now;
                now
            }
        };
        if want != active {
            stream.set_active(want);
            active = want;
        }
    }
}

/// Scale a cue to the standard loudness (see `LOUDNESS_TARGET`).
fn level(mut samples: Vec<f32>) -> Vec<f32> {
    let window = LOUDNESS_WINDOW.min(samples.len()).max(1);
    let loudest = samples
        .windows(window)
        .step_by(48)
        .map(|part| (part.iter().map(|s| s * s).sum::<f32>() / window as f32).sqrt())
        .fold(0.0f32, f32::max);
    let peak = samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
    if loudest > 0.0 && peak > 0.0 {
        let gain = (LOUDNESS_TARGET / loudest).min(PEAK_CEILING / peak);
        for sample in &mut samples {
            *sample *= gain;
        }
    }
    samples
}

fn decode_pcm16_mono_48k(wav: &[u8]) -> Option<Vec<i16>> {
    if wav.get(0..4)? != b"RIFF" || wav.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut format_ok = false;
    let mut data: Option<&[u8]> = None;
    let mut at = 12usize;
    while at.checked_add(8)? <= wav.len() {
        let name = wav.get(at..at + 4)?;
        let length = u32::from_le_bytes(wav.get(at + 4..at + 8)?.try_into().ok()?) as usize;
        at += 8;
        let contents = wav.get(at..at.checked_add(length)?)?;
        if name == b"fmt " && length >= 16 {
            let kind = u16::from_le_bytes(contents[0..2].try_into().ok()?);
            let channels = u16::from_le_bytes(contents[2..4].try_into().ok()?);
            let rate = u32::from_le_bytes(contents[4..8].try_into().ok()?);
            let bits = u16::from_le_bytes(contents[14..16].try_into().ok()?);
            format_ok = kind == 1 && channels == 1 && rate == 48_000 && bits == 16;
        }
        if name == b"data" {
            data = Some(contents);
        }
        at = at.checked_add(length)?.checked_add(length % 2)?;
    }
    let bytes = data?;
    if !format_ok || bytes.is_empty() || bytes.len() % 2 != 0 {
        return None;
    }
    Some(
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| i16::from_le_bytes(*pair))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_sound_is_valid_short_pcm() {
        let samples = decode_pcm16_mono_48k(NAVIGATION_WAV).unwrap();
        assert_eq!(samples.len(), 2400);
        assert!(samples.iter().any(|sample| *sample != 0));
    }

    #[test]
    fn all_cues_are_valid_pcm() {
        for (cue, wav, frames) in [
            (UiSoundCue::Navigation, NAVIGATION_WAV, 2400),
            (UiSoundCue::Ok, OK_WAV, 2880),
            (UiSoundCue::Back, BACK_WAV, 3360),
            (UiSoundCue::QuickMenuOpen, MENU_OPEN_WAV, 4320),
            (UiSoundCue::QuickMenuClose, MENU_CLOSE_WAV, 4320),
            (UiSoundCue::Screenshot, SCREENSHOT_WAV, 2880),
            (UiSoundCue::Launch, LAUNCH_WAV, 7200),
        ] {
            assert_eq!(decode_pcm16_mono_48k(wav).unwrap().len(), frames);
            assert!(cue.index() < CUE_COUNT);
        }
    }

    #[test]
    fn quick_menu_pair_is_distinct_quiet_and_has_silent_boundaries() {
        assert_ne!(MENU_OPEN_WAV, MENU_CLOSE_WAV);
        for wav in [MENU_OPEN_WAV, MENU_CLOSE_WAV, SCREENSHOT_WAV] {
            assert_ne!(wav, BACK_WAV);
            assert_ne!(wav, OK_WAV);
            let samples = decode_pcm16_mono_48k(wav).unwrap();
            assert_eq!(samples.first(), Some(&0));
            assert_eq!(samples.last(), Some(&0));
            let peak = samples
                .iter()
                .map(|sample| i32::from(*sample).abs())
                .max()
                .unwrap();
            assert!((1500..=2000).contains(&peak));
        }
    }

    #[test]
    fn every_cue_plays_at_the_same_loudness_without_clipping() {
        let sounds = UiSounds::new(true);
        assert!(CUE_TRIM[UiSoundCue::Navigation.index()] < 1.0);
        for (samples, trim) in sounds.samples.iter().zip(CUE_TRIM) {
            let window = LOUDNESS_WINDOW.min(samples.len());
            let loudest = samples
                .windows(window)
                .step_by(48)
                .map(|part| (part.iter().map(|s| s * s).sum::<f32>() / window as f32).sqrt())
                .fold(0.0f32, f32::max);
            let peak = samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
            // At the target (with its trim), unless the peak ceiling holds a
            // very spiky cue back.
            let (target, ceiling) = (LOUDNESS_TARGET * trim, PEAK_CEILING * trim);
            assert!(peak <= ceiling + 1e-4, "peak {peak}");
            assert!(
                (loudest - target).abs() < 1e-3 || (peak - ceiling).abs() < 1e-4,
                "loudest {loudest}, peak {peak}"
            );
        }
    }

    #[test]
    fn malformed_wave_is_rejected() {
        assert!(decode_pcm16_mono_48k(b"not a wave").is_none());
        let mut invalid = NAVIGATION_WAV.to_vec();
        invalid[20] = 3; // WAV format is no longer integer PCM.
        assert!(decode_pcm16_mono_48k(&invalid).is_none());
    }

    #[test]
    fn audio_disabled_by_preference_does_not_open_device() {
        let mut sound = UiSounds::new(false);
        sound.prepare();
        sound.play_navigation();
        sound.play(UiSoundCue::QuickMenuOpen);
        sound.play(UiSoundCue::QuickMenuClose);
        assert!(sound.controller.is_none());
    }

    #[test]
    fn inactive_horizon_cancels_old_sound_and_does_not_queue_new_cues() {
        let mut sound = UiSounds::new(true);
        sound.request.post(UiSoundCue::Back);
        let old = sound.request.0.load(Ordering::Acquire);
        sound.set_foreground(false);
        let cancelled = sound.request.0.load(Ordering::Acquire);
        assert_ne!(old, cancelled);
        assert_eq!(cancelled & 0xff, u64::from(CANCEL_CUE));
        sound.play(UiSoundCue::QuickMenuOpen);
        sound.play(UiSoundCue::QuickMenuClose);
        assert_eq!(sound.request.0.load(Ordering::Acquire), cancelled);
        assert!(sound.controller.is_none());
    }

    #[test]
    fn each_cue_is_a_new_request_even_when_repeated() {
        let request = Request::default();
        request.post(UiSoundCue::Ok);
        let first = request.0.load(Ordering::Acquire);
        request.post(UiSoundCue::Ok);
        let second = request.0.load(Ordering::Acquire);
        assert_ne!(first, second);
        assert_eq!(second & 0xff, UiSoundCue::Ok.index() as u64);
    }
}
