# ADR 0067 — Satin Drift for the game focus frame

Status: Accepted for Phase 9.5.44.10 trial.

## Context

The previous 4.2-second whole-frame breathing opacity animation looked too repetitive. The requested alternative is a restrained, brief satin-like reflection on Horizon's existing accent-coloured four-corner focus indicator.

## Decision

- Preserve each of the four SVG path commands, its position, scale, stroke width, caps, joins, shell gap, and visibility rules.
- Keep the `Theme.focus` accent colour everywhere: a single broad gradient brightens only the middle of each *existing* bracket by up to 13% (HSV value), in a staggered diagonal progression.
- Use a 7.8-second cycle with approximately 2.7 seconds of gentle reflection and the remainder stationary. Animation occurs only while the focus frame is active.
- Reduced Motion and High Contrast receive an entirely static `Theme.focus` stroke.
- Keep fixed gradient stop positions: no runtime percent-conversion workaround or extra geometry. No bundled third-party asset, library, or Flatpak permission is needed.

## Scope

This changes only the `FocusFrame` stroke brushes. It does not affect Home layout, selected game scales, navigation, the API-key editor, clipboard handling, or source artwork.

## Verification

Compare the four original Path blocks with the replacement; only their `stroke:` expressions may change. Test the UI in GNOME Builder, including accent customization, inactive focus frames, High Contrast and Reduced Motion. The patch's static tests do not substitute for a full Slint compilation.
