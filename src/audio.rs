//! Optional UI feedback sounds (Navigate, OK, Back).
//! This module owns UI sound playback. Presentation and input adapters remain silent.
//!
//! Sounds play through GStreamer on their own thread, to PipeWire when it
//! can connect (`platform::audio_output`). The UI thread only posts a cue
//! and never waits: an audio server that stalls (it happens, for example
//! with a Bluetooth headset changing profile) silences the clicks but can
//! never freeze Horizon. The output stays open while the user navigates and
//! closes after a quiet spell, so an idle Horizon holds no audio stream.
use std::{
    sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError},
    time::Duration,
};

use gstreamer::{self as gst, prelude::*};
use gstreamer_app as gst_app;
use tracing::warn;

use crate::platform::audio_output::AudioOutput;

const NAVIGATION_WAV: &[u8] = include_bytes!("../ui/assets/horizon-navigation.wav");
const OK_WAV: &[u8] = include_bytes!("../ui/assets/horizon-ok.wav");
const BACK_WAV: &[u8] = include_bytes!("../ui/assets/horizon-back.wav");

const SAMPLE_RATE: i32 = 48_000;
/// The output closes after this long without a sound.
const IDLE_CLOSE: Duration = Duration::from_secs(20);
/// Cues waiting for the sound thread; more than this are dropped, not queued.
const QUEUE: usize = 4;

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

enum Command {
    /// Open the output ahead of the first sound.
    Prepare,
    Play(UiSoundCue),
    /// Sounds were turned off: close the output.
    Close,
}

pub struct UiSounds {
    enabled: bool,
    samples: Option<[Vec<u8>; 3]>,
    worker: Option<SyncSender<Command>>,
}

impl UiSounds {
    pub fn new(enabled: bool) -> Self {
        // A malformed bundled asset is a packaging error, not a user's runtime error.
        let samples = [NAVIGATION_WAV, OK_WAV, BACK_WAV].map(|wav| {
            pcm_bytes(
                &decode_pcm16_mono_48k(wav)
                    .expect("bundled Horizon UI WAV must be PCM16 mono 48 kHz"),
            )
        });
        Self {
            enabled,
            samples: Some(samples),
            worker: None,
        }
    }

    pub fn play_navigation(&mut self) {
        self.play(UiSoundCue::Navigation);
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if !enabled && self.enabled {
            self.send(Command::Close);
        }
        self.enabled = enabled;
    }

    /// Open the output before the user navigates. Never blocks.
    pub fn prepare(&mut self) {
        if self.enabled {
            self.send(Command::Prepare);
        }
    }

    /// Call only after a semantic UI action succeeds; no sound on ignored
    /// input. Never blocks.
    pub fn play(&mut self, cue: UiSoundCue) {
        if self.enabled {
            self.send(Command::Play(cue));
        }
    }

    fn send(&mut self, command: Command) {
        if self.worker.is_none() {
            let Some(samples) = self.samples.take() else {
                return;
            };
            let (sender, commands) = mpsc::sync_channel(QUEUE);
            if !crate::services::worker::spawn("ui-sounds", move || run(&samples, &commands)) {
                return;
            }
            self.worker = Some(sender);
        }
        if let Some(worker) = &self.worker {
            match worker.try_send(command) {
                // A busy or stalled output drops cues instead of queueing them.
                Ok(()) | Err(TrySendError::Full(_)) => {}
                Err(TrySendError::Disconnected(_)) => self.worker = None,
            }
        }
    }
}

/// The sound thread: opens the output on demand, plays cues, and closes it
/// after a quiet spell or when sounds are turned off.
fn run(samples: &[Vec<u8>; 3], commands: &mpsc::Receiver<Command>) {
    if let Err(error) = gst::init() {
        warn!(%error, "UI sounds unavailable; menus remain usable");
        return;
    }
    let output = AudioOutput::detect();
    let mut player: Option<Player> = None;
    loop {
        let command = if player.is_some() {
            commands.recv_timeout(IDLE_CLOSE)
        } else {
            commands.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        match command {
            Ok(Command::Prepare) => {
                if player.is_none() {
                    player = Player::open(output);
                }
            }
            Ok(Command::Play(cue)) => {
                if player.is_none() {
                    player = Player::open(output);
                }
                if let Some(open) = &player
                    && let Err(error) = open.push(&samples[cue.index()])
                {
                    warn!(%error, "UI sound could not play");
                    player = None;
                }
            }
            Ok(Command::Close) | Err(RecvTimeoutError::Timeout) => player = None,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// An open output: a live source feeding the sink. Dropping it stops it.
struct Player {
    pipeline: gst::Pipeline,
    source: gst_app::AppSrc,
}

impl Player {
    fn open(output: AudioOutput) -> Option<Self> {
        Self::try_open(output)
            .inspect_err(|error| warn!(%error, "UI audio output unavailable; menus remain usable"))
            .ok()
    }

    fn try_open(output: AudioOutput) -> Result<Self, String> {
        let caps = gst::Caps::builder("audio/x-raw")
            .field("format", "S16LE")
            .field("layout", "interleaved")
            .field("rate", SAMPLE_RATE)
            .field("channels", 1)
            .build();
        let source = gst_app::AppSrc::builder()
            .caps(&caps)
            .format(gst::Format::Time)
            .is_live(true)
            .do_timestamp(true)
            .build();
        let convert = make("audioconvert")?;
        let resample = make("audioresample")?;
        let sink = output.sink()?;
        // Play each cue the moment it arrives.
        if sink.has_property("sync") {
            sink.set_property("sync", false);
        }
        let pipeline = gst::Pipeline::new();
        pipeline
            .add_many([source.upcast_ref(), &convert, &resample, &sink])
            .map_err(|error| error.to_string())?;
        gst::Element::link_many([source.upcast_ref(), &convert, &resample, &sink])
            .map_err(|error| error.to_string())?;
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|error| error.to_string())?;
        Ok(Self { pipeline, source })
    }

    fn push(&self, pcm: &[u8]) -> Result<(), String> {
        self.source
            .push_buffer(gst::Buffer::from_slice(pcm.to_vec()))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

fn make(name: &str) -> Result<gst::Element, String> {
    gst::ElementFactory::make(name)
        .build()
        .map_err(|error| error.to_string())
}

fn pcm_bytes(samples: &[i16]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect()
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
        assert!(sound.worker.is_none());
    }

    #[test]
    fn samples_are_little_endian_pcm() {
        assert_eq!(pcm_bytes(&[1, -2]), [1, 0, 0xfe, 0xff]);
    }
}
