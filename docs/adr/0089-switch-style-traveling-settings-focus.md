# ADR 0089: Switch-Style Traveling Settings Focus Outline

## Status
Accepted

## Context
Phase 9.5.44.30 fixed the Settings focus animation so it actually advanced continuously, and Phase 9.5.44.31 increased its contrast. However, the animation still changed the entire outline color uniformly at the same time. The user clarified that the intended behavior is closer to the Nintendo Switch Settings focus treatment: a crisp outline with a brighter segment traveling around the perimeter, while the row interior remains neutral and the indicator does not glow.

## Decision
Retain the existing outline-only Settings focus geometry and the utility icon focus treatment. Replace the Settings whole-border pulse with a Switch-like traveling accent highlight.

Implementation details:
- continue using `SelectionFocusSurface` as the shared focus component;
- keep the utility path (`rotating-highlight: true`) unchanged;
- for Settings (`idle-pulse: true`, `rotating-highlight: false`), drive the border with a continuously advancing 3.6-second angle clock;
- render the border as a mostly-base-accent outline with a narrow, soft-edged lighter accent band traveling around the perimeter;
- keep background fill, glow, and shadow disabled for the Settings rows and input controls.

## Consequences
The Settings focus should read more like the Nintendo Switch: a clean outline with motion concentrated in a moving bright segment rather than a full synchronous color change.
