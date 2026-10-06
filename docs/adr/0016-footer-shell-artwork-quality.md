> **Partially superseded by ADR 0020:** selected-card animation now uses one coherent visual transform; the footer, shell geometry, and artwork-quality decisions remain active.

# ADR 0016: Footer, game shell, focus geometry, and artwork quality

## Status
Accepted

## Context

The Phase 3.12 footer occupied too much vertical space, the game shell looked
lighter/thinner than the reference, and the focus brackets used a different
corner language from the shell. Selected demo covers could also appear softer
because the entire card was transformed after layout.

## Decision

- Keep the top chrome unchanged.
- Reduce the footer from 112 px to 92 px at the 1280×720 baseline.
- Align controller, separators, clock, and action hint to the footer's exact
  vertical midpoint.
- Increase the base game shell from 164 px to 176 px and increase artwork inset
  from 13 px to 18 px, producing a visibly thicker frame without shrinking the
  artwork materially.
- Use a 26 px shell radius and derive the focus elbow from the same 26-unit
  radius family.
- Keep one focus layer only, with no halo.
- Resize selected cards by animated width/height rather than `transform-scale`,
  allowing their contents to be rendered at the target size.
- For future imported artwork, prefer >=1024×1024 sources and size-aware
  derived caches. See `docs/ARTWORK.md`.

## Consequences

The home screen moves closer to the reference while keeping all geometry
centralized. Fullscreen/HiDPI artwork is not architecturally limited to a small
thumbnail, and selected-card sharpness no longer depends on scaling a
pre-rendered card layer.
