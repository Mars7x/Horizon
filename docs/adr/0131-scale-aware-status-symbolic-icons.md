# ADR 0131 — Scale-aware symbolic SVG rendering for status icons

Status: Accepted for Phase 9.5.44.76 (runtime validation pending)

## Context

Horizon previously used Slint native Path transcriptions for the network and
controller symbols, while utility SVGs already used adaptive SVG rasterization
relative to `AppWindow.ui-scale`. At smaller display/UI scales the two icon
rendering paths looked inconsistent. The utility approach avoids unnecessary
large intermediate textures in normal/downscaled layouts and allocates more
resolution for enlarged fullscreen layouts.

## Decision

- Reuse the **same sample-resolution formula** as `utility-icons.slint`: 1×
  when UI scale ≤1, otherwise UI scale capped at 4×.
- Render the unchanged GNOME source assets directly with Slint `Image`,
  using `image-fit: contain`, the adaptive target size and the inverse
  `transform-scale` so status geometry is invariant.
- Apply `Theme.foreground` with Slint `colorize`; do not recolor SVG files.
- Use this shared symbolic renderer for all eight network variants and the
  existing GNOME controller icon. Route the UI scale from `AppWindow` to both
  the header and footer.
- Keep the icon layout cells, BatteryIndicator native primitives, all live
  status observation logic and Home game-focus SVG glow entirely unchanged.
- Retain source SVGs and CC0/CC-BY-SA attribution unmodified.

## Validation

Check the eight status mappings, the unchanged SVG assets, proper scale factor
propagation, and no changes to battery/network or focus state. Verify visuals
on small windows as well as 4K/ultrawide screens; compile with Slint 1.18.1
and run cargo tests in the user's build environment.
