# ADR 0017: Outer focus separation and no Home applet

## Status

Accepted.

## Context

The Phase 3.13 focus geometry matched the shell radius family, but the frame
still occupied too much of the shell footprint and visually read as embedded in
the card. The top header also still contained a dedicated Home applet, which no
longer matches Horizon's intended navigation direction.

## Decision

Keep the focus as a single rounded stroked bracket layer, but increase its
centralized offset so the frame sits visibly outside the selected game shell
with a small air gap. Preserve the existing selected-card scale-up, focus
stroke, shell thickness, and radius language.

Remove the dedicated Home icon from the centered header utility row. Recompute
the row width from the six remaining 48 px icons and five 24 px gaps (408 px)
so the utility group remains centered on the window.

## Consequences

- Focus reads as an outer selection frame rather than part of the card shell.
- Focus spacing remains tunable from one theme metric.
- The top header no longer implies a Home-tab navigation model.
- Future header additions must preserve whole-row centering instead of relying
  on the removed Home slot.
