# ADR 0108: Stabilize the Playing badge during card focus changes

Status: Accepted — Phase 9.5.44.51

## Context

The GameTile cover animates its real width and height between 220 px and
242 px (1.10×), expanding around the centre of its layout slot. In Phase
9.5.44.50 the Playing badge was pinned near the shell's bottom edge, so it
shifted 11 px vertically when the card became selected. The badge also
animated its own `y` property for Playing-state entry/exit, making the badge's
position reactive to unrelated selection scaling.

## Decision

Keep the capsule within the shell, but nest it inside a neutral, unanimated
layout anchor. The anchor stays centered and offsets 40% of the difference
between the animated shell height and `Metrics.game-tile-size`. Because the
shell itself expands from the centre, its lower edge moves by half that
difference; the badge therefore follows only the remaining 10% (2.2 px at
selected scale), avoiding a noticeable jump while retaining a slight visual
connection to the expanded artwork. Centralize the factor as
`Metrics.playing-pill-growth-compensation`. The anchor's position inherits
the shell's existing width/height animation and has **no separate `animate y`**.

The child `playing-pill` continues to animate *only* its local `y` and
`opacity` when `GameCardData.is-playing` changes. Preserve the Phase 9.5.44.50
210 ms entry, 155 ms exit, 7 px travel, Reduced Motion policy, neutral
borderless styling, white text and accent status dot. No lifecycle or source
changes.

## Consequences

- Playing remains centered inside every game cover with only a subtle
  ~2.2 px focus-related shift rather than the previous ~11 px.
- Focus scaling cannot restart the source-driven pill entry/exit animation.
- Card press/release spring, Steam/Heroic observation, focus brackets, title
  labels, artwork, accessibility behaviour and packaging are unchanged.
