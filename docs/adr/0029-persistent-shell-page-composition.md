# ADR 0029: Persistent shell and routed page composition

## Status

Accepted for Phase 4.2; full-shell utility-route composition later extended by ADR 0035.

## Context

Phase 4.1 introduced Rust-owned `AppRoute` and `Navigator` state, but the Slint tree still rendered Home as the complete application surface. Header and footer therefore belonged to Home even though they are application chrome that must remain present across Library, Activity, and Settings.

Duplicating that chrome inside every page would make controller focus, status updates, transitions, and future visual changes diverge between routes.

## Decision

`AppWindow` owns the persistent shell chrome and a single central routed page host.

The page host renders one component according to the presentation-only `AppRouteView` value published by Rust. Slint may compare that enum to choose a component, but it must not decide route history or mutate navigation state.

Home removes its local `TopNavigation` and `Footer`. Because the new page host begins below the top chrome, Home offsets its carousel by `Metrics.top-chrome-height` so the existing scene remains at the same screen-space coordinates.

Library, Activity, and Settings are added as intentionally minimal presentation placeholders. Their domain/data/controller behavior remains deferred.

## Consequences

- Header/footer status is defined once for the entire application.
- Route composition is visible and testable independently of future page content.
- Home keeps its established proportions after becoming a routed page.
- Later focus-region and transition work has one stable shell boundary.
- Utility activation and controller focus between shell/page regions remain Phase 4.3 work.
