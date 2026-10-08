# ADR 0133 — Pixel-aligned Ethernet and verified charging semantics

Status: accepted for implementation; pending build and device verification.

## Context

The GNOME 16×16 network-wired icon is enlarged to 22px in the header and
then may be scaled again by the application UI. At small scales its straight
bars and rounded nodes look soft after repeated raster sampling. In the same
review we audited the charging bolt logic through SDL, UPower, Rust
presentation, and Slint. UPower numeric State 5 is PendingCharge, not Charging.

## Decision

- Keep the 22px centred wired slot and original GNOME SVG. Render only the
  ordinary Ethernet silhouette with scene-native pixel-aligned Slint shapes
  derived from the GNOME symbol, credited under CC BY-SA 3.0 US. Preserve the
  detailed wired no-route SVG and all Wi-Fi icons/scale behaviour.
- Treat UPower State 1 as active charging; State 5 is connected but charge
  pending/paused, so show no bolt. SDL Charging remains authoritative; do not
  infer charging from BlueZ battery percentages or unsupported 2.4GHz modes.
- Retain the 20%-and-under red indicator, charge symbol styling, animations,
  host-over-controller priority, and existing Flatpak permissions.

## Verification

Static source checks verify full-slot centring, whole-pixel geometry, preserved
network state mappings, correct UPower state distinction and regression tests.
Rust/Slint compilation, small/fullscreen visual sampling, and live charging
device tests remain necessary.
