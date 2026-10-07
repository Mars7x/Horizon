# Database

Horizon uses SQLite through `rusqlite`. SQL is confined to `src/persistence/`; presentation, UI, domain, and source adapters must not issue SQL directly.

## Location

The native database path is resolved by the platform layer:

```text
$XDG_DATA_HOME/io.github.Mars7x.Horizon/library.sqlite3
```

If `XDG_DATA_HOME` is unset, Horizon follows the XDG default under:

```text
$HOME/.local/share/io.github.Mars7x.Horizon/library.sqlite3
```

Inside the Flatpak, `XDG_DATA_HOME` already points at Horizon's per-application writable data area, so no broad host filesystem permission is required.

## Migration policy

Every application-schema change is a numbered SQL migration under `src/persistence/migrations/`. `schema_migrations` is the bootstrap ledger used to record applied versions.

Phase 5 starts with:

```text
0001_initial_library.sql
```

Startup applies missing migrations in one transaction per migration. Migration history must be contiguous. A database from a newer Horizon schema is rejected instead of being opened with older code.

Released migrations are immutable. Fixes to a released schema are new numbered migrations; do not edit an already-shipped migration in place.

## Schema version 1

`games` stores Horizon-owned logical game identity and display title.

`game_sources` associates a logical game with source-owned identity. The database enforces uniqueness of `(source_id, external_id)` and foreign-key ownership by `games`.

The initial repository records `first_seen_at` and `last_seen_at` as Unix timestamps. Rediscovering the same source key keeps the same `GameId`, refreshes its title, and updates `last_seen_at`.

Titles are not used for deduplication. Two equal titles from different source keys remain separate games until Horizon has an explicit, reliable reconciliation rule.

## SQLite configuration

Each Horizon connection enables foreign-key enforcement and uses a bounded busy timeout. The development build uses rusqlite's `bundled` feature so its SQLite engine is deterministic across native and Flatpak builds.


## Phase 6 source-batch writes

The schema remains at version 1 in Phase 6. The repository boundary now supports batch upserts because one successful source discovery is treated as a coherent snapshot. `SqliteLibraryRepository` writes each source snapshot inside one transaction. If any write in that batch fails, the entire source snapshot rolls back rather than leaving a committed prefix.

Source discovery and registry logic remain outside persistence. SQLite receives only normalized `DiscoveredGame` values and source-owned membership identities through the service/repository boundary.

## Phase 7 authoritative source reconciliation

An additive import cannot remove games that a source no longer reports. Phase 7.0.5 adds an explicit authoritative-membership operation to the repository boundary. When a source proves that a scan represents complete installed membership, SQLite upserts the normalized games, removes source references absent from that membership, and deletes only `games` rows left with zero source references. The entire reconciliation is one transaction.

Partial/degraded snapshots continue to use additive upsert and never remove existing source references. No schema migration is required because this is repository behavior over the existing version-1 relational schema. See ADR 0042.

## Future schema areas

Phase 5 intentionally does not pre-create columns for launching, source-specific metadata, user library UX, or activity. Those features get migrations when their owning phases are implemented.

In particular, future source-reported lifetime playtime and Horizon-observed sessions must remain separate data. They must never be collapsed into one ambiguous counter.

## Tests

The persistence tests use in-memory SQLite and cover:

- first-run migration and migration idempotence;
- rejection of future schema versions;
- foreign-key enablement;
- stable identity on rediscovery;
- title refresh on rediscovery;
- no unsafe title-based deduplication;
- deterministic stable-ID ordering without embedding presentation sorting policy in persistence.
