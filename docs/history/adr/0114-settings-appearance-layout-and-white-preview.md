# ADR 0114 — Settings layout, selection semantics and effective White accent preview

Status: Accepted (Phase 9.5.44.57)

## Context

The initial Appearance category in 9.5.44.55 allowed independently positioned
Settings home rows to collide, and the selected Theme choice used both an accent
stroke/checkmark and an independently animated focus ring. The large nine-option
accent grid repeated colour names, while the White swatch remained pure white
in Light mode despite Horizon automatically converting that accent to readable grey.

## Decision

- Place the two top-level settings categories in one `VerticalLayout` with
  explicit row heights and spacing. A single neutral base stroke becomes the
  accent-coloured stroke on actual controller focus. No oversized overlapping
  `SelectionFocusSurface` on these rows.
- Separate stored choice and input focus: Theme choices and System accent use
  the effective accent as text colour when selected; only the focused choice
  receives an accent stroke. Remove selection checks and duplicate rings.
- Keep colour option names for accessibility, not as nine visible labels. Show
  a compact 9-swatch palette ordered Red, Orange, Yellow, Green, Teal, Blue,
  Purple, Pink, White. Selected/focused swatches share one visible border.
- White's swatch must be the *effective* accent colour computed by the same Rust
  contrast-aware `Palette` resolver as application controls, in both themes and
  High Contrast. Expose one semantic theme property rather than hardcoding a
  second grey. Animate the swatch background as theme changes over 260 ms;
  Reduced Motion uses zero duration.
- Preserve existing Rust-owned indices, navigation, portal monitoring, and
  atomic preference storage. No new dependencies or Flatpak permissions.

## Limits

White's saved preset remains literal RGB white; only its displayed picker
preview reflects the effective UI colour. Other colour swatches remain their
namesake hues even though their focus strokes may be contrast-adjusted.
