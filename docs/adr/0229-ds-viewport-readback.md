# ADR 0229 — DS viewport readback

Status: Accepted. The user requested larger performance gains after profiling
identified 8× DS frame transfers as more expensive than emulation.

For GPU-rendered DS with None filtering, sample each screen to its physical
viewport size in a worker-owned OpenGL colour framebuffer before readback.
The core still renders at its selected internal resolution. UI geometry and
per-screen clips are unchanged; view/layout/physical-size snapshots determine
the next presentation. GL nearest sampling preserves sharp pixels; exact
texel-boundary ties may resolve to either adjacent source texel per driver.

Viewport presentation no longer warms full-size capture allocations during play.
Before audio starts, warm one serialization/capture/preview operation and discard
its boot picture/history point. This keeps the first rewind snapshot from
allocating a large cold buffer during audible gameplay and retains the existing
audio queue/latency policy.

Presentation images never replace the full core image. Screenshots, save-state
previews and rewind retain their full-resolution capture boundary. Other filters,
native DS, mGBA, unknown viewport sizes and enlarging viewports keep their current
pipeline. Failed presentation framebuffer allocation logs a warning once and
falls back to full-size presentation for that session.

Restore the core’s GL read/draw framebuffer, texture, pack alignment and scissor
state after presentation, including failure paths. Free presentation textures
and framebuffer on the same worker before destroying the context. Keep only a
bounded latest frame between worker, filter and UI. This is a narrow DS host
optimization, not a new renderer framework, core or process API.

Regression coverage compares GPU output against CPU nearest sampling (including
legal boundary ties), mixed screen sizes and resizes; checks full capture pixels
and GL state preservation; and exercises real DS playback/pacing/audio. GPU
performance, supported architectures and packaged UI require actual runtime
verification, independently of pure geometry and pixel tests.
