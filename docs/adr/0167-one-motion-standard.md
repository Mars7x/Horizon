# ADR 0167: One motion standard

**Status:** Implemented. Builds. Offscreen renders of every page and dialog, and of Library's re-sort frame by frame, are pixel-identical to before at rest and through the re-sort.

## Context

An audit of every animation found a dozen literal durations beside the `Motion` tokens:
- three hand-built dialogs at 170–200 ms, with different rises and easings;
- a toggle whose track and knob ran at different speeds;
- a 260 ms swatch fade;
- 130 and 165 ms text-field timings;
- progress bars on carousel timing;
- marquee constants duplicated in two components.

Easing varied for the same kind of change.

Library and Achievements also read `animation-tick()` in bindings that stay evaluated at rest. Library's title swap and re-sort, and Achievements' arriving rows, kept the window redrawing every frame after their animation had ended.

Home's launch release used `Timer.restart()`, which does nothing on a stopped timer. After the first launch, every carousel move kept the release spring.

## Decision

- **Tokens by meaning:** hover, focus, value swap, carousel, page, dialog, scroll and status. Plus loop and marquee tokens.
- **Easing by kind:**
  - moves and resizes: ease-out;
  - fades and colours: ease-in-out;
  - cameras and timelines: linear;
  - one spring (Home's launch release).
- **`ModalDialog`:** one component for every modal.
- **`Stopwatch`:** elapsed time for choreography. It reads `animation-tick()` only while running.
- **One grid refresh** (`GridRefresh`) for every in-place grid change. It is Library's former Source animation; the separate Sort animation is gone, and Album's Game and Type filters gain it (the user chose this).
- Fix the release timer (`stop()` + `start()`).

See [UI standards: Motion](../UI_STANDARDS.md#motion).

## Consequences

Pages at rest no longer redraw because of finished choreography. The remaining continuous redraws are deliberate: focus breathing, the caret, the spinner and a playing video's clock. Under Reduced Motion and high contrast, a page at rest draws nothing.
