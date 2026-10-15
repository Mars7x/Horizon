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
| config | appearance.json, controller.json, profile.json, steamgriddb.json, steam-account.json | Persistent preferences/credentials |
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
