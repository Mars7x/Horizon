# ADR 0068 — 3DS-inspired focused-game idle float and accent aura

Status: Experimental visual choice for Horizon Phase 9.5.44.11.

## Motivation

Satin Drift on the focus brackets did not communicate the gentle floating character the user wants. Instead, trial a restrained console-like idle motion and ambient accent glow while preserving the existing corner indicator.

## Implementation

- Preserve all four original `FocusFrame` path commands, dimensions, positions, stroke widths, and rounded caps/joins. Remove Satin Drift's moving stroke gradient; the main brackets are solid `Theme.focus` at all times.
- Under the unchanged brackets, render two low-opacity copies of each existing path with slightly thicker strokes. These match the bracket geometry and serve as a soft, local accent-coloured aura rather than drawing a continuous rectangular outline. Opacity changes smoothly in a 5.4-second cycle independent from the floating motion; don't add white, colour shifts, or third-party artwork.
- Apply a small vertical displacement (±1.4 Slint logical pixels over 3.9 seconds) to the `visual` parent of **both** the game cover and the focus frame in `GameTile`. Never change the game slot or carousel camera geometry, and never move the indicator independently of its game cover. The existing selected-scale and launch press effects remain intact.
- Ease the floating displacement strength in/out over 280 ms when focus changes; disable the effect while launching.
- Reduced Motion and High Contrast use a fully static, solid-accent frame, with zero idle float and no halo. When utility focus owns navigation, the selected-game frame remains hidden as before.

## Verification

Inspect selected game in dark/light theme and customized accent; check title, connector, camera, selected scaling, and game launch remain aligned. Move quickly between cards and utilities, disconnect/reconnect controller, toggle Reduced Motion and High Contrast. The glow should be restrained, with no independent bracket drift or jitter. Run a Slint/Rust build in GNOME Builder; static validation cannot establish successful compilation.

No external art, libraries, network permissions, or X11 paths are added.
