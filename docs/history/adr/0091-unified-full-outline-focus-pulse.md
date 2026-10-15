# ADR 0091: Unified full-outline focus pulse for Settings and utilities

## Status
Accepted

## Context
Phase 9.5.44.33's arc-length highlight was visibly misaligned on Settings rectangles, appearing as an extra white segment inside the accent border. The user asked to discard traveling highlights, return to a whole-outline pulse like the game-title focus animation, and standardize the utility focus animation too.

## Decision
Use the existing `SelectionFocusSurface` for Settings and utilities, with one shared 3.8-second cosine cycle matching the `FocusFrame` game's synchronized accent-lightening rhythm. Each focus ring blends from configured `Theme.focus` toward `Theme.focus-highlight` and back, all around the outline simultaneously. The game-cover focus remains unchanged, with its own bracket expansion and Gaussian glow.

Remove the previously separate `rotating-highlight` and `idle-pulse` mode switches and all sampled guide paths/extra moving strokes. The same colour animation now applies to the circular utility ring and the rounded rectangular Settings ring.

Do not change focus geometry or selection hitboxes. The Settings row remains outline-only, with neutral interior and no glow/shadow. The existing utility ring keeps its own accepted filled/shadow appearance and selection lift; these are not part of the animation. Reduced Motion and High Contrast keep a static accent-coloured outline.

## Consequences
The Settings border can no longer display a misaligned independent animated stroke. Both utility and Settings rings now follow the same full-outline colour cycle, while retaining their different appropriate geometries. A real GNOME Builder/Slint build and UI test remain necessary.
