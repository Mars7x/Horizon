# Phase 4 navigation shell

Phase 4 turns Horizon's single-screen prototype into a controller-first application shell. Navigation policy stays in Rust; Slint renders the published route/focus state and animations.

## Ownership

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
                   retained route layers
```

`src/navigation/` is UI-framework agnostic. It owns route identity, history, shell focus, and the five utility submenu identities. `src/presentation/navigation.rs` receives semantic actions, applies global Back/Home/Menu behavior, activates routes, restores route-local focus, and maps `AppRoute` to Slint's presentation-only `AppRouteView`.

Slint never pushes/pops history or decides whether a header utility is a route.

## Route model

`AppRoute` deliberately separates the persistent shell from the utility submenu family:

- `Home`
- `Library`
- `Utility(UtilityPage)`

`UtilityPage` contains:

- `Friends`
- `Album`
- `Activity`
- `Web`
- `Settings`

This is a real navigation model, not a presentation workaround. Every top utility opens `AppRoute::Utility(...)`, participates in normal Back history, uses the same full-shell page treatment, and can later receive its own page controller without changing input adapters or inventing a second overlay path.

## History semantics

`Navigator::navigate_to(route)` pushes the previous route before activating a new one. Navigating to the already-active route is a no-op.

`Navigator::go_back()` restores the previous route. Back at the root is a no-op.

`Navigator::go_home()` is stronger than normal navigation: it makes Home the root and clears all prior history. Global Home also restores Home content focus and selects the first game.

## Shell composition

`AppWindow` keeps every route layer instantiated so crossfades can overlap and presentation-only state can survive navigation.

- Home and Library render inside the central bounds between the top navigation and footer.
- Friends, Album, Activity, Web, and Settings render across the full logical surface.
- Any utility submenu route hides both top navigation and footer immediately.
- All five utility routes use the same reusable `UtilitySubmenuPage` presentation component during Phase 4.
- The global shell Menu remains a separate modal surface above routes.

Header status, clock, controller state, and footer controls remain shell-owned; utility pages do not duplicate them.

### Home geometry preservation

Home still subtracts `Metrics.top-chrome-height` from its internal scene origin because its routed host begins below the persistent header. This preserves the established title/carousel/focus geometry from the single-screen prototype.

## Input invariants

- SDL and keyboard adapters know only semantic `UiActionEvent` values.
- Back, Home, and Menu are global actions handled before page-local dispatch.
- Home carousel behavior remains owned by `HomeController`.
- Utility submenu placeholders intentionally have no page-local actions yet.
- Repeated global actions are ignored defensively even if an adapter misbehaves.

## Utility-row focus

The persistent shell has two Rust-owned focus regions: page content and the top utility row. This region switch is available only on routes that display shell chrome: Home and Library.

On Home/Library:

- Up enters the utility row.
- Down returns to content.
- Left/Right move across Friends, Album, Activity, Web, and Settings and clamp at the ends.
- Accept opens the selected utility submenu route.

On Home, vertical transfer uses rendered center geometry and reciprocal anchors. Moving from a game to a utility and immediately reversing direction returns to the exact originating game; horizontal movement invalidates that temporary pair.

Only one focus region renders selection chrome at a time. While the utility row owns focus, Home keeps its selected game logically but hides game scale, focus brackets, title/connector, z-raise, and selected shadow.

## Route-local focus restoration

Each route has a durable `FocusSnapshot`. Home/Library may remember content or top-utility focus. Every `Utility(...)` route is full-shell and therefore always saves/restores as content focus while preserving the corresponding utility identity.

Before navigation, `NavigationController` saves the source route snapshot. After navigation or Back, it restores the destination snapshot. Temporary Home game↔utility transfer anchors never cross a route boundary.

Example: opening Web from Home saves Home's utility-row focus, opens `Utility(Web)` with content focus, and Back restores the previous Home focus state.

## Global Back / Home / Menu

Back priority is now simple:

1. close the global shell Menu if open;
2. otherwise pop route history;
3. otherwise no-op at the root.

There is no separate utility-overlay modal path. Friends, Album, Activity, Web, and Settings all use the same routed submenu semantics.

Menu toggles the global shell Menu without changing route history or route-local focus. Home closes the Menu, clears route history, activates Home, forces Home content focus, and selects the first Home game.

## Reduced-motion-aware page transitions

Each route is hosted by a reusable `PageTransitionLayer`. Route changes use the centralized 220 ms crossfade plus `Metrics.page-transition-offset` vertical settle. Home/Library use central bounds; all five utility submenu routes use the full logical surface.

Top/footer chrome does not animate with routes. It disappears immediately when a utility route becomes active and returns immediately when Home/Library becomes active. The global shell Menu remains above route layers.

Retained inactive pages cannot receive pointer input. Route-specific selection visuals must also be gated by route activity, so an outgoing Home page cannot flash focus chrome while fading.

Reduced Motion uses the same navigation path: `Motion.page-duration` becomes `0ms`, making the route presentation update immediately.

## Next pass

Phase 4.7 hardens controller disconnect/reconnect, resize/fullscreen and ultrawide behavior, focus restoration under stress, rapid navigation, and route/back-stack tests.

The current utility-route decision is documented by ADR 0035, which supersedes the transient-overlay portion of ADR 0030.
