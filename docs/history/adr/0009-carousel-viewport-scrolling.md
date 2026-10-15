# ADR 0009: Carousel viewport scrolling

## Status

Superseded in part by ADR 0011.

## Context

The initial home carousel kept the selected game at a fixed horizontal anchor
and translated the entire row for every selection change. Once the shelf
background was introduced, this produced two visible problems: focus felt
stationary rather than moving through the library, and early games could render
outside the shelf when a later game was selected.

The desired console behavior is for focus to move across visible games and for
the row to scroll only when the selected game reaches a viewport edge.

## Decision

`GameCarousel` is the owner of visual scroll offset and shelf clipping.

- Rust remains the owner of `selected-index`.
- Slint keeps a local `scroll-offset` because it is transient presentation
  geometry, not application state.
- Selection changes call one `ensure-selection-visible()` function.
- The function moves the row only when the selected slot would leave the
  viewport.
- Existing scroll is preserved when the newly selected slot remains visible,
  creating a dead zone when navigating back in the opposite direction.
- The carousel root is the shelf and uses clipping to prevent child rendering
  outside the shelf.
- Home-page title/connector placement reads the selected tile's actual center
  from the carousel instead of using a fixed anchor.

## Consequences

The first game remains fully contained by the shelf, focus can visibly move
left/right across the row, and scrolling occurs only at the edges. Future home
layout work must not restore an index-derived fixed anchor or duplicate scroll
calculations outside `GameCarousel`.


> ADR 0011 retains the dead-zone/focus-movement goal but replaces the fixed, clipping shelf with a world-space scene camera.
