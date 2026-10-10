# ADR 0165 — Zoom transition everywhere

**Status:** Implemented. Compiles; tests, clippy and fmt pass. Offscreen renders confirm the fade timing; the scale half needs the real (Skia) renderer, since Slint's software renderer leaves `transform-scale` unimplemented.

## Decision

After trying Rise (ADR 0164) and comparing it with a Zoom preview, the user chose Zoom for every page and sub-page change. Forward, the incoming layer grows in from 0.94 while the outgoing one grows past to 1.06 and fades; Back mirrors it. `TransitionLayer` in `ui/components/transition.slint` is now Zoom-only (Rise's travel, on-top swapping and dimming are removed), timed by `Motion.page-duration`. Home's top bar and footer zoom and fade with Home around the window centre. Activity details zoom over the overview, whose content is covered as details fade in (the separate dimming scrim is gone).

Prewarming routes and deriving the phase from the shown key (ADR 0164) are unchanged.
