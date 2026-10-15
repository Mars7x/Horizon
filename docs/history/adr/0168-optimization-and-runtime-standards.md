# ADR 0168: Optimization phase and runtime standards

**Status:** Implemented. Builds, and the gate passes. Its video and sound parts are superseded by [ADR 0169](0169-ffmpeg-and-pipewire-media.md) (FFmpeg and native PipeWire replace GStreamer). Measurements were taken in the Flatpak SDK on this machine (aarch64); the user should confirm video start-up and idle behaviour in the running app.

## Context

The user reported a pause at the start of every video and asked for an optimization and standardization pass over all of Horizon.

## Findings and decisions

**Video start-up pause (the cause was audio, not GStreamer).** Frame timing in the SDK (1080p VP8 + Vorbis clip, scaled to 720p):

- With sound through the PulseAudio socket:
  - frames 0–5 arrived on time;
  - then each frame took about 200 ms for about 400 ms;
  - 29 of 45 frames were shown in 1.5 s.
- With no sound, every frame was on time.

The pipeline is clocked by the audio sink, and the PulseAudio stream's clock barely moves while the stream starts.

Decision: Horizon stays on GStreamer and changes the clock:
- Sound goes straight to PipeWire (`pipewiresink`) when its sink can connect. Detection connects once, during the Album's warm-up.
- Otherwise the pipeline runs on the system clock and `pulsesink` follows it.

Both paths measured on-time frames from the first frame: 47/47 and 46/47. Replacing GStreamer (libmpv, FFmpeg) would not fix a clock problem. It would also mean bundling codecs the runtime already ships.

**Images (Glycin considered).**
- `image-rs` already decodes PNG and JPEG in memory-safe Rust. These are the same decoders Glycin uses for those formats.
- Glycin's addition is a sandboxed process per image. It needs loaders that `org.freedesktop.Platform` doesn't ship, and its per-image process cost would slow the thumbnail grid.

Decision: one module, `services::image`:
- the format is detected from content;
- decodes are limited to 16K per side and 256 MB;
- header-only size checks.

Two sites used to fully decode an image only to read its size. Glycin can still sit behind this module later for HEIF/AVIF/JXL.

**Idle wake-ups.** At rest Horizon woke the UI thread about 170 times a second:
- SDL every 8 ms;
- the status drain every 50 ms;
- the artwork and achievements drains every 120 ms;
- lifecycle polls.

Decisions:
- Worker results wake the UI through `services::worker` and `presentation::wake`, removing three polling timers.
- SDL polls every 8 ms only while a controller can navigate: 100 ms with no controller, 250 ms while another app has the foreground.
- `Motion.loops-paused` stops the focus breathing and other loops in the background. The harness confirms Home, Library, Settings and Album draw nothing then.

The lifecycle polls (250–500 ms, about 9 wake-ups a second) are unchanged: they return at once when no session is active, and playtime tracking is sensitive.

**Start-up.** Cached covers decode in parallel. On this machine's cache, 26 covers took 66 ms in sequence and 12 ms in parallel.

**Standardization.**
- One worker channel, one `spawn` helper (named threads, no panics), one image reader.
- One `ActivityController::refresh` instead of seven copies.
- Album's dedicated `album-media-ready` callback is gone.

## Audio never blocks the UI (follow-up)

After the PipeWire grant, Horizon froze before showing its window. SDL3's PipeWire audio backend (`PIPEWIRE_PREFERRED_Init`) waits for the server's device listing with no timeout, on the main thread, and the host audio server had stalled (GNOME Settings' sound streams timed out too, after WirePlumber errors on a Bluetooth headset; restarting PipeWire and WirePlumber recovered it). Decisions:
- UI sounds move from SDL to GStreamer on their own thread (`audio.rs`). The UI posts cues and never waits; a stalled server only silences them. Measured with the server stalled: 20 cues cost the UI thread 0.18 ms.
- Each video's state changes and seeks run on its own control thread; the UI's handle only sends commands and queries.
- The output choice (`platform::audio_output`) is shared and detected off the UI thread.

SDL remains for controllers only.

## Sandbox

`--socket=pulseaudio` alone doesn't let `pipewiresink` connect. At the user's choice the manifest adds `--filesystem=xdg-run/pipewire-0`, so the shipped app uses PipeWire, keeping sound and picture in step from the first sample. The system-clock path remains the fallback (older hosts, or the permission removed).
