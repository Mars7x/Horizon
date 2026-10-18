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
| Home | Established carousel, Library tile, four utilities/profile avatar; Accept plays imported games |
| Library | Full-shell grid/sort/filter; Accept opens imported games in the embedded player |
| Activity | Retained session totals, recent history and distinct lifetime counters |
| Album | Horizon-owned pictures/clips, viewer, video playback and confirmed deletion |
| Achievements | Retained page and Milestones layout; no provider, “Achievements unavailable” |
| Settings | Appearance, Controller → Button mapping, Cores → mGBA, Third-Party → SteamGridDB and Steam Account |
| Profile | Local name/picture, Steam profile picture and read-only Steam friends |

Utilities are **Album, Activity, Achievements, Settings**, in that order.
Steam account setup uses a SteamID64 and Web API key; interactive Steam login
is not implemented. SteamGridDB is independent of Steam account features.
RetroAchievements is intended future work, not an available backend.

Storage uses the fresh [purpose-first layout](STORAGE.md), with readable game
folders, game-named saves and independent playback/core preference files. Old
layout files remain untouched and are not migrated. Four manual save slots and a separate automatic exit state support seamless
resume, enabled by default. States include previews and compatibility checks. Stored screenshots, previews,
profiles and artwork caches use lossless JXL; profile/Album JXL animations are
supported, with import decoders retained for external PNG/JPEG/GIF/APNG images.

## Inactive boundaries

The mGBA systems launch only their imported content through the embedded player; no capture
sources are registered. External URI/foreground-handoff execution remains dormant.
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
[ADR 0192](adr/0192-repository-maintenance.md).
