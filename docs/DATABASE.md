# Database

Latest schema: `10`

[SqliteLibraryRepository](../src/persistence/sqlite.rs) owns SQL and the shared
connection. Data lives in `library.sqlite3` at the location chosen by
[data paths](../src/platform/data_paths.rs). The domain and UI do not issue SQL.

## Migration contract

[Migration code](../src/persistence/migrations.rs) applies contiguous numbered
SQL files transactionally and records a ledger entry only after success.
Reopening is idempotent. Newer unsupported schemas and history gaps are rejected.
Deployed migrations are immutable; append a new migration to change schema.
[Repository contracts](../scripts/repository-contracts.json) pin existing SQL
checksums. Never “fix” a deployed file or its checksum to bypass a failure.

| Migration | Purpose |
| --- | --- |
| 0001 | Logical games and source identity |
| 0002 | Observed sessions and separate lifetime counters |
| 0003 | Historical managed-session tracking compatibility |
| 0004 | Source-runtime tracking compatibility |
| 0005 | Lutris membership retirement |
| 0006 | Crash-resilient session checkpoints |
| 0007 | Bottles membership retirement |
| 0008 | Flatpak game membership retirement |
| 0009 | Archive Steam identities without deleting games/history |
| 0010 | Archive Heroic identities without deleting games/history |

## Identities and membership

A logical GameId is not its title. SourceId plus ExternalGameId forms a source
reference; equal titles do not authorize merging identities. Active-library
queries require `game_sources`. `retired_game_sources` preserves original IDs,
external IDs and timestamps separately, with restrictive game ownership.
Archived games are excluded from active covers/installed-most-played slots;
Album can still name Horizon-owned captures carrying legacy references.

Reconciliation must preserve archived identities and games with session/lifetime
history. Successful source batches are atomic. Partial/unavailable snapshots
cannot prune prior membership. Earlier retirement migrations retain their
original behavior; later archives cannot restore data those earlier versions
deleted. No production source currently imports a snapshot.

## History and checks

Observed sessions retain start/end/checkpoint, state and tracking method.
Historical enum values remain readable after their producer is retired. Provider
lifetime counters remain distinct and are never summed with observed time.
Account credentials and media originals do not live in SQLite.

Rust tests cover repository behavior and migration policy. The toolchain-free
[retirement SQL checks](../scripts/check-retirement-migrations.py) exercise fresh
schema, upgrades, retention and failure rollback using in-memory databases only.
They do not establish Rust compilation or access user data.
