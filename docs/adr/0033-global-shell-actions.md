# ADR 0033: Global shell actions and modal Menu

## Status

Accepted — Phase 4.5.

## Context

Horizon already normalized keyboard and controller input into `UiAction`, but
Back and Home behavior had grown incrementally and Menu remained intentionally
unused. Phase 4 needs one deterministic shell policy before page-specific
controllers and transitions expand.

The policy must not leak route knowledge into SDL/keyboard adapters, and a Menu
surface must not become a fake route merely to reuse navigation history.

## Decision

`Back`, `Menu`, and `Home` are the global shell-action set.

Canonical input mappings remain:

- keyboard: Escape/Back → Back, Menu → Menu, Home → Home;
- controller: East → Back, Start → Menu, Guide → Home;
- Return/Space and South remain contextual Accept.

Global actions are fresh-press only. Adapters reject repeat events and
`NavigationController` also ignores repeated global events defensively.

A new pure-Rust `ShellMenuState` owns whether the global Menu surface is open.
It is modal presentation state, not `AppRoute`, and therefore never changes
route history or route-local focus memory.

Back priority is:

1. close shell Menu;
2. pop route history;
3. no-op at the root.

All five top utilities are normal routed submenu destinations, so they do not
introduce a second modal priority path. Menu toggles the shell Menu from any
route.

Home closes the shell Menu, clears navigation history, activates Home,
returns Home to content focus, and resets the Home carousel to its first game.
The previous Home game selection is not restored by the global Home action.

Slint receives only `shell-menu-open` and renders a presentation-only placeholder
surface. Real page/context menu actions are intentionally deferred.

## Consequences

- Keyboard and gamepad global controls have one documented/tested semantic contract.
- Page controllers never need to special-case Back/Menu/Home.
- Menu behavior is visible and testable without inventing a new route.
- Route/focus restoration remains independent of modal shell state.
- Global Home has a deterministic content destination: the first Home game.
- Later page-specific menu contents can replace the placeholder without changing
  input adapters or top-level navigation policy.
