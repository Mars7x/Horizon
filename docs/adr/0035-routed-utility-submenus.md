# ADR 0035: Routed full-shell utility submenus

## Status

Accepted — Phase 4.6.4. Supersedes the transient-overlay portion of ADR 0030.

## Context

Phase 4.3 originally treated Friends, Album, and Web as transient overlays while Activity and Settings were durable routes. Later UI direction established that all five top utility icons should open the same kind of full-shell submenu surface with the same Back/Home behavior.

Keeping two navigation mechanisms for visually equivalent destinations would create a permanent special case: some utility icons would use route history and focus restoration while others would use modal overlay state. That would complicate later production implementations and violate Horizon's no-monkey-patching rule.

## Decision

Introduce a first-class `UtilityPage` navigation concept:

- `Friends`
- `Album`
- `Activity`
- `Web`
- `Settings`

`AppRoute` is now structured as:

- `Home`
- `Library`
- `Utility(UtilityPage)`

Every top utility maps directly to one `AppRoute::Utility(...)` destination. There is no separate utility-overlay navigation state.

All utility submenu routes:

- participate in the normal `Navigator` back stack;
- fill the entire logical design surface;
- hide top navigation and footer chrome while active;
- restore as content focus because the persistent utility row is not visible;
- use the same retained `PageTransitionLayer` contract and Reduced Motion behavior;
- currently render through one reusable presentation-only `UtilitySubmenuPage` component.

The global shell Menu remains modal and separate from route history. Back closes that Menu first, then pops normal route history. Global Home closes the Menu, clears history, returns Home to content focus, and selects the first Home title.

## Consequences

- Friends, Album, Activity, Web, and Settings now have one consistent navigation model.
- Future production implementations can attach page controllers/services to a real route instead of replacing an overlay workaround.
- The obsolete `UtilityOverlay`, `UtilityDestination`, `UtilityOverlayView`, and `utility-overlay.slint` paths are removed.
- Activity and Settings keep the same user-visible full-shell behavior they already had.
- The route enum grows, but the hierarchy remains explicit because utility pages are grouped under `AppRoute::Utility` rather than flattened into unrelated root sections.
