# ADR 0102: Anchor Playing to source-confirmed game lifecycle

## Status
Accepted — supersedes ADR 0101's transient Playing badge behavior.

## Context
ADR 0101 drew Playing beside the selected title and cleared it with temporary
handoff feedback as soon as Horizon lost focus. A real launched game therefore
lost its badge just as it took over the foreground. Successful external URI
dispatch also cannot establish that a game is truly running.

## Decision
- Keep launch handoff/press feedback temporary and release the cover after the
  existing 125 ms timer; neither transition changes the Playing badge.
- Track source `RuntimeObservationId` per game, showing Playing only after a
  `RuntimeObservationEvent::Started`. Remove its association on `Terminal`,
  including lost/failed observations. Keep independent concurrent observations.
- Track genuine managed session receipts by `ManagedSessionId` until their
  terminal completion event. The production Steam/Heroic path need not become a
  managed Gamescope session merely to display this badge.
- Store lifecycle-backed `is-playing` in each presented `GameCardData` and
  reapply it when artwork is refreshed, avoiding stale card snapshots.
- Render a compact, theme-aware pill inside the cover near its bottom edge.
  No dependence on selected title, focus, navigation, or window activation.
- Leave Heroic/external-only launch dispatch without Playing when a source
  cannot provide a verifiable runtime or managed-session lifecycle.

## Consequences
The badge stays with each game, persists through focus handoff and selection
changes, and ends on the observed runtime lifecycle. Source-unobservable games
do not display an unverified Playing status. No new permissions, process-name
heuristics, or periodic UI-only timers are introduced.