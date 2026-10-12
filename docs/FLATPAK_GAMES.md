# Flatpak games source

Games installed as Flatpaks (from the Shop or anywhere else) are a normal
`GameSource`, `flatpak`, shown as **Flatpak**. Decision record: ADR 0183.

## What counts as a game

An exported desktop entry in either installation's
`exports/share/applications/`:

- `Type=Application`, `Categories` containing `Game`, not `NoDisplay=true`
  or `Hidden=true`;
- its app (`X-Flatpak=`, else the file name) is a valid Flatpak app ID;
- not Horizon, and not in `NOT_GAMES` (`src/sources/flatpak.rs`): stores and
  launchers of other stores, streaming clients and game tools. Emulators and
  single-game launchers count as games.

The title is the entry's `Name`; the identity is the app ID. One app with
several game entries is one game, using the entry named for the app. The
user installation wins over the system installation.

## Where it reads

| Installation | Exports | Granted read-only |
| --- | --- | --- |
| User | `$HOST_XDG_DATA_HOME` or `~/.local/share` + `/flatpak/exports/share` | `xdg-data/flatpak/exports`, `xdg-data/flatpak/app` |
| System | `/var/lib/flatpak/exports/share` | `/var/lib/flatpak/exports`, `/var/lib/flatpak/app` |

Exported files are links into the installation's `app` folder, hence its
grant. No exports folder at all means the source is unavailable (saved games
kept). An entry that can't be read makes the scan partial, so nothing is
removed on that scan.

## Launching and sessions

- Launch: `flatpak run <id>` on the host (`platform::flatpak_host::run`),
  not waited for.
- Running: `flatpak ps --columns=application`, kept current by a background
  thread while a session is watched (`RunningApps`), so the UI thread's
  500 ms runtime poll never waits for the host. A game counts as running as
  long as its sandbox does.
- Playtime: Horizon-observed sessions only. Flatpak reports no lifetime
  playtime, and none is shown as zero.

## Artwork

The exported hicolor PNG icons (512, 256, 192, 128, 96, 72, 64, 48 px)
for the entry's `Icon`; the artwork service keeps the largest true square.
With a SteamGridDB key, games are looked up by title as usual. An app that
exports only an SVG icon shows its monogram card until SteamGridDB finds one.

## Live library reload

After a Shop job succeeds, `LibraryReloader` (`src/presentation/library_reload.rs`)
reads every source again off the UI thread, saves it like startup does, and
merges it into the shared `Catalog`. Indices never move: new games are
appended, removed ones are marked absent. See ADR 0183 and
[Library](LIBRARY.md).

## Code

- `src/sources/flatpak.rs`: discovery, the game rules, `FlatpakHost`,
  `valid_app_id`.
- `src/platform/flatpak_host.rs`: `run`, `running`, `HostFlatpak`.
- `src/platform/launcher.rs`: `SourceLaunchTarget::FlatpakApp`.
- `src/presentation/catalog.rs`, `src/presentation/library_reload.rs`.

## Known limits

- SVG-only icons aren't drawn.
- Extra installations under `/etc/flatpak/installations.d` aren't read.
- Reinstalling a game makes a new library entry; its old play history stays
  with the old one.
