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

The current migration sequence is:

```text
0001_initial_library.sql
0002_activity_sessions.sql
0003_managed_session_tracking.sql
0004_source_runtime_tracking.sql
0005_remove_lutris_source.sql
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

An additive import cannot remove games that a source no longer reports. Phase 7.0.5 adds an explicit authoritative-membership operation to the repository boundary. When a source proves that a scan represents complete installed membership, SQLite upserts the normalized games and removes source references absent from that membership in one transaction.

Beginning with schema version 2, a logical `games` row whose final source reference disappears is deleted only when it also has no historical `play_sessions` and no `source_lifetime_playtime`. Active-library queries still require `game_sources`, so preserving history does not make an uninstalled title visible on Home.

Partial/degraded snapshots continue to use additive upsert and never remove existing source references. No schema migration is required because this is repository behavior over the existing version-1 relational schema. See ADR 0042.

## Schema version 2: Activity

`0002_activity_sessions.sql` adds two intentionally separate tables:

- `play_sessions`: Horizon-observed sessions with game/source identity, start/end timestamps, an explicit tracking method, and `open`/`completed`/`interrupted` state;
- `source_lifetime_playtime`: provider-reported cumulative playtime keyed by `(game_id, source_id)`.

Only completed sessions have an end timestamp. Startup recovery changes leftover `open` rows to `interrupted` without inventing an end time, so they contribute no fabricated duration.

Source-reported lifetime values are never summed into Horizon-observed totals. See `ACTIVITY.md` and ADR 0044.

## Future schema areas

Launching metadata, user library UX, and richer provider metadata get migrations when their owning phases require them. Released migrations remain append-only.

## Tests

The persistence tests use in-memory SQLite and cover:

- first-run migration and migration idempotence;
- rejection of future schema versions;
- foreign-key enablement;
- stable identity on rediscovery;
- title refresh on rediscovery;
- no unsafe title-based deduplication;
- deterministic stable-ID ordering without embedding presentation sorting policy in persistence;
- observed session start/completion and aggregate activity;
- interruption recovery without fabricated duration;
- lifetime-playtime separation;
- preservation of historical activity after a source game is uninstalled.


## Schema v3: managed session tracking

Migration `0003_managed_session_tracking.sql` preserves all existing activity
rows while widening the `play_sessions.tracking_method` constraint to accept:

- `foreground_handoff`
- `managed_session`

No existing Phase 8 row changes meaning during migration. Managed sessions still
use the same `play_sessions` table because they are Horizon-observed sessions;
provider lifetime totals remain separate.


## Schema v4: source runtime tracking

Migration `0004_source_runtime_tracking.sql` preserves all existing activity
rows while widening `play_sessions.tracking_method` to accept a third explicit
method:

- `foreground_handoff`
- `managed_session`
- `source_runtime`

`source_runtime` is used only when a registered source owns a host-side runtime
signal for the exact source/game identity. Steam is the first provider. These
rows remain Horizon-observed session history and are not combined with the
separate `source_lifetime_playtime` table.


## Schema v5: retire Lutris active membership

Migration `0005_remove_lutris_source.sql` removes every `game_sources` row whose
`source_id` is `lutris`. Because active-library queries require current source
membership, retired Lutris-only titles disappear from Home/Library immediately.

The migration deliberately does **not** delete historical `play_sessions` or
`source_lifetime_playtime`. A logical `games` row is removed only when it has no
remaining source reference and no Activity/lifetime history. This keeps past
Activity readable while ensuring Lutris is no longer an active provider.
