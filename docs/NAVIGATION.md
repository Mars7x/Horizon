# Phase 4 navigation shell

Phase 4 turns Horizon's single-screen prototype into an application shell. Phase 4.1 established the Rust-owned route model and action-routing boundary. Phase 4.2 makes that route state control real page composition while keeping shell chrome persistent.

## Ownership

Top-level navigation remains Rust-owned.

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
                         │
                         ▼
                   routed page shell
```

`src/navigation/` is pure Rust and has no Slint, SDL, Flatpak, portal, or persistence dependency. It owns the active route and back stack.

`src/presentation/navigation.rs` is the boundary that:

- receives semantic `UiActionEvent` values;
- applies global Back/Home behavior;
- forwards page-local actions to the active page controller;
- maps the Rust `AppRoute` to the presentation-only Slint `AppRouteView` enum.

Slint never pushes or pops navigation history.

## Routes

The top-level route type contains:

- `Home`
- `Library`
- `Activity`
- `Settings`

Friends, Album, and Web are not top-level routes yet. Their final page/overlay behavior belongs to the utility-navigation pass.

## History semantics

`Navigator::navigate_to(route)` pushes the previous route before activating a new route. Navigating to the already-active route is a no-op and does not grow history.

`Navigator::go_back()` restores the previous route when one exists. Back at the root is a no-op.

`Navigator::go_home()` is intentionally different from normal navigation: it makes Home the root and clears the back stack. Pressing Back after the global Home action must not return to the page that Home just left.

## Phase 4.2 shell composition

`AppWindow` now owns three persistent layers:

1. top chrome (`TopNavigation` and its surface);
2. one routed central page host;
3. the footer (`Footer`).

Only the central page host changes with `AppWindow.current-route`. Header status, clock, controller state, and footer controls are no longer duplicated by individual pages.

The central host instantiates exactly one of:

- `HomePage`
- `LibraryPage`
- `ActivityPage`
- `SettingsPage`

The non-Home pages are intentionally presentation-only placeholders. Phase 4.2 does not add persistence, source imports, activity data, or settings behavior.

### Home geometry preservation

Before Phase 4.2, `HomePage` occupied the entire design surface and also contained the header/footer. The routed content host now begins below `Metrics.top-chrome-height`, so `HomePage` subtracts that inset from the carousel's established Y coordinate. This keeps the title, connector, shelf, cards, and focus indicator in the same screen-space positions as Phase 4.1.

Do not move the Home scene merely because it is now a child of the central host.

## Input invariants

- SDL and keyboard adapters remain unaware of routes/screens.
- `UiAction::Back` and `UiAction::Home` are interpreted by the navigation controller.
- Other actions are forwarded to the active page controller.
- Repeated activation/global actions remain filtered by the input adapters; the navigation controller also refuses repeated Back/Home defensively.
- The existing Home carousel selection and fresh-edge wrapping behavior remain owned by `HomeController`.
- Placeholder Library/Activity/Settings pages intentionally have no page-local input behavior yet.

## Next pass

Phase 4.3 should make the utility/header navigation actionable and introduce deterministic focus movement into and out of the top utility region. It should use the existing Rust route API rather than moving route policy into Slint.
