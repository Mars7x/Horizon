//! Where Horizon's sound goes (UI sounds and video), through GStreamer.
//!
//! PipeWire's own sink when it can connect: it clocks playback from the
//! first sample, unlike the PulseAudio socket. Otherwise GStreamer's
//! automatic choice. Connecting talks to the audio server, which can stall
//! when the server is unwell, so everything here runs on background threads,
//! never on the UI thread.
use std::sync::OnceLock;

use gstreamer::{self as gst, prelude::*};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioOutput {
    /// PipeWire's own sink, which also clocks the pipeline.
    PipeWire,
    /// GStreamer's automatic sink (PulseAudio), with the system clock.
    Default,
}

static DETECTED: OnceLock<AudioOutput> = OnceLock::new();

impl AudioOutput {
    /// The output to use, connecting once to find out. May block while the
    /// audio server answers: call only off the UI thread.
    pub fn detect() -> Self {
        *DETECTED.get_or_init(|| {
            let connects = gst::init().is_ok()
                && gst::ElementFactory::make("pipewiresink")
                    .build()
                    .is_ok_and(|sink| {
                        let ready = sink.set_state(gst::State::Ready).is_ok();
                        let _ = sink.set_state(gst::State::Null);
                        ready
                    });
            let output = if connects {
                Self::PipeWire
            } else {
                Self::Default
            };
            tracing::debug!(?output, "sound output");
            output
        })
    }

    /// The output if already detected; never blocks.
    pub fn detected() -> Option<Self> {
        DETECTED.get().copied()
    }

    /// A sink for this output. The PipeWire sink connects later, when its
    /// pipeline starts.
    pub fn sink(self) -> Result<gst::Element, String> {
        let name = match self {
            Self::PipeWire => "pipewiresink",
            Self::Default => "autoaudiosink",
        };
        gst::ElementFactory::make(name)
            .build()
            .map_err(|error| error.to_string())
    }
}
