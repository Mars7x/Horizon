# Horizon — coding-agent instructions

Read this file **before** editing Horizon. It describes rules for the current repository, not a roadmap. Start with [docs/INDEX.md](docs/INDEX.md) and [docs/PROJECT_STATE.md](docs/PROJECT_STATE.md); check the actual source before relying on an older phase write-up.

## Ground truth and scope

1. **The checked-out source and manifests are authoritative for behavior.** `docs/PROJECT_STATE.md` summarizes a dated source snapshot; current feature guides describe intended contracts. ADRs and phase-by-phase appendices record decisions *at the time*, and some have been superseded. Do not resurrect retired implementations because an older document mentions them.
2. **Confirm the working baseline** (`git status`, relevant files, branch and build errors) before proposing or patching anything. Phase labels describe development milestones; `Cargo.toml` is still `0.1.0` and is not a promise about release readiness.
3. Fix the reported issue only. Do not opportunistically redesign Home, refactor unrelated routes, rewrite production code around guessed types, or claim a build passed without actually running it. State uncertainty and missing build tools explicitly.
4. Horizon is a conventional desktop Flatpak moving toward **embedded emulator cores** (ADR 0190). External game launchers are retired. Core hosting, ROM/BIOS management, and the choice of in-process versus child-process execution are not implemented and require a separate decision. Do not resume Gamescope/session work or install host packages.

## Non-negotiable project preferences

- **Preserve Home** unless the requested change explicitly affects Home. Use its current focus frame, selected-cover scaling, motion, and marquee as reference when matching other pages; do not silently replace the Home implementation.
- Four top utilities in this order: **Album, Activity, Achievements, Settings**. Shop is retired. There is no Web or Friends utility; Steam friends live on the **Profile page** (opened from Home's avatar, ADR 0178). They have real `AppRoute::Utility` destinations. The **Library is full-shell**, not a shell-header page. `src/navigation/mod.rs` is the source of truth.
- **Steam friends** are a read-only list on the Profile page through the shared Steam account/API boundary (`services::friends`, `SteamWebApiClient::friends`). A private friend list is reported as private, never as no friends. No messaging, invites or other credential workflow.
- Shop, Flatpak games and host Flatpak execution are retired. Do not reintroduce them. Keep normal desktop portals, fullscreen and Wayland integration.
- **Steam account settings belong in Settings → Third-Party → Steam Account**, not in Achievements. Never place credentials, secret values, or request URLs containing secrets in UI state, logs, docs, screenshots, or commits. Local file permissions are not encryption.
- **Steam scope is limited to profile pictures and read-only friends through the existing account settings.** SteamGridDB is independent and remains. No Steam screenshot access, achievement API/cache loading or local Steam installation lookup. Achievements and Activity Milestones have no provider until separately authorized RetroAchievements work; unavailable is not zero.
- Source-reported lifetime playtime and Horizon-observed session playtime are **different measurements**. Never sum them. Keep the known limitations of foreground handoff and recovered/checkpointed sessions visible.
- Every full-screen page except Home uses the shared page frame (header, header values, breadcrumb, list rows, marquee text, hint bar): follow [docs/UI_STANDARDS.md](docs/UI_STANDARDS.md) rather than hand-building layout.
- All new animation must respect Reduced Motion; keyboard, mouse and controller behavior must remain coherent. Route/history/focus decisions belong in Rust, not in Slint.
- Preserve third-party attribution, SPDX/license texts, and `THIRD_PARTY_NOTICES.md`. User-supplied utility SVG geometry/colors should be preserved unless editing was requested; use the existing scale-aware renderer. **`README.md` must remain blank.**

## Stack and source map

- **Rust 2024**, minimum version from `Cargo.toml` (`rust-version = 1.92` in this snapshot); **Slint ~1.18**; SDL3, SQLite/rusqlite, XDG portals, Flatpak and Wayland-first Slint Winit/Skia.
- `src/domain/`: source-neutral identities, rules; no SQL, UI, SDL or platform types.
- `src/services/`: library/import/launch/activity/artwork/achievements/account coordination. Extend a shared boundary rather than duplicating provider logic.
- `src/sources/`: no production game adapters are registered. No production capture sources are registered either; Album reads Horizon-owned captures only. Generic source boundaries remain available; do not restore retired launchers.
- `src/persistence/`: durable data and numbered immutable migrations. Never edit an already deployed migration to change schema.
- `src/platform/`: portals, backend startup, data paths, status, host integration. Never assume host paths/programs are available inside Flatpak. `data_paths.rs` is the only code that chooses file locations; follow [docs/STORAGE.md](docs/STORAGE.md) (no `io.github.Mars7x.Horizon` subfolder inside Flatpak's private directories).
- `src/input/` and `src/navigation/`: device normalization, route and focus policy; `src/presentation/`: Rust→Slint projections/controllers.
- `ui/app.slint`, `ui/pages/`, `ui/models/`, `ui/components/`, `ui/theme/`: render and forward intent; avoid filesystem/network/business logic in Slint. Prefer existing components/tokens over duplicated layout constants.
- `flatpak/io.github.Mars7x.Horizon.yml`: primary sandbox/build manifest, runtime branch **26.08**. `scripts/check.sh` is the repository quality gate.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for detailed layer boundaries, [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for build instructions, and [docs/INDEX.md](docs/INDEX.md) for feature ownership.

## Common regression traps

- **Slint 1.18.1 syntax and semantics matter.** Validate bindings, `self.` qualification, types, layout units, and focus animation against the actual working components. Avoid runtime gradient-percent coercion or segmented text-slice marquee masks: earlier attempts caused build errors and visible seams. See `docs/HOME_UI.md` and ADRs.
- Focus-frame spacing must follow an **animated selected cover** rather than a fixed grid slot; match Home without modifying it. Route-local focus restoration is different from global Home reset.
- Activity carousel titles: no visible ellipsis; unselected long titles are clipped with a soft edge; only selected titles marquee. Game-details history rows are **read-only and unfocusable**; Up/Down scrolls the history viewport. Activity opens at its first cover on a fresh visit, but Back from game details restores the current cover.
- DB imports are atomic per successful source; incomplete scans must not delete installed identities. Uninstalled games may retain historical sessions but must not occupy active-library or installed-most-played slots.
- Retained lifetime counters are **not** Horizon-observed playtime. Steam account credentials serve profile pictures and friends only; Steam achievements are retired.
- Treat a blank/unavailable data source as unknown, not as a real zero. Protect prior user artwork and persisted settings; do not make network fetching a UI-thread operation.
- Avoid broad Flatpak filesystem, D-Bus, or device grants. There is no host-execution path; `flatpak_host` was removed. No Steam filesystem grants are required; profile pictures/friends use the shared network account boundary.
- Home, Library and Activity share one `presentation::catalog::Catalog` whose indices never move during a run (a reload appends and marks removals); index only slots from `iter`/`position` or a change list.
- Rust standard `PartialEq` implementations for tuples stop at 12 items; avoid giant state-comparison tuples (the Phase 10.4.1.1 E0369 hotfix).

## Safe change workflow

1. Read this guide, [project state](docs/PROJECT_STATE.md), the relevant feature document and recent ADR(s); inspect the real call sites and test coverage.
2. Identify the owner of the behavior, and make the smallest correct change. Prefer source-neutral abstractions over source-name checks scattered in presentation.
3. Add or update tests for domain, navigation, persistence, credential switching, and error cases where applicable. Treat startup, resizing, controller reconnection, rapid route changes, missing sources, and reduced motion as meaningful edge cases.
4. Run **when available**:

   ```sh
   cargo fmt --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-targets --all-features
   bash scripts/check-third-party.sh
   ```

   `bash scripts/check.sh` runs the first three and the attribution check. Validate Slint and Flatpak through GNOME Builder / `scripts/build-flatpak.sh` for relevant changes. Record precisely what ran, failed, or could not run; static scans do **not** equal compilation.
5. Keep relevant **current-state feature docs** accurate; add an ADR for a meaningful architecture/policy change. Retain old ADRs as history and mark reversals rather than deleting their evidence.
6. For Horizon patch deliveries, provide **only changed/new files**, list **exact deletions separately**, and include exact `git add`/`git rm` and `git commit` commands. Never include `.git`, `target`, build directories, cached credentials, or unrelated files. Full repo ZIP only when explicitly requested.

## Release and security constraints

- No production `unwrap`/`expect` for recoverable errors, leaked API keys, fabricated stats, swallowed failures, or dependencies introduced without need.
- Never double-count playtime, alter stored game identity based solely on a title, break migrations, or discard data from a temporarily unavailable launcher.
- Respect licensed third-party assets and user-authored SVGs. Run `scripts/check-third-party.sh` when assets/notices change.
- Flatpak and `aarch64` need actual builds. Installing an SDK is not proof that the application compiled on ARM. **Do not mark Phase 10.4.1.1 as build-verified without the user's successful build report.**
