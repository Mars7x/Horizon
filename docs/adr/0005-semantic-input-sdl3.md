# ADR 0005: SDL3 behind a semantic input layer

## Status

Accepted.

## Context

Horizon is controller-first but must also support keyboard navigation. Allowing Slint pages to interpret device-specific buttons or key values would duplicate mappings and couple application navigation to hardware details.

SDL's event pump is main-thread-bound. Horizon already has a main-thread Slint event loop.

## Decision

- Define device-independent `UiAction` values in Rust.
- Convert keyboard and SDL3 gamepad input into `UiAction` under `src/input/`.
- Route semantic actions to Rust-owned presentation/application controllers.
- Poll SDL3 with a Slint main-thread timer rather than moving SDL to a worker thread.
- Keep the SDL adapter responsible for gamepad enumeration and hotplug state.
- Grant Flatpak `--device=input`, not `--device=all`, for controller access.

## Consequences

- Slint components do not know controller brands or SDL button constants.
- Keyboard and controller behavior share one navigation path.
- New input devices can be added by writing adapters without changing home-screen state logic.
- SDL polling remains compliant with SDL's threading contract.
- The Flatpak gains one narrowly scoped device permission.
