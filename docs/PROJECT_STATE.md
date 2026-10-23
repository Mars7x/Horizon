# Project state

Horizon is an early-development Rust/Slint Linux desktop Flatpak. Its approved
direction is embedded emulator cores. Game folder selection automatically scans and imports
supported files: Game Boy, Game Boy Color, Game Boy Advance and Nintendo DS. Bundled offline
hash catalogs provide exact title recognition, with unverified filename fallbacks
for unknown files. Existing local XML catalogs remain supported. Imported games
launch in Horizon using bundled mGBA (GB/GBC/GBA) and melonDS DS (DS) libretro
cores on an in-process worker. See
[playback](GBA_PLAYBACK.md). BIOS management, DSi, further cores and a permanent
multi-system hosting API remain unimplemented.

First launch presents [Welcome/Profile setup](ONBOARDING.md), then optional game
import and Ready. Completed setup is skipped; unfinished confirmed steps resume.

## Current surfaces

| Surface | Current behavior |
| --- | --- |
| Home | Established carousel, Library tile, three utilities/profile avatar; Accept plays imported games |
| Library | Full-shell grid/sort/filter; Accept opens imported games in the embedded player |
| Activity | Retained session totals, recent history and distinct lifetime counters |
| Album | Horizon-owned screenshots/animated images, viewer and confirmed deletion |
| Settings | Appearance; Controller → Button mapping and DS button mapping; Cores → mGBA and melonDS; Third-Party → SteamGridDB and Steam Account |
| Player | GB/GBC/GBA and DS in a zoomed shell with quick menu, visual rewind, playback speed, four state slots and a crash-safe automatic resume point; DS layouts, swap, lid, blow and GPU internal resolution |
| Profile | Local name/picture, Steam profile picture and read-only Steam friends |

Utilities are **Album, Activity, Settings**, in that order.
Steam account setup uses a SteamID64 and Web API key; interactive Steam login
is not implemented. SteamGridDB is independent of Steam account features.
Game options on +/Start/F10 provide save management, properties, persistent
Hide/Unhide and confirmed ROM uninstall. Save management opens the save folder,
backs up progress or explicitly resets battery saves and states with confirmation.
ROM uninstall retains saves, states, captures and history.
RetroAchievements is intended future work, not an available backend.

Storage uses the fresh [purpose-first layout](STORAGE.md), with readable game
folders, game-named saves and independent playback/core preference files. Old
layout files remain untouched and are not migrated. Four manual save slots and a separate automatic resume
state (written on exit, every minute of play and when Horizon loses focus) support seamless
resume, enabled by default. A game new to Horizon imports a same-named save from
beside its file once. DS layout and swap are remembered per game. States include previews and compatibility checks. Stored screenshots, previews,
profiles and artwork caches use lossless JXL; profile/Album JXL animations are
supported, with import decoders retained for external PNG/JPEG/GIF/APNG images.

## Inactive boundaries

The cartridge systems launch only their imported content through the embedded player; no capture
sources are registered. Only melonDS may draw with the GPU (its internal
resolution, through a hidden OpenGL context); mGBA stays software-only. External URI/foreground-handoff execution remains dormant.
Optional SteamGridDB lookup can supply covers for imported games. The reusable importer
opens from first-run setup.
The shared append-only catalog and historical SQLite identities remain intact.

There is no Shop, external launcher integration, Steam screenshot/achievement
integration, host command execution, dedicated session or compositor support.
Normal Wayland, fullscreen, scaling, SDL input, desktop portals and application
lifecycle handling remain. Linux x86_64 and aarch64 remain targets; compatibility
requires real builds and core-by-core assessment when cores are introduced.

## Verification

A source snapshot or SDK installation does not prove a successful build. Record
actual commands, toolchain, failures and tested architecture when delivering a
change. See [Development](DEVELOPMENT.md). Current decisions are in
[ADR 0190](adr/0190-flatpak-embedded-emulator-direction.md),
[ADR 0191](adr/0191-steam-profile-and-friends-only.md) and
[ADR 0192](adr/0192-repository-maintenance.md); DS playback follows
[ADR 0216](adr/0216-nintendo-ds-playback.md) to
[ADR 0219](adr/0219-ds-lid-microphone-language-and-per-game-view.md).
