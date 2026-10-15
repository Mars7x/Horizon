# Current project state — architecture recovery

This describes the prepared working-tree source for the transition in [ADR 0190](adr/0190-flatpak-embedded-emulator-direction.md). Cargo remains at `0.1.0`; this is not a release claim.

## Implemented now

- Rust 2024 and Slint desktop Flatpak, application ID `io.github.Mars7x.Horizon`, runtime/SDK 26.08. Linux x86_64 and aarch64 remain targets; neither architecture is newly build-verified by this recovery.
- Home retains its carousel, selected cover, focus frame, motion, marquee, theme, input behavior and Library tile. Four utilities: **Album, Activity, Achievements, Settings**. Profile opens from the avatar. Library remains full-shell.
- No production game sources are registered. Steam and Heroic game discovery, URI launches, runtime monitoring, local lifetime ingestion and source artwork scanning are retired. No emulator core, game importer or playable replacement is implemented yet; Home/Library have no active games after retirement.
- Album retains Horizon-owned captures, active-account Steam screenshots and their thumbnails, existing user-confirmed deletion rules, video playback and PipeWire audio. Archived identities retain known screenshot titles.
- Steam Account settings, Web API credentials, profile/avatar and read-only friends remain. This is manual SteamID64/API-key configuration, **not OAuth or interactive Steam login**; no such login was implemented to preserve.
- Achievements and Activity Milestones retain the existing read-only Steam API/cache implementation. Previously imported Steam identities remain eligible through archived metadata. Fresh installations have no imported identities; adding an independent account game catalog is deferred. Unavailable/private data is never fabricated as zero.
- SteamGridDB keys, preferences, title lookup, downloading, caching and refresh remain, along with existing artwork files and provenance. With no active games there are no cover jobs. Source-owned Steam icon-cache scanning is retired.
- Historic observed sessions and source lifetime counters remain separate. Activity retains totals, recent sessions and durable records; installed-most-played slots exclude retired games. Generic lifecycle/recovery services remain but have no production launcher to observe.
- SDL3 input, controller glyphs/naming, select/back settings, keyboard/mouse input, reduced motion, scaling, fullscreen and normal desktop lifecycle handling remain.

## Persistence and sandbox

Schema is **0010**. Immutable migrations 0001–0008 remain. Migration 0009 creates `retired_game_sources` and archives Steam membership; 0010 archives Heroic. Neither deletes games, identities, sessions or lifetime counters. Active-library queries continue to use `game_sources`; Album and Achievements read the archive separately. Reconciliation protects archived game rows. Older retirement migrations retain their original behavior; this cannot recover data already deleted by those earlier versions.

Heroic/Legendary filesystem grants are removed. Steam roots remain read-only except the existing Album userdata grants. Networking, portals, graphics, controller input, PipeWire and read-only status permissions remain. There is no host command execution, session helper or compositor integration. Gamescope implementation was already removed in `bf87675` and `06756c5`; its plans/ADR remain historical.

## Validation boundary

Recovery environment: Cargo, rustc and rustfmt are unavailable; `/usr/bin/flatpak` resolves to an unavailable host-execution wrapper, and flatpak-builder is absent. Rust/Slint compilation, Clippy, Rust tests, Flatpak builds and interactive UI checks cannot be claimed. Run the repository quality gate and actual x86_64/aarch64 Flatpak builds before release. In particular, Phase 10.4.1.1 remains unverified without the user's successful build report.

SQL migration checks use temporary in-memory SQLite databases, not user data. See [Development](DEVELOPMENT.md) for reproducible validation and [ADR 0190](adr/0190-flatpak-embedded-emulator-direction.md) for intentionally deferred core decisions.
