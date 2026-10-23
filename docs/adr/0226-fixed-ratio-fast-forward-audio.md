# 0226 — Fixed-ratio fast-forward audio

Status: Accepted

## Context

Fast-forward estimated reachable audio speed from an average of individual core,
GPU presentation and rewind-capture costs. That estimate excluded pacing waits
and could differ from actual emulated throughput. Black 2 could reach 2× while
the estimator lowered its audio decimation ratio, changing pitch and overfilling
the queue. With audio-clocked pacing this feedback could also slow gameplay.
The DS deadline cap in [0225](0225-ds-fast-forward-pacing-and-compact-toggle.md)
prevented one pacing failure but did not remove the variable-pitch source.

## Decision

Audio follows only the selected playback speed: normal passes samples unchanged,
and double speed uses the existing stereo low-pass filter with exact 2:1
decimation. Remove the frame-cost estimator and its core setter. Short work
spikes cannot retune the filter or inflate the audio used to pace the game.
Retain DS's selected deadline cap and mGBA's audio-clocked handling of uneven
Game Boy batches. Speed changes still reset the filter and discard stale audio.

This replaces the adaptive-audio policy discussed in 0225; its compact control
and DS deadline decision remain current. A core/GPU that cannot sustain the
selected speed can still run short of audio; do not claim buffering can create
missing emulated frames. Emulated throughput and display-limited presented FPS
are separate measurements.

For upscaled DS with the None filter, the software presentation stage may sample
nearest pixels at the physical screen viewports before upload, avoiding large
textures that the UI would immediately shrink. This changes neither the core's
selected internal resolution nor captured source frames. Preserve per-screen
clip bounds, aspect ratio and existing native/unknown-viewport/filter paths.

## Verification

Check exact output count across uneven batches, stereo separation and frequency
alias rejection. Run the native player against representative DS and mGBA games,
measuring normal/double emulation speed, presented frames and audio gaps/drops.
Use actual Flatpak builds/runtime checks and record hardware/architecture limits.
