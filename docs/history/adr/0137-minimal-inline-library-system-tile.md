# ADR 0137 — Minimal Home Library destination

## Status

Accepted for Phase 10.1.2, following explicit review of the Home tile.

## Context

Phase 10.1.0 added a Library tile with the correct Home carousel footprint
and focus frame, but its centered grid symbol sat above a repeated “Library”
label. Combined with the selected-title pill above the carousel, the result
resembled a game cover and duplicated the destination name.

## Decision

Keep the existing card size, neutral shell and art-frame backgrounds, selected
scale, camera placement, pointer target, and the original authored `FocusFrame`
unchanged. Center the existing 82×82 logical-pixel 2×2 grid (four 36×36 squares
with 10px spacing) on both axes within the art frame. Remove the inner “Library”
text; show the name only in `SelectedGameLabel` when the tile is selected.

Do not add a decorative background pattern, gradient, glint, utility styling,
extra accent fill, second focus outline, game count, or “All games” caption.
This explicitly chooses the simple system-tile option rather than a more
ornamental variation.

## Unchanged invariants

This is a Slint-only visual revision. The tile continues to activate Library
on Accept or pointer click. It remains the final carousel element after up to
15 played-then-alphabetical game cards, including when the game list is empty.
Game cards, game-cover focus geometry and motion, UI sounds, and navigation
wrapping stay unchanged. Phase 10.2.0 will redesign the actual Library page.

## Verification

Compile with Slint 1.18.1 and test the final tile selected and unselected in
Light/Dark and Reduced Motion at different window sizes. Ensure icon centering,
single external title pill, original Home focus, and correct activation.
