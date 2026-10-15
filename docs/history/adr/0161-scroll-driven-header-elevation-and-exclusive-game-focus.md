# ADR 0161 — Scroll-driven top elevation and exclusive achievement game focus

Status: prepared in Phase 10.4.3.2, pending GNOME Builder compilation and visual verification.

> **Partially superseded by [ADR 0162](0162-achievement-input-and-lazy-badge-parity.md):** the immediate, non-animated game-row outline was replaced by the shared Settings focus reveal (fade in/out). The scroll-driven elevation decision still stands.

## Problem

Phase 10.4.3.1 always displayed the top-edge header penumbra in the Achievements games, Achievements details and Library galleries, including at the first row. The individually animated game-row focus borders could briefly leave teal outlines across several rows when focus moved quickly, giving the appearance of multiple selections.

## Decision

- Retain each page's existing full-shell-width, 11px gradient and common theme colors. Multiply its opacity by the fraction of the **current animated scroll offset** through the first row, clamped to `[0, 1]`. It is invisible at zero, fades in as content actually moves below the stationary header, and fades out when moving back to the beginning. This also respects Reduced Motion because the camera already does.
- In Achievements games, bind the focus outline's color and width directly to `selected-index == i` without the per-row focus-out animation. That makes selection exclusive under rapid keyboard/controller or mouse navigation. Retain hover surface animation and scroll camera easing; keep achievement detail rows unfocusable.
- Library's gallery is already animated using its `gallery.y` position. Derive the shadow opacity from the difference between that position and its unscrolled `peek-limit`, not from the target `scroll-row` alone.
- Do not change the Library footer shadow, clipping bounds, Home focus behavior, Rust navigation, cached Steam data or any source integration.

## Validation

Test first/open frame (no top shadow), one-row movement down/up (smooth reversible fade), deep scrolling (persistent shadow), fast held navigation (only one selected row), return from details, resize, light/dark and Reduced Motion. Full Rust/Slint compilation is required in GNOME Builder; static inspection alone does not establish build compatibility.
