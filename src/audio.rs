//! Optional UI feedback sounds (Navigate, OK, Back).
//! This module owns UI sound playback. Presentation and input adapters remain silent.
//!
//! Sounds play on a native PipeWire stream (`platform::pipewire`) that asks
//! the graph for about 5 ms per cycle. Playing a cue is one atomic store,
//! which PipeWire's next cycle picks up: nothing queues between a press and
//! the sound. A small controller thread opens the stream, pauses it after a
//! quiet spell (so the device can sleep) and resumes it on the next cue.
//! The UI thread never waits on PipeWire: a stalled server only silences
//! the clicks.
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender, TrySendError},
    },
    time::{Duration, Instant},
};

use tracing::warn;

use crate::platform::pipewire::{AudioStream, Role};

const NAVIGATION_WAV: &[u8] = include_bytes!("../ui/assets/horizon-navigation.wav");
const OK_WAV: &[u8] = include_bytes!("../ui/assets/horizon-ok.wav");
const BACK_WAV: &[u8] = include_bytes!("../ui/assets/horizon-back.wav");

const SAMPLE_RATE: u32 = 48_000;
/// Frames per PipeWire cycle while sounds are live (about 5 ms).
const LATENCY_FRAMES: u32 = 256;
/// The stream pauses after this long without a cue.
const IDLE_PAUSE: Duration = Duration::from_secs(10);
/// A failed open is retried no sooner than this.
const RETRY_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiSoundCue {
    Navigation,
    Ok,
    Back,
}

impl UiSoundCue {
    const fn index(self) -> usize {
        match self {
            Self::Navigation => 0,
            Self::Ok => 1,
            Self::Back => 2,
        }
    }
}

/// The latest cue: a sequence number (so the same cue can replay) and the
/// cue's index, packed for one atomic store.
#[derive(Default)]
struct Request(AtomicU64);

impl Request {
    fn post(&self, cue: UiSoundCue) {
        let sequence = (self.0.load(Ordering::Relaxed) >> 8) + 1;
        self.0
            .store(sequence << 8 | cue.index() as u64, Ordering::Release);
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
    samples: Arc<[Vec<f32>; 3]>,
    request: Arc<Request>,
    controller: Option<SyncSender<Command>>,
    last_failed_open: Option<Instant>,
}

impl UiSounds {
    pub fn new(enabled: bool) -> Self {
        // A malformed bundled asset is a packaging error, not a user's runtime error.
        let samples = [NAVIGATION_WAV, OK_WAV, BACK_WAV].map(|wav| {
            decode_pcm16_mono_48k(wav)
                .expect("bundled Horizon UI WAV must be PCM16 mono 48 kHz")
                .into_iter()
                .map(|sample| f32::from(sample) / 32_768.0)
                .collect()
        });
        Self {
            enabled,
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
        if !self.enabled || self.controller.is_some() {
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
        if crate::services::worker::spawn("ui-sounds", move || {
            control(&samples, &request, &commands)
        }) {
            self.controller = Some(sender);
        } else {
            self.last_failed_open = Some(Instant::now());
        }
    }

    /// Call only after a semantic UI action succeeds; no sound on ignored
    /// input. Never blocks.
    pub fn play(&mut self, cue: UiSoundCue) {
        if !self.enabled {
            return;
        }
        self.prepare();
        self.request.post(cue);
        self.send(Command::Wake);
    }

    /// While another app (a game) has the foreground the stream stays paused.
    pub fn set_foreground(&mut self, foreground: bool) {
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
    samples: &Arc<[Vec<f32>; 3]>,
    request: &Arc<Request>,
    commands: &mpsc::Receiver<Command>,
) {
    let voice = Arc::clone(samples);
    let cues = Arc::clone(request);
    let mut seen = cues.0.load(Ordering::Acquire);
    let mut playing: Option<(usize, usize)> = None;
    let stream = match AudioStream::open(
        "Horizon UI sounds",
        Role::Game,
        SAMPLE_RATE,
        1,
        LATENCY_FRAMES,
        Box::new(move |out: &mut [f32], _delay| {
            // A newer cue replaces the one playing (no pile-up on held input).
            let latest = cues.0.load(Ordering::Acquire);
            if latest != seen {
                seen = latest;
                playing = Some(((latest & 0xff) as usize, 0));
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
        }),
    ) {
        Ok(stream) => stream,
        Err(error) => {
            warn!(%error, "UI sounds unavailable; menus remain usable");
            return;
        }
    };
    let mut foreground = true;
    let mut active = true;
    loop {
        let command = if active {
            commands.recv_timeout(IDLE_PAUSE)
        } else {
            commands.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        let want = match command {
            Ok(Command::Wake) => foreground,
            Ok(Command::Foreground(now)) => {
                foreground = now;
                false
            }
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if want != active {
            stream.set_active(want);
            active = want;
        }
    }
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
        assert_eq!(samples.len(), 4080);
        assert!(samples.iter().any(|sample| *sample != 0));
    }

    #[test]
    fn all_cues_are_valid_pcm() {
        for (cue, wav, frames) in [
            (UiSoundCue::Navigation, NAVIGATION_WAV, 4080),
            (UiSoundCue::Ok, OK_WAV, 9120),
            (UiSoundCue::Back, BACK_WAV, 6306),
        ] {
            assert_eq!(decode_pcm16_mono_48k(wav).unwrap().len(), frames);
            assert!(cue.index() < 3);
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
