# ADR 0088 — More visible Settings outline pulse

## Status
Accepted

## Context
After Phase 9.5.44.30 restored a running Settings focus animation, the colour shift was still too subtle, particularly with vivid teal accent colours.

## Decision
Use the existing continuous, synchronized 3.6-second cosine cycle. In Settings focus mode only, blend the active 2px ring from the original `Theme.focus` accent to a substantially lighter accent (`60%` white mixed with `40%` original accent), then back. The curve has zero velocity at both endpoints, so there is no abrupt flash.

Keep the border geometry, opacity, neutral Settings interior, lack of glow/shadow, and one-time focus entrance transition unchanged. The utility indicator keeps its separate rotating highlight; the game-cover four-corner indicator is unaffected. Reduced Motion and High Contrast remain static `Theme.focus`.

## Consequences
The colour cycle is noticeably brighter at its peak for teal, orange and other custom accents. This is a uniform colour transition around the whole ring, not a white traveling streak or additional layer.
