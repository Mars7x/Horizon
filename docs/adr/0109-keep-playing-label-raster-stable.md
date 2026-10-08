# ADR 0109: Keep Playing-label glyph baselines stationary

Status: Accepted — Phase 9.5.44.52

## Context

Phase 9.5.44.50 animated the entire Playing pill 7 px vertically. Phase
9.5.44.51 reduced its additional focus-related motion to ~2.2 px, but the
`Playing` text still shifted through fractional pixel coordinates. Rendering
the glyphs while that movement settles can make the final frame look like a
brief text pop even though the capsule motion itself is smooth.

## Decision

Keep the same persistent, bottom-centred pill anchor. Compensate for exactly
half the animated artwork-height growth, cancelling the card's centre-based
expansion at the pill's screen position.

Split the pill's presentation into two permanent sibling layers:

- An animated neutral capsule background retains the 210 ms enter/155 ms exit,
  7 px rise/sink, and opacity transition.
- A stationary label-and-dot layer fades independently over 160 ms on entry and
  120 ms on exit. Its `y` never changes, so text glyph baselines stay stable.

Use the existing `GameCardData.is-playing` as the sole trigger. Both layers
respect Reduced Motion; neither introduces a timer or changes session state.
The neutral borderless capsule, white label, and accent dot remain unchanged.

## Consequences

- No subpixel translation of the `Playing` text in the final animation frames.
- No focus-related vertical drift in the label while the cover scales.
- Entrance and exit remain soft, short, and reversible, without removing the
  component when a game stops.
- Steam/Heroic tracking, artwork, title navigation, permissions, and
  dependencies are unchanged.

This refines the presentation details of ADRs 0107 and 0108; their historical
decisions and intent remain documented.