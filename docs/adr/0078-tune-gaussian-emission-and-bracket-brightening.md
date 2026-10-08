# ADR 0078: Tune Gaussian Bracket Emission and Accent Brightening

## Status
Accepted

## Context
Phase 9.5.44.20 moved the focus glow to a Gaussian emission derived from the original bracket paths, which fixed the visible duplicate-outline problem. The result was close to the intended 3DS-inspired behavior, but at maximum expansion the glow still felt slightly too strong. The user also wanted the bracket color itself to shift a bit more noticeably while the glow changes, similar to the 3DS cursor.

## Decision
Keep the Gaussian emission approach from Phase 9.5.44.20, but reduce the maximum glow opacity and slightly increase the synchronized brightening of the crisp bracket strokes.

Specifically:
- lower glow opacity from `0.23 + 0.16 * cycle` to `0.22 + 0.11 * cycle`;
- increase bracket brightening from `0.08 * cycle` to `0.12 * cycle`.

## Consequences
The glow remains continuously present and synchronized with the 3.8-second expansion cycle, but appears more restrained at peak expansion. The brackets themselves now participate more clearly in the animation, giving a result closer to the 3DS-style cursor behavior without introducing duplicate outlines or shell movement.
