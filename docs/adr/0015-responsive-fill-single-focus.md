# ADR 0015: Responsive filled viewport and single-layer focus

## Status

Accepted. Supersedes the letterboxed 16:9 viewport portion of ADR 0013 and the
focus halo portion of ADR 0014.

## Context

A uniformly scaled fixed 1280×720 surface preserved proportions, but on 21:9
windows it left large unused side bands. The home focus frame also had a second
translucent under-stroke that read as a halo rather than the single blue bracket
layer in the reference. Footer content had also been optically lifted too far.

## Decision

Horizon still derives one uniform UI scale from the 1280×720 baseline, but the
logical surface dimensions are `window_size / ui_scale`. The logical viewport
therefore expands in the dimension with spare space and always fills the native
window. Responsive components use that expanded parent width/height; individual
icons, game tiles, text, and radii remain uniformly scaled rather than stretched.

The home focus frame uses exactly one four-corner stroke layer. It hugs the game
shell with a small centralized offset and scales together with the selected
card. No halo/duplicate stroke is permitted. Footer content uses a small optical
lift rather than the previous large offset.

## Consequences

- 21:9 and other wide windows use their full horizontal area.
- 16:9 retains the authored baseline composition.
- Square/rounded UI geometry keeps its aspect ratio.
- Carousel camera boundaries naturally become wider on ultrawide displays.
- Focus visuals are cleaner and closer to the supplied mockup.
