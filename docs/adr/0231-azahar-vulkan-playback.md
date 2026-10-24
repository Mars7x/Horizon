# ADR 0231: Initial Azahar playback through Vulkan

Status: Accepted by explicit user direction for 3DS support and Vulkan.

## Decision

Bundle the official Azahar 2126.2 libretro core from pinned, unmodified source
and recursive dependencies. Build Vulkan only. Extend the existing worker and
RGBA/audio presentation boundary narrowly for this approved core; this does not
authorize arbitrary core loading, host execution or a generic Vulkan plugin API.

Use native combined-screen output and synchronous fenced Vulkan readback initially.
Keep melonDS's OpenGL renderer intact. Large 3DS files are hashed through a held
file descriptor and passed by full path without allocating a ROM-sized buffer.
Use per-game Azahar user directories so save management and resets remain scoped.

Initial support includes import, launch, buttons/analog/touch, audio, screenshots,
normal/2× speed and in-game saves. Hide state/rewind/resume actions for 3DS. Azahar's
serialize-size query serializes the machine and must not be polled during play.
No CIA installation, key management, system-file discovery or settings frontend
is included. Missing required dumped system data remains a compatibility limit.

## Consequences

Vulkan 1.1 and a suitable graphics device are required; no OpenGL fallback is
provided for 3DS. Native readback has a per-frame fence and copy cost; a future
optimization must preserve synchronization and be measured. Per-game isolation
can duplicate system/cache data, but prevents one game's reset affecting another.
Manual states, rewind, extra resolution/layout controls and a shared system-data
workflow require separate work and validation. Builds and runtime checks are
reported separately for each architecture. All upstream licenses and source
pins are retained; no Nintendo content is bundled.
