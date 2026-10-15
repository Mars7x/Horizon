# ADR 0142 — Subtle Library toolbars and shoulder mapping (Phase 10.2.4.2)

Status: Prepared; GNOME Builder compilation and visual review pending.

## Context

The 10.2.4 Library placed thin separator rules above and below the artwork
gallery. They looked like arbitrary grey lines rather than meaningful chrome.
The inline Source/Sort labels could touch, and the thin scroll rail was not
needed because the adjacent-row preview already exposed collection continuity.

## Decision

- Adopt a soft, theme-relative surface tint and low-contrast shadow for the
  pinned header and metadata toolbar, taking inspiration from libadwaita's
  elevated toolbar language. Remove both hard separator rules. Do not add a
  toolkit dependency or transplant GNOME controls; remain pure Slint.
- Use compact, independently clickable Source and Sort controls with vertically
  stacked labels and values, consistent left alignment, explicit widths and
  spacing, and hover-only neutral feedback. No selectable toolbar elements.
- Use LeftBumper to cycle the source and RightBumper to toggle title sorting.
- Remove the progress rail and thumb, retaining clean cropped prior/next row
  peeks and their existing non-clickable pointer rule.

## Invariants

No changes to Home, FocusFrame, source backends, playtime, launch semantics,
Rust library catalog ownership, vertical camera motion, source-neutral routing,
existing controller Back/history, or bounded virtualization.

## Validation

Review dark/light/high-contrast layout and verify the shoulder mappings and
click regions in GNOME Builder. No Rust/Slint toolchain is assumed in the
patch authoring environment.
