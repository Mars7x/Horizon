# Storage

[Data paths](../src/platform/data_paths.rs) selects every storage location.
Startup passes paths to workers; services do not resolve HOME/XDG roots.
Under Flatpak, its private data/config/cache roots are used directly. Native
builds use an `io.github.Mars7x.Horizon` subdirectory beneath each XDG root.

## Layout

```text
data/
  library/library.sqlite3
  saves/gba/<game>/
    game.json
    <game name>.sav
    backups/<game name>.previous.sav
  album/gba/<game>/
    game.json
    screenshots/<UTC timestamp>.jxl
    recordings/<UTC timestamp>.<extension>
  album/horizon/screenshots/                 # captures without a game
  profile/
    profile.json
    pictures/custom.jxl
    pictures/custom-animation.jxl
    pictures/steam.jxl
    pictures/steam-animation.jxl
  artwork/custom/                            # reserved for custom originals
config/
  appearance.json
  controller.json
  playback.json
  cores/mgba.json
  integrations/steam.json
  integrations/steamgriddb.json
cache/
  artwork/local/
  artwork/steamgriddb/
  album/
```

Only implemented writers create files. There is no recording implementation or
custom-cover picker. Profile stills and normalized animations are lossless JXL. Retained pre-JXL
profile files remain readable until the user replaces/removes the picture. The existing local/SteamGridDB artwork and Album cache
locations already follow the new grouping; they are not moved or reclassified.

## Game identity and readable names

A game folder looks like `Pokemon - Emerald Version [abcd1234]`. The suffix is a
short ROM-hash prefix; a collision extends the prefix rather than merging games.
`game.json` stores format version, system, full source identity (`sha256:` and
all 64 hex digits for GBA), original title and a filesystem-safe file stem.
This metadata is shared by [save and Album storage](../src/services/game_storage.rs).

Full identity chooses an existing folder. Display-title changes do not rename a
folder or point at a new empty save. Multiple folders claiming the same identity,
corrupt metadata and unsupported versions fail rather than choosing arbitrarily.
Sanitization removes path separators, control/unsafe characters and reserved
names, preserves Unicode and bounds UTF-8 filename length. Titles alone never
identify saves or authorize sharing between ROM versions.

Metadata is published without replacing an existing identity document. Game
folders reject symlink components. Album scans restore references from metadata,
ignore incomplete files and do not follow media/directory symlinks. Deleting a
capture removes empty capture-type folders; the game's identity document stays
so subsequent captures retain their folder name.

## Durability and ownership

| Contents | Ownership and lifetime |
| --- | --- |
| Library database, saves, Album originals, profile files | Durable user data |
| Custom artwork originals, when supported | Durable user data, never cache |
| Preferences and integration credentials | Private persistent configuration |
| Downloaded/normalized artwork, thumbnails and durations | Disposable, reproducible cache |

Private root/config directories use owner-only permissions. Credential files are
mode 0600; permissions are not encryption. Credentials never belong in game
metadata, caches, UI state, logs or commits. Cache version changes invalidate only
their owner's cache, using its `.version` marker; originals are never cache.

Save writes run on the player worker every five seconds and at normal quit when
RAM changed. A flushed replacement is published atomically, preserving the prior
valid save under `backups/`. Invalid/unreadable saves, occupied partial paths and
failed writes preserve existing files and surface an error. A failed game save
pauses playback and allows retry. `.part` captures are invisible until complete.
No other application's files are scanned or deleted.

`playback.json` owns shared focus freezing and automatic resume; `cores/mgba.json` owns only mGBA
preferences. Settings load/save on the existing worker. Each action commits one
owner's file atomically; a failed save does not update the published preferences.

## Save states

Four manual slots and one automatic resume point use complete bundles:

```text
saves/gba/<game>/states/mgba/
  resume/<game name>.resume.{state,json,jxl}
  slot-01/<game name>.slot-01.{state,json,jxl}
  slot-02/…
  slot-03/…
  slot-04/…
  backups/slot-<number>-<unique write id>/…
```

Metadata records format version, full ROM identity, slot, UTC timestamp, exact
core name/version/binary hash, byte count and SHA-256 hashes of state and preview.
Version-2 previews are native 240×160 lossless JXL and stay with durable states.
Version-1 bundles with PNG previews remain readable and are preserved. Regular saves remain
core-independent. State reads are bounded, reject symlink components, validate
metadata/checksums/dimensions and run on the player worker. No state directory
is created before the first write. Resume orders slots by metadata timestamp
ceilings, including retained backups, and fully validates only candidates that
can still be newest. Unknown/unreadable metadata forces normal validation, never
an empty-slot assumption. Exact timestamp ties keep the lower slot first.

A private staging directory receives all three flushed files. Replacement moves
the previous bundle to backups, then publishes the complete staged bundle.
Interrupted publication or damaged current data can recover the newest valid
previous bundle. Successful writes retain one complete compatible prior bundle
per slot; unknown, damaged, incompatible or extra-file backups are preserved.
Incomplete staging directories never appear as slots. Failed writes preserve
existing states and show an error; normal exit waits for the automatic state.
Neither state loads nor state backup cleanup deletes battery saves or Album data.
See [GBA playback](GBA_PLAYBACK.md) for resume behavior.

## Fresh layout

The user approved a fresh structure without migration. Retired database, save,
profile, core/integration preference and Album paths are not read, moved or
deleted. Appearance, controller and cache paths already match this layout and
continue to be used. Existing files remain untouched; the new database starts
an independent library/setup.
SQL schema remains 12 and deployed migrations remain immutable. This is not
permission to delete old data. See [ADR 0205](adr/0205-human-readable-storage.md).
