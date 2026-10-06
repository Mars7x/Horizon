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

- pure Rust `AppRoute` values for Home, Library, Activity, and Settings;
- a pure Rust `Navigator` that owns current route and back history;
- global Home semantics that clear history and establish Home as the root;
- a presentation `NavigationController` between semantic input and page controllers;
- explicit Rust-to-Slint `AppRouteView` mapping;
- unit tests for route/history behavior without rendering or SDL hardware.

The active route is published to Slint, but Phase 4.1 still renders the existing Home page only. Actual page composition belongs to Phase 4.2. See `NAVIGATION.md` and ADR 0028.

## Phase 4.2 boundary

Phase 4.2 turns the published route into visible shell composition:

- `AppWindow` owns persistent top and bottom chrome;
- a central clipped page host renders exactly one route component;
- Home is now a real routed page rather than the entire application surface;
- Library, Activity, and Settings have presentation-only placeholder pages;
- Home's established scene geometry is preserved across the shell refactor;
- page switching is driven only by Rust-published `AppRouteView` state.

Phase 4.2 does not add utility focus navigation, page transitions, persistence, imports, launching, or production Activity/Settings behavior. See `NAVIGATION.md` and ADR 0029.

## Phase 4.3 shell focus boundary

The navigation layer also owns screen-level focus between page content and persistent header utilities. Pure Rust types in `src/navigation/` describe the focus region, selected utility, and whether a utility maps to a route or transient overlay. `presentation::NavigationController` publishes those decisions to Slint. Slint may emit a utility activation callback for pointer input, but it must not choose routes, mutate history, or decide overlay policy.

## Phase 4.4 route-focus boundary

Route history and shell focus remain separate concerns but are coordinated by the presentation navigation controller. `RouteFocusMemory` in `src/navigation/` stores one durable `FocusSnapshot` per `AppRoute`. A snapshot contains only the owning shell region and selected utility; temporary Home spatial-transfer anchors are deliberately excluded.

Before a top-level route change, `NavigationController` saves the source route's snapshot. After the route changes it restores the destination route's snapshot before further input is dispatched. Back therefore restores both route and route-local shell focus. Global Home resets Home to content focus and the first Home game while clearing route history. Slint continues to render only the published state and does not own restoration policy.

See ADR 0032.

## Phase 4.5 global-action boundary

`UiAction::Back`, `UiAction::Menu`, and `UiAction::Home` are classified as global shell actions in the input model, but their behavior remains in `NavigationController`. Keyboard and SDL adapters only normalize physical input.

The global shell menu is modal presentation state (`ShellMenuState`), not an `AppRoute`. Opening it therefore does not push navigation history or alter route-local focus memory. `NavigationController` publishes only a boolean to Slint, which renders `ShellMenuOverlay`. Back/Menu/Home determine modal lifetime in Rust; global Home additionally resets the Home selection to the first game through `HomeController`.

See ADR 0033.

