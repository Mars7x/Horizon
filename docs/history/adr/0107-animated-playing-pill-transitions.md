# ADR 0107: Animate Playing badge entry and exit without removing its element

Status: Accepted — Phase 9.5.44.50

## Context

The Playing pill was created with `if root.game.is-playing` in `GameTile`.
This destroyed the visual element as soon as playback ended, so a normal Slint
property animation could animate arrival but not departure. The pill must stay
attached to its game card and must never infer game lifecycle itself.

## Decision

Keep a single, non-conditional Playing pill inside each game card's `shell`.
Bind its `opacity` to the existing `GameCardData.is-playing` value and its
`y` position to a small presentation-only offset. Animate opacity and y in
both directions with a 210 ms enter and 155 ms exit, drifting 7 px. Centralize
timing/travel in `Motion` and make Reduced Motion use zero duration and offset.

Use a Switch-inspired dark neutral capsule with white lettering and no border,
including no accent-coloured outline. The small status dot continues to use the
application accent; neither card selection nor the accent changes the capsule's
perimeter. Centralize pill colours in `Theme` for predictable light/dark styling.

The element continues to occupy its usual card-relative location when Playing
is true. When false it is completely transparent. The component has no
interactive surface, lifecycle timers, or source-specific logic.

## Consequences

- Exiting playback now fades/slides away instead of disappearing on the same frame.
- Quick consecutive runtime state changes smoothly retarget the same animations.
- The dark capsule contrasts against light and dark cover art, including in
  High Contrast; the accent is limited to the small status dot.
- Card selection, artwork refresh, Steam/Heroic runtime observations, and title
  pills remain unchanged.
- This is UI-only; Heroic's best-effort observation limitations remain.
