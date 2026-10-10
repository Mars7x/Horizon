# Source framework

Phase 6 defined the provider boundary that launcher adapters plug into. Phase 7 exercised it with Steam; Phase 9 expanded the same unchanged generic contracts to additional providers. The current production set is Steam and Heroic. Historical Bottles support was retired in Phase 9.5.44.43.

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

Capability metadata describes behavior exposed through generic contracts. Phase 7 added the first concrete consumer of `Launch`; Phase 9.5 added `ManagedSession` and `RuntimeObservation`; Phase 9.5.29 adds the first concrete `Artwork` consumer through a source-neutral artwork service. Steam now advertises `LifetimePlaytime` and reads local `localconfig.vdf` where available; provider-reported totals remain distinct from Horizon-observed sessions. See `src/sources/steam.rs` and `STEAM.md`.

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

A successful source snapshot is persisted as one repository batch. The SQLite adapter wraps that batch in one transaction, so a failed write cannot leave only part of a source snapshot committed. Partial snapshots are additive. Authoritative snapshots additionally reconcile source membership in that same transaction. References absent from the complete membership set are removed. Since Phase 8, a logical game row is deleted only when it has no current source references and no retained activity/source-lifetime history; active-library queries still require a current source reference. See ADR 0042 and ADR 0044.

## Concrete adapters

All production providers use the same registry/import/persistence/launch path:

- **Steam** — local VDF/app manifests and `steam://rungameid/<appid>`; see `STEAM.md` and ADR 0039.
- **Heroic** — Phase 9 imports installed Epic/Legendary entries from local Heroic/Legendary metadata and launches with the Heroic protocol; Phase 9.5.44.53 adds its `gui=false` window-hiding parameter (Heroic 2.22.0+). Phase 9.5.44.46 also reads lifetime playtime; see `HEROIC.md`, ADR 0103, and ADR 0110.

ADR 0045 records the multi-provider decisions. The adapters may use different local schemas and identity namespaces, but `SourceImportService`, SQLite persistence, `GameLaunchService`, Home presentation, launch feedback, and Activity remain provider-neutral.

Phase 9.5.44.46 activates `LifetimePlaytime` for Heroic only: its adapter normalizes Heroic's local `timestamp.json` cumulative minutes to seconds. The source-neutral `SourcePlaytimeSync` coordinates with the existing Activity service and lifetime persistence table. Source-reported totals remain separate from Horizon-observed sessions. Steam still does not advertise `LifetimePlaytime`. Production artwork is available through the generic `Artwork` capability, with Steam as the first provider.

## Phase 9 local-only rule

Phase 9 does not log in to launcher accounts or call provider web APIs. Discovery uses only launcher-owned local metadata already present on the machine. Expected absence remains `Unavailable`; malformed/unreadable detected metadata remains a source failure and is isolated from other providers.

Flatpak filesystem access is read-only and provider-scoped. Horizon may read the conventional native XDG subtree and each launcher's own Flatpak app-data tree, but must not add broad `home`, `host`, or arbitrary external-library permissions to increase discovery coverage.

The same authoritative-membership contract from ADR 0042 applies. A provider may prune stale source references only after a complete scan. If a provider cannot prove discovery is complete, it publishes a partial snapshot instead.


## Phase 9.5 managed-session capability

Normal provider launch and managed session launch are separate capabilities.

```text
GameSource
  ├─ Launch          -> SourceLaunchTarget (portal/external)
  └─ ManagedSession  -> SourceManagedLaunchTarget (host helper only)
```

`GameLaunchService` does not check provider names. It asks the registered
descriptor which capabilities exist. If the optional managed-session executor
cannot start a session, the service falls back to the source's normal `Launch`
target.

The Flatpak does not receive arbitrary host execution. It sends only
`(SourceId, ExternalGameId)` to `io.github.Mars7x.Horizon.Session1`; the host
helper re-resolves the source adapter and target independently.

The initial managed-session provider (Bottles) is retired. Steam and Heroic remain
external URI launches until their actual game process/session can be guaranteed
inside the managed compositor. See `MANAGED_SESSIONS.md` and ADR 0046.


## Phase 9.5.28 Lutris retirement

Lutris is no longer registered as a production source. Horizon no longer reads
`pga.db`, launches `lutris:` URIs, or requests Lutris filesystem access. Migration
`0005_remove_lutris_source.sql` removes persisted Lutris `game_sources` membership
so those titles leave the active library. Logical game rows that own Activity or
provider-lifetime history are retained, preserving historical records without
keeping Lutris as an active source. See ADR 0048.


## Phase 9.5.29 artwork capability

Artwork uses the same capability-driven source boundary as launch/session
features:

```text
GameSource::artwork_candidates(ExternalGameId)
                  ↓
             ArtworkService
                  ↓
         1:1 SquareArtwork RGBA
                  ↓
             presentation
```

Adapters expose candidates only. `ArtworkService` chooses/decodes the best
available candidate and normalizes it to Horizon's 1:1 contract. Slint never
branches on a provider name and never opens provider paths directly.

Steam is the first provider to advertise `Artwork`. Heroic continues
to use the procedural fallback until it can expose genuine 1:1 provider artwork.
See `ARTWORK.md` and ADR 0049.

## Phase 9.5.36 native-square artwork contract

`GameSource::artwork_candidates` exposes provider-owned candidates that are
expected to be square. The generic `ArtworkService` decodes every candidate and
enforces `width == height` before the image can reach presentation.

There is no `CoverArt` fallback class. Adapters must not offer portrait,
landscape, capsule, hero, or header art for Horizon's primary game tile merely
because it has more pixels.

The generic rule is therefore:

```text
true provider 1:1 asset -> eligible
non-1:1 provider asset  -> rejected
no eligible square      -> FallbackCover
```

Steam remains the first provider to implement the capability. Heroic
continues to use the procedural fallback until it can expose genuine 1:1 artwork.

### Phase 9.5.44.48 — Automatic Heroic runtime observation

The Phase 9.5.44.47 manual wrapper approach is superseded by an automatic
Heroic log/timestamp observer. Heroic owns the launch, and the Heroic adapter
alone interprets its `launch.log` activity and `timestamp.json` completion.
Generic launch, Activity and Home continue to consume the unchanged
`SourceRuntimeState` boundary. This is best-effort launch-session observation,
not proof that Heroic's game process is alive: unfinished logs expire after
18 hours, and failed/detached launches can create false/early transitions.

The Flatpak gains a narrow native-state read-only grant for Heroic logs; the
existing Heroic Flatpak app-data grant is already sufficient. There are no
helper installs, manual per-game settings, process scans or new dependencies.
See `docs/HEROIC.md` and ADR 0105.


### Phase 9.5.44.49: Launch-scoped runtime observation

`GameSource::runtime_state_for_observation` is an optional, source-neutral
observation-time boundary. The default delegates to `runtime_state` so
providers with authoritative runtime status, including Steam, do not change.
Heroic uses the observation's wall-clock arming time to reject earlier
incomplete log files and selects the newest native/Flatpak log generation
instead of OR-ing possibly stale game states. This is best-effort rather than
verified game-process supervision; see `docs/HEROIC.md` and ADR 0106.
