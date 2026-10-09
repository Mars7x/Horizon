# ADR 0150: Game Activity history and full-width cover row

**Status:** Accepted for Phase 10.3.2 (pending GNOME Builder build verification).

## Problem

The minimal Activity overview has a cover row without a deeper action. Users
need to inspect every Horizon-observed session for a selected game, while the
overview must remain controller-first and uncluttered. The previous carousel
also used a narrowly centered, clipped viewport that left arbitrary dead space
at the shell sides.

## Decision

- Accept on an Activity cover opens a nested game-details view. Back returns to
  the exact cover selection; global Home stays governed by shell navigation.
- Each new visit to Activity from another shell route starts with the first
  game selected. Leaving and re-entering does not restore an old carousel
  position; returning from nested details within the same visit does.
- Reuse the selected game artwork and the existing Home-owned Play Game
  launch boundary. Never launch on a simple overview selection.
- Query all recorded `play_sessions` by `GameId` newest-first, and the separate
  provider lifetime snapshot(s) from `source_lifetime_playtime`. No provider
  playtime is transformed into fabricated session rows.
- Rust retains complete history while Slint renders a moving page-sized slice
  for efficient controller Up/Down navigation; summary and header remain fixed.
- Keep the per-game details screen graph-free. Milestones remains an inert
  placeholder on the overview; do not add achievement logic.
- Replace the old inset Activity gallery viewport with a full-shell-width
  camera viewport. Covers are aligned to the same page margins as the header,
  with adaptive spacing so complete cards extend across the available row.
  Only the actual window boundary clips moving/offscreen elements.
- Respect the existing focus and reduced-motion conventions. No new source,
  migration, dependency, database write path or broader Flatpak permission.

## Verification

Check GNOME Builder compilation, OK/Back and pointer activation, long-history
scrolling, source lifetimes never summed with observed time, fast repeated
navigation, narrow and ultrawide gallery sizing, and screen-edge clipping.
