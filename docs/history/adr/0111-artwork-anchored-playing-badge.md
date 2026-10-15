# ADR 0111: Anchor Playing to the actual artwork frame

Status: Accepted — Phase 9.5.44.54

## Context

The visual game shell scales from 220 px to 242 px when selected and is
centred within the carousel slot. Phase 9.5.44.51 partially compensated
for the badge movement relative to the screen. Phase 9.5.44.52 increased
compensation to 50%, cancelling the badge's screen-space vertical travel
while the artwork continued to move. This left a visibly changing gap
between the badge and the lower edge of the art.

The intended invariant is instead an **artwork-relative position**: the
Playing status sits bottom-centred at the same inset throughout cover
selection, deselection, launch press, and release.

## Decision

Place the persistent `playing-pill-anchor` inside the rounded, clipped
`artwork-frame`, after the image/fallback cover, so the artwork viewport
owns the badge's position and clipping.

- `x = (artwork width - badge width) / 2`.
- `y = artwork height - badge height - 12 px`.
- Preserve the badge's 90×30 px fixed dimensions, distinct from the
  scalable artwork dimensions.
- Remove `Metrics.playing-pill-growth-compensation`; the artwork-local
  bottom inset is a named `Metrics` design token.
- Retain independent capsule enter/exit slide-and-fade versus label/dot
  opacity transitions. No change to the session lifecycle model.

## Consequences

The badge moves with the cover while the card changes size and maintains
an invariant 12 px gap from the actual artwork bottom edge. The focus
transition introduces no separate badge animation or counter-motion.

The label is still stationary *relative to its badge* during Playing
entry/exit, avoiding the earlier end-of-transition pop from locally sliding
glyph baselines. It necessarily follows the cover's position during focus
scaling, because artwork-relative placement is now the design priority.

This supersedes the focus-growth compensation decision in ADRs 0108 and
0109 without undoing their separate Playing-state animation architecture.
No new dependencies, source adapters, permissions, or artwork assets.
