# Phase 4 navigation shell

Phase 4 turns Horizon's single-screen prototype into an application shell. Phase 4.1 established the Rust-owned route model and action-routing boundary. Phase 4.2 made that route state control real page composition while keeping shell chrome persistent. Phase 4.3 adds Rust-owned focus movement into the persistent utility row and activates its five items.

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

Friends, Album, and Web are intentionally not top-level routes. Phase 4.3 presents them as transient shell overlays, while Activity and Settings activate their existing routes.

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

## Phase 4.3 utility focus

The shell now has two Rust-owned focus regions: page content and the top utility row. Up enters the utility row, Down returns to page content, and Left/Right move across Friends, Album, Activity, Web, and Settings. The strip clamps at its ends. On Home, vertical focus transfer uses spatial-order pairing: the selected game maps to the nearest proportional utility position on Up, and the focused utility maps back to the corresponding game on Down. With eight demo games, Friends/Album/Activity/Web/Settings pair with games 0/2/4/5/7. Other routed pages retain the last selected utility.

Only the region that owns focus may render its focus indicator. The Home game brackets disappear while the utility row owns focus and return on the spatially paired game when focus moves back down. The selected game itself remains selected; only its focus affordance is suppressed.

Activity and Settings activate normal top-level routes. Friends, Album, and Web open transient presentation-only overlays so auxiliary tools do not pollute route history. Back closes an overlay before popping route history; global Home closes overlays, restores content focus, and resets the route stack to Home. Pointer clicks on the header enter the same Rust-owned activation path.

## Next pass

Phase 4.4 should expand focus-region behavior where pages gain additional regions and make focus restoration explicit per routed page. It should preserve the input and shell boundaries established here.
