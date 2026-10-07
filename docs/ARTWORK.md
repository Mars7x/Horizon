# Artwork policy

Horizon's Phase 3 demo covers are procedural Slint geometry and are therefore
resolution-independent. Real imported game artwork arrives in a later phase,
but the rendering/cache policy is fixed now so fullscreen and HiDPI output do
not inherit low-resolution thumbnails.

## Source quality

- Prefer square cover/icon sources of at least **1024×1024** when a source
  provides multiple artwork sizes.
- Keep the highest-quality original that the source legally/localy exposes;
  do not replace it with a 164/176 px UI thumbnail in persistent storage.
- Never upscale a small source and then save the upscale as if it were the
  original.

## Utility artwork

The top Friends, Album, Activity, Web, Settings, and Shop icons are Horizon-owned authored SVGs. Their source files in `ui/assets/` are the single visual source of truth. `utility-icons.slint` loads them directly with `@image-url`; do not transcribe their paths, apply `Image.colorize`, substitute theme colors, raster-export them, or create alternate light/dark variants. The authored white outline, shadow/filter, stroke geometry, and intrinsic colors must remain intact.

The 80×80 SVG canvases are uniformly fitted into fixed 40×40 utility cells with `image-fit: contain`. Because Horizon applies one app-wide logical scene scale, the shared `AuthoredUtilityIcon` renderer uses a larger internal raster target before scaling the complete unchanged image back to its 40×40 logical size. This prevents fullscreen blur without creating alternate raster assets or changing SVG content. Layout may position the complete image and the focus system may move it by its existing small lift, but the artwork itself is never recolored, redrawn, or non-uniformly distorted. See ADR 0040.

## Application icon

`data/io.github.Mars7x.Horizon.svg` is Horizon-owned artwork supplied by the project owner. The Flatpak installs this exact SVG as the scalable application icon; desktop/metainfo identity remains `io.github.Mars7x.Horizon`.

## Rendering

- Decode/render artwork for the current presentation size and display scale.
- The UI may cache derived renditions for performance, but cache keys must
  include requested pixel size / scale so fullscreen or HiDPI does not reuse a
  low-resolution windowed thumbnail.
- Selected-game emphasis scales the complete card coherently. Because real cover
  sources are high-resolution, scaling the UI element does not justify storing or
  reusing a low-resolution thumbnail.
- Preserve aspect ratio and crop intentionally; never stretch artwork.

## Phase 3 demo art

The current `FallbackCover` is generated from vector/shape primitives. It does not
change its own font sizes or detail scale during selection. `GameTile` scales
the complete shell + artwork + focus treatment as one coherent visual unit,
which avoids the visible text/detail re-layout that occurred when those
properties animated independently. Real bitmap covers should still use
high-resolution sources so this modest UI scale-up remains crisp.


## Fallback cover rendering
Fallback covers are procedural Slint content and should remain resolution-independent. The selected tile grows by animating its actual geometry, not by transform-scaling an already-rendered fallback subtree. Placeholder text uses stable logical typography during the grow/shrink animation so it stays sharp at the selected endpoint without a visible breathing effect.


## Procedural fallback rendering density

Games without cover art use Horizon's procedural placeholder. Because the application can scale well beyond its 1280×720 design baseline, the placeholder receives the active UI scale and renders its internal Slint text/shapes at higher logical density before fitting into the game shell. This avoids treating fallback art like a low-resolution thumbnail. The visible layout remains unchanged.


Phase 3.14.10 rendering update:
- Slint renderer is now Winit + Skia instead of FemtoVG because FemtoVG scales cached glyph bitmaps and made fullscreen placeholder text visibly soft.
- Removed the high-density DemoCover workaround that caused breathing during selection.
- Superseded for top utilities by ADR 0040: Friends, Album, Activity, Web, Settings, and Shop now render Horizon-owned SVG artwork directly and unmodified.

## Phase 7 fallback artwork

The Phase 7 Home library contains real persisted games, but production source artwork is still outside this vertical slice. The earlier `DemoCover` component is therefore renamed to `FallbackCover`: it is no longer demo content, but a source-neutral procedural fallback for any real library game that has no imported artwork.

Fallback palette/monogram data is derived in presentation code from the durable game identity/title. It must not inspect `SourceId` or render Steam-specific branding. When real artwork support is introduced, source adapters/services should expose artwork through a generic boundary and the fallback should remain available for missing/broken assets.
