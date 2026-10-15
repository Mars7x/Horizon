# ADR 0152: Activity focus geometry and activation parity

**Status:** Prepared for Phase 10.3.2.2; pending GNOME Builder verification.

## Context

Activity 10.3.2.1 reused Home's FocusFrame, but the Activity cover shell stayed
at its unselected size. The frame was therefore visibly detached from the
artwork when compared with Library; Activity also lacked the Home-style
press-in feedback when opening an individual game's session history.

## Decision

- Reuse `Metrics.game-selected-scale` (1.10 in the current Home theme) for
  selected Activity game shells, without modifying Home or Library.
- Position the original FocusFrame relative to the shell's animated geometry
  with 6px nominal clearance, rather than to the nominal tile box. Maintain
  its original accent, stroke, focus animation and per-tile activation.
- Reserve enough clipping room for the enlarged shell and frame while keeping
  the artwork row at the same visual position.
- Give controller Accept and pointer clicks the same 125ms press-in before
  opening game details. Temporarily contract to scale 1.025 and smoothly
  rebound during the normal detail-page transition.
- Cancel pending activation on selection or route change and avoid duplicates.
  Do not delay opening when Reduced Motion is enabled.

## Non-goals

Do not alter Home's GameTile, Library cards, the FocusFrame component, any
source adapter, the Activity session query, playtime semantics, or database
schema. Pressing Accept in Activity opens history; it does not launch games.

## Verification

Verify at 800x450, 1280x720, 1920x1080 and ultrawide sizes; inspect shell
scale against Home, bracket clearance against Library, clipping on the top
focus arms, fast repeated Accept/Back/Left/Right, pointer activation, cancelled
opens, and Reduced Motion. Build using Slint 1.18.1 in GNOME Builder.
