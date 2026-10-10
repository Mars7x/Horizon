//! Optional, non-blocking UI feedback (Navigate, OK, Back) via existing SDL3.
//! This module owns audio playback. Presentation and input adapters remain silent.
use std::time::{Duration, Instant};

use sdl3::{Sdl, AudioSubsystem, audio::{AudioFormat, AudioSpec, AudioStreamOwner}};
use tracing::warn;

const NAVIGATION_WAV: &[u8] = include_bytes!("../ui/assets/horizon-navigation.wav");
const OK_WAV: &[u8] = include_bytes!("../ui/assets/horizon-ok.wav");
const BACK_WAV: &[u8] = include_bytes!("../ui/assets/horizon-back.wav");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiSoundCue { Navigation, Ok, Back }

impl UiSoundCue {
    const fn index(self) -> usize {
        match self { Self::Navigation => 0, Self::Ok => 1, Self::Back => 2 }
    }
}

const RETRY_DELAY: Duration = Duration::from_secs(5);
// One silent output period primes the backend before the first user cue;
// this is not added to, or baked into, any of the three original WAV assets.
const STARTUP_SILENCE: [i16; 480] = [0; 480];

/// SDL objects remain on Slint's main thread, as with Horizon's SDL gamepad input.
struct Playback {
    // Drop the bound stream before its SDL subsystem/context.
    stream: AudioStreamOwner,
    _subsystem: AudioSubsystem,
    _sdl: Sdl,
}

impl Playback {
    fn open() -> Result<Self, String> {
        let sdl = sdl3::init().map_err(|error| error.to_string())?;
        let subsystem = sdl.audio().map_err(|error| error.to_string())?;
        let spec = AudioSpec {
            freq: Some(48_000),
            channels: Some(1),
            format: Some(AudioFormat::s16_sys()),
        };
        let device = subsystem.open_playback_device(&spec).map_err(|error| error.to_string())?;
        let stream = device.open_device_stream(Some(&spec)).map_err(|error| error.to_string())?;
        // SDL opens the device initially paused. Resume only after a short
        // silent primer is available, so the first *audible* sample is not
        // also the first packet the backend ever sees.
        stream.put_data_i16(&STARTUP_SILENCE).map_err(|error| error.to_string())?;
        stream.flush().map_err(|error| error.to_string())?;
        stream.resume().map_err(|error| error.to_string())?;
        Ok(Self { stream, _subsystem: subsystem, _sdl: sdl })
    }
}

pub struct UiSounds {
    enabled: bool,
    samples: [Vec<i16>; 3],
    playback: Option<Playback>,
    last_failed_open: Option<Instant>,
}

impl UiSounds {
    pub fn new(enabled: bool) -> Self {
        // A malformed bundled asset is a packaging error, not a user's runtime error.
        let samples = [NAVIGATION_WAV, OK_WAV, BACK_WAV].map(|wav| {
            decode_pcm16_mono_48k(wav)
                .expect("bundled Horizon UI WAV must be PCM16 mono 48 kHz")
        });
        Self { enabled, samples, playback: None, last_failed_open: None }
    }

    pub fn play_navigation(&mut self) { self.play(UiSoundCue::Navigation); }

    pub fn set_enabled(&mut self, enabled: bool) {
        if !enabled && self.enabled {
            // Immediately stop queued audio, including when the toggle is clicked.
            if let Some(playback) = &self.playback {
                let _ = playback.stream.clear();
            }
        }
        self.enabled = enabled;
    }

    /// Open the reusable device once, before the user navigates. Call from
    /// Slint's event loop after initial UI setup; never from an audio callback.
    pub fn prepare(&mut self) {
        if !self.enabled || self.playback.is_some() { return; }
        if self.last_failed_open.is_some_and(|at| at.elapsed() < RETRY_DELAY) { return; }
        match Playback::open() {
            Ok(playback) => {
                self.playback = Some(playback);
                self.last_failed_open = None;
            }
            Err(error) => {
                warn!(%error, "UI audio device unavailable; menus remain usable");
                self.last_failed_open = Some(Instant::now());
            }
        }
    }

    /// Call only after a semantic UI action succeeds; no sound on ignored input.
    pub fn play(&mut self, cue: UiSoundCue) {
        if !self.enabled { return; }
        self.prepare();
        if let Some(playback) = &self.playback {
            // No sound pile-up if the user navigates quickly or uses held-repeat.
            let result = playback.stream.clear()
                .and_then(|()| playback.stream.put_data_i16(&self.samples[cue.index()]))
                // Short effects need an explicit flush so SDL's conversion
                // buffer releases their complete tail promptly.
                .and_then(|()| playback.stream.flush());
            if let Err(error) = result {
                warn!(%error, "UI sound could not play");
                self.playback = None;
                self.last_failed_open = Some(Instant::now());
            }
        }
    }
}

fn decode_pcm16_mono_48k(wav: &[u8]) -> Option<Vec<i16>> {
    if wav.get(0..4)? != b"RIFF" || wav.get(8..12)? != b"WAVE" { return None; }
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
        if name == b"data" { data = Some(contents); }
        at = at.checked_add(length)?.checked_add(length % 2)?;
    }
    let bytes = data?;
    if !format_ok || bytes.is_empty() || bytes.len() % 2 != 0 { return None; }
    Some(bytes.as_chunks::<2>().0.iter().map(|pair| i16::from_le_bytes(*pair)).collect())
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
    fn startup_primer_is_silent_and_does_not_touch_assets() {
        assert_eq!(STARTUP_SILENCE.len(), 480);
        assert!(STARTUP_SILENCE.iter().all(|sample| *sample == 0));
        assert_eq!(decode_pcm16_mono_48k(NAVIGATION_WAV).unwrap().len(), 4080);
        assert_eq!(decode_pcm16_mono_48k(OK_WAV).unwrap().len(), 9120);
        assert_eq!(decode_pcm16_mono_48k(BACK_WAV).unwrap().len(), 6306);
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
        assert!(sound.playback.is_none());
    }
}
