# ADR 0013: Responsive design surface, Inter typography, and portal clock format

## Status

Accepted.

## Context

The Phase 3 shell was authored at 1280×720 by setting `Window.width` and
`Window.height`. In Slint, explicit window width/height become fixed layout
constraints, preventing normal resize/maximize behavior. The home shell also
needed one project-wide typeface and a clock that follows the desktop's 12/24
hour preference rather than a hard-coded mock value.

The reference composition is strongly 16:9. Allowing individual components to
stretch independently at arbitrary aspect ratios would distort the intended
console layout.

## Decision

- The `Window` uses `preferred-width`/`preferred-height` plus minimum
  constraints instead of fixed `width`/`height`. F11 toggles Slint
  `Window.full-screen` before application-navigation key handling.
- Horizon keeps a 1280×720 logical design surface and uniformly scales it with
  `transform-scale` to the largest size that fits the current window.
- The design surface is centered when the window aspect ratio differs from
  16:9; the window background fills any remaining area.
- `Typography.family` is `Inter`, inherited through
  `AppWindow.default-font-family` by all Slint text. The Flatpak packages a
  pinned Inter 4.1 variable font from the upstream repository.
- The live clock is owned by `presentation::clock::ClockController`.
- The system 12/24-hour choice is read through `org.freedesktop.portal.Settings`
  using the GNOME `org.gnome.desktop.interface/clock-format` setting exposed by
  the portal backend. Horizon listens for live changes.
- Portal/clock DBus logic remains in `src/platform/clock.rs`; Slint receives
  only formatted strings.
- The profile and system-status groups use symmetric `Metrics.top-edge-inset`
  positioning against the full design surface.

## Consequences

- The window can be resized and maximized; fullscreen-sized windows scale the
  complete shell rather than leaving a 1280×720 UI in one corner.
- 16:9 visual proportions stay stable across 1080p, 1440p, 4K, and other sizes.
- Non-16:9 windows may have centered unused background space rather than a
  distorted layout.
- Flatpak builds have a deterministic Inter installation. Native development
  builds require Inter to be available through the host fontconfig setup or
  Slint will fall back according to system font configuration.
- The clock updates live when GNOME's clock format changes and omits AM/PM in
  24-hour mode.
