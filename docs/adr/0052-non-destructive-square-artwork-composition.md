# ADR 0052: Non-destructive square artwork composition

Status: Superseded by ADR 0053

## Context

Phase 9.5.31 allowed a larger Steam portrait/library cover to replace a
low-resolution square icon, then center-cropped that cover to 1:1. The higher
source resolution fixed blurry icons, but the destructive crop removed authored
content near the top and bottom of portrait covers. In Home this visibly cut
game titles, faces, logos, and other composition-critical artwork.

Horizon still requires one source-neutral 1:1 presentation contract.

## Decision

`ArtworkService` keeps the Phase 9.5.31 quality selection rule, but no longer
center-crops the visible `CoverArt` source.

For a non-square cover:

1. Produce a 512×512 decorative background from a center crop of the same
   source, resize it to the square, blur it, and subdue its brightness.
2. Resize the complete original cover with an aspect-preserving contain fit.
3. Center that complete cover over the decorative background.
4. Cache the final 512×512 composition as `square-v3`.

Cropping is therefore restricted to the decorative backdrop. Every pixel edge
of the authored foreground cover remains represented.

Native `SquareIcon` candidates continue to use the existing square path.

`GameTile` renders the already-square result with non-destructive `contain`
fitting as a defensive presentation invariant.

## Consequences

- Portrait/landscape Steam library art no longer loses titles, faces, logos, or
  top/bottom/side composition.
- Horizon retains a full-bleed square visual rather than introducing plain
  black letterbox/pillarbox bars.
- The blurred backdrop is derived entirely from the same local provider image;
  it does not add a new network or licensing source.
- `square-v3` invalidates old `square-v2` cache entries so the bad crops do not
  remain visible after the code change.
- Native high-resolution square icons remain preferable and require no backdrop
  composition.
