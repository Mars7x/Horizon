# ADR 0055: Pixel art uses nearest-neighbour rendering end to end

Status: Accepted

## Context

Horizon normalizes provider-owned square artwork to a 512×512 presentation
cache and Slint may scale that image again as the Home tile changes size or the
window/device scale factor changes.

Using a smooth reconstruction filter at either stage blurs discrete pixel art
and creates colors that were not present in the authored raster.

## Decision

Horizon classifies the selected native-square provider source before
normalization.

Pixel-art-like artwork:
- uses nearest-neighbour resizing for cache normalization;
- carries a `pixelated` rendering flag through `SquareArtwork` and
  `GameCardData`;
- is rendered by Slint with `image-rendering: pixelated`, which uses
  nearest-neighbour scaling.

Non-pixel-art artwork continues to use Lanczos3 normalization and Slint's
`smooth` image rendering.

The classifier uses local pixel-transition structure and quantized color
complexity rather than source resolution alone. Sources at 128×128 and below
remain nearest-neighbour by default because smoothing is disproportionately
destructive at those icon sizes.

Cache version `square-v6` invalidates artwork normalized before this rendering
mode existed.

## Consequences

- Detected pixel art is never intentionally smoothed by either Horizon's image
  normalization or Slint's presentation scaling.
- Photographic and continuously shaded artwork retains smooth resampling.
- Classification is heuristic because Steam does not provide a canonical
  pixel-art metadata flag. Future explicit provider/user metadata may override
  the heuristic without changing the presentation contract.
