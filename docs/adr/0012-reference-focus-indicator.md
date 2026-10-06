# ADR 0012: Match the reference focus indicator with filled corner geometry

## Status
Accepted

## Context

The first focus implementation used four thin stroked paths outside a game card
and enlarged the complete selected card. Rendering the implementation beside the
original visual reference exposed two consistent differences:

1. The reference focus marks are broad, filled, capsule-ended corner brackets,
   not thin line strokes.
2. The reference game artwork remains approximately the same size as neighboring
   artwork. Selection is communicated primarily by the larger bracket footprint,
   rather than by scaling the whole tile.

Measured from the supplied reference crop, each bracket is approximately 6.5%
of the full focus-frame width in thickness and roughly 25% of the frame width in
arm length. The old implementation was approximately half as thick and visibly
shorter.

## Decision

- Render each corner as a filled Slint `Path` with explicit outer elbow, inner
  elbow, and rounded terminal geometry.
- Use a 54px corner arm and a 20px focus-frame offset at the current 164px tile
  scale. The filled path geometry yields about 13.5px visual thickness.
- Keep the selected game card at the same scale as unselected cards.
- Continue using `Theme.focus` so the indicator follows the desktop accent
  preference.
- Keep the focus frame as presentation state attached to the selected game; no
  input or domain behavior changes are introduced.

## Consequences

The focus treatment is visually much closer to the supplied mockup and no longer
resembles a thin rectangular selection outline. Future visual tuning should
adjust the centralized metrics/path geometry rather than replacing the indicator
with borders or scaling the entire card.
