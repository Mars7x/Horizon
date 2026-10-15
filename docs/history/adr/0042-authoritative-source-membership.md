# ADR 0042: Authoritative source membership reconciliation

## Status

Accepted for Phase 7.0.5.

## Context

The Phase 6 import contract was intentionally additive. That is safe for partial or degraded discovery, but it means a source entry can remain in Horizon forever after the game is uninstalled. Steam's installed-library scan can provide a complete membership set when all detected Steam roots are readable, so keeping stale Steam rows is no longer correct.

A source may also know that an external ID is still installed while temporarily lacking enough metadata to publish a fresh `SourceGame`. Reconciliation therefore cannot assume that the set of normalized games is always identical to the set of present source identities.

## Decision

`SourceSnapshot` now distinguishes conservative snapshots from authoritative membership snapshots. An authoritative snapshot carries the complete set of source-owned external IDs that must remain present. The normalized `SourceGame` list may be a subset of that membership when metadata for a present ID is temporarily unavailable.

`SourceImportService` remains provider-neutral. Partial snapshots use additive batch upsert. Authoritative snapshots call `LibraryService::synchronize_source_snapshot`, which persists normalized games and removes source references absent from the authoritative membership in one transaction. A `games` row is deleted only when no source references remain.

Steam uses readable `appmanifest_<appid>.acf` files as the primary installed-membership record for a readable library and falls back to the `libraryfolders.vdf` `apps` map when an external library is outside Horizon's sandbox. Explicit non-game appinfo types are excluded from Steam game membership. If any detected Steam root fails discovery, the merged snapshot remains partial and no destructive reconciliation occurs.

## Consequences

- Uninstalled Steam games disappear from Horizon after a successful complete scan.
- Old over-imported/stale Steam rows are cleaned automatically; users do not need to delete the database.
- A degraded/failed source scan cannot destructively erase prior library membership.
- Installed identities with temporarily unavailable title metadata can remain preserved without being fabricated into new UI cards.
- Future sources can opt into the same contract without source-name branches in services or persistence.
