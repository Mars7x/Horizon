# ADR 0149 — Stable Activity strip camera without edge arrows

Status: Proposed in Phase 10.3.1.3 (requires GNOME Builder verification).

## Problem

Phase 10.3.1.1 used small edge chevrons to indicate offscreen games and
changed each cover's `visible` state whenever the selection moved beyond the
last fully visible slot. Under fast held controller navigation the chevrons
flickered and the cards popped between visible windows, interrupting the
expected continuous motion.

## Decision

- Keep the Activity carousel's existing 12-cover maximum and the original
  Home tile-local `FocusFrame` treatment.
- Remove the edge arrows and their fixed gutters; do not add dots, a progress
  slider, a cropped resting cover, or additional focusable controls.
- Mount every cover in one shared horizontal strip clipped by a fixed-size
  viewport. Animate the **strip's** camera X, not the individual cover X or
  per-card `visible` state.
- Show only a whole number of cover slots when the camera is at rest. Size
  and center the viewport to precisely contain those slots plus the focus
  bracket allowances. Pan before the selection reaches the edge when there
  is room; scrolling itself provides the navigation cue.
- Keep Left/Right selected-index clamping, pointer selection, Home artwork
  updates, and the existing Motion.carousel-duration setting (which already
  honors Reduced Motion). Do not modify the service or persistence model.

## Boundaries

This is a presentation-only change. It does not implement game drilldowns,
Milestones, new statistics, new source permissions, or changes to game sessions.
