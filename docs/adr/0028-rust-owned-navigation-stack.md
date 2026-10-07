# ADR 0028: Rust-owned top-level navigation stack

## Status

Accepted for Phase 4.1; route shape later generalized by ADR 0035. Rust-owned history and controller boundaries remain active.

## Context

Horizon entered Phase 4 with one production-shaped Home screen. Keyboard and SDL3 input were already normalized into semantic `UiActionEvent` values, but those events were delivered directly to `HomeController`. Adding Library, Activity, Settings, global Back/Home behavior, focus regions, and transitions on top of that direct connection would make either the input adapters or Slint components responsible for screen-level policy.

That would violate the existing architecture: raw input adapters must stay screen-agnostic, and Slint should render state and emit intent rather than own business/navigation rules.

## Decision

Introduce a pure Rust `Navigator` under `src/navigation/` with four top-level routes: Home, Library, Activity, and Settings.

`Navigator` owns:

- the current route;
- a back stack;
- push-style `navigate_to` semantics;
- Back behavior;
- global Home behavior that clears history and returns to the Home root.

Introduce `presentation::NavigationController` between `InputManager` and page controllers. It interprets global navigation actions, forwards page-local actions to the active page controller, and publishes the active route to Slint as a presentation-only `AppRouteView` value.

Phase 4.1 does not yet switch visible Slint page components. The current Home page remains the rendered shell until Phase 4.2.

## Consequences

- SDL and keyboard adapters remain unchanged and screen-agnostic.
- Navigation transitions can be tested without Slint rendering or controller hardware.
- Back/Home semantics have one authoritative implementation.
- Future page components cannot silently invent independent route history.
- Phase 4.2 can focus on shell composition because the route model already exists.
- Focus-region state is intentionally not part of this ADR; that is added after the page shell exists.
