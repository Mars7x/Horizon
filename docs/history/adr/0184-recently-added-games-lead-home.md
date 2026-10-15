# ADR 0184: Recently added games lead Home, and the row glides

Date: 2026-10-12

## Status

Accepted. Amends Phase 10.1.0's Home order (latest play only).

## Context

With Flatpak games joining the library live (ADR 0183), a game installed
from the Shop landed in Home only by alphabetical fallback, often not at
all. A console home row puts a new game first.

## Decision

- `recent_game_ids` orders installed games by their latest moment: the
  latest play-session start, or `games.created_at` when the game joined the
  library more than 60 s after the earliest game row (the first import, so
  a first run doesn't call every game "new"). Games with neither still fill
  the remaining slots A–Z. No schema change.
- Home publishes each card's previous place and a sequence counter; the
  carousel glides cards to their new places, fades and raises a new game in,
  and slides the Library tile, with the shared `GridRefresh` timing.
- Changes are measured from the row as last seen. One made while Home is
  hidden plays when Home is shown again, after the page transition
  (`NavigationController::publish_route_change` → `reveal_row_changes`).
- Reduced Motion makes it instant.

## Consequences

- A reinstalled game (a new game row) also counts as newly added.
- A game played moving to the front glides the same way.
