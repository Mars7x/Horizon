# ADR 0183: Flatpak games are a source, and the library reloads live

Date: 2026-10-12

## Status

Superseded (2026-10-12): the Shop, the Flatpak game source and the host `flatpak` access were removed when Horizon moved to embedded emulator cores. Originally: Accepted. Extends ADR 0181's host-execution exception; answers its
"Installed apps don't join the Library yet".

## Context

The Shop installs games from Flathub, but they didn't reach the Library:
Steam and Heroic were the only sources, and the library was read once, at
startup. A Flatpak game is an app like any other on the host; Horizon's
sandbox can't see the host's installed apps or start them on its own.

## Decision

**A `flatpak` source (`src/sources/flatpak.rs`), always on.**

- **Discovery is read-only files, no host command.** Horizon reads the
  exported `.desktop` entries and hicolor icons of the user installation
  (`~/.local/share/flatpak/exports/share`) and the system installation
  (`/var/lib/flatpak/exports/share`). Exported files are links into each
  installation's `app` folder, so the manifest grants four read-only paths:
  `xdg-data/flatpak/{exports,app}:ro` and `/var/lib/flatpak/{exports,app}:ro`.
- **A game** is an exported entry with `Type=Application`, the `Game`
  category, not `NoDisplay`/`Hidden`, and a valid app ID (from `X-Flatpak`,
  else the file name). Stores, launchers of other stores, streaming clients
  and game tools are excluded by exact app ID (`NOT_GAMES`: Steam, Heroic,
  Lutris, Bottles, itch, Minigalaxy, Steam Link, Moonlight, ProtonUp-Qt, …),
  and so is Horizon. Emulators and single-game launchers (Prism Launcher)
  are games.
- **Identity** is the app ID; title is the entry's `Name`. An app with
  several game entries uses the one named for the app. The user
  installation wins over the system one for the same app.
- **Completeness:** no exports folder at all is `Unavailable(NotInstalled)`
  (games kept). Every entry read is an authoritative snapshot; an entry that
  can't be read (mid-install, missing grant) makes it partial, so no
  installed game is dropped.
- **Launch is `flatpak run <id>`** through `platform::flatpak_host` (new
  `SourceLaunchTarget::FlatpakApp`). The launcher doesn't wait for the game;
  a thread reaps it.
- **Sessions:** `RuntimeObservation` from `flatpak ps`. Runtime state is
  polled every 500 ms on the UI thread, so the source never asks the host
  there: a background thread keeps the running list current while it's
  being asked for (every second, stopping 5 s after the last question).
  A failed `flatpak ps` is an error, not "stopped". No source-reported
  lifetime playtime: Flatpak keeps none.
- **Artwork:** the exported square PNG icons (512 down to 48); with a
  SteamGridDB key, the usual title lookup. SVG-only icons aren't drawn
  (Horizon has no SVG rasterizer) and fall back to the monogram card.
- **App ID check** (`sources::flatpak::valid_app_id`) is shared:
  `services::flathub::valid_app_id` and `flatpak_host` use it.

**Host execution** stays confined to `src/platform/flatpak_host.rs`, with
two more fixed subcommands, each with a checked ID as one argument:
`flatpak run <id>` and `flatpak ps --columns=application`. The source
reaches them through the `FlatpakHost` trait and runs nothing itself.

**The library reloads while Horizon runs**
(`src/presentation/library_reload.rs`). After a Shop install, update or
uninstall succeeds, every source is discovered again on a worker thread
(with its own registry), saved on the UI thread through the same
`SourceImportService::save_all` as startup, and merged into one shared
`Catalog` (`src/presentation/catalog.rs`) that Home, Library and Activity
read. Catalog indices never move during a run: new games are appended, a
removed game keeps its slot marked absent and every page skips it. That
keeps Home's index-keyed cards, Playing state and artwork jobs valid
without remapping. Home makes cards for new games and starts a SteamGridDB
pass (covers already shown stay; cached ones return immediately), Library
keeps its source filter and selection, Album learns the new titles.
Reloads coalesce: one asked for while one runs runs once more after it.

There is **no toggle** for the source. Turning a source off would drop its
games from the library, and turning it back on would make new game rows:
play history isn't reattached to a rediscovered game (the same happens to
any source's uninstall and reinstall today).

## Consequences

- Shop installs appear in Home and the Library within a moment, without a
  restart; uninstalled games leave.
- Four more read-only filesystem grants. The `app` folders expose every
  installed app's files read-only (as Flatseal's do).
- Achievements still use the startup library (Steam-only, so a Flatpak
  install doesn't change it); a Steam game installed mid-run joins the
  Library on the next reload but Achievements only after a restart.
- Reinstalling a game makes a new game row; its earlier play history stays
  under the old one (pre-existing behavior, not new here).
- Extra Flatpak installations (`/etc/flatpak/installations.d`) aren't read.
