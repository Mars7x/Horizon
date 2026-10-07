# ADR 0050: Quality-aware square artwork and normalized cache

## Status

Accepted for Phase 9.5.31.

## Context

Phase 9.5.29 proved the generic 1:1 artwork boundary, but selecting only the
largest square icon still produced visibly poor Home tiles for games whose best
local icon was small. Steam may simultaneously cache substantially larger
library portrait artwork, and newer clients may store that artwork below a
content-hash directory under the AppID.

Repeatedly opening provider ZIP/ICO/cache candidates on every Horizon startup
also makes artwork work proportional to library size even when nothing changed.

## Decision

`SourceArtworkCandidate` distinguishes two provider-owned intents:

- `SquareIcon`: intended square iconography;
- `CoverArt`: larger artwork that may be center-cropped by Horizon.

`ArtworkService` remains the only layer that decides candidate quality and
presentation normalization.

A square source at least 384 px wide/high is preferred. Below that threshold,
a larger cover candidate may replace it when its shortest side contains more
usable source pixels. Selection always happens before normalization.

The chosen source becomes one canonical 512×512 RGBA image. Square icons are
preserved whole; cover art receives a standardized centered square crop. The
result is persisted as a PNG in Horizon's XDG cache. Fresh cache entries bypass
provider scanning/decoding, while stale entries are refreshed opportunistically
and remain usable if a provider temporarily evicts its own artwork.

Steam exposes local `library_600x900` / `library_capsule` candidates in both
legacy and one-level content-hash cache layouts in addition to the established
icon candidates. No runtime network permission is added for this phase.

## Consequences

- Home receives consistent 1:1 512×512 artwork.
- Low-resolution Steam icons no longer automatically beat larger official
  library artwork.
- High-quality 384–512 px native square icons remain preferred and are not
  unnecessarily replaced by cropped portraits.
- Normal startup can decode one known Horizon cache PNG instead of repeatedly
  probing Steam ZIP/ICO/library candidates.
- Center cropping can lose content near the top/bottom/sides of provider cover
  artwork. The crop is deliberately standardized and can later be replaced by
  an explicit focal-point policy without leaking provider logic into Slint.
- The cache is disposable and must never become durable game identity/state.
