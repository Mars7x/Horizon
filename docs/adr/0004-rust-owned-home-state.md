# ADR 0004: Rust owns home-screen selection state

## Status

Accepted.

## Context

The Phase 2 home screen needs a dynamic game model and selected-game state. Slint can mutate array properties and local state directly, but allowing the UI to become the source of truth would make it harder to connect the same state to controller input, persistence, importers, activity tracking, and future navigation services.

## Decision

The Phase 2 game model and selected-game state are owned by `presentation::home::HomeController`.

Slint:

- renders `GameCardData`
- emits `select-game(index)` intent
- animates visual changes derived from the selected index

Rust:

- validates selection indices
- updates selected index/title
- supplies the model

## Consequences

- Phase 3 can route SDL3 `UiAction` events into the same selection state without rewriting the carousel.
- Future real library models can replace the demo model without changing the component boundary.
- UI code stays declarative and does not become a second application-state store.
