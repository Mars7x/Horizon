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

**Overscroll (follow-up).** At the user's request, a touchpad pulling past either end now stretches the content (rubber band, approaching 80 px) and springs back 90 ms after the last touchpad movement (340 ms, eased out), in every `ScrollCamera` and Home's carousel. Without scroll phases from winit, "fingers lifted" is inferred from that pause. Wheel notches don't overscroll, and Reduced Motion disables it. Touchscreen drag scrolling doesn't exist yet, so it isn't covered.

**Focus after scrolling (follow-up).** Settling back onto whole rows when focus moved felt like the camera snapping back after a touchpad scroll. `ScrollCamera` now never snaps. It keeps its pixel position, and a focus move (`focus-key`, with the item's `focus-row` and `focus-span`) moves it only when that item isn't fully in view, by the least distance that shows it. The camera then reports its top row to the owner, so Rust's row-based scroll for the same move is overridden rather than obeyed. Only an owner-initiated row change (read-only lists' Up/Down, a fresh visit) rests it on a row. Achievements' games list no longer computes its own row on focus, and its camera no longer resets when a game's achievements open. On Home, after a swipe, selection changes use a fully-visible check instead of the camera zone until the games are republished. In the same change, a click on an Album tile opens it straight away instead of selecting it first.
