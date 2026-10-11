# ADR 0173: Wheel and touchpad scroll by pixels

**Status:** Implemented. The build and gate pass. Offscreen, simulated touchpad and wheel input leave Library's grid between rows with the selection unchanged, and a focus move settles it back onto whole rows. Home's carousel pans by exact pixels and clamps at both ends. Not yet felt on real hardware.

## Context

Wheel and touchpad scrolling stepped the camera a whole row (or game) at a time and glided between rows. An earlier fix accumulated touchpad travel into those steps. The speed was then right, but the user felt it moving "from focusable item to focusable item", including with a mouse, and asked that it never work that way anywhere. Scrolling must also never change focus.

## Decision

- One `ScrollCamera` (`components/scroll.slint`) serves every vertical list and grid: Library, Album, Achievements' games and entries, and Activity history.
  - At rest it follows the owner's row, as before.
  - `scroll(delta)` switches it to a free pixel camera, clamped to the content.
  - A touchpad follows directly (2.5× travel, no glide). A wheel notch moves 180 px with the standard glide.
- The camera reports each crossing into another top row through `scrolled(rows)`. Owners keep their existing "move by N rows" handlers, which now only keep `rest-row` and the mounted window in step. No Rust changes were needed.
- It returns to rows when focus moves (`focus-key`), on a fresh visit, or when the owner changes the row to one it didn't report (Up/Down in a read-only list).
- Home's carousel pans by the same pixel distances, without a glide for a touchpad.
- `Wheel` now only classifies events (notch or touchpad) and converts them to distances. The row accumulator from earlier the same day is gone.

## Consequences

- Scrolling looks continuous everywhere, and focus never moves.
- A focus move after free scrolling snaps the camera up to the top row it stopped in, before the usual focus scroll.
- There is no momentum: a touchpad flick stops when the fingers lift, because Slint/winit report no scroll phases.
