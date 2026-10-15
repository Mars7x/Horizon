# Album

> **Current scope (ADR 0191):** Album reads Horizon-owned captures only. Steam screenshot discovery, viewing, thumbnail access and deletion are removed, along with all Steam filesystem grants. No existing screenshot files are deleted. Archived identities still name Horizon-owned game captures.


The Album shows Horizon's own screenshots and video clips, newest first. See [ADR 0166](../adr/0166-album-captures-and-media-pipeline.md).

## Using it

| Where | Input | Does |
| --- | --- | --- |
| Grid | D-pad / stick | Move between captures (reading order; no wrap) |
| Grid | A, or click any tile | Open the capture full-screen (a click opens it straight away) |
| Grid | X | Start selecting: A (or a click) ticks captures, X asks to delete the ticked ones, B finishes |
| Grid | LB / click "Game" | Next game filter: All games, then each game with captures, most recent first |
| Grid | RB / click "Type" | All → Screenshots → Videos |
| Grid | Mouse wheel | Moves the camera only, never the selection |
| Viewer | Left / Right, click the screen edges, or swipe sideways | Previous / next capture, sliding in from that side; stops at either end |
| Viewer (video) | A, or click the video | Play / pause; at the end, A plays it again |
| Viewer (video) | LB / RB | Back / forward 10 seconds |
| Viewer | X | Asks to delete this capture (Cancel is focused first) |
| Viewer (video) | Click the timeline | Seek |
| Viewer | B | Back to the grid, on the capture that was open |

Changing Game or Type runs the shared grid refresh (as Library's Source and
Sort do): captures in both grids glide to their new places, the rest fade out
or in (ADR 0172).

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
- The generic `CaptureSource` boundary remains available, but no production
  capture sources are registered. No Steam installation is scanned.

Supported formats are listed by extension: PNG, JPEG, GIF and JPEG XL (`.jxl`)
photos; MP4, M4V, MKV, WebM and MOV videos (whatever the runtime's FFmpeg can
decode). Photos are then decoded by their content, whatever the name says
(`services::image`).

- **Animated GIFs** play in the viewer, looping silently with no controls, in
  place of their first frame (which is the grid thumbnail). A small thread
  decodes each frame as it falls due, at the file's own pace (delays under
  20 ms play at 100 ms, as browsers do), so memory stays at a frame or two
  however long the GIF is. It stops whenever the video would: another capture,
  closing the viewer, leaving the page.
- **JPEG XL** decodes through `jxl-oxide` (ADR 0175), as a still image; an
  animated JXL shows its first frame.

## Deleting

Horizon-owned captures are deleted after the confirmation dialog.
`HorizonAlbum::delete` refuses paths outside Horizon's album folder, removes
the selected files and tidies empty game folders. Deletion failures are
reported in the bottom bar. Steam screenshots are neither listed nor deleted.

## Video timeline

The timeline is a steady clock running at real-time speed, so the bar moves
perfectly evenly even when frames reach the screen unevenly (start-up
bursts, a busy frame). Each shown frame carries its stream time and length
from the decoder. The frames correct the clock only when it drifts more than
two frames from the picture, or on a still (start, pause, seek). The clock
never runs more than one and a half frames past the frame on screen, so a
stall, or the start-up while audio opens, holds the bar instead of running
ahead.

- A seek anchors at its target at once. Frames decoded before the seek
  (`seek_epoch`) never move the timeline back.
- At the end, the clock rests on the duration.
- Nothing polls the pipeline: an idle or paused video costs no timer.
- **Starting: still, then smooth.** Sound reaches your ears after the
  output's latency (about 250 ms over Bluetooth, a few ms on speakers).
  Pressing play, seeking or resuming shows the still picture and holds it
  until the sound there is heard, then the picture runs and keeps running:
  it never starts, stops and starts again, and no sound is skipped (the
  user's choice). If no sound arrives within a second, the picture starts
  anyway. From then on the sound steers the clock by at most 4 ms per frame,
  which absorbs a latency estimate that settles after start-up without a
  visible jump. Measured over Bluetooth: the still picture after 83-100 ms,
  motion about 230 ms later, then a frame every 31-33 ms with none dropped.

## Video player

`services::album::VideoBackend` is the contract and
`platform::video::FfmpegVideo` implements it (ADR 0169):

- **Decoding:** FFmpeg, through Horizon's small C shim (`csrc/horizon_media.c`)
  compiled against the SDK's headers. The runtime ships FFmpeg, with the full
  codec set from its `codecs-extra` extension, so nothing is bundled. Frames
  come out scaled to fit the window as packed RGBA (bilinear), and decoding
  uses every core.
- **Sound:** a native PipeWire stream (`platform::pipewire`, `csrc/horizon_audio.c`),
  48 kHz stereo, fed straight from a half-second queue on PipeWire's
  real-time thread.
- **Threads:** each video has a decoder thread (opens the file, decodes in
  file order, a few frames and half a second of sound ahead) and a presenter
  thread (shows each frame on time). The UI only sends requests: opening,
  pausing and seeking never wait on the disk or the audio server.
- **Copies:** one copy from FFmpeg into a reused frame buffer, one into
  Slint's (a single memcpy). While a frame waits to be drawn, newer ones are
  dropped before copying.
- A zero-copy GPU path (hardware decoding into textures Slint draws) is
  possible later behind the same trait.

## Writing captures (for the screenshot and recording features)

Call `HorizonAlbum::new_capture_path(game, extension, now)`. It creates the
folder and returns a name that does not exist yet. Write to
`part_path(&path)` (`<name>.part`), then rename to `path`. `.part` is not a
media extension, so a half-written file never appears. Pass the game's
`SourceGameRef` when a game is on screen, or `None` for Horizon itself.

## How it stays fast

- **Nothing on the UI thread blocks.** The scan runs on its own thread per
  visit. Decoding runs on two `MediaLoader` workers. Each video has its own
  decoder and presenter threads. Results return through one `services::worker`
  channel that wakes the UI thread once per burst, so an idle Album uses no
  CPU (no polling timer).
- **Demand-driven decoding.** The controller tells the loader what the screen
  needs *now*, replacing everything still queued. The open photo and its
  neighbours come first, then thumbnails for visible rows, then for the
  overscan rows.
- **Bounded memory.** Only rows near the camera are mounted (two overscan rows
  each side). Scrolling shifts that window in place (rows that leave are
  removed, rows that arrive added), so the tiles that stay keep their
  elements and the focus frame and lift still animate; only a grid refresh
  rebuilds every tile. Up to 96 thumbnails (480×270) and 4 full-screen photos stay
  decoded. Thumbnails are released a second after leaving the Album.
- **Thumbnail cache.** `cache/album/<key>.jpg` (and `<key>.json` with a
  video's duration), keyed by path, size and modified time, so a changed file
  gets a new thumbnail. Entries for vanished captures are pruned after each
  scan. Bump `ALBUM_CACHE_VERSION` to change the format.
- **Right-sized pixels.** Full-screen photos are decoded at the window's
  physical size, never larger. Videos are scaled by FFmpeg to fit the
  window.
- **Instant first image.** Opening or stepping shows the thumbnail
  immediately; the full-resolution photo sharpens it in place when ready.

## Not yet

- Taking screenshots and recordings: that comes with the gamescope work.
- HDR/AVIF captures and captions. Steam media integration is retired.
