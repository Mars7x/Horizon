# ADR 0022: Use Skia for scaled text and placeholder artwork

## Status
Accepted

## Context
Horizon intentionally scales its logical design surface for fullscreen and HiDPI output. With Slint 1.18's FemtoVG renderer, glyphs are cached/rasterized and then scaled, which makes the procedural placeholder monograms and captions look soft at larger window scales. A previous attempt to compensate by creating an internally enlarged placeholder scene introduced visible text breathing during selection and did not address the renderer-level cause.

## Decision
Use Slint's Winit + Skia renderer and remove the placeholder-specific internal density workaround. Placeholder artwork returns to stable logical typography and geometry. Selection continues to animate the card's real dimensions.

## Consequences
- Inter text and placeholder typography are rasterized by Skia at the effective output scale instead of relying on FemtoVG's scaled glyph atlas.
- The whole application retains the existing responsive design-surface architecture.
- Development Flatpak builds may need to fetch/build additional Skia dependencies.
- Do not reintroduce render-scale/density hacks inside DemoCover.
