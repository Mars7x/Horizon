# Architecture

Horizon keeps behavior in Rust and rendering in Slint. A future core host must
fit this separation; no hosting API or in-process/child-process choice is made here.

| Layer | Responsibility | Source |
| --- | --- | --- |
| Domain | Validated identities, session states, settings and media concepts | [domain](../src/domain/) |
| Services | Library, import, launch/activity, artwork, media, account and friend coordination | [services](../src/services/) |
| Persistence | SQLite repositories, atomic writes, private settings and migrations | [persistence](../src/persistence/) |
| Source boundaries | Capability-driven game/capture interfaces; empty production registries | [sources](../src/sources/mod.rs) |
| Platform | Data paths, portals, Winit/Skia, native media and read-only host status | [platform](../src/platform/) |
| Input/navigation | Semantic actions, repeat, routes, history and focus | [input](../src/input/), [navigation](../src/navigation/) |
| Presentation | Controllers, workers, shared catalog and Rust→Slint projections | [presentation](../src/presentation/) |
| UI | Intent forwarding, shared components, pages and theme tokens | [UI](../ui/) |

The domain has no SQL, SDL, Slint or platform types. Services receive resolved
paths and normalized values. SQL belongs in persistence; filesystem/network work
never belongs in Slint. Provider capabilities stay behind the source boundary,
not string checks spread through presentation.

## Composition and workers

[Startup](../src/app.rs) resolves paths, opens/migrates SQLite, constructs the
empty source registry and reads the active library. Runtime services share the
repository. Home, Library and Activity share an append-only catalog whose indices
never move during a run; reloads append and mark removals.

Blocking network/image/media work runs off the UI thread through
[worker delivery](../src/services/worker.rs) and
[presentation wakeups](../src/presentation/wake.rs). Account changes must prevent
stale responses from repopulating another account's UI. UI/controller objects
remain on the appropriate main thread; SDL event-pump ownership stays explicit.

## Retained integrations

- Steam Account supplies profile pictures and read-only friends only through
  [the shared API boundary](../src/services/steam_account.rs).
- [SteamGridDB](../src/services/steamgriddb.rs) supplies optional square artwork
  with its own credentials, cache and background workers.
- Album reads Horizon's own folder. FFmpeg decoding and PipeWire sound use
  [C shims](../csrc/) behind Rust media interfaces.
- Normal desktop file choice and browser links use XDG portals. There is no
  shell execution, host launcher adapter or session helper.

Generic lifecycle/import interfaces are retained, not a promise that their
external-URI assumptions will suit emulator hosting. Evaluate them alongside
crash containment, input ownership, video/audio, saves and architecture support
in a separately approved core-hosting decision. See [Database](DATABASE.md),
[Storage](STORAGE.md) and [current decisions](adr/).
