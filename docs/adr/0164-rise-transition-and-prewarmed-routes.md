# ADR 0164 — "Rise" replaces crossfade; routes prewarm before animating

**Status:** Implemented. Compiles; tests, clippy and fmt pass. Checked with offscreen frame-by-frame renders, including a simulated 300ms first frame; hands-on testing pending.

## Problem

Every page and sub-page change was a crossfade with a 12px settle, which the user grew tired of. Separately, opening Library on a fresh start sometimes lagged and showed no transition: the route change and the first frame of the incoming page (uploading every cover to the GPU for the first time) happened together, so by the next frame the animation's time had already elapsed and it jumped to the end.

## Decision

- One transition everywhere, chosen by the user from a side-by-side preview: **Rise**. Forward, the new layer rises from below over the current one, which dims to 60%; Back, the top layer drops away as the one below brightens. Implemented once as `TransitionDriver` + `TransitionLayer` (`ui/components/transition.slint`) and used for routes, Settings sections, a game's achievements and Activity details. Section headers stay still.
- Route transitions **prewarm**: the incoming page is drawn once at near-zero opacity, and the animation starts only on the following frame. A slow first frame now delays the transition instead of skipping it. Home's chrome and the hint bar follow the same phases.
- Direction is presentational and derived in Slint (returning to Home, or to a shallower Settings depth, is Back); route history stays in Rust.

## Guardrails

Reduced Motion makes Rise instant. Small in-place value swaps (header values, Library re-sort) keep their own short animations; they are not page transitions. Supersedes the crossfade parts of ADR 0160 and ADR 0163.
