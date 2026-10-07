# ADR 0048: Retire Lutris source support

- Status: Accepted
- Date: 2026-10-07

## Context

Horizon added Lutris in Phase 9 through a read-only `pga.db` adapter and external
`lutris:` URI handoff. The current product direction is to keep the production
source set focused on Steam, Bottles, and Heroic. Retaining an unused Lutris
adapter would continue to expand the Flatpak filesystem surface, source tests,
and lifecycle expectations without a current product requirement.

## Decision

1. Remove `src/sources/lutris.rs` and stop registering Lutris in
   `production_source_registry()`.
2. Remove Lutris-specific Flatpak filesystem permissions.
3. Delete the active Lutris source documentation. Historical ADRs remain as a
   record of earlier architecture and may still mention Lutris.
4. Add schema migration 0005 to remove persisted `game_sources.source_id =
   'lutris'` membership.
5. Preserve historical `play_sessions`, `source_lifetime_playtime`, and their
   owning logical game rows. Historical Activity is data, not active source
   support.
6. Keep the source framework generic. Do not add `if source == "lutris"` cleanup
   paths to services, presentation, or the helper.

## Consequences

Fresh and upgraded installations expose only Steam, Bottles, and Heroic through
the production registry. Existing Lutris-only titles disappear from the active
library after migration, while completed Activity history remains available.
The Flatpak no longer has permission to read Lutris metadata.

Reintroducing Lutris later would be a new source decision and should define its
current discovery, lifecycle tracking, sandbox, and testing requirements rather
than reviving the retired adapter implicitly.
