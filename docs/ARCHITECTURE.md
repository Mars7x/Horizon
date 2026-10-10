# Architecture

> **Current-state note:** This document includes the original per-phase architecture record. The implementation is now beyond those phases. Start with [PROJECT_STATE.md](PROJECT_STATE.md) and [INDEX.md](INDEX.md) for the current feature inventory, and inspect the checked-out source before relying on an older phase boundary. Do not reintroduce a retired route, provider, or animation from an early phase description.

**Current layers:** `src/navigation/` and `src/input/` own semantic navigation; `src/presentation/` maps controllers to `ui/`; `src/services/` coordinates source-neutral use cases; `src/domain/` defines pure concepts; `src/persistence/`, `src/sources/` and `src/platform/` implement infrastructure. `ui/app.slint` composes Home, full-shell Library and seven utility routes. Production game adapters are **Steam and Heroic**. Shared Steam account configuration lives in `src/services/steam_account.rs`, outside Achievements, to permit future (not yet implemented) Steam Friends support.

## Architectural history (original phase boundaries)

## Dependency direction

The project uses strict inward dependency flow:

1. **UI (Slint)** displays presentation state and emits user intent.
2. **Presentation** converts application state into UI-friendly models and applies resolved appearance tokens.
3. **Services** coordinate use-cases such as import, launch, library, and activity.
4. **Domain** contains pure application concepts and rules.
5. **Infrastructure adapters** implement persistence, launcher sources, SDL input, and portals behind explicit interfaces.

The domain must not depend on Slint, SQLite, SDL, Flatpak, DBus, or source-specific formats.

## Rules

- No application/business logic in `.slint` files.
- No source-specific branching outside source adapters/registries.
- No hard-coded platform paths outside platform/source infrastructure.
- No SQL in UI or presentation code.
- No broad Flatpak permissions as a workaround for missing architecture.
- No production `unwrap()` for recoverable failures.
- Database schema changes use numbered migrations once persistence is introduced.
- UI colors, spacing, typography, radii, and motion values come from the Phase 1 design system.
- UI chrome and generated interface geometry use semantic theme colors. Authored artwork may carry intrinsic colors when its documented contract requires byte-for-byte/direct rendering; do not recolor such artwork through theme tokens.
- Portal/DBus types terminate at the infrastructure boundary and are translated into Horizon domain types.
- New capabilities must fit an existing boundary or justify a documented architecture change.

## Phase 1 boundary

Phase 1 adds:

- semantic color palette
- spacing/radius/typography/motion tokens
- light and dark appearance resolution
- XDG system color-scheme support
- XDG accent-color support
- XDG higher-contrast support
- XDG reduced-motion support
- live portal updates
- deterministic fallbacks when portal settings are unavailable
- a reusable focus-frame component using the semantic accent/focus color

See `APPEARANCE.md` and ADR 0003.


## Phase 2 boundary

Phase 2 adds the first production-shaped presentation slice:

- Rust-owned demo game model and selection state
- Slint `HomePage` composition
- reusable `GameCarousel` and `GameTile` components
- reusable top navigation, selected-game label, and footer
- original generated demo artwork
- selected-tile scaling and accent focus brackets
- animated carousel translation
- reduced-motion compliance for all new animations

Phase 2 does **not** add keyboard/controller navigation, persistence, importers, or launch logic. Those remain separate roadmap phases. See `HOME_UI.md` and ADR 0004.

## Phase 3 boundary

Phase 3 adds the input architecture:

- device-independent `UiAction` values
- keyboard normalization in Rust
- SDL3 mapped-gamepad normalization in Rust
- left-stick axis normalization with hysteresis/repeat in the SDL adapter
- one action path into the Rust-owned `HomeController`
- controller enumeration and hotplug handling
- connected-controller presentation state
- main-thread SDL polling through a Slint timer
- Flatpak `input` device permission and a pinned SDL3 module

Slint forwards keyboard events but does not interpret individual keys. SDL button constants terminate inside `src/input/sdl.rs`. Phase 3 deliberately does not add launch behavior, persistence, or multi-page navigation. See `INPUT.md` and ADR 0005.

## Slint platform initialization

Horizon compiles Slint with Winit Wayland/X11 and Skia while deliberately
leaving `backend-default` disabled. `src/platform/slint_backend.rs` explicitly
selects `winit` + `skia` before any Slint platform-dependent API is called.
Do not replace this with a Flatpak-only environment variable or enable Qt as a
workaround. See ADR 0007.


## Phase 4.1 boundary

Phase 4.1 introduces the top-level navigation model without changing the visible page composition yet:

- pure Rust `AppRoute`/`Navigator` route state (later generalized by ADR 0035 into Home, Library, and `Utility(UtilityPage)`);
- a pure Rust `Navigator` that owns current route and back history;
- global Home semantics that clear history and establish Home as the root;
- a presentation `NavigationController` between semantic input and page controllers;
- explicit Rust-to-Slint `AppRouteView` mapping;
- unit tests for route/history behavior without rendering or SDL hardware.

The active route is published to Slint, but Phase 4.1 still renders the existing Home page only. Actual page composition belongs to Phase 4.2. See `NAVIGATION.md` and ADR 0028.

## Phase 4.2 boundary

Phase 4.2 turns the published route into visible shell composition:

- `AppWindow` owns top and bottom shell chrome;
- Home/Library use the central shell bounds;
- full-shell destinations replace the chrome rather than duplicating it;
- Home is a routed page rather than the entire application surface;
- Home's established scene geometry is preserved across the shell refactor;
- page switching is driven only by Rust-published `AppRouteView` state.

Later Phase 4.6 refinements generalized the full-shell destination model so all five top utilities are routed submenu pages. See `NAVIGATION.md`, ADR 0029, and ADR 0035.

## Phase 4.3 shell focus boundary

The navigation layer owns screen-level focus between page content and persistent header utilities. Pure Rust types in `src/navigation/` describe the focus region, selected utility, and the `UtilityPage` route opened by each utility. `presentation::NavigationController` publishes those decisions to Slint. Slint may emit a utility activation callback for pointer input, but it must not choose routes or mutate history.

## Phase 4.4 route-focus boundary

Route history and shell focus remain separate concerns but are coordinated by the presentation navigation controller. `RouteFocusMemory` in `src/navigation/` stores one durable `FocusSnapshot` per `AppRoute`. A snapshot contains only the owning shell region and selected utility; temporary Home spatial-transfer anchors are deliberately excluded.

Before a route change, `NavigationController` saves the source route's snapshot. Home/Library may remember either content or top-utility focus. Every `AppRoute::Utility(...)` destination is full-shell with no visible utility row, so those snapshots are normalized to content focus on save/restore while retaining their utility identity. Back therefore restores both route and valid route-local focus. Global Home resets Home to content focus and the first Home game while clearing route history. Slint continues to render only the published state and does not own restoration policy.

See ADR 0032.

## Phase 4.5 global-action boundary

`UiAction::Back`, `UiAction::Menu`, and `UiAction::Home` are classified as global shell actions in the input model, but their behavior remains in `NavigationController`. Keyboard and SDL adapters only normalize physical input.

The global shell menu is modal presentation state (`ShellMenuState`), not an `AppRoute`. Opening it therefore does not push navigation history or alter route-local focus memory. `NavigationController` publishes only a boolean to Slint, which renders `ShellMenuOverlay`. Back/Menu/Home determine modal lifetime in Rust; global Home additionally resets the Home selection to the first game through `HomeController`.

See ADR 0033.


## Phase 4.6 route-transition presentation boundary

Route policy remains entirely in Rust. `NavigationController` publishes `AppRouteView`; Slint maps that presentation enum to retained `PageTransitionLayer` instances for Home, Library, and all five utility submenu routes. Home/Library retain the central shell bounds while every `UtilityPage` fills the logical surface. The layer owns only visual interpolation and pointer isolation during overlap. It cannot push/pop routes, alter focus memory, or change Back/Home/Menu behavior.

Route motion is limited to opacity plus a small vertical settle using `Metrics.page-transition-offset` and `Motion.page-duration`. The latter already resolves to zero through the appearance pipeline when the host requests Reduced Motion, so accessibility preference changes do not require a second navigation code path. Shell chrome is outside the transition contract and is suppressed for the lifetime of any utility submenu route.

Retaining the route components lets outgoing and incoming pages overlap for a real crossfade and preserves transient presentation-only state. Durable state must still live in the existing Rust controllers/domain layers; retained Slint component state must never become a substitute for application state.

See ADR 0034.


## Phase 4.7 shell-hardening boundary

Phase 4.7 does not add a new application layer. It strengthens existing boundaries:

- `src/input/sdl.rs` treats gamepad add/remove as topology changes and clears device-derived hold/repeat latches there; presentation never receives raw device lifecycle state.
- `src/navigation/RouteFocusMemory` enforces route-valid focus snapshots instead of requiring every caller to remember which routes expose shell chrome.
- `presentation::NavigationController` publishes the active route plus the immediate transition source. Slint uses that pair only to bound visual overlap; route history remains exclusively in `Navigator`.
- `AppWindow` keeps responsive sizing presentation-only and guards transient zero-sized surfaces without changing the 1280×720 reference-space contract.

Stress coverage stays framework-independent where possible: deep route/back-stack churn, global Home reset, focus normalization, spatial helper bounds, and input latch resets are pure Rust tests. See ADR 0036 and `SHELL_HARDENING.md`.


## Phase 5 domain/persistence boundary

Phase 5 introduces the durable library without coupling it to presentation or source-specific formats:

- `src/domain/` owns validated `GameId`, `GameTitle`, `SourceId`, `ExternalGameId`, `SourceGameRef`, and `LibraryGame` concepts;
- `src/services/library.rs` owns the source-neutral `DiscoveredGame` input and `LibraryRepository` boundary;
- `src/persistence/` is the only layer that knows SQLite or SQL;
- `src/platform/data_paths.rs` resolves the XDG application-data location, then passes the resulting path inward;
- the app initializes/migrates the database at startup but Phase 5 intentionally leaves the demo Home presentation in place.

The stable rediscovery key is `(SourceId, ExternalGameId)`. Equal game titles are not sufficient evidence for cross-source deduplication. Schema evolution begins at `0001_initial_library.sql` and all later changes must be new numbered migrations. See `DOMAIN.md`, `DATABASE.md`, and ADR 0037.


## Phase 6 source-framework boundary

Phase 6 adds the generic provider layer without adding a concrete launcher adapter:

- `src/sources/` owns `GameSource`, normalized source discovery types, descriptors, capabilities, and the source registry;
- `SourceRegistry` rejects duplicate stable `SourceId` values and exposes providers through one trait boundary;
- adapters return `SourceGame` values without a `SourceId`; `SourceImportService` attaches the registered descriptor identity, preventing provider output from claiming another source;
- expected provider absence is represented separately from discovery failure; one failing source does not block later sources;
- source snapshots require unique `ExternalGameId` values within a provider;
- `SourceImportService` converts successful snapshots into Phase 5 `DiscoveredGame` values and uses `LibraryService`;
- repository batch writes are atomic per successful source snapshot in the SQLite adapter.

No concrete provider parser, launch command, production artwork pipeline, or playtime implementation is part of Phase 6. See `SOURCES.md` and ADR 0038.

## Phase 7 Steam vertical-slice boundary

Phase 7 exercises the Phase 5/6 abstractions end-to-end without relaxing their dependency direction:

- `src/sources/steam.rs` alone knows Steam roots, VDF layout, Steam app IDs, and the `steam://rungameid` URI shape;
- `SourceImportService` remains provider-neutral and persists Steam discovery through the existing `LibraryService`/repository boundary;
- `SourceLaunchTarget` is a source-neutral value produced by launch-capable adapters;
- `GameLaunchService` selects a registered launch-capable source from a durable `LibraryGame` reference without matching provider names;
- `src/platform/launcher.rs` owns XDG OpenURI dispatch and does not know Steam;
- `HomeController` consumes `LibraryGame` values and publishes `GameCardData`; Slint never sees Steam IDs, VDF objects, database rows, or source adapters.

The application performs source discovery/import before constructing the Phase 7 Home presentation. Discovery failure is isolated per source and leaves the persisted library usable; persistence failure still aborts the import pass. The old hard-coded demo catalog is gone, and an empty durable library remains honestly empty.

The Flatpak grants narrow read-only access to Steam's own conventional metadata roots. It does not grant broad home/host/external-library access. Launching is delegated through the OpenURI portal instead of spawning a host Steam binary. See `STEAM.md` and ADR 0039.


## Phase 7.0.5 authoritative membership and launch-feedback boundary

The source framework now separates normalized discovered games from authoritative installed membership. Sources remain conservative by default; a complete source scan may publish an authoritative external-ID set. `SourceImportService` chooses additive import or source synchronization from that generic snapshot contract, and `SqliteLibraryRepository` performs reconciliation atomically. Provider-specific code never issues deletes or SQL. See ADR 0042.

Launch acknowledgement remains presentation/application state rather than source state. `HomeController` publishes only a pending game index and human-readable launch status around the existing source-neutral `GameLaunchService`. Slint renders the press/status feedback but does not decide launch policy. OS window deactivation clears the transient handoff state while ADR 0041 independently suspends controller UI input. See ADR 0043.


## Phase 8 activity boundary

Activity follows the existing one-way architecture. `src/services/activity.rs` coordinates launch handoff/session lifecycle through an `ActivityRepository` boundary. `src/persistence/sqlite.rs` owns the SQL, while `src/presentation/activity.rs` formats an `ActivityOverview` into Slint models. The Activity page never reads SQLite directly.

`HomeController` knows only the small source-neutral `LaunchActivitySink` hook. A successful launch dispatch arms the next foreground handoff with Horizon `GameId` + `SourceId`; Steam does not own session lifecycle. The root OS-window activation signal is shared conceptually with controller ownership, but input and activity remain separate services.

The runtime repository is shared behind `Rc<RefCell<_>>` only after startup discovery/import has finished. This allows one SQLite connection to back runtime activity writes without moving persistence into presentation. See `ACTIVITY.md` and ADR 0044.

## Phase 9 multi-source boundary

Phase 9 expanded the concrete adapter set without changing the inward dependency direction established by Phases 5–7. After the Phase 9.5.28 Lutris retirement, the current production adapters are:

- `src/sources/bottles.rs` owns Bottles `bottle.yml`/`External_Programs` details and the `bottles:` launch URI;
- `src/sources/heroic.rs` owns Heroic/Legendary local metadata and the `heroic:` launch URI;
- `src/sources/support.rs` contains only source-layer helpers shared by adapters, such as host XDG resolution and URI-component encoding;
- startup registers every provider through `SourceRegistry`; `SourceImportService`, `LibraryService`, SQLite, Home, Activity, and the platform launcher remain unchanged and provider-neutral.

Provider-specific installation scopes are encoded only when the upstream identifier is installation-local. Bottles program identities are namespaced by native/Flatpak scope. Heroic's store runner is part of its external identity so additional Heroic stores can be implemented later without breaking persisted Epic identities.

Phase 9 retains URI-based external handoff. It does not add host executable spawning, `flatpak run`, Gamescope, process ownership, or managed sessions. Those belong to the planned managed-session phase and should extend the generic launch capability rather than replace the source boundary.

See `SOURCES.md`, `BOTTLES.md`, `HEROIC.md`, ADR 0045, and ADR 0048.


## Phase 9.5 managed-session boundary

Managed sessions add one optional host boundary without reversing Horizon's
dependency direction:

```text
Slint / presentation
        |
GameLaunchService + ActivityService
        |
SourceRegistry / GameSource
        |
ManagedSessionExecutor (platform boundary)
        |
user D-Bus
        |
horizon-session-helper (host)
        |
Gamescope
```

The application-side service knows only `ManagedSessionExecutor`. The D-Bus
implementation is in `src/platform/session_helper.rs`. The host broker is
`src/session_helper.rs`.

The D-Bus request carries source/game identity, not a command. The host helper
uses the source registry again to resolve an adapter-owned
`SourceManagedLaunchTarget`. This intentionally prevents the broker from
becoming a generic `flatpak-spawn --host` equivalent.

Managed sessions are optional. The launch service falls back to the existing
`LaunchExecutor`/OpenURI route if the capability, helper, Gamescope, or managed
target is unavailable.

See `docs/MANAGED_SESSIONS.md` and ADR 0046.


## Phase 9.5.28 source-set cleanup

Lutris is retired as an active source. The production registry is constructed in
`src/sources/mod.rs` and now contains Steam, Bottles, and Heroic only. The host
helper uses that same registry, so removal applies consistently to both the
Flatpak and helper processes without source-name conditionals elsewhere.

Schema migration 0005 removes only active Lutris source membership. It preserves
logical game rows that still own Activity or lifetime-playtime history, matching
the existing separation between active library membership and historical identity.


## Phase 9.5.29 artwork boundary

Primary game artwork is source-neutral and always normalized to a 1:1 square.
Provider adapters expose `SourceArtworkCandidate` values through
`GameSource::artwork_candidates`; `ArtworkService` owns decoding, quality
selection, and square normalization. Presentation converts the resulting RGBA
buffer into a Slint `Image`. No provider filesystem path, AppID, or image-format
rule enters Slint. Steam is the first concrete `Artwork` provider. See ADR 0049.
