# ADR 0122 — Correct Settings focus colour mixing and activation boundaries

## Context

Phase 9.5.44.63 made all Settings controls appear focused by using
`divider.mix(focus, focus_reveal)`. Slint `color.mix(other, factor)` weights
**the left-hand colour** by `factor`: at reveal=0 this drew the focus
colour, and at reveal=1 it drew the divider. Consequently focus feedback
was visually reversed, obscuring the active root category.

Settings subpages also remain temporarily visible during their exit opacity
transition. Callback dispatch must not assume fading pages are still active.

## Decision

- Use `divider.mix(focus, 1.0 - focus_reveal)` for Settings rows,
  Appearance theme/system choices, and accent swatches. Animate the scalar
  reveal only, leaving the 3.2-second shared focus brush free to breathe.
- Gate each Settings page's pointer activation callback against the
  Rust-published `view` and modal state. Preserve crossfade visuals.
- Define and test the Settings category destination mapping explicitly:
  0=Appearance, 1=Third-Party. Controller Accept still uses the selected
  Rust index; pointer click uses the clicked row's index.
- Do not change Home game-card focus brackets, utility focus lift, colours,
  pulse clock, navigation repeat, audio, or saved preferences.

## Verification

Run `cargo check`, `cargo test`, and test on keyboard/gamepad/mouse:
Only one row has an accent outline; moving from Appearance to Third-Party
changes that outline; Accept and clicking Third-Party open Third-Party;
switching pages during the 220ms crossfade never activates an outgoing page.
Check Reduced Motion and High Contrast as well.
