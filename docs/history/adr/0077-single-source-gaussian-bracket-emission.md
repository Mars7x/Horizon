# ADR 0077: Single-source Gaussian emission for the four focus brackets

## Status
Experimental — requires GNOME Builder / Skia runtime validation.

## Context
Phase 9.5.44.19 placed a second, 2px-wider native stroke underneath each crisp bracket. At the scale of Horizon’s Home menu, the widening is visibly read as another outline rather than a glow. The supplied Nintendo 3DS cursor reference shows one well-defined coloured bracket with softer emission close to its edges and gentle size/brightness modulation.

## Decision
Remove all duplicate solid vector strokes. Retain the four original Slint `Path` shapes, commands, widths, positions and dynamic theme-accent colouring unchanged. Derive each glow strictly from a copy of the corresponding original SVG path *rasterized with Gaussian blur* using the native SVG decoder's `feGaussianBlur` filter; no PNG assets and no manually approximated second geometry. The paths are in the same coordinate space (58 units) and each mask has 12-unit transparent padding. In Slint, the filtered mask is an `Image` directly below the one crisp `Path`, dynamically tinted using `Image.colorize: Theme.focus`.

Glow follows the original bracket expansion and is visible at all four corners together: opacity 0.23 at rest → 0.39 at maximum 1.3px expansion, on the existing 3.8-second cycle. The original crisp corner strokes retain their own gentle brightness change. The shell/artwork never move. Reduced Motion, High Contrast, and inactive focus disable the emission as before.

## Compatibility and validation
This uses standard SVG Gaussian blur filters supported by `resvg`, plus Slint 1.18 image tinting. It introduces **no new dependencies or X11 functionality**. Because `slint-build` and the Skia runtime are unavailable in the preparation environment, actual rendering of the SVG filter must be checked in GNOME Builder. If the platform SVG decoder strips filters, revert this experimental visual patch rather than reinstating the hard-edged duplicate strokes. No bundled third-party imagery is introduced; the masks are first-party copies of Horizon's existing focus geometry.
