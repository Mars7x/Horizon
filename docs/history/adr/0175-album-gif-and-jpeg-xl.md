# ADR 0175: Album GIF and JPEG XL

**Status:** Implemented. The build, gate and new tests pass: GIF frames decode one at a time, an animated GIF loops and a still one doesn't, a dropped animation stops, and a lossless JXL fixture decodes by content, even renamed. Not yet seen in the running app.

## Context

The user asked for the Album to support GIF and JXL. `services::image` decoded PNG, JPEG and ICO only, through `image-rs`.

## Decision

- **GIF** uses `image`'s own `gif` feature: memory-safe Rust and no new dependency. The grid and the viewer's first picture are the first frame.
  - Animated GIFs play in the viewer, looping silently with no controls, because a GIF is a moving picture rather than a clip.
  - `media::play_animation` runs a thread that decodes each frame when it falls due (`image::gif_frames`, under the usual limits) and sends it to the viewer as video frames are sent: a generation check, and a pending frame replaced rather than queued. Decoding everything up front would hold a whole GIF in memory: a 640×360 GIF with 150 frames is about 140 MB.
  - Delays under 20 ms play at 100 ms, as browsers do. The animation stops wherever video does (`stop_video`).
- **JPEG XL** uses `jxl-oxide` 0.12 (MIT OR Apache-2.0, pure Rust), with only its `image` integration. That registers a decoding hook and content-detection signatures with `image`, once, before any read. It honours `set_limits`, so the 16K-per-side and 256 MB caps apply.
  - JXL is decoded as a still image. Animated JXL is rare and shows its first frame.
  - No system libjxl is linked.
- `MediaKind` lists `.gif` and `.jxl` as photos. The content still decides how they decode.
- `THIRD_PARTY_NOTICES.md` records jxl-oxide and its Brotli/allocator crates, and `check-third-party.sh` requires the entry. `tests/**` (Horizon-made fixtures) is now in REUSE.toml.

## Consequences

- Many JXL photos decode noticeably slower than JPEG on one thread. The viewer shows the thumbnail until the full picture arrives, as it already does.
- An animated GIF costs one decode thread while it is open in the viewer, and nothing otherwise.
