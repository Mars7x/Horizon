# ADR 0140 — Library flow polish (Phase 10.2.3)

Status: Implemented; awaits GNOME Builder validation.

## Context

Phase 10.2.2 produced the approved independent full-shell Library. Its header
included a redundant Back affordance and disconnected filter/sort pills. Its
artwork viewport masked all rows after the last wholly visible row, making
large collections appear to contain only two rows at the reference size. The
metadata changed abruptly on focus steps; the game count font was vertically
cropped; a per-selection ordinal added unnecessary noise.

## Decision

1. Keep Library route history and source-neutral Rust selection/launch logic;
   remove only the visible Back pill, not keyboard/controller Back.
2. Consolidate source/sort controls in one two-segment surface. Preserve separate
   hit areas, focus targets, sort and source cycles, and shoulder input shortcuts.
3. Remove the position field end-to-end and expand the count text's line box.
4. Signal game identity changes via a revision owned by LibraryController and
   sent after updating both game title and source. Cache successive text pairs
   in Slint for a Home-length (190ms) fade/settle; respect Reduced Motion.
5. Keep the same clipped, overscanned gallery and complete-row selection math,
   but expose a small non-interactive partial next row when possible. Fill the
   clipping edge with a short background fade. Use the Home camera duration and
   easing for vertical movement; retain the existing shared Home FocusFrame.

## Invariants and limits

No new dependencies, source adapters, assets, storage migrations, input actions,
permissions, external services, or route changes. The hover/selection visuals
remain restrained; an incomplete row is never clickable. If the window is too
short to accommodate a preview, it is omitted rather than covering metadata.

## Validation

Inspect keyboard/controller focus, pointer interactions, scroll boundaries,
resizing, empty and filtered libraries, 16:9/16:10/21:9, Reduced Motion and
existing GUI animations in GNOME Builder. Build is not available in the patch
preparation environment; the user must validate against the complete local tree.
