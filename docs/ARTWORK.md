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

Canonical artwork is cached in `cache/artwork/local/<game-key>.png` (see
[Files on disk](STORAGE.md)), with a `<game-key>.filter` file recording whether
it is drawn as pixel art. The folder's `.version` is `LOCAL_ARTWORK_CACHE_VERSION`.

Version `square-v6` invalidated the earlier cache so artwork is rebuilt with the
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

## Phases 9.5.40–9.5.43 — SteamGridDB square artwork

The original `ArtworkService` keeps source-owned local art and its existing
`square-v6` normalization path. SteamGridDB uses its own
`cache/artwork/steamgriddb/` folder; it never changes source files or the source
normalization cache. The API matches Steam games via exact AppID first. If
SteamGridDB has no mapping for the AppID, it can use a unique exact title
match; Heroic and Bottles likewise use unique exact titles. Fuzzy or
ambiguous matches are never accepted.

Downloads run in a background worker. The API supplies eligible static PNG/JPEG
square-grid URLs; the downloader independently verifies HTTPS and the
SteamGridDB host suffix, disables redirects, bounds response bytes, then
validates actual decoded dimensions and image format. Grids must be native
512×512 or 1024×1024 squares. If no eligible native-square grid is available,
SteamGridDB square PNG icons at 256/512/768/1024px are tried before
falling back to existing source artwork; portrait/landscape art is never
cropped into a cover. Downloaded sources run through
`normalize_remote_square`, the *same* quality-aware, pixel-art-preserving
normalization as local source candidates, and retain the presentation bit for
Slint pixelated/smooth scaling. A normalized 512×512 PNG is cached with a JSON
sidecar recording game ID, grid asset ID, author, source URL, matching method,
and normalized-pixel checksum. Corrupt cache pairs are ignored.

Artwork priority:

- Default/Off: if a local source-owned square exists, display it; otherwise
  try SteamGridDB; then use the generated placeholder.
- Prefer SteamGridDB/On: display existing source art while SteamGridDB is
  resolving; replace it with eligible/downloaded (or cached) SteamGridDB art
  once ready, or keep original source art if requests fail.
- If the API key is absent: no SteamGridDB requests or cached results are
  applied. Removing the key restores local source art immediately.

The network worker does not own or manipulate Slint handles. A presentation
thread timer polls bounded `mpsc` results; generation IDs reject stale replies
after a setting change. Network failures do not interrupt Home navigation.

SteamGridDB assets remain owned/licensed by their respective uploaders or rights
holders. Attribution provenance lives with the cached asset and is not a grant
to redistribute images. Horizon does not bundle downloaded artwork. For
operational details see [SETTINGS.md](SETTINGS.md).

### Phase 9.5.44.18 — Retry behavior

Cached negatives from earlier versions are intentionally ignored (`.miss-v3`
replaces `.miss-v2`), so a fresh lookup runs after updating. New unmatched or
missing-square responses are throttled for five minutes, not 24 hours, and
reported separately from successful cached/downloaded artwork. Actual API
functionality with a real user key must be verified on a running build.

### Phase 9.5.44.36 — Correct SteamGridDB API paths

The shared API base ends in `/api/v2/`. Appending new path segments to that
URL without removing its trailing empty segment mistakenly sent requests to
`/api/v2//games/steam/<id>`, `/api/v2//search/...`, and `/api/v2//grids/...`.
The SteamGridDB server may return 404 for these incorrect paths, which was
misclassified as a missing game or missing art. The request builder now removes
the empty path segment before appending endpoint paths. Tests assert exact
paths for Steam platform IDs, title search, grids, and icons.

The cache reader now looks for `.miss-v5` markers, ignoring `.miss-v4` and earlier
negative entries produced by the faulty URL builder. Existing successful
positive caches remain valid and source-owned artwork is never overwritten.
Unmatched games and absent eligible square artwork still receive short-lived
five-minute negative markers, but only after a correctly formed request.

To verify the fix, rebuild in GNOME Builder and check that SteamGridDB's
Third-Party status shows matches and downloaded/cached artwork. If matching
works but downloads fail, the stage-specific diagnostics remain available.

### Phase 9.5.44.37 — Highest-score square grids first

SteamGridDB v2 supplies an integer `score` for grid and icon metadata. Horizon
now sorts all eligible HTTPS square-grid candidates from the first API response
page by descending SteamGridDB score. It downloads the highest-ranked valid
grid; if an image is invalid, it tries the next score-ranked candidate, up to
eight attempts. Equal scores retain API response order; absent scores rank
last. Native image resolution does not outrank vote score, although the same
512×512 normalization and pixel-art safety rules still apply.

Only when no square grid can be downloaded does Horizon try square icons,
which are also score-ranked. This preserves the user's preference for square
grids over icons. SteamGridDB votes do **not** certify that a community upload
is official publisher artwork.

Cache provenance schema 2 now records the selected image score. Earlier cached
artwork is re-evaluated automatically against the new selection policy rather
than continuing to show previously selected lower-ranked assets. Old negative
markers (`.miss-v5` and earlier) are ignored; genuine new misses use `.miss-v6`.
Existing source files remain untouched. The saved SteamGridDB preference still
controls source art versus SteamGridDB priority exactly as before.
