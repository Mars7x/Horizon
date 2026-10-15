# Storage

[Data paths](../src/platform/data_paths.rs) is the only code that selects file
locations. Startup resolves paths and passes them to services. No ad-hoc HOME/XDG
lookups in services, and no assumed host paths inside the Flatpak sandbox.

Under Flatpak, XDG data/config/cache roots are already private to Horizon; put
files directly there. Do not nest another application-ID directory. Native
development uses an `io.github.Mars7x.Horizon` subdirectory beneath the XDG roots.

| Root | Contents | Lifetime |
| --- | --- | --- |
| data | library.sqlite3, album/, chosen/Steam profile images and animated originals | Durable user data |
| data | saves/gba/ followed by the ROM SHA-256 and `.sav`; previous save in `.sav.bak` | Durable battery-save data |
| config | appearance.json, controller.json, cores.json, profile.json, steamgriddb.json, steam-account.json | Persistent preferences/credentials |
| cache | artwork/local/, artwork/steamgriddb/, album/ | Disposable derived media/provenance |

Config directories use owner-only permissions; sensitive files use mode 0600.
This is not encryption. Atomic replacement must preserve prior valid settings
when saving fails. No secrets in UI state, logs, screenshots, cache metadata or
commits. Account and SteamGridDB credentials remain separate.

Versioned caches hold a `.version` marker and are invalidated only through their
owner's format constant. Originals, explicit artwork choices and database history
are not cache. Old Steam achievement caches may remain on disk but are no longer
read or written; cleanup does not delete them.

Album's file is its record; `.part` files are unfinished and invisible to scans.
Chosen profile pictures are Horizon-owned copies; deleting them does not delete
the source file. Steam pictures are downloaded through the API, not read from
local Steam directories. No other application's files are scanned or deleted.

Any new durable file needs a data_paths function, a row here, defined ownership
and error behavior. Schema changes follow [Database](DATABASE.md); assets follow
[Licensing](LICENSING.md).

GBA save locations are selected by data_paths. The player reads bounded existing
save files, preserves invalid/unreadable saves and writes changed RAM on its
worker every five seconds and at normal quit. Temporary replacement files are
flushed before rename; the previous valid save is retained as a backup. A failed
save pauses the game and exposes retry through Save and quit. Save states and
external-emulator save import are not implemented.

`cores.json` stores shared playback freezing and curated mGBA preferences.
Legacy nested freeze values are read without rewriting the file at load; the
shared field takes precedence and is used for subsequent atomic saves. The settings worker serializes
atomic writes; invalid files and failed writes preserve prior data. The player
reads and validates this file on its worker before loading the core.
