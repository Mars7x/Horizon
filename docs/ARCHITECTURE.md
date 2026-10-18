# Architecture

Horizon keeps behavior in Rust and rendering in Slint. The first approved host is
an in-process mGBA libretro worker, scoped to mGBA's systems (Game Boy, Game Boy
Color and GBA). Nintendo DS uses the separately approved melonDS DS core on
the same worker; this remains a narrow two-core host rather than a plugin framework.
[ADR 0216](adr/0216-nintendo-ds-playback.md) records the DS hosting review.
[ADR 0198](adr/0198-first-gba-playback.md) and
[ADR 0209](adr/0209-game-boy-and-game-boy-color.md) record the decision.

| Layer | Responsibility | Source |
| --- | --- | --- |
| Domain | Validated identities, session states, settings and media concepts | [domain](../src/domain/) |
| Services | Library, import, launch/activity, artwork, media, account and friend coordination | [services](../src/services/) |
| Persistence | SQLite repositories, atomic writes, private settings and migrations | [persistence](../src/persistence/) |
| Source boundaries | Capability-driven interfaces; cartridge recognition and import-only adapters | [sources](../src/sources/mod.rs) |
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
cartridge-system registry and reads the active library. Runtime services share the
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

Generic lifecycle/import interfaces remain, while embedded launches bypass their
external-URI/foreground-handoff assumptions. Further core or process architecture
changes require a separate decision. See [Database](DATABASE.md),
[Storage](STORAGE.md) and [current decisions](adr/).

The [player service](../src/services/gba_player.rs) revalidates a complete ROM
snapshot against its imported SHA-256, owns save writes and observed sessions,
and calls the [mGBA host](../src/platform/gba_core.rs) on one worker. Slint receives
only the latest software RGBA frame; PipeWire consumes a bounded audio queue.
SDL remains on its existing main-thread event pump. Gameplay held state is
separate from menu repeats; the Rust player consumes shell actions while open.
No external emulator executable, host command or broader Flatpak grant is used.

[Game storage](../src/services/game_storage.rs) resolves readable folders by full
identity metadata on storage workers; data_paths owns their layout.
[Core settings persistence](../src/persistence/core_settings.rs) aggregates shared
playback and mGBA preference files without introducing cross-file UI commits.
[Save-state storage](../src/services/save_states.rs) validates complete state/metadata/preview
bundles on the same player worker. The narrow mGBA host owns serialization and
rollback on rejected loads. Rust owns slot selection, overwrite confirmation and
automatic resume policy; Slint receives only row projections and decoded frames.
See [Storage](STORAGE.md) for the fresh layout and state bundles.

JPEG XL is the stored raster format for screenshots, state previews, profiles and
normalized artwork/Album caches. The original libjxl shim encodes lossless sRGB
RGBA frames on workers; shared image services import source formats and decode
bounded JXL animations through jxl-oxide's frame API. Videos and authored SVGs
keep their existing boundaries. Versioned state/profile readers preserve older
durable files; only disposable cache versions are invalidated.
