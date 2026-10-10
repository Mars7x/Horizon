# Horizon documentation index

**For a new coding agent:** read [../AGENTS.md](../AGENTS.md), then [PROJECT_STATE.md](PROJECT_STATE.md), then [DEVELOPMENT.md](DEVELOPMENT.md). Use feature documentation for local invariants and inspect the code before patching. The blank repository-root `README.md` is deliberate.

## Current entry points

| Question | Start here | Primary code |
| --- | --- | --- |
| What currently exists? | [Project state](PROJECT_STATE.md) | `src/navigation/mod.rs`, `src/app.rs` |
| How do I build, test and debug? | [Development](DEVELOPMENT.md) | `Cargo.toml`, `scripts/`, Flatpak manifest |
| Which layer owns behavior? | [Architecture](ARCHITECTURE.md) | `src/{domain,services,presentation,persistence,sources,platform,input}/` |
| Which pages/routes exist? | [Navigation](NAVIGATION.md) | `src/navigation/`, `src/presentation/navigation.rs`, `ui/app.slint` |
| Home visuals and focus? | [Home UI](HOME_UI.md), [Typography](TYPOGRAPHY.md) | `ui/pages/home.slint`, `ui/components/` |
| How must every other page look? | [UI standards](UI_STANDARDS.md) | `ui/components/page-header.slint`, `list-row.slint`, `marquee-text.slint`, `hint-bar.slint` |
| Library focus, sources, sorting? | [Library](LIBRARY.md) | `src/presentation/library.rs`, `ui/pages/library.slint` |
| Observed playtime and session history? | [Activity](ACTIVITY.md) | `src/services/activity.rs`, `src/presentation/activity.rs` |
| Steam achievements and recent unlocks? | [Achievements](ACHIEVEMENTS.md) | `src/services/achievements.rs`, `src/presentation/achievements.rs` |
| External credentials or appearance? | [Settings](SETTINGS.md), [Appearance](APPEARANCE.md) | `src/services/steam_account.rs`, `src/presentation/settings.rs` |
| Imports and provider capabilities? | [Sources](SOURCES.md), [Steam](STEAM.md), [Heroic](HEROIC.md) | `src/sources/`, `src/services/import.rs` |
| Database or migrations? | [Database](DATABASE.md), [Domain](DOMAIN.md) | `src/persistence/`, `src/domain/` |
| Art, attribution, permission review? | [Artwork](ARTWORK.md), [Licensing](LICENSING.md) | `ui/assets/`, `REUSE.toml`, `THIRD_PARTY_NOTICES.md` |

Additional focused guides: [Input](INPUT.md), [Clock](CLOCK.md), [Live status](STATUS.md), [Managed sessions](MANAGED_SESSIONS.md), [Shell hardening](SHELL_HARDENING.md), and [Flatpak notes](../flatpak/README.md).

## Historical records are not the current spec

The numbered [architecture decision records](adr/) and older phase sections explain *why* the project evolved. Some explicitly document implementations later retired or reverted (e.g., previous source adapters, marquee experiments, early route shell geometry, placeholder Activity/Milestones). Never implement an old phase target as if it were pending current work. Check the current code and the most recent applicable decision first.

When a behavior changes, update its feature guide and this index if ownership changes; add an ADR if the decision has lasting consequences. Prefer correcting inaccurate current assertions to appending another contradictory status paragraph.
