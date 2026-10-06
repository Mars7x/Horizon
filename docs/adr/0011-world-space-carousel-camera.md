# ADR 0011: World-space home carousel camera

## Status

Accepted. Supersedes the shelf-clipping portion of ADR 0009.

## Context

The first dead-zone carousel implementation kept the rounded shelf fixed to the
screen and moved only the game row inside it. Although focus movement improved,
the visual reference behaves more like a camera moving across one continuous
home scene: the title, connector, shelf, game row, and selected focus geometry
share horizontal motion.

A fixed shelf also produced asymmetry: one rounded end could remain visible
while the opposite side appeared arbitrarily cut off, even though the games had
already moved relative to it.

## Decision

`GameCarousel` owns one world-space `scene` and one transient `camera-offset`.

The following elements live in that same scene:

- selected-game title pill;
- title connector and endpoint;
- rounded shelf/backdrop;
- game row;
- selected focus frame (through the selected `GameTile`).

The Slint component root is the camera viewport. It clips the complete scene at
the screen boundary; the shelf itself no longer clips the game row.

Selection remains Rust-owned. Slint keeps `camera-offset` because camera
position is transient presentation state.

The selected tile may move freely inside a centralized horizontal camera safe
zone. When it crosses a safe-zone boundary, the complete scene pans just enough
to restore it. Camera position is clamped so the shelf's left rounded end is
visible at the beginning and its right rounded end resolves to the same inset at
the end.

## Consequences

The backdrop and games can no longer drift relative to each other. Moving deep
into the library feels like a camera pan across one continuous scene rather than
content sliding through a stationary card. The title and connector naturally
leave/enter the viewport with the selected game.

Future carousel changes must preserve the separation between Rust-owned
selection and Slint-owned camera geometry, and must not restore shelf-local
clipping or a fixed selection anchor.
