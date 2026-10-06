# ADR 0025: Main-view reference density

## Status
Accepted

## Context
The home carousel was structurally correct but occupied too little of the central screen compared with the reference mockup. A large empty band remained between the carousel shelf and footer, and the game cards appeared smaller than the reference composition. The footer separator also visually split the shell even though the reference footer reads as a continuation of the same surface.

## Decision
- Keep the top chrome height unchanged.
- Keep the current footer height unchanged.
- Remove the horizontal divider at the top of the footer.
- Move the selected-title/carousel scene upward.
- Increase the shelf height and game-card dimensions so the main view occupies more of the space between top chrome and footer.
- Preserve the existing responsive camera, selection, input, and shell/focus architecture.

## Consequences
The home view should feel less vertically sparse and closer to the proportions of the reference while still scaling to ultrawide/fullscreen layouts.
