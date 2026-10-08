# ADR 0084 — Smooth native Settings focus pulse

## Context

In Phase 9.5.44.26, Settings focus continuously modulated both the 2px accent border and its soft shadow using `animation-tick()` and a cosine expression. On the large Third-Party row, the animation appeared uneven. This is especially plausible when a large 10px blurred shadow has to be recomposited continuously.

## Decision

Retain the focus geometry, dynamic accent colours, continuous Switch-inspired pulse, and 190ms reveal from Phase 9.5.44.26. For Settings only:

- Use a native Slint `animate pulse-progress` loop, alternating `0 → 1 → 0` with 1800ms ease-in-out-sine legs (3.6s for a full cycle).
- Keep the focus border on screen throughout the cycle and reduce peak interpolation toward the accent-derived focus highlight from 52% to 42%.
- Keep the 10px soft shadow at a constant 15% opacity to avoid continuously rebuilding a large blurred surface.
- Disable the loop when unfocused, under Reduced Motion, or under High Contrast, reverting to the standard solid accent outline.
- Keep the API-key input background neutral by disabling the shared focus shadow at that call site; retain its accent-coloured outline, but never add focused surface fill.

The utility's circular rotating focus and the game tile's separate four-corner 3DS-inspired focus are deliberately unchanged. Settings rows and buttons retain their existing selection fills; only the API-key field disables the tinted focus shadow.

## Verification

Static source checks only in the packaging environment; build with Slint 1.18.1 and verify frame pacing in GNOME Builder. Check several row widths and whether rapid controller focus changes cleanly stop the old loop.
