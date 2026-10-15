# ADR 0119 — Uniform focus pulse and coupled utility lift

Status: proposed for Phase 9.5.44.62, pending Slint build and visual confirmation.

## Context

Phase 9.5.44.61 mistakenly restored a rotating focus highlight and removed the
utility icon's subtle 2px focus lift. The intended reference is the quiet,
stationary, **uniform accent-brightness pulse**, not a sweep around the ring.
The icon and its outline must remain concentric while rising together.

## Decision

- Keep the shared `SelectionFocusSurface` for Settings rows, Appearance,
  Third-Party controls, dialogs, and top utilities. All use one full-perimeter
  accent stroke with a smooth, uniform idle brightness pulse.
- Keep the separate authored Home game-bracket geometry, but use the same pulse
  curve and timing (`Motion.focus-idle-cycle`, 3200ms).
- Vary only the existing stroke's opacity from 86% to 100%, so the accent hue
  never changes or moves around the outline. The pulse is a single cosine
  cycle, not a rotating gradient, moving glint, or second overlaid ring.
- Fade in/out on focus using the existing 140ms focus transition. Reduced
  Motion and High Contrast use a static full-opacity outline.
- Apply the same focused -2px offset to **both** utility SVG icon and 48px
  ring, using identical `Motion.focus-duration` and ease-in-out motion. The
  fixed 40px hit area, navigation coordinates and original SVG colors remain
  unchanged. No utility background tint.
- Preserve Phase 9.5.44.60 fresh-press-only edge wrapping, and all Phase
  9.5.44.61 navigation/OK/Back sounds, Appearance preferences, and playtime.

## Verification

Validate the shared pulse expressions and no-gradient invariant across both
focus components. Confirm that six utility icons and rings use identical
focus-lift values, that pulse is suppressed in Reduced Motion/High Contrast,
and that focus navigation and audio are unchanged. Run `cargo check`,
`cargo test`, and verify visually in Horizon before marking complete.
