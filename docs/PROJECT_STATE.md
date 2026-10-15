# Project state

Horizon is an early-development Rust/Slint Linux desktop Flatpak. Its approved
direction is embedded emulator cores. No emulator core, ROM importer, BIOS
manager or core-hosting implementation exists yet. Process isolation and the
permanent core API remain undecided.

## Current surfaces

| Surface | Current behavior |
| --- | --- |
| Home | Established cover carousel, Library tile, four utilities and profile avatar |
| Library | Full-shell grid/sort/filter implementation; no active games after source retirement |
| Activity | Retained session totals, recent history and distinct lifetime counters |
| Album | Horizon-owned pictures/clips, viewer, video playback and confirmed deletion |
| Achievements | Retained page and Milestones layout; no provider, “Achievements unavailable” |
| Settings | Appearance, Controller, Third-Party → SteamGridDB and Steam Account |
| Profile | Local name/picture, Steam profile picture and read-only Steam friends |

Utilities are **Album, Activity, Achievements, Settings**, in that order.
Steam account setup uses a SteamID64 and Web API key; interactive Steam login
is not implemented. SteamGridDB is independent of Steam account features.
RetroAchievements is intended future work, not an available backend.

## Inactive boundaries

No production game or capture sources are registered. Generic import, reload,
launch, runtime-observation, artwork and session services remain for later
assessment. No production source currently generates launches or cover-fetch jobs.
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
