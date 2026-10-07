# ADR 0038: Source registry and import framework

## Status

Accepted — Phase 6.

## Context

Phase 5 established durable source-owned game identity and a source-neutral library repository, but Horizon still needs a provider boundary before adding Steam or any other real launcher. Implementing the first provider directly inside application startup or `LibraryService` would make its paths, parser errors, and source-name checks part of the application architecture and encourage every later provider to add another special case.

Optional launchers also need a distinction between normal absence and actual discovery failure. Finally, importing one provider game at a time would allow a database error to commit only a prefix of one discovery snapshot.

## Decision

Horizon adds a first-class source framework under `src/sources/`:

- `GameSource` is the provider adapter trait;
- `SourceDescriptor` carries a stable `SourceId`, display name, and explicit capabilities;
- `SourceRegistry` owns heterogeneous adapters and rejects duplicate source IDs;
- `SourceDiscovery` distinguishes unavailable providers from available snapshots;
- `SourceSnapshot` validates unique source-owned `ExternalGameId` values;
- `SourceGame` is the normalized adapter output and deliberately omits `SourceId`.

`SourceImportService` in `src/services/` orchestrates discovery. It attaches the registered descriptor's `SourceId` to normalized `SourceGame` values to create Phase 5 `DiscoveredGame` values, then calls `LibraryService`. A discovery failure is recorded for that provider and scanning continues. A persistence failure aborts the import pass and identifies the owning source.

The repository boundary gains a batch upsert operation. `SqliteLibraryRepository` implements a full source snapshot batch inside one SQLite transaction. Phase 6 does not change the database schema.

No provider-specific source adapter is added in this phase. Steam is the first consumer in Phase 7.

## Consequences

- Adding a provider does not require source-name branches in UI, presentation, persistence, or generic import services.
- Optional missing launchers are expected state rather than noisy errors.
- A broken provider does not prevent unrelated providers from being discovered.
- Adapter output cannot spoof another registered source's identity.
- Duplicate external IDs in one provider snapshot fail before persistence.
- Successful snapshots are durable atomically per provider.
- Phase 7 can focus on Steam-specific discovery/launch behavior without redesigning generic import orchestration.
- Launch and playtime service contracts are intentionally not guessed before a real vertical slice exercises those boundaries.
