# ADR 0096: Slightly Slower Shared Focus Rhythm

## Status
Accepted

## Context
Phase 9.5.44.38 standardized game-title, utility, and Settings focus animations around a shared 2.8-second rhythm. The result was somewhat faster than intended.

## Decision
Change only `Motion.focus-idle-cycle` from `2800ms` to `3200ms`.

The expansion distance (`2.2px`), accent-colour highlight strength (`0.80`), Gaussian game-bracket glow, focus geometry, and Reduced Motion / High Contrast behavior remain unchanged. All three focus types continue to share exactly the same idle timing.

## Consequences
The full idle cycle is 0.4 seconds longer (~14% slower) without changing the visual amplitude or introducing separate timings for Settings and utilities.
