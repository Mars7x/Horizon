# ADR 0101: Release the game cover and separate Playing from the title

## Status
Accepted

## Context
The Home cover stayed visually pressed while awaiting foreground handoff, and `Launching…` squeezed the selected game title into a two-line label. The selected cover should spring back and the accepted launch should have a distinct `Playing` pill.

## Decision
* The 90 ms press-in is still rendered using real Slint geometry; a one-shot 125 ms release clears only the pressed visual, independently from the launch handoff state.
* The cover releases over 230 ms with Slint's `ease-out-back` spring-like easing. A temporary release flag restores the original focus-duration/easing to ordinary selection navigation. Reduced Motion disables interpolation.
* A successful source dispatch/managed-session receipt changes the transient Home presentation to `Playing` and shows a separate compact accent pill adjacent to the unchanged game title. No `Launching…` string is displayed. Failed dispatch still shows `Launch failed` and suppresses the Playing badge.
* Handoff remains protected against duplicate Accept and carousel movement until focus transfers; session completion and loss of window activation clear the transient badge. Stale press-release timers are guarded by a monotonically increasing generation.
* `Playing` is user-facing **launch dispatch feedback**, not proof of process liveness on all source adapters. Real playtime/lifecycle remains the responsibility of Activity and runtime observation.
* Game-title marquee, focus brackets, artwork, and the standard 3.2-second idle focus rhythm remain unchanged.

## Consequences
Users see a visible press and spring-back rather than a card pinned to its compressed state. Launch status no longer changes title-pill geometry.
