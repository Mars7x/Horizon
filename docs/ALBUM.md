# Album

The Album shows every screenshot and video clip Horizon can find, newest
first: Horizon's own captures and the captures sources keep (Steam's
screenshots). See [ADR 0166](adr/0166-album-captures-and-media-pipeline.md).

## Using it

| Where | Input | Does |
| --- | --- | --- |
| Grid | D-pad / stick | Move between captures (reading order; no wrap) |
| Grid | A, or click the selected tile | Open the capture full-screen |
| Grid | X | Start selecting to delete: A (or a click) ticks captures, X asks to delete the ticked ones, B finishes |
| Grid | LB / click "Game" | Next game filter: All games, then each game with captures, most recent first |
| Grid | RB / click "Type" | All → Screenshots → Videos |
| Grid | Mouse wheel | Moves the camera only, never the selection |
| Viewer | Left / Right, or click the screen edges | Previous / next capture, sliding in from that side; stops at either end |
| Viewer (video) | A, or click the video | Play / pause; at the end, A plays it again |
| Viewer (video) | LB / RB | Back / forward 10 seconds |
| Viewer | X | Asks to delete this capture (Cancel is focused first) |
| Viewer (video) | Click the timeline | Seek |
| Viewer | B | Back to the grid, on the capture that was open |

Changing Game or Type runs the shared grid refresh (as Library's Source and
Sort do): the old grid fades and the new captures arrive column by column.

**Fullscreen viewer.** Opening a capture grows its tile to fill the whole
window, on black. Back shrinks it into its place in the grid (the tile of
whichever capture is open, if you browsed). It opens from and closes onto
the selected tile's lifted size. While closing, the picture crossfades into
the tile's own thumbnail, so a video stopped mid-way lands on exactly what
the tile shows. The date, position, hints and the
video timeline sit on soft gradients at the top and bottom. They show on
opening and on any button press or mouse movement, fade after three seconds,
and stay up while a video is paused or a capture can't be shown. With a
mouse, click the hints (B Back, A Play…) like any other page's.

Every Album visit starts fresh (newest capture, top of the grid) and rescans
for new captures; the Game and Type filters are kept. Videos start playing
when they come on screen, and pause when Horizon loses the foreground (a
game starts). Leaving the Album stops playback.

## Where captures come from

- **Horizon:** `data/album/` (see [Files on disk](STORAGE.md)). The file is
  the record; there is no database table. A capture deleted outside Horizon
  disappears on the next visit.

  ```text
  album/<source>/<external id>/<UTC stamp>.<ext>   a game's captures
  album/horizon/<UTC stamp>.<ext>                   Horizon's own screens
  ```

  Folder names are `encode_path_segment` of the ids (unsafe characters become
  `%XX`). The stamp is `YYYYMMDD-HHMMSS-mmm` in UTC, with `-2`, `-3`… for
  captures in the same millisecond.
- **Sources** implement `CaptureSource` (`src/sources/mod.rs`): a read-only
  directory listing, no decoding, on the scan thread. Steam lists
  `userdata/<account>/760/remote/<appid>/screenshots/` for the active account
  only (the one `loginusers.vdf` marks most recent, or the only one), so
  another person's screenshots on a shared PC never appear. Steam's own
  `thumbnails/` copy is offered as a preview. Horizon never changes source
  files.

Supported formats are decided by extension: PNG and JPEG photos; MP4, M4V,
MKV, WebM and MOV videos (whatever GStreamer can decode).

## Deleting

Only Horizon's own captures can be deleted: their tiles get tick boxes when
selecting, and other tiles dim. A source's captures (Steam's screenshots)
belong to that source. Horizon's sandbox can only read them, and they are
deleted in Steam. `HorizonAlbum::delete` refuses any path outside Horizon's
album folder, removes the file permanently after the confirmation dialog,
and tidies empty game folders. If a file can't be deleted, the bottom bar
says so.

## Video timeline

The timeline is a steady clock running at real-time speed, so the bar moves
perfectly evenly even when frames reach the screen unevenly (start-up
bursts, a busy frame). Each shown frame carries its stream time and length
from GStreamer. The frames correct the clock only when it drifts more than
two frames from the picture, or on a still (start, pause, seek). The clock
never runs more than one and a half frames past the frame on screen, so a
stall, or the start-up while audio opens, holds the bar instead of running
ahead.

- A seek anchors at its target at once. Frames decoded before the seek
  (`seek_epoch`) never move the timeline back.
- At the end, the clock rests on the duration.
- Nothing polls the pipeline: an idle or paused video costs no timer.
- The first Album visit loads GStreamer's decoders and audio output in the
  background (`VideoBackend::warm_up`), so the first video starts as fast as
  later ones.

Frames are copied once from GStreamer's buffer into Slint's (one memcpy
when rows are packed, never into a zero-filled buffer first). While a frame
waits to be drawn, newer ones are dropped before copying.

## Writing captures (for the screenshot and recording features)

Call `HorizonAlbum::new_capture_path(game, extension, now)`. It creates the
folder and returns a name that does not exist yet. Write to
`part_path(&path)` (`<name>.part`), then rename to `path`. `.part` is not a
media extension, so a half-written file never appears. Pass the game's
`SourceGameRef` when a game is on screen, or `None` for Horizon itself.

## How it stays fast

- **Nothing on the UI thread blocks.** The scan runs on its own thread per
  visit. Decoding runs on two `MediaLoader` workers. Video runs on
  GStreamer's threads. Results return through one channel and a single
  `album-media-ready` wake-up, so an idle Album uses no CPU (no polling timer;
  only a 4 Hz clock while a video plays).
- **Demand-driven decoding.** The controller tells the loader what the screen
  needs *now*, replacing everything still queued. The open photo and its
  neighbours come first, then thumbnails for visible rows, then for the
  overscan rows.
- **Bounded memory.** Only rows near the camera are mounted (two overscan rows
  each side). Up to 96 thumbnails (480×270) and 4 full-screen photos stay
  decoded. Thumbnails are released a second after leaving the Album.
- **Thumbnail cache.** `cache/album/<key>.jpg` (and `<key>.json` with a
  video's duration), keyed by path, size and modified time, so a changed file
  gets a new thumbnail. Entries for vanished captures are pruned after each
  scan. Bump `ALBUM_CACHE_VERSION` to change the format.
- **Right-sized pixels.** Full-screen photos are decoded at the window's
  physical size, never larger. Videos are scaled by GStreamer to fit the
  window, and frames are copied once from GStreamer's buffer into Slint's.
  While a frame waits to be drawn, newer ones are dropped before copying.
- **Instant first image.** Opening or stepping shows the thumbnail
  immediately; the full-resolution photo sharpens it in place when ready.

## Video backend

`services::album::VideoBackend` is the contract and `platform::video::GstVideo`
implements it with GStreamer (`playbin`, `videoconvertscale`, `appsink`). The
runtime ships GStreamer and its codec extensions; hardware decoders are used
when GStreamer ranks them first. Without GStreamer, `NoVideo` is used: videos
still list, with a "Can't play this video" message. A zero-copy GPU path
(sharing decoder textures with Slint's renderer) is possible later behind the
same trait.

## Not yet

- Taking screenshots and recordings: that comes with the gamescope work.
- Deleting or exporting captures (Horizon's own only; never a source's), for
  example exporting to Pictures through the portal.
- Steam game recordings, HDR/AVIF captures, and captions.
