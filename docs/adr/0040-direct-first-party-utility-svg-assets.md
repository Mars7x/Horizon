# ADR 0040: Render Horizon utility artwork directly from authored SVG assets

## Status

Accepted. This decision supersedes the utility-icon rendering portions of ADRs 0018, 0019, 0020, 0023/0024, and the related Phase 3.14 notes. It does not change the separate controller/footer icon provenance or rendering path.

## Context

The top utility row previously used third-party symbolic artwork whose geometry was transcribed into Slint `Path` elements and recolored from semantic `Theme.nav-*` tokens. That was useful for the earlier symbolic placeholders, but it is the wrong abstraction for the new Horizon-owned artwork.

Friends, Album, Activity, Web, and Settings now each have an authored 80×80 SVG supplied by the project owner. Their color, white outline, shadow/filter treatment, stroke widths, and geometry are all intentional parts of the artwork. Reconstructing them as Slint paths or using `Image.colorize` would create a second representation that can drift from the source and would defeat the requirement that the SVGs appear as authored.

## Decision

- Store the five source SVGs in `ui/assets/` as Horizon-owned project artwork.
- Preserve the supplied SVG file bytes. Renaming the repository file is permitted; editing its SVG contents is not.
- `ui/components/utility-icons.slint` provides semantic Friends/Album/Activity/Web/Settings components that all use one shared `AuthoredUtilityIcon` renderer and load the corresponding asset with `@image-url`.
- The authored SVG bytes remain the sole artwork source. Because Horizon scales the complete logical scene for fullscreen/responsive rendering, `AuthoredUtilityIcon` renders the SVG into a larger internal image target and uniformly scales that rendered image back into the fixed 40×40 logical cell. This compensates for Slint image rasterization before the app-wide scene transform without transcribing or editing the SVG.
- Render with `image-fit: contain` so the square SVG canvas keeps its aspect ratio. The internal rendering scale is a renderer-quality detail only; layout, hit testing, focus geometry, and the visible icon size remain 40×40.
- Do not use `Image.colorize`, theme tint properties, transcribed `Path` geometry, rasterized copies, or per-theme variants for these icons.
- Remove the obsolete `Theme.nav-*` utility color tokens and the old third-party utility source assets.
- Keep focus treatment separate from the artwork. The existing focus orb/ring and restrained 2 px positional lift may surround/move the complete image, but must not alter the image itself.
- Continue to use semantic theme colors for generated shell chrome. Intrinsically colored authored artwork is an explicit design-system exception rather than a general license for hard-coded UI colors.

## Consequences

There is one authoritative representation of each utility icon: the SVG asset itself. Visual changes to a utility icon must be made deliberately in that source artwork rather than duplicated in Slint geometry. Slint resolves `@image-url` at compile time; the shared authored-icon renderer may choose a larger raster target for crisp app-wide scaling, but it must not alter the asset's colors, paths, strokes, filters, or proportions.

The old GNOME Friends/Album/Activity/Web/Settings assets and their derived `utility-icons.slint` path geometry are no longer part of Horizon. `applications-games-symbolic.svg` remains a separate third-party controller/footer asset and keeps its existing attribution.
