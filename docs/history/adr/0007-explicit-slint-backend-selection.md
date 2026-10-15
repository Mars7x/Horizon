# ADR 0007: Explicit Slint backend selection

## Status

Accepted.

## Context

Horizon disables Slint's default Cargo features and enables only the Winit
Wayland/X11 backend features plus the FemtoVG renderer. This keeps the UI stack
small and prevents an accidental Qt dependency.

Compiling a backend is not the same as selecting Slint's default platform. With
`backend-default` disabled, a call such as `slint::set_xdg_app_id()` can fail
with `PlatformError::NoPlatform` unless an enabled backend is selected first.

## Decision

Horizon explicitly selects `winit` + `femtovg` at startup through
`slint::BackendSelector` in `src/platform/slint_backend.rs`.

Backend initialization occurs before `set_xdg_app_id()`, component creation, or
any other Slint platform-dependent call.

## Consequences

- Horizon remains independent of Qt.
- Native and Flatpak launches use the same deterministic backend policy.
- Wayland and X11 availability remain compile-time Winit features.
- Renderer/backend policy is centralized instead of hidden in environment
  variables or Flatpak-only configuration.
