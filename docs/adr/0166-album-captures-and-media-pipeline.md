# ADR 0166 — Album: file-backed captures and a demand-driven media pipeline

**Status:** Implemented. Compiles; tests (including real GStreamer probe and playback), clippy and fmt pass. Offscreen renders and a harness run of the real controller over real files confirm the grid, viewer, slide and filters. Video playback inside the running app and hardware decoding still need hands-on testing.

## Context

Album was a placeholder. It must show photos and videos, slide between captures, stay efficient, and be ready for Horizon's own screenshot and recording features, which will arrive with the gamescope compositor work. The user chose to include Steam's screenshots now, to play video through GStreamer, and to keep Horizon's captures in its private data folder.

## Decision

- **The file is the record.** Horizon's captures live in `data/album/<source>/<external id>/<UTC stamp>.<ext>` (or `album/horizon/`). There is no database table, so there is no migration, and nothing can fall out of step with the files. The game and time are in the path; a writer uses `.part` and renames.
- **Capture sources are separate from game sources.** `CaptureSource` (Steam first) is a read-only directory listing, `Send + Sync`, run on the Album's scan thread. It never touches library import.
- **Layers.** Domain (`domain::album`: `Capture`, `MediaKind`, `CaptureOrigin`). Services (`services::album`: catalog, thumbnail cache, `MediaLoader`, and the `VideoBackend` trait). Platform (`platform::video::GstVideo`). Presentation (`presentation::album::AlbumController`), with Slint (`ui/pages/album.slint`) only laying out and reporting its grid shape.
- **Demand-driven decoding.** Each change says what the screen needs now and replaces the queue. Display images go before thumbnails, and visible rows before overscan. Memory is bounded by small LRU caches and released after leaving. There is no polling: workers wake the UI through `upgrade_in_event_loop`.
- **Video.** GStreamer scales to the window and hands RGBA frames straight from its buffer, keeping at most one waiting. Playback is a trait object owned by the UI thread; dropping it stops the pipeline.
- **Fullscreen viewer.** At the user's request the viewer fills the window on black, with an overlay that hides after three seconds. It grows out of the selected tile and shrinks back into it. The frame animates to the capture's fitted size, so the image's cover fit equals contain at the end and nothing jumps.
- **Slide transition.** In the viewer, Left/Right slide the next capture in from that side, driven by the shared `TransitionDriver` (key = step count). Reduced Motion makes it instant.
- **Grid rules shared with Library.** Selection, vertical and horizontal stepping, and wheel camera rules move to `navigation::grid`.

## Consequences

Adding a capture source is one `CaptureSource` impl. The screenshot feature needs only `HorizonAlbum::new_capture_path` and an atomic write. Video support can be swapped (zero-copy GPU path) behind `VideoBackend`. GStreamer becomes a dependency (gstreamer-rs 0.25, against the runtime's GStreamer 1.28).

## Later additions

- **Delete.** X (`UiAction::Secondary`) deletes Horizon's own captures: multi-select in the grid, a confirm dialog in fullscreen. Source captures are never deletable here.
- **Video clock.** Each shown frame anchors the timeline at its own stream time, and Slint glides to the next frame (at most one frame gap). The first version polled the pipeline once a second and ran ahead during start-up, then snapped back; that polling is gone.

