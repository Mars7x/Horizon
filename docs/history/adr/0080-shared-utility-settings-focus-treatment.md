# ADR 0080: Shared Utility and Settings Focus Treatment

## Status
Accepted

## Context
The top-utility selector already establishes Horizon's non-game focus language: a subtle accent-coloured ring, raised surface, soft shadow, and restrained 4-second rotating gradient. Settings rows, buttons, key entry, and the eye control instead used several unrelated outlines and a left-edge accent bar. Game tiles have a separate, intentionally distinctive four-corner focus design.

## Decision
Extract the utility focus styling into a shared `SelectionFocusSurface` Slint component. Use it without changing the utility ring's existing circle size or animation. Use the same ring, gradient, fade-in and shadow for the Settings controls, but with rounded-rectangle geometry that matches each target.

- Utility: 48x48 circular, with the existing -2px focused lift and icon movement.
- Settings rows: rounded rectangular full-row focus, surface fill and soft shadow; remove the redundant left-edge accent bar.
- Settings buttons: matching focus ring just outside the button, with a small gap so focus remains visible around filled primary buttons.
- API-key field: same focus outline without an extra fill, leaving text and clipping behavior intact.
- Eye control: small rounded-rectangle focus ring matching the input and buttons, without extra shadow.
- High Contrast or Reduced Motion: solid accent focus without ongoing gradient rotation; normal focus appearance animations respect `Motion.focus-duration`.

Keep the original mouse/pointer controls, accessibility roles, and Rust-owned navigation state. Do not modify `FocusFrame` or any game tile: their accent-bracket animation remains separate.

## Consequences
Non-game interactive controls now share a consistent focus appearance and animation, with shape determined by control geometry rather than the hard-coded utility circle. Controller selection remains recognizable without changing any route, game-title focus, input-event, or SteamGridDB logic.
