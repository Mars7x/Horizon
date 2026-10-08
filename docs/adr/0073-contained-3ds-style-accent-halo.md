# ADR 0073 — Contained 3DS-style accent halo

Status: visual refinement (Horizon Phase 9.5.44.16, following 9.5.44.15).

## Problem

The original Phase 9.5.44.14 corner glow was too broad and bright. It made the four accent-coloured focus brackets look like neon lamps against the dark background. The provided Nintendo 3DS HOME Menu reference looks more like a soft luminosity closely following the cursor corners.

## Decision

- Preserve the four exact `FocusFrame` corner paths, their stroke widths, rounded caps, dynamic `Theme.focus` accent brush, and 1.3 px / 3.8 s synchronized expand/contract movement. The game artwork and shell remain still.
- Continue drawing one soft alpha-mask image **behind** each existing crisp bracket, all four simultaneously and at constant strength. Never create a second hard outline or move light sequentially between corners.
- Regenerate the project's four first-party RGBA masks from the same original path commands using a **3.0 logical pixel Gaussian radius**, replacing the earlier 5.0 radius. Keep 18 viewbox units of transparent padding to prevent clipping and the same 470×470 output size so no positioning changes are necessary.
- Reduce the runtime mask opacity from **0.58 to 0.36**. The result should be a tighter, subtler accent-coloured halo, not a large haze.
- Respect inactive focus, Reduced Motion, and High Contrast exactly as before: suppress the blur and render the sharp accent brackets.

## Asset provenance and tests

`ui/assets/focus/{top-left,top-right,bottom-left,bottom-right}.png` are first-party outputs generated from Horizon's existing path geometry with CairoSVG and Pillow at 5× raster scale. These tools are not required by the application at runtime. The masks are dynamically recoloured via Slint `Image.colorize: Theme.focus` and do not contain Nintendo or third-party art.

Validate on native GNOME Wayland with dark and light themes, varied accent colours, custom scaling, controller navigation, and reduced-motion/high-contrast modes. This visual patch has passed static path/mask/archive checks but has not been compiled or visually verified in a live Slint build.
