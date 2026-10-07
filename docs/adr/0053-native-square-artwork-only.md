# ADR 0053: Native square artwork only

Status: Accepted

## Context

Horizon requires primary game artwork to be 1:1.

Phase 9.5.31 tried to improve low-resolution Steam icons by allowing larger
portrait/library artwork to replace them. Phase 9.5.35 stopped destructive
cropping by fitting those covers into a square composition, but the underlying
provider artwork was still not 1:1.

That violates the intended artwork contract: Horizon should use actual square
game artwork, not manufacture a square from a different aspect ratio.

## Decision

Primary game artwork must be natively 1:1 at the provider source.

- `SourceArtworkCandidate` represents square/icon-oriented candidates only.
- `ArtworkService` validates decoded dimensions and rejects every candidate for
  which `width != height`.
- Steam no longer exposes library portrait/capsule artwork to the primary
  artwork pipeline.
- Among valid square candidates, the highest native resolution wins.
- Horizon may resize a square to its canonical 512×512 cache representation;
  resizing does not alter aspect ratio.
- When no valid square candidate exists, presentation uses `FallbackCover`.
- Cache version `square-v4` invalidates earlier normalized results that may have
  originated from non-square provider assets.

## Consequences

Some games may show a lower-resolution square icon even when Steam has a much
larger portrait cover. This is intentional. Source fidelity and the 1:1
contract take precedence over substituting a different class of artwork.

Future providers must expose genuine 1:1 artwork if they want to participate in
the primary artwork capability.
