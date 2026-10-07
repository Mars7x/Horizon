# Phase 4.7 shell hardening

Phase 4.7 closes the navigation-shell phase by stress-testing the boundaries
introduced in Phases 3 and 4. It does not add library persistence or game-source
behavior.

## Hardened invariants

### Controller reconnect

Controller addition/removal is an input-layer topology change. Analog and D-pad
hold/repeat latches are reset whenever topology changes so a removed controller
cannot leave a synthetic repeat running. A remaining or newly connected
controller starts from neutral semantic navigation state.

### Rapid route changes

Rust publishes `current-route` plus exactly one `transition-from-route`. Slint
may crossfade that immediate pair only. When a second route change arrives
before the first animation finishes, the older outgoing page stops
participating immediately. Route history is still the pure Rust `Navigator` and
is unaffected by animation progress.

### Focus restoration

`RouteFocusMemory` owns route validity. Home and Library may restore either
content or top-utility focus. Every full-shell utility submenu is normalized to
content focus at the memory boundary. Temporary Home game/utility reciprocal
anchors are still dropped on route restoration.

### Resize/fullscreen/ultrawide

The design remains based on a 1280×720 reference with one uniform scale. Extra
logical width or height is consumed by responsive shell regions rather than
letterboxing or stretching individual tiles. The scale denominator is guarded
against transient zero-sized surfaces and central content height never becomes
negative.

## Manual stress matrix

Before merging a shell-affecting change, verify these cases in addition to
`./scripts/check.sh`:

1. Hold D-pad or left stick, disconnect the active controller, then reconnect it;
   focus must remain still until a new deliberate input.
2. Repeat with a second controller left connected throughout.
3. Toggle F11 repeatedly on Home, Library, and a utility submenu; no route,
   selection, or focus state may reset.
4. Resize through narrow, 16:9, 16:10, and ultrawide windows; there must be no
   letterboxing, stretched game tiles, negative content regions, or focus loss.
5. Rapidly open a utility submenu and press Back/Home before the 220 ms
   transition completes; only the active and immediate outgoing page may be
   visible.
6. Build a deep route history and unwind it with Back; routes must return in
   exact reverse order. Global Home must clear the entire history in one action.
7. Return to Home after route churn; remembered shell focus must be valid, and
   global Home must still select the first title with content focus.
