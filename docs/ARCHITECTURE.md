# Architecture

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
- UI components use semantic theme colors; literal colors belong only in the central theme fallback definitions.
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
