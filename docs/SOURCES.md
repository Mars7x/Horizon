# Source framework

Phase 6 defines the provider boundary that future Steam, Heroic, Lutris, Bottles, and other adapters plug into. It deliberately does not implement a real provider yet; the first production adapter is the Phase 7 Steam vertical slice.

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

The capability metadata does not itself implement those features. Their service contracts are added when the owning vertical slices need them.

## Registry

`SourceRegistry` owns heterogeneous `GameSource` implementations behind one trait boundary. Registration rejects duplicate `SourceId` values. Registry iteration is deterministic by source ID so tests and diagnostics do not depend on insertion order.

Provider names are never used to select behavior outside `src/sources/`. Future services consume the common trait and capability metadata.

## Discovery outcomes

Discovery separates expected absence from failure:

- `Unavailable(NotInstalled)` means the optional launcher is not present;
- `Unavailable(NotConfigured)` means the adapter has no usable configured installation/account yet;
- `Available(snapshot)` means discovery completed, including a valid empty library;
- `Err(SourceError)` means discovery genuinely failed.

An unavailable optional source is not an application error. A discovery failure is retained in that source's import report and does not prevent later registered sources from being scanned.

## Snapshot identity contract

`SourceSnapshot` requires every `ExternalGameId` to be unique within that one source snapshot. Duplicate source-owned IDs are rejected before persistence because `(SourceId, ExternalGameId)` is the authoritative rediscovery key.

`SourceGame` contains only normalized source-owned identity and title. Raw VDF, JSON, database rows, launcher enums, filesystem paths, or provider SDK types must not escape the adapter.

## Import coordination

`SourceImportService` scans every registered source and produces one report per source. Successful source snapshots are converted to Phase 5 `DiscoveredGame` values and sent through `LibraryService`.

Source discovery failures are isolated. Persistence failures are different: the import pass stops and returns the failing `SourceId`, because continuing after a database write failure could hide a broken durable state assumption.

A successful source snapshot is persisted as one repository batch. The SQLite adapter wraps that batch in one transaction, so a failed write cannot leave only part of a source snapshot committed.

## Phase boundary

Phase 6 does not add:

- Steam/Heroic/Lutris/Bottles parsing;
- launch commands;
- production artwork retrieval;
- playtime/session tracking;
- source-backed Home/Library presentation.

Those are built on this framework in later phases rather than embedded into it now.
