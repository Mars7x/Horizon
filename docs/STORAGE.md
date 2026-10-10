# Files on disk

`src/platform/data_paths.rs` is the only code that decides where Horizon keeps
files. Services never read `XDG_*` or `HOME` themselves; `app.rs` resolves each
location once at startup and passes it in.

## Layout

Under Flatpak the XDG base directories are already private to Horizon, so files
go directly inside them, with no extra `io.github.Mars7x.Horizon` folder:

```text
~/.var/app/io.github.Mars7x.Horizon/
├── data/
│   ├── library.sqlite3              library, sessions, source lifetime playtime
│   └── album/                       Horizon's own screenshots and clips (see ALBUM.md)
├── config/                          0700; files 0600 (not encryption)
│   ├── appearance.json              theme, accent, UI sounds
│   ├── steamgriddb.json             SteamGridDB API key, artwork preference
│   └── steam-account.json           SteamID64 and Steam Web API key
└── cache/                           disposable; safe to delete
    ├── artwork/
    │   ├── local/                   covers normalized from each source's files
    │   └── steamgriddb/             covers downloaded from SteamGridDB
    ├── achievements/
    │   └── steam-<SteamID64>/       achievement snapshot and badge thumbnails
    └── album/                       capture thumbnails and video durations
```

Outside Flatpak (for example `cargo run`), the base directories are shared with
other programs, so the same tree lives one level down:
`$XDG_DATA_HOME/io.github.Mars7x.Horizon/`, `$XDG_CONFIG_HOME/io.github.Mars7x.Horizon/`
and `$XDG_CACHE_HOME/io.github.Mars7x.Horizon/`, falling back to `~/.local/share`,
`~/.config` and `~/.cache`. Flatpak is detected by `FLATPAK_ID`.

## Rules

- **data** holds what cannot be rebuilt (the library database, Horizon's
  captures). Schema changes use numbered migrations; see
  [Database](DATABASE.md). Captures need no schema: each file is its own
  record ([Album](ALBUM.md)).
- Files other programs own (Steam's screenshots, artwork, metadata) are read
  in place and never copied into data or changed.
- **config** holds user choices and credentials. One file per concern, named
  after it. Never put secrets anywhere else.
- **cache** holds only what can be rebuilt. Each cache folder has a `.version`
  file holding its owner's format constant (`LOCAL_ARTWORK_CACHE_VERSION`,
  `STEAMGRIDDB_CACHE_VERSION`, `ACHIEVEMENTS_CACHE_VERSION`, `ALBUM_CACHE_VERSION`). Bump the constant
  when the format changes and the folder is cleared on the next start. Caches
  are never migrated.
- A new kind of file gets a function in `data_paths.rs` and a line in this
  document, not an ad-hoc path in a service.
