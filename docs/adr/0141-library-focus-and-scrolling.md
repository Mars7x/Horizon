# ADR 0141 — Library focus, navigation and scroll cues (Phase 10.2.4)

Status: Implemented in changed-files patch; awaits complete GNOME Builder build.

## Problem

The Phase 10.2.3 single moving focus frame created a sliding selection animation
unlike Home. The segmented Source/Sort controls diverted directional focus and
were visually heavier than necessary. Its fade made peeking cover art appear
artificial, and a bottom-only peek could make the end of the collection look
as though earlier rows did not exist. Row-edge hold handling also interrupted
continuous reading-order navigation, and text pairs reanimated unchanged values.

## Decision

1. Keep `FocusFrame` untouched and render it per visible library cover, exactly
   as Home activates a selected cover. Animate activation opacity in place,
   never `x` or `y` of the focus frame. Keep game-shell scale lift.
2. Use a single, bounded linear horizontal index across the whole filtered
   collection; held Right/Left traverse row boundaries without jumping across
   collection ends. Bounded vertical navigation retains column semantics.
3. Restrict Source/Sort to pointer activation and established LB/RB shortcuts;
   remove all Library header focus state and its cross-layer UI plumbing.
4. Track changing selected-game identity in Rust as before, but compare previous
   and current text separately inside Slint. Identical title/provider values
   resolve immediately without a new fade/position shift.
5. Use a fixed gallery viewport around the complete visible rows and allow a
   shallow clean-cropped preview before/after them when content exists. Remove
   the fade completely. A compact track/thumb next to the gallery conveys
   collection position in all states; neither previews nor progress are focusable.

## Invariants

Source-neutral Rust library ordering/launching, bounded model overscan, screen
routes, keyboard/controller Back, global Home behavior, cover artwork rendering,
Slint design tokens, and Reduced Motion remain unchanged. No Steam/Heroic
source adapters, persistence, migrations, or Flatpak permissions are affected.

## Validation

Rust Slint/Cargo toolchain is not installed in the patch creation environment.
Build and inspect in GNOME Builder with left/right holds, differing/same
metadata, first/middle/last-row cues, click/bumper source/sort, filter empties,
various aspect ratios, and reduced-motion mode.
