# Database

Latest schema: `15`

[SqliteLibraryRepository](../src/persistence/sqlite.rs) owns SQL and the shared
connection. Data lives in `library/library.sqlite3` at the location chosen by
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
| 0011 | Reviewed ROM content, file locations and import preferences |
| 0012 | Confirmed first-run setup progress and completion |
| 0013 | DS-size ROM file limit |
| 0014 | Durable per-game visibility preference |
| 0015 | Imported-file size limit widened to 4 GiB for 3DS cartridges |

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
deleted. The `gb`, `gbc` and `gba` sources import readable files after folder selection; their startup snapshots are empty
and partial. A complete scan of the chosen folder that still finds some of its
imported files removes the missing files' `rom_files` rows and moves a game whose
last file went into `retired_game_sources`; importing that content again moves it
back with its GameId.

## History and checks

Observed sessions retain start/end/checkpoint, state and tracking method.
Historical enum values remain readable after their producer is retired. Provider
lifetime counters remain distinct and are never summed with observed time.
Account credentials and media originals do not live in SQLite.

Rust tests cover repository behavior and migration policy. The toolchain-free
[retirement SQL checks](../scripts/check-retirement-migrations.py) exercise fresh
schema, upgrades, retention and failure rollback using in-memory databases only.
They do not establish Rust compilation or access user data.

ROM identity uses the system's source (`gb`, `gbc` or `gba`) plus SHA-256; SHA-1 and size support catalog matching.
`rom_files` retains path bytes and catalog provenance. A changed file at the same
path retains both content records; a future launch must validate content again.
Import preferences and selected files commit together. No automatic pruning runs.
At startup, exact bundled SHA-1/size matches can replace earlier unverified
filename titles and record catalog provenance in one transaction. Existing
verified titles and unknown/conflicting hashes are preserved; GameIds, paths,
sessions and source identities do not change. No schema change is needed.

`game_preferences` stores the hidden flag by GameId. It survives ROM retirement
and reimport. Confirmed uninstall removes only exact `rom_files` records and
archives the last active source membership; games and activity records remain.
