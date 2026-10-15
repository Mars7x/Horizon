# ADR 0146 — Controller-first Activity playtime overview (Phase 10.3.0)

Status: prepared for build and user visual verification.

## Context

The old Activity summary used four large statistical tiles, a session list and
multiple ranked panels. In design review the user rejected busy dashboard and
sidebar layouts. The approved direction is a simplified, game-focused Option C:
most-played game covers, one weekly playtime chart and a reserved Milestones
placeholder. Milestones must NOT be implemented yet.

## Decision

- Use exactly one directional focus target group: horizontally scrolling
  most-played square covers, each using the original Home `FocusFrame` rather
  than a sliding focus frame. Clamp at the first/last game. Leave the chart and
  Milestones placeholder noninteractive; pointer selection is allowed.
- Share `GameCardData` from the existing Home artwork cache and publish a small
  `ActivityCoverData` Slint model, instead of creating a provider-specific media
  or activity-artwork service.
- Most-played ordering and cover captions are based on committed Horizon-observed
  sessions. Steam/Heroic lifetime totals remain separate; never sum them.
- Show a compact last-seven-local-days total and calendar-month total. Query
  local-calendar intervals from the generic Activity repository and clip
  recorded sessions at their actual interval boundaries. An open session must
  not falsely contribute finalized day-level data.
- Use Theme.background and the same cover surface/radius/focus accent as Library;
  chart and reserved placeholder are quiet informational panels. Reuse existing
  page transitions and Reduced Motion policy; no additional UI theme or icons.
- Keep milestone placeholder purely textual ('Milestones' / 'Coming later').
  No trophy system, achievements, stored milestone state, focus action or
  staged fake progress.

## Follow-ups

An individual-game details page, session-history drilldown, and interactive
chart period selection would each need separate design approval. They are not
part of this initial revamp. The initial phase must be verified in GNOME Builder
at controller/keyboard, 1280x720, 1080p, and ultrawide layouts.
