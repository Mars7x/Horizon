# ADR 0169: FFmpeg and native PipeWire replace GStreamer

**Status:** Implemented. The build and gate pass. Measured in the Flatpak SDK, playing through Bluetooth headphones. The real Album controller plays an H.264/AAC clip in the harness. The user should confirm UI-sound latency and video playback in the running app.

Supersedes the video and sound parts of [ADR 0166](0166-album-captures-and-media-pipeline.md) and [ADR 0168](0168-optimization-and-runtime-standards.md).

## Context

After ADR 0168, the user still heard a hitch in video playback and found UI sounds slightly slower, and asked to move off GStreamer. Three things were behind that:
- **UI sounds:** they went through a GStreamer pipeline into `pipewiresink` (ranked "none", not meant as a default sink), with its buffering, and were reopened after 20 idle seconds.
- **The start-up hold:** most of it is the output's latency. Over Bluetooth the sound is heard about 250 ms after it is written, and a player that waits for the sound holds the picture that long.
- **Weight:** GStreamer's plugin registry and pipelines are heavy for what Horizon needs.

## Decision

**Decoding: FFmpeg, through a small C shim (`csrc/horizon_media.c`).**
- The shim has one handle per file: open, seek, and next decoded unit.
- Video comes out scaled to the window as packed RGBA. Audio is resampled to 48 kHz stereo float.
- Struct layouts stay in C, compiled with the SDK's headers. No generated bindings, so no libclang.
- The runtime ships FFmpeg, with the full codec set from `codecs-extra`. Nothing is bundled.

**Sound: native PipeWire streams, through a second shim (`csrc/horizon_audio.c`).**
- PipeWire calls Horizon's fill on its real-time thread, with the current output latency.
- Opening never waits on the server.

**Player (`platform::video::FfmpegVideo`).** Each video has two threads:
- **decoder:** half a second of sound and a few frames ahead;
- **presenter:** one clock.

The clock:
- Pressing play, seeking or resuming shows the still picture and holds it until the sound there is actually heard (the output latency, about 250 ms over Bluetooth). Then the picture starts and keeps going.
- So the picture never plays, stops and plays again (the user's complaint), and no sound is skipped. The user chose this over starting the picture at once and skipping the late sound.
- If no sound arrives within a second, the picture starts anyway.
- After that, the sound steers the clock by at most 4 ms per reading, so the settling latency estimate never jumps the picture.
- Silent clips, and the tail after the sound ends, run on system time.

**UI sounds.** One native stream asking for 256-frame (about 5 ms) cycles:
- A cue is an atomic store that the next cycle picks up.
- It pauses after 10 quiet seconds and while a game has the foreground.

**Sandbox.** Nothing uses PulseAudio any more. The Flatpak drops `--socket=pulseaudio` and keeps `--filesystem=xdg-run/pipewire-0`.

**Interface.** The `VideoBackend` contract is kept. `VideoSink::duration` is new: the file opens off the UI thread, so its length arrives later.

## Measurements

**Video, through Bluetooth (about 250 ms output latency):**
- The still picture shows 83–100 ms after the press.
- It holds about 230 ms until the sound is heard.
- Then a frame every 31–33 ms, none dropped and no stop after starting, in all three runs.
- With GStreamer and PulseAudio, frames stalled for about 400 ms after start-up.

**UI sounds:**
- `play()` costs the UI about 10 µs.
- The stream asks for 5 ms cycles; the device's own latency comes on top.

## Consequences

- GStreamer's crates are gone. FFmpeg and PipeWire are dynamically linked runtime libraries. `cc` and `pkg-config` become build dependencies.
- Two small test clips are in `tests/fixtures/`.
- Hardware decoding (VA-API) and a zero-copy texture path can come later behind the same trait.
- Starting a video holds its first picture for one output latency: about 250 ms over Bluetooth, a few ms on speakers.
