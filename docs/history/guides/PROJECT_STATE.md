# Current project state — architecture recovery

This describes the prepared working-tree source for the transition in [ADR 0190](../../adr/0190-flatpak-embedded-emulator-direction.md). Cargo remains at `0.1.0`; this is not a release claim.

## Implemented now

- Rust 2024 and Slint desktop Flatpak, application ID `io.github.Mars7x.Horizon`, runtime/SDK 26.08. Linux x86_64 and aarch64 remain targets; neither architecture is newly build-verified by this recovery.
- Home retains its carousel, selected cover, focus frame, motion, marquee, theme, input behavior and Library tile. Four utilities: **Album, Activity, Achievements, Settings**. Profile opens from the avatar. Library remains full-shell.
- No production game sources are registered. Steam and Heroic game discovery, URI launches, runtime monitoring, local lifetime ingestion and source artwork scanning are retired. No emulator core, game importer or playable replacement is implemented yet; Home/Library have no active games after retirement.
- Album shows Horizon-owned captures only, retaining its viewer, user-confirmed deletion, video playback and PipeWire audio. Steam screenshot discovery/viewing/deletion and automatic local controller-file lookup are removed (ADR 0191). SDL hardware recognition and controller navigation remain.
- Steam Account settings, Web API credentials, profile/avatar and read-only friends remain. This is manual SteamID64/API-key configuration, **not OAuth or interactive Steam login**; no such login was implemented to preserve.
- Steam achievements, badge downloads, cache loading and startup/route/account-change workers are removed. Achievements and Activity Milestones retain their layouts with “Achievements unavailable”; no provider is active. RetroAchievements is planned, not implemented. Existing Steam achievement cache files are left untouched and ignored.
- SteamGridDB keys, preferences, title lookup, downloading, caching and refresh remain, along with existing artwork files and provenance. With no active games there are no cover jobs. Source-owned Steam icon-cache scanning is retired.
- Historic observed sessions and source lifetime counters remain separate. Activity retains totals, recent sessions and durable records; installed-most-played slots exclude retired games. Generic lifecycle/recovery services remain but have no production launcher to observe.
- SDL3 input, controller glyphs/naming, select/back settings, keyboard/mouse input, reduced motion, scaling, fullscreen and normal desktop lifecycle handling remain.

## Persistence and sandbox

Schema is **0010**. Immutable migrations 0001–0008 remain. Migration 0009 creates `retired_game_sources` and archives Steam membership; 0010 archives Heroic. Neither deletes games, identities, sessions or lifetime counters. Active-library queries continue to use `game_sources`; Album can read archived titles for Horizon-owned captures. No achievement code reads the archive. Reconciliation protects archived game rows. Older retirement migrations retain their original behavior; this cannot recover data already deleted by those earlier versions.

Heroic/Legendary and all Steam filesystem grants are removed. Steam profile pictures and friends need network account access only. No local Steam installation is read. Networking, portals, graphics, controller input, PipeWire and read-only status permissions remain. There is no host command execution, session helper or compositor integration. Gamescope implementation was already removed in `bf87675` and `06756c5`; its plans/ADR remain historical.

## Validation boundary

Recovery environment: Cargo, rustc and rustfmt are unavailable; `/usr/bin/flatpak` resolves to an unavailable host-execution wrapper, and flatpak-builder is absent. Rust/Slint compilation, Clippy, Rust tests, Flatpak builds and interactive UI checks cannot be claimed. Run the repository quality gate and actual x86_64/aarch64 Flatpak builds before release. In particular, Phase 10.4.1.1 remains unverified without the user's successful build report.

SQL migration checks use temporary in-memory SQLite databases, not user data. See [Development](DEVELOPMENT.md) for reproducible validation and [ADR 0190](../../adr/0190-flatpak-embedded-emulator-direction.md) for intentionally deferred core decisions.
