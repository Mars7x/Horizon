# Album

Album shows Horizon-owned screenshots and animated images, newest first. No external capture
sources are registered, and Steam directories are neither read nor modified.
Existing screenshots/cache files outside Horizon's album are not deleted.

[Catalog](../src/services/album/catalog.rs) lists files, [media workers](../src/services/album/)
decode them, and [AlbumController](../src/presentation/album.rs) owns grid/viewer,
game filtering, selection and dialogs for [the page](../ui/pages/album.slint).

## Files and media

The durable folder is `data/album/`, resolved by [data paths](../src/platform/data_paths.rs).
Captures are filed under `<system>/<readable game folder>/screenshots/`,
with a versioned `game.json` carrying full identity. Captures without a game
use `horizon/screenshots/`. UTC filenames
use `YYYY-MM-DD_HH-MM-SS-mmm`, with a collision suffix when needed. The current
writer creates screenshots only. The new layout does not scan or migrate old
source/hash folders. See [Storage](STORAGE.md).
Writes use a temporary `.part` path, published atomically only when complete. The file is the record; no media
table is needed. External changes appear on the next visit. Thumbnails
are disposable caches, not the original images. Version 6 thumbnails are lossless
JPEG XL. Captures within 20% of the grid's 16:9 (GBA's
3:2 is) fill their tile: large ones are centre-cropped to 16:9 before shrinking
to at most 960×540, keeping detail for 4K tiles. Others (DS screenshots side by
side or stacked, portrait and square pictures) show whole, letterboxed on black,
and their thumbnails keep the whole picture. The viewer grows from exactly what
the tile shows, so a letterboxed capture keeps its shape and never zooms.
Small originals keep their native pixels and alpha. Only the disposable thumbnail
cache is regenerated after an update; originals and full-screen decoding are unchanged.
Small still images use nearest pixel sampling when enlarged in the grid, viewer,
slide and landing layers;
downscaled images keep smooth sampling.

Pictures use JPEG XL (`.jxl`) only. Other extensions and recording folders are
ignored and preserved without conversion or deletion. There is no clip filter,
media decoder, play/pause control, duration or seek bar. Animated JPEG XL plays
on a bounded image worker, preserving frame timing and transparency.
Closing/leaving cancels animation; backgrounding pauses it and Reduced Motion
keeps it still. Thumbnails use the first frame.

The GBA pause panel's Take screenshot action captures the native unfiltered
240×160 game frame. Lossless sRGB RGBA8 JPEG XL encoding/writes run on the core
worker through the runtime libjxl encoder and the shared
HorizonAlbum path allocator, under the game's readable GBA folder with its full ROM identity in `game.json`.
No pause UI, speed badge or video filter is included. Complete captures appear
on the next Album visit. Failed writes preserve existing media and report an error.

## Interaction

The responsive grid has a Game filter, item selection and a full-screen
viewer. Controller/keyboard and pointer use the same actions; the footer shows
only currently available actions. Viewer siblings slide horizontally, and Back
closes local state before leaving Album. Inactive/outgoing pages reject actions.

Changing capture, leaving the page or closing the viewer stops the image
animation worker; generations reject stale frames. Decoding, thumbnails and directory scans must not block the UI thread.
Bounded caches/workers and cancellation protect rapid browsing and resizing.

## Deletion

Only selected Horizon-owned files are deleted, after the confirmation dialog
with Cancel focused. HorizonAlbum refuses paths outside its own root, including
escape/symlink cases; it removes selected files and tidies empty capture-type folders. Game metadata remains
so subsequent captures keep their folder name.
Failures remain visible. No Steam screenshot/thumbnail deletion code remains.
Image fixtures under [tests](../tests/fixtures/) exercise decoding; real display
scaling and animation still require a built app.

See [ADR 0223](adr/0223-image-only-album.md).
