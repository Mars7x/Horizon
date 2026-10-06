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

The shell now has two Rust-owned focus regions: page content and the top utility row. Up enters the utility row, Down returns to page content, and Left/Right move across Friends, Album, Activity, Web, and Settings. The strip clamps at its ends.

Home uses live rendered geometry rather than proportional index pairing. For a fresh vertical transfer, Rust compares element centers and chooses the shortest center-to-center path: game center → utility center on Up, or utility center → game center on Down. Vertical transfer is also reciprocally sticky. If game A transfers to utility B and the user immediately reverses direction, focus returns to game A. Likewise, if utility B transfers to game A and the user immediately presses Up without changing games, focus returns to utility B. Horizontal movement in the destination region invalidates that exact pair, after which the next vertical move is resolved from the current rendered centers.

Only the region that owns focus may render selection chrome. While the utility row owns focus, the Home game brackets, selected-game title pill, connector line, and connector dot are hidden. The game remains selected internally so focus can restore cleanly when returning to content.

Activity and Settings activate normal top-level routes. Friends, Album, and Web open transient presentation-only overlays so auxiliary tools do not pollute route history. Back closes an overlay before popping route history; global Home closes overlays, restores content focus, and resets the route stack to Home. Pointer clicks on the header enter the same Rust-owned activation path.

## Next pass

Phase 4.4 should expand focus-region behavior where pages gain additional regions and make focus restoration explicit per routed page. It should preserve the input and shell boundaries established here.


Phase 4.3.3's geometry/anchor behavior is recorded in ADR 0031.

While the utility row owns focus, Home retains the selected game index for navigation state, but renders no game-selection treatment: no scale-up, focus brackets, title/connector, raised z-order, or selected shadow. Those visuals return only when focus comes back to content.

Utility focus uses a compact translucent accent orb, thin animated accent ring, subtle shadow, and restrained 2px icon lift. The icon and orb do not scale; the fixed-size orb fades in/out instead. No utility title is shown. The underlying 40x40 utility cells never resize, so the center coordinates used by spatial navigation remain stable.
