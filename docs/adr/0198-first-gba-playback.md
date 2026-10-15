# ADR 0198 — First GBA playback through mGBA

Status: Accepted following explicit user approval of mGBA through libretro inside
Horizon. This resolves the first-core hosting decision deferred by ADR 0190;
future cores/process changes require their own review.

## Decision and alternatives

Bundle mGBA 0.10.5's software libretro core and run one instance on a worker in
Horizon's process. Its native C API was also considered: it exposes useful mGBA
facilities but couples Horizon to its concrete core structures. The libretro v1
callbacks provide the narrow video/audio/input/save boundary needed here.

An isolated helper could contain a native crash but requires process supervision,
frame/audio transport and synchronized input/save lifecycle. The user chose the
smaller in-process first implementation. A core crash can therefore terminate
Horizon; do not claim crash isolation or invent a permanent multi-core framework.
The unsafe ABI is confined to the platform host. No external emulator application,
RetroArch frontend, launcher URI, host execution or compositor work is introduced.

## First playable contract

- Home and Library dispatch the imported GBA SHA-256 to the in-app player.
  Re-read one available location into a bounded snapshot and verify those same
  bytes before core loading. Missing/changed files give a retryable import error.
  Clear the legacy foreground-handoff latch on embedded dispatch so returning to
  the same window permits a subsequent Home or Library launch.
- Run all core calls on one worker. Slint receives one latest aspect-preserving
  software frame; native PipeWire audio uses a bounded queue and nonblocking
  real-time callback. SDL retains main-thread ownership.
- Gameplay uses held press/release state, independent of menu repeat/confirm
  settings. Pause consumes menu actions; Start/B never navigate the shell while
  the game is running. Window deactivation pauses and releases inputs.
- Use mGBA's built-in BIOS implementation. Do not bundle a Nintendo BIOS or add
  BIOS configuration, sensors, shaders, cheats, multiplayer or new permissions.
- Persist changed battery RAM by content hash with atomic replacement and a
  previous-save backup. Checkpoint while running, save on quit, and preserve
  existing data on errors. Save failure leaves a paused game with retry available.
- Begin a source-runtime observed session only after a real frame is produced;
  checkpoint it and close it after normal saving. A paused but loaded core remains
  in that runtime session. Failed sessions are interrupted, never fabricated as
  completed play. Provider lifetime totals remain separate.

## Packaging and verification

The manifest pins source URL/checksum and builds only the core, without desktop
frontends or optional integrations. Retain MPL-2.0, inih BSD, blip-buf LGPL and the
libretro-header MIT notice. Source is available through the pinned release.
Load the packaged path in the product; an explicit native development environment
override supports testing without a system core installation.

Verify x86_64 and aarch64 separately. Generated original homebrew tests exercise
real core video, audio callbacks, held input and battery memory without commercial
ROMs or BIOS data. Window tests cover input routing, pause, saving and relaunch.
SDK compilation alone is not a successful packaged build or physical gamepad test.

Sources: [mGBA release build](https://github.com/mgba-emu/mgba/blob/0.10.5/CMakeLists.txt),
[libretro adapter](https://github.com/mgba-emu/mgba/blob/0.10.5/src/platform/libretro/libretro.c),
[native core API](https://github.com/mgba-emu/mgba/blob/0.10.5/include/mgba/core/core.h),
[license](https://github.com/mgba-emu/mgba/blob/0.10.5/LICENSE).
