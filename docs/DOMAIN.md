# Domain model

Phase 5 introduces the first durable Horizon library concepts. These types live in `src/domain/` and remain independent of Slint, SQLite, SDL, Flatpak, portals, and launcher-specific formats.

## Library identity

A Horizon game has an internal `GameId` and a validated `GameTitle`. External launchers identify discovered games through a `SourceGameRef`, which is the pair:

- `SourceId`: the source adapter's stable identifier;
- `ExternalGameId`: the identifier that source uses for the game.

The pair `(SourceId, ExternalGameId)` is authoritative for rediscovery. Titles are presentation metadata, not identity. Horizon deliberately does **not** merge two source records merely because their titles match; doing so would silently combine unrelated games. A later source/library phase may add explicit reconciliation rules without changing this foundation.

`LibraryGame` represents one logical Horizon game together with one or more source references. The Phase 5 repository creates a new logical game for a previously unseen source key and updates the existing logical game when that exact source key is seen again.

## Validation

Domain constructors reject invalid values before they reach persistence:

- `GameId` must be positive;
- titles must contain non-whitespace text and are stored trimmed;
- source IDs and external IDs must contain non-whitespace text.

Persistence re-validates values read from SQLite. Corrupt or incompatible stored data is reported as an error rather than fabricated into domain state.

## Service boundary

`src/services/library.rs` owns the source-neutral library use-case boundary:

- `DiscoveredGame` is the normalized input future source adapters will produce;
- `LibraryRepository` describes the persistence operations the service needs;
- `LibraryService` coordinates those operations without importing SQLite types.

Phase 5 does not add source adapters. Steam, Heroic, Lutris, and other provider-specific parsing belongs to Phase 6 and later.
