# ADR 0031: Geometry-aware Home ↔ utility focus transfer

## Status

Accepted.

## Context

Phase 4.3 initially paired Home games and top utilities by proportional list
index. That was deterministic but did not match a console-style spatial focus
model once the carousel camera moved or the logical viewport became wider than
the 1280 px design baseline. It could also move Down to a different game even
when the user had only moved Up and immediately back Down.

## Decision

Vertical Home ↔ utility transfer is based on rendered horizontal centers.

- Slint reports presentation geometry only: logical viewport width, utility
  center spacing, Home game stride, and the first rendered game center after the
  carousel camera offset.
- Rust computes the nearest utility/game by center distance.
- Entering utilities from a Home game records that exact game's index as a
  temporary return anchor.
- If no horizontal utility movement occurs, Down restores the exact source game.
- Any actual Left/Right utility movement invalidates the exact return anchor;
  Down then chooses the nearest currently rendered game center.
- While utilities own focus, Home's focus brackets, selected title pill,
  connector line, and connector dot are hidden.

## Consequences

Focus behavior remains controller/keyboard agnostic and Rust-owned while still
following the geometry the user actually sees. The solution also remains valid
when the carousel camera pans or an ultrawide viewport changes the screen-space
relationship between games and the centered utility strip.
