# ADR 0082: Predictable Settings Focus Reveal

## Status
Accepted

## Context
Phase 9.5.44.23 shared the utility ring with Settings. The rotating gradient was designed for the small circular utility indicator; across wide Settings rows and rectangular editor controls its bright section looks like a sudden fast flash rather than a clear controller focus state.

## Decision
Retain `SelectionFocusSurface` as the shared focus component. Introduce a `rotating-highlight` flag (default enabled for the utility) and configurable `reveal-duration`. All Settings call sites disable gradient rotation and use a 190 ms opacity ease-out when focused. The ring remains the same dynamic accent-coloured 2 px border, shadow, and optional subtle fill as the utility. Buttons keep their outer ring separated from their filled background. Reduced Motion continues to use 0 ms; High Contrast uses a solid accent-coloured ring.

Do not change the utility ring's circular geometry, existing gradient timing or lift, the game-cover `FocusFrame`, focus ownership or keyboard/controller navigation.

## Consequences
A Settings selection appears in one short, deliberate accent-coloured transition and then stays solid, regardless of the shape and width of its control. Moving focus fades between controls without the white-looking traveling streak; utilities retain their current subtle rotating highlight.
