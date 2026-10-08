# ADR 0087: Continuous Settings Focus Clock

## Status
Accepted

## Context
Phase 9.5.44.29 intended to continuously pulse the Settings outline, but its `pulse-progress` animation targeted the fixed value `1.0` whenever focus remained active. The user supplied two pixel-identical screenshots taken apart in time, demonstrating no visible border change in either capture. Increasing the blend amount alone cannot repair a stationary animation clock.

## Decision
Replace the fixed-target, infinite-repeat property animation with an explicit `animation-tick()`-driven cosine oscillation. This follows the existing timing pattern used in Horizon's game-focus component. The periodic progress ranges continuously from zero to one and back over 3.6 seconds, with zero slope at the extrema.

Keep the existing accent-to-`Theme.focus-highlight` blend range of Phase 9.5.44.29. The Settings outline stays 2px and its background and shadow remain static (Settings callers already disable tinted fill and shadow). Utility rotating-highlight visuals and the game-cover four-bracket focus remain untouched.

For Reduced Motion and High Contrast, the pulse is disabled and the focus outline is static.

## Validation
The periodic formula is checked for multiple sample timestamps across a cycle and the Settings-only gating, shared component wiring, and package integrity are checked statically. Full Slint compilation and interactive rendering still require GNOME Builder testing.
