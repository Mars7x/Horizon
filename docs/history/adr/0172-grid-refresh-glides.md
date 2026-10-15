# ADR 0172: The grid refresh glides covers to their new places

**Status:** Implemented. The build and gate pass. A Library reorder was rendered frame by frame offscreen. The Album uses the same rule but has not yet been watched in the running app.

## Context

The user didn't like the shared grid refresh from ADR 0167 for Library's Source and Sort and Album's Game and Type. In it, the old grid faded out, then every cover rose in column by column. That hid what had changed, because a game that stayed looked exactly like one that arrived. The user picked, from four options, covers gliding to their new places.

## Decision

- **Covers in both grids glide.** Rust gives each newly mounted card its place in the old order (`from-index`), but only if it was mounted before (on screen or in the overscan rows); otherwise -1. Slint draws the card at its old viewport position, which accounts for the old camera and gallery offset, and moves it to the new one over 260 ms, eased out. Moving cards draw above the rest.
- **Leavers fade where they stood.** Each frozen old card carries `stays`. Staying cards are hidden in the outgoing layer, so only real leavers fade (120 ms).
- **Newcomers fade in at their place** (170 ms after a 90 ms delay, eased in and out), with no rise, scale or column stagger.
- Library matches games by id; Album matches captures by path. Album remounts even an unchanged window after a filter change, so every tile gets its origin. A thumbnail arriving mid-glide keeps its tile's origin.
- Reduced Motion still makes it instant. Focus lift is animated as usual again, since covers no longer scale on arrival.

## Consequences

- Sort reads as a visible reorder, and a Source change barely moves games that stay put.
- Covers that come from far off-screen fade in instead of flying across the grid.
- **Replaces** ADR 0167's column-by-column reveal (`GridRefresh.length` no longer takes a column count).
