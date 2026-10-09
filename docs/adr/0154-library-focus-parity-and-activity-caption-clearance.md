# ADR 0154 — Library focus parity and Activity caption clearance

Status: Proposed for Phase 10.3.2.4; awaiting GNOME Builder verification.

## Context

The Library still scaled selected cover shells to 1.045, leaving them smaller
than Home's selected 1.10 scale. The FocusFrame was drawn around a fixed grid
slot instead of the animated artwork shell, with an additional scale applied to
its vector stroke and corners. Consequently its contracted and expanded focus
poses differed from Home. In Activity, the text for a selected cover was
positioned based on the *unselected* square card, visually colliding with the
expanded focus brackets; the second caption line and lower panels also lacked
sufficient consistent vertical clearance.

## Decision

- Use `Metrics.game-selected-scale` and `Metrics.focus-offset` for both current
  and outgoing Library cover shells and attached focus frames. Preserve Home's
  FocusFrame component, path geometry, stroke width and pulse as authored.
- Place Activity captions below the maximum selected shell + focus frame,
  allowing 22px between frame bounds and title. Give the observed-time caption
  a dedicated 11px gap after the 25px title line.
- Move the `Most played` heading and its `Tracked by Horizon` legend above the
  highest possible focus bracket, maintaining a visible gap.
- Derive Activity carousel height and lower panels' Y position from the caption
  layout instead of hardcoded values that could clip or overlap content.
- Do not change Home, Activity navigation, source data, game launching, or any
  other UI animations.

## Validation

Inspect Activity cover/title/playtime spacing and the lower-panel gap on compact,
1080p and ultrawide windows; confirm Library focus shape matches Home through
both selection expansion and contraction, source/sort replacement and Reduced
Motion. Verify build with Slint 1.18.1 inside GNOME Builder.
