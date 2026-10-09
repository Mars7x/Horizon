# ADR 0151: Activity history layout and edge-preview carousel

**Status:** Proposed for Phase 10.3.2.1; pending GNOME Builder build/visual verification.

## Context

Phase 10.3.2 introduced full session history, but its detail screen had an
oversized horizontal header, overlapping summary labels, full-width session
rows and an unwanted game launch button. The main Activity carousel also
started moving too early and gave no visual indication that additional games
were offscreen. Long cover titles stayed truncated.

## Decision

- Reuse the Library/Home solid background, cover shell, focus and typography.
  A wide screen uses a compact game summary column beside a scrolling session
  history; a smaller window stacks a reduced playtime summary above history.
- Remove the Play Game action at *every* boundary (Slint callback, navigation
  handler, activity controller and its unused launch dependency). Merely
  reviewing history must never trigger a launch.
- Keep the full session query, stable `GameId`, bounded row window, real
  timestamps, and separate Horizon observed vs provider lifetime totals.
  No provider lifetime time is manufactured into historical sessions.
- Give the nested detail view the same crossfade/12px settle language as
  Settings. Animate row selection with the existing focus tokens; Reduced
  Motion collapses animations to zero duration.
- Render the Activity cover strip across the full window, with a natural
  partial next cover at the right edge and no arrow/scrollbar. Do not begin
  following the selection until it leaves the initially complete-cover area.
- Use Home's marquee *timing and behavioral rules* for selected long cover
  labels (2s hold, length-sensitive linear travel, 2.4s end hold, invisible
  reset) and non-animated elision for unselected cards. Keep the masking
  anchored at the cover label's clip viewport; avoid the segmented alpha
  mask that previously caused moving glyph seams.
- Preserve re-entry reset to the first Activity game, and preserve the selected
  cover only while returning from details in the same Activity visit.

## Non-goals

No new game source, data migration, Rust dependency, provider history fetch,
achievement/milestone feature, per-game chart, or Flatpak permission change.

## Verification

Build with the project's pinned Slint version in GNOME Builder. Inspect title
marquee at rest/travel/reset, rapid controller repeats at the visible edge,
selection after entering and backing out of details, sessions list scrolling,
reduced-motion behavior, 800x450 compact mode and fullscreen/ultrawide widths.
