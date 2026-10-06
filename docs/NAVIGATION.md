# Phase 4 navigation architecture

Phase 4 turns Horizon's single-screen prototype into an application shell. Phase 4.1 establishes the route model and action-routing boundary without replacing the current Home view yet.

## Ownership

Top-level navigation is Rust-owned.

```text
keyboard / SDL3
      │
      ▼
 UiActionEvent
      │
      ▼
NavigationController ──────────────► active page controller
      │                                  │
      ▼                                  ▼
  Navigator                          HomeController
      │                                  │
      └────────────► Slint presentation state
```

`src/navigation/` is pure Rust and has no Slint, SDL, Flatpak, portal, or persistence dependency. It owns the active route and back stack.

`src/presentation/navigation.rs` is the boundary that:

- receives semantic `UiActionEvent` values;
- applies global Back/Home behavior;
- forwards page-local actions to the active page controller;
- maps the Rust `AppRoute` to the presentation-only Slint `AppRouteView` enum.

Slint never pushes or pops navigation history.

## Phase 4.1 routes

The top-level route type contains:

- `Home`
- `Library`
- `Activity`
- `Settings`

Friends, Album, and Web are not added as top-level routes by Phase 4.1. Their final page/overlay behavior belongs to the later utility-navigation pass.

## History semantics

`Navigator::navigate_to(route)` pushes the previous route before activating a new route. Navigating to the already-active route is a no-op and does not grow history.

`Navigator::go_back()` restores the previous route when one exists. Back at the root is a no-op.

`Navigator::go_home()` is intentionally different from normal navigation: it makes Home the root and clears the back stack. Pressing Back after the global Home action must not return to the page that Home just left.

## Phase 4.1 UI behavior

The visual shell remains the existing Home screen. The active route is already published to `AppWindow.current-route`, but `ui/app.slint` does not switch page components until Phase 4.2.

This is deliberate. Phase 4.1 proves the navigation policy and input boundary independently before page composition, focus regions, or transition animation are added.

## Input invariants

- SDL and keyboard adapters remain unaware of routes/screens.
- `UiAction::Back` and `UiAction::Home` are interpreted by the navigation controller.
- Other actions are forwarded to the active page controller.
- Repeated activation/global actions remain filtered by the input adapters; the navigation controller also refuses repeated Back/Home defensively.
- The existing Home carousel selection and fresh-edge wrapping behavior remain owned by `HomeController`.

## Next pass

Phase 4.2 will turn the published route into a real page shell while keeping the header/footer and existing Home composition intact. It should not introduce SQLite, source imports, or launching.
