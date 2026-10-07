# Source framework

Phase 6 defined the provider boundary that Steam, Heroic, Lutris, Bottles, and other adapters plug into. Phase 7 now exercises that boundary with the first production adapter: Steam. The generic contracts remain provider-neutral.

## Boundary

Provider-specific code lives under `src/sources/` and implements `GameSource`. An adapter may know its launcher's paths, formats, installation conventions, and quirks. It must not know SQLite, Slint, route state, or presentation policy.

The normalized discovery flow is:

```text
GameSource
   │
   ├─ SourceDescriptor
   │    ├─ stable SourceId
   │    ├─ display name
   │    └─ explicit capabilities
   │
   └─ SourceDiscovery
        ├─ Unavailable(reason)
        └─ Available(SourceSnapshot)
                  │
                  └─ SourceGame { ExternalGameId, GameTitle }
                           ↓
                    SourceImportService
                           ↓
                      DiscoveredGame
                           ↓
                       LibraryService
                           ↓
                    LibraryRepository
```

The adapter does not place its own `SourceId` on every game. `SourceImportService` attaches the ID from the registered adapter descriptor. This prevents a malformed adapter result from silently claiming another source's identity.

## Source descriptors and capabilities

Every registered source has one `SourceDescriptor` with a stable `SourceId` and human-readable display name. Capability metadata uses explicit `SourceCapability` values rather than code such as `if source == "steam"` outside the adapter layer.

Phase 6 defines capability vocabulary for:

- launching;
- artwork;
- source-reported lifetime playtime.

Capability metadata describes behavior exposed through generic contracts. Phase 7 adds the first concrete consumer of `Launch`: a source-neutral launch target/service plus a platform executor. Artwork and lifetime-playtime contracts remain deferred until a real slice needs them.

## Registry

`SourceRegistry` owns heterogeneous `GameSource` implementations behind one trait boundary. Registration rejects duplicate `SourceId` values. Registry iteration is deterministic by source ID so tests and diagnostics do not depend on insertion order.

Provider names are never used to select behavior outside `src/sources/`. Services consume the common trait and capability metadata. `SourceRegistry::get` allows launch coordination to resolve the adapter that owns a persisted `SourceGameRef` without provider-name branching.

## Discovery outcomes

Discovery separates expected absence from failure:

- `Unavailable(NotInstalled)` means the optional launcher is not present;
- `Unavailable(NotConfigured)` means the adapter has no usable configured installation/account yet;
- `Available(snapshot)` means discovery completed, including a valid empty library;
- `Err(SourceError)` means discovery genuinely failed.

An unavailable optional source is not an application error. A discovery failure is retained in that source's import report and does not prevent later registered sources from being scanned.

## Snapshot identity and completeness contract

`SourceSnapshot` requires every `ExternalGameId` to be unique within that one source snapshot. Duplicate source-owned IDs are rejected before persistence because `(SourceId, ExternalGameId)` is the authoritative rediscovery key.

Snapshots are conservative by default. A source must opt into authoritative membership only when it can identify the complete installed membership for that scan. Authoritative membership is stored separately from normalized `SourceGame` values so a currently installed identity can be preserved even when its fresh title metadata is temporarily unavailable. Every normalized game in an authoritative snapshot must belong to that membership set.

`SourceGame` contains only normalized source-owned identity and title. Raw VDF, JSON, database rows, launcher enums, filesystem paths, or provider SDK types must not escape the adapter.

## Import coordination

`SourceImportService` scans every registered source and produces one report per source. Successful source snapshots are converted to Phase 5 `DiscoveredGame` values and sent through `LibraryService`.

Source discovery failures are isolated. Persistence failures are different: the import pass stops and returns the failing `SourceId`, because continuing after a database write failure could hide a broken durable state assumption.

A successful source snapshot is persisted as one repository batch. The SQLite adapter wraps that batch in one transaction, so a failed write cannot leave only part of a source snapshot committed. Partial snapshots are additive. Authoritative snapshots additionally reconcile source membership in that same transaction: references absent from the complete membership set are removed and only truly orphaned logical games are deleted. See ADR 0042.

## Phase 7 concrete consumer

Steam lives in `src/sources/steam.rs` and is the first concrete `GameSource`. It reads only Steam-owned local metadata, emits normalized `SourceGame` values, and prepares a generic URI launch target. `SourceImportService`, SQLite persistence, `GameLaunchService`, and Home presentation remain source-neutral. See `STEAM.md` and ADR 0039.

Phase 7 still does not add Heroic/Lutris/Bottles adapters, source-reported playtime, Horizon-observed sessions, or a production artwork cache. Those later features must extend the same contracts rather than introducing source-name checks.
