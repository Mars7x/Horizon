# ADR 0037: Domain library identity and SQLite persistence

## Status

Accepted — Phase 5.

## Context

Phase 4 completed Horizon's navigation shell. Real source importing now needs durable library identity, but allowing source adapters or presentation code to own SQL would couple the application to provider formats and make later schema evolution fragile.

Game title alone is also not a safe identity key. Different stores can contain unrelated games with equal names, while one source can rename a game without changing its own identifier.

## Decision

Horizon introduces three separate layers for the persistent library:

1. `src/domain/` owns pure game/source identity types and validation.
2. `src/services/library.rs` owns the `DiscoveredGame` use-case input and `LibraryRepository` boundary.
3. `src/persistence/` implements that boundary with `rusqlite` and numbered SQL migrations.

A source-owned game is identified by `(SourceId, ExternalGameId)`. Rediscovering that exact pair updates the existing logical game. A previously unseen pair creates a new logical game. Horizon does not infer equivalence from title strings.

The first SQLite migration creates `games` and `game_sources`. The relationship allows a logical game to carry multiple source references later without redesigning the database, even though Phase 5's generic discovery path does not yet attempt cross-source merging.

Database location is selected by the platform layer using XDG data-directory rules. Persistence receives the resolved path and does not decide platform paths itself.

The app initializes and migrates the database at startup, but the existing demo Home model remains in place during Phase 5. The first production source-backed UI slice belongs to Phase 7.

## Consequences

- Source adapters can be added without SQL or UI coupling.
- Database upgrades are explicit and testable from schema version 1.
- Title changes do not create duplicate records for the same source identity.
- Horizon avoids unsafe title-based deduplication.
- Database failures are surfaced through typed application errors.
- Phase 5 adds persistence without prematurely implementing importers, launching, playtime, or production library UI.

## Licensing note

`rusqlite` and `libsqlite3-sys` are MIT-licensed. Horizon enables rusqlite's `bundled` feature, which compiles SQLite; SQLite's core is dedicated to the public domain. Dependency license inventory remains a release gate for the resolved Cargo graph.
