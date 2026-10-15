# ADR 0022: Native fallback rendering and fresh-only carousel wrapping

## Status
Accepted

## Decision
Horizon renders procedural fallback game covers at native selected geometry rather than transform-scaling a rendered subtree. Semantic input events also retain whether they are fresh or repeated. Carousel boundary wrapping is allowed only for fresh Left/Right events; repeated events clamp at the boundary.

## Rationale
Native geometry keeps fallback typography and vector shapes sharp at fullscreen/HiDPI scales. Preserving repeat state lets Horizon offer deliberate one-press wraparound without causing an unintended wrap when a user holds a keyboard key, D-pad, or analog stick from a nearby game.
