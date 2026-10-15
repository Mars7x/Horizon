# ADR 0014: Reference focus scale and controller repeat

## Status
Accepted.

## Context
The Phase 3.9 focus pass made the four corners too block-like and removed the
small scale-up that made selection feel lifted in the original visual target.
The SDL adapter also emitted only the initial D-pad press, unlike keyboard key
repeat and the already implemented analog-stick repeat.

## Decision
- The selected game card scales to 1.10 while keeping its layout slot fixed.
- The focus frame scales with the card and sits just outside the shell.
- Focus corners are thick stroked paths with strongly rounded joins and caps,
  rather than filled L-shaped blocks.
- SDL D-pad presses are tracked as held digital navigation state. Horizon owns
  the initial delay and repeat interval, just as it does for analog navigation.
- Non-directional gamepad buttons remain single-fire actions.

## Consequences
The home selection more closely resembles the rounded, lifted mockup treatment,
and holding a D-pad direction continuously navigates the same way as holding an
arrow key or analog direction.
