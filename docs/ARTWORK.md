# Artwork policy

Horizon primary game artwork is **native 1:1 only**.

The 1:1 requirement applies to the provider source asset itself, not merely to
the final presentation buffer. Horizon does not crop, pad, letterbox, blur-fill,
or otherwise convert portrait/landscape artwork into a square game tile.

## Phase 9.5.38 square-only artwork pipeline

```text
GameSource
  └─ artwork_candidates(ExternalGameId)
          ↓
    provider-owned square candidates only
          ↓
      ArtworkService
        ├─ decode
        ├─ reject width != height
        ├─ choose highest-resolution true square
        ├─ resize only (aspect ratio unchanged)
        └─ cache 512×512 PNG
          ↓
       GameCardData
          ↓
         Slint
```

The source adapter may identify files that are expected to be square, but
`ArtworkService` validates the decoded dimensions. A mislabeled 256×255,
600×900, or any other non-1:1 image is rejected.

If no valid square artwork exists, Horizon uses `FallbackCover`. It does not use
a non-square image as a quality fallback.

## Steam

Steam is the first provider with artwork support. Horizon considers only local
square/icon-oriented Steam assets:

- the largest valid square PNG representation inside
  `steam/games/<linuxclienticon-hash>.zip`;
- the largest decodable square frame inside
  `steam/games/<clienticon-hash>.ico`;
- hashed App Icon files under `appcache/librarycache/<appid>/`;
- legacy `<appid>_icon.jpg` / `<appid>_icon.png`;
- per-AppID `icon.jpg` / `icon.png`.

Steam `library_600x900`, `library_capsule`, hero, header, and other non-square
library artwork are not candidates for Horizon primary game art.

Steam container formats are resolved explicitly before they enter the generic
artwork service. Horizon enumerates all usable PNG entries in a Linux icon ZIP
and all frames in a client ICO, rejects non-square representations, and keeps
the largest true-square representation from each container. For ICO frames at
the same resolution, higher color depth wins.

The generic artwork service then chooses the largest true-square candidate
across those container results and ordinary App Icon files. Horizon may resize
that square to its canonical 512×512 cache buffer, but it never changes the
source aspect ratio.

This means a game with only a 184×184 square Steam icon will use that square
icon rather than replacing it with a sharper portrait cover. If there is no
usable square asset at all, Horizon uses its procedural fallback.

## Persistent cache

Canonical artwork is cached under:

```text
$XDG_CACHE_HOME/horizon/artwork/square-v6/<game-key>.png
```

or, when `XDG_CACHE_HOME` is unavailable:

```text
$HOME/.cache/horizon/artwork/square-v6/<game-key>.png
```

`square-v6` invalidates the earlier cache so artwork is rebuilt with the
pixel-art rendering classification introduced in 9.5.38. A cached image created
with smooth interpolation must never keep a pixel-art source blurred after the
policy changes.

The cache is disposable presentation data and is refreshed from provider-owned
square sources. No additional network permission is required.

## Rendering

`GameTile` receives an already-square image and uses non-destructive fitting.
Presentation must not introduce cropping.

## Provenance

Horizon does not bundle or redistribute Steam game artwork. It displays and
locally caches artwork already present in the user's Steam installation. The
underlying artwork remains the property of its respective publisher/developer.


## Pixel-art rendering

Pixel art must not be smoothed.

`ArtworkService` classifies the selected true-square source before
normalization. Pixel-art-like sources use nearest-neighbour scaling into the
canonical 512×512 cache. Smooth/photographic artwork continues to use Lanczos3.

The classification looks for discrete raster characteristics: hard or flat
neighbor transitions, comparatively few soft gradient transitions, and bounded
quantized color complexity. Very small square sources (128×128 or below) stay
on the nearest-neighbour path by default.

The classification bit is carried into `GameCardData`. Slint then sets the
`Image.image-rendering` property to `pixelated` for those cards, so later
fullscreen, HiDPI, selection-size, or fractional UI scaling cannot reintroduce
linear interpolation. Slint documents `pixelated` image rendering as
nearest-neighbour scaling.

This is intentionally a two-stage invariant:

```text
provider square source
       ↓
pixel-art classification
       ↓
pixel art ── nearest-neighbour cache normalization
normal art ─ Lanczos3 cache normalization
       ↓
GameCardData.pixelated-artwork
       ↓
Slint image-rendering:
pixelated OR smooth
```
