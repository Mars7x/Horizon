# ADR 0095 — Faster, standardized focus idle motion

## Context

The game title's four-corner focus and the shared utility/Settings focus were using the same nominal 3.8-second cycle, but the movement and color changes were too subtle. Independently hard-coded periods and highlight factors could drift apart in future changes. The user wants the game's bracket expansion to be more pronounced and faster, and all other focus outlines to breathe faster and more visibly, without changing their shapes.

## Decision

Introduce a small set of shared `Motion` theme values:

- `focus-idle-cycle: 2800ms` — both game bracket and shared utility/Settings animations advance through the same continuous cosine phase.
- `focus-idle-highlight-strength: 0.80` — all selected focus strokes blend more strongly toward `Theme.focus-highlight` at the maximum phase. The configured accent returns unchanged at the minimum phase.
- `game-focus-idle-expansion: 2.2px` — increases the outward movement of the four original game title brackets (previously 1.3px) without moving the cover, shell or carousel layout.

The game-title Gaussian emission opacity is **not** increased, because the previously stronger glow was rejected. It continues to track the same phase as bracket expansion and color shift, now at the faster shared rate. The existing four bracket paths and their scale-factor handling are unchanged.

Settings retains an outline-only 2px ring, neutral interior, and no shadow or glow. Utility controls retain their circular shape, selected surface and static shadow; their *stroke color cycle* uses the exact shared period and highlight strength. No traveling path, sweeping gradient, or additional focus layer is added.

## Accessibility and verification

Reduced Motion and High Contrast continue to suppress idle color and bracket movement. The change requires a Slint build and on-device inspection for scaling and timing. No UI/layout, clipboard, SteamGridDB or controller routing logic is changed.
