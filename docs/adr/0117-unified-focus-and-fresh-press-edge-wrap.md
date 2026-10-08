# ADR 0117 — Unified focus treatment and fresh-press-only edge wrap

Status: accepted for Phase 9.5.44.60, pending build and visual validation.

## Decisions

1. Every menu surface (Settings rows, Appearance Theme/System choices, accent
   swatches, editor actions, and header utilities) uses one reusable Slint
   focus outline: `SelectionFocusSurface`, driven by `Theme.focus` and
   `Motion.focus-duration` (140 ms, or 0 ms with Reduced Motion). There is
   no second focused border or focus halo. The Home game cover keeps its
   authored single-layer focus brackets, but shares the same accent, motion
   duration and reduced-motion policy. Different control shapes may require
   matching rounded-rectangle or circular corners; the stroke behavior is
   uniform. Selected preferences are NOT keyboard/controller focus.
2. Semantic directional events already distinguish fresh and repeated input.
   One shared Rust rule, `navigation::step_with_edge_wrap`, permits wrap ONLY
   on a fresh event received WHILE ALREADY at an end. A repeat holds/clamps at
   the edge even after arriving there during that hold. The opposite direction
   behaves symmetrically. One-item rows stay put.
3. Home, Settings root, Third-Party action rows, Appearance's Theme/Accent
   rows, and the top utility strip all use this rule. Appearance's cross-row
   spatial Up/Down transitions remain intact; its Theme-top-to-UI-Sounds and
   UI-Sounds-bottom-to-Theme wrap is fresh-only. Modal editor grids keep their
   finite spatial adjacency and do not invent a wrap through unrelated actions.
4. SDL digital/analog adapters and Slint keyboard events preserve the repeated
   bit. No added input timer or Slint-owned navigation policy. Focus navigation
   remains separate from activating a choice, and only actual movement produces
   the existing navigation sound.

## Consequences

Prior Phase 4 header clamping invariant is superseded *only for a fresh press
at the boundary*. Phase 9.5.44.59 Appearance's unconditional wrap is superseded.
The existing visual Home carousel/carousel camera and game-card spring behavior
are unchanged. Reduced Motion and High Contrast continue to take priority.

## Verification

Rust unit tests exercise shared edge semantics, header wrapping and Appearance
boundaries; run them with `cargo test` in the user's build environment. Also
visually test actual keyboard, D-pad and analog held repeat and rapid releases,
and check no doubled focus outlines in both themes.
