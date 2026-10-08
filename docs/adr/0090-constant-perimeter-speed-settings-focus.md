# ADR 0090 — Constant-perimeter-speed Settings highlight

## Context
Phase 9.5.44.32 rotates a linear gradient over the whole bounding box. On wide rectangular Settings rows, equal angular increments do not correspond to equal travel distances along the border: the bright region appears to accelerate and decelerate.

## Decision
Keep the existing 2px accent-coloured, outline-only Settings focus and the utility's rotating circular focus intact. For Settings only, use a transparent rounded-rectangle `Path` guide and Slint `Path.point-at()` to sample positions by fraction of path length. Draw a short, crisp accent-derived highlight from consecutive samples of that same guide. The continuous animation clock advances normalized distance at a constant rate, clamped to comfortable speeds across small and wide controls. Do not add a tinted background, glow, dropshadow, or movement of the control itself.

Use the original dynamic accent colour for the baseline ring and a lighter shade of the same colour for the highlight; honor Reduced Motion and High Contrast by reverting to a static accent ring.

## Consequences
Highlight motion is constant in distance along the outline, including around rounded corners, instead of constant in angle. This consumes slightly more path evaluations for focused Settings controls than a single rotating gradient. If this proves too expensive on very large rows, a renderer-level dashed-path implementation is preferable to returning to a rotating linear gradient.
