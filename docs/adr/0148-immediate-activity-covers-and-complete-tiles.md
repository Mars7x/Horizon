# ADR 0148 — Immediate Activity cover data and complete carousel tiles

Status: Accepted in Phase 10.3.1.1 (pending GNOME Builder verification).

## Context

Phase 10.3.0 populated its Activity showcase at startup but subsequently
remounted the entire Slint cover model every three seconds when Activity was
visible. Home artwork updates were not propagated to that model until the
periodic refresh. This created apparent delayed covers and unnecessary row
resets. The Activity strip also clipped a partial next card to suggest
horizontal scrolling, inconsistent with Horizon's complete-cover framing.

## Decision

- Maintain one long-lived Activity `VecModel` with stable game identity.
- Refresh Activity data synchronously before a navigation route switch exposes
  the page, rather than waiting for the periodic recents timer.
- On periodic updates, mutate only playtime rows whose text has changed;
  replace the vector only if its ordered game identities actually change.
- Forward Home artwork card notifications to the matching Activity row, while
  preserving its playtime text and focus selection.
- Preserve selected `GameId` on reorders, if still present.
- Fit a whole number of complete covers in the gallery. Replace fractional
  right/left cover cues with subtle, noninteractive directional chevrons in
  dedicated gutters. Leave the existing focus brackets and input mappings intact.

## Boundaries

No changes to sources, lifetime-playtime semantics, SQL, schema, session
lifecycle, Slint theme, Flatpak permissions, game launching, or Milestones.
Individual game Activity detail pages remain a separate future feature.
