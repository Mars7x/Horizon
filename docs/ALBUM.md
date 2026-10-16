# Album

Album shows Horizon-owned pictures and clips, newest first. No external capture
sources are registered, and Steam directories are neither read nor modified.
Existing screenshots/cache files outside Horizon's album are not deleted.

[Catalog](../src/services/album/catalog.rs) lists files, [media workers](../src/services/album/)
decode them, and [AlbumController](../src/presentation/album.rs) owns grid/viewer,
filters, selection and dialogs for [the page](../ui/pages/album.slint).

## Files and media

The durable folder is `data/album/`, resolved by [data paths](../src/platform/data_paths.rs).
Captures are filed under `<system>/<readable game folder>/screenshots/` or
`recordings/`, with a versioned `game.json` carrying full identity. Captures
without a game use `horizon/screenshots/` or `horizon/recordings/`. UTC filenames
use `YYYY-MM-DD_HH-MM-SS-mmm`, with a collision suffix when needed. The current
writer creates screenshots only. The new layout does not scan or migrate old
source/hash folders. See [Storage](STORAGE.md).
Writes use a temporary `.part` path, published atomically only when complete. The file is the record; no media
table is needed. External changes appear on the next visit. Thumbnails/durations
are disposable caches, not the original media. Version 4 thumbnails are lossless
PNG. Large photos are centre-cropped to the grid's 16:9 shape before shrinking
to at most 960×540, so portrait/square pictures retain enough detail for 4K tiles.
Small originals keep their native pixels and alpha. Only the disposable thumbnail
cache is regenerated after an update; originals and full-screen decoding are unchanged.
Small still images use nearest pixel sampling when enlarged in the grid, viewer,
slide and landing layers;
downscaled images and videos keep smooth sampling.

Supported extensions include PNG, JPEG, GIF, JPEG XL, MP4, M4V, MKV, WebM and
MOV; decoding follows content/runtime support. Animated GIFs play silently in
place of stills; animated JPEG XL uses its first frame. FFmpeg video decoding
and PipeWire audio use [native shims](../csrc/) behind the VideoBackend contract.
No download or recorder implementation is implied by the viewer.

The GBA pause panel's Take screenshot action captures the native unfiltered
240×160 game frame. PNG encoding/writes run on the core worker through the shared
HorizonAlbum path allocator, under the game's readable GBA folder with its full ROM identity in `game.json`.
No pause UI, speed badge or video filter is included. Complete captures appear
on the next Album visit. Failed writes preserve existing media and report an error.

## Interaction

The responsive grid has Game and Type filters, item selection and a full-screen
viewer. Controller/keyboard and pointer use the same actions; the footer shows
only currently available actions. Viewer siblings slide horizontally, and Back
closes local state before leaving Album. Inactive/outgoing pages reject actions.

Videos expose playback and seeking; seek generations reject stale frames/audio.
Changing capture, leaving the page or closing the viewer stops playback and
workers. Decoding, thumbnails and directory scans must not block the UI thread.
Bounded caches/workers and cancellation protect rapid browsing and resizing.

## Deletion

Only selected Horizon-owned files are deleted, after the confirmation dialog
with Cancel focused. HorizonAlbum refuses paths outside its own root, including
escape/symlink cases; it removes selected files and tidies empty capture-type folders. Game metadata remains
so subsequent captures keep their folder name.
Failures remain visible. No Steam screenshot/thumbnail deletion code remains.
Media fixtures under [tests](../tests/fixtures/) exercise decoding; runtime audio,
seeking, animated images and real scaling still require a built app.
