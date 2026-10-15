# ADR 0153 — Activity title display and read-only session scrolling

Status: Accepted for Phase 10.3.2.3 (pending build validation).

## Context

Activity used an ellipsis for unfocused long cover titles, even though the
selected tile already displayed a clipped-and-faded title before its Home-like
marquee began. The per-game history also highlighted individual rows with a
focus border despite those rows having no action. Finally, the wide-screen
Session history heading and first row were vertically offset from Playtime.

## Decision

- Use the selected marquee's resting clipped appearance for every long cover
  title; never elide Activity cover text. Only the selected title may animate.
- Keep session history read-only and fully available through a Rust-owned
  bounded viewport; Up/Down moves the viewport, not focus. Eliminate session
  selection state, row activation, and session-focused visual accents.
- Align the wide layout's section headings and first data surfaces at identical
  y coordinates; retain the compact stacked layout.
- Do not change Home, Library, source-reported totals, persistence, navigation
  out of the detail page, or Reduced Motion semantics.

## Validation

Verify title clipping and marquee restart for selected/unselected covers,
Up/Down clamps on short and long histories, viewport resize, and small-screen
layout in GNOME Builder with Slint 1.18.1.
