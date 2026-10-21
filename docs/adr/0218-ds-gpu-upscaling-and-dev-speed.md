# ADR 0218 — DS GPU upscaling, faster launches, crash-safe resume and representative dev builds

Status: Accepted. The user asked for HD scaling for melonDS, smooth 4× speed
and faster starts with save states.

melonDS DS is built with its OpenGL renderer (`ENABLE_OPENGL=ON`; the compute
renderer stays off). When Internal resolution is above Native, Horizon creates a
hidden OpenGL 3.2 core context on the game worker through the system's EGL,
loaded with the existing `libloading` dependency at run time, and implements
libretro hardware rendering for that core only: a framebuffer, the
framebuffer/proc-address callbacks and the context reset/destroy order. Frames
are read back only when the screen can take one, into the existing software
presentation, filters and screenshots. No new permission is needed
(`--device=dri` is already granted); no new crate. If no context can be made,
the game plays at Native. This does not open hardware rendering to other cores.

Launches stay responsive: the cartridge's SHA-256 identity check now runs while
the core loads it, and nothing runs or saves until it matches. Resuming reads a
state without decoding its preview. The worker first sends the picture of the
state it will resume (any core release, for display only), and the player
zooms into it dimmed under a spinner instead of a "Starting game" dialog.

When a core can't reach the chosen fast-forward speed, the game and its audio
follow the speed it can reach instead of breaking up. Development builds
optimise (Horizon at level 1, dependencies at 3) so measurements and everyday
testing aren't distorted by unoptimised code, which had halved DS speed.

The automatic resume point is refreshed every minute of play and when Horizon
loses focus, so a crash loses at most about a minute. Only the snapshot runs on
the game thread; the bundle is written by a separate thread with the same
atomic, synced replacement, one write at a time, and state actions wait for it.
