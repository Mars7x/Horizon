# ADR 0023: High-density fallback artwork and optical symbolic sizing

## Status
Accepted.

## Context
Horizon currently scales a responsive logical design surface. Procedural fallback game covers are made from Slint gradients, shapes, and text. When the whole surface is enlarged for fullscreen or HiDPI output, fallback artwork can appear softer than native-resolution cover art even when no selection animation is occurring. Separately, the supplied GNOME Web and Settings symbolic assets use filled silhouette geometry and look visually heavier than the outline-based Friends, Album, and Activity assets when all are given identical 28 px boxes.

## Decision
Fallback covers receive the active UI scale and render their internal procedural scene at that higher logical density before being reduced into the card slot. This is a rendering-density detail only: the visible dimensions and selection animation stay unchanged. Real imported cover art continues to use high-resolution source images and should not be upscaled from small thumbnails.

The original third-party SVG files remain unchanged. Web and Settings keep their exact derived geometry but are displayed at a 24 px optical size inside the same 40 px utility slots, while the lighter outline icons remain 28 px. This is presentation sizing, not modification of the licensed source assets.

## Consequences
Fallback monograms, platform labels, highlights, and vector shapes should remain substantially sharper when Horizon is enlarged. Utility icons have more consistent apparent stroke weight without redrawing or mutating the upstream SVG assets.
