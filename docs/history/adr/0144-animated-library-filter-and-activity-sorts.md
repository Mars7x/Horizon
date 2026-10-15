# ADR 0144 — Animated Library source and meaningful sort modes

Status: Accepted for Phase 10.2.4.4 (pending build verification).

## Decision

Use one-way RB cycling for Alphabetical, Recently played, Time played, and
Recently added; remove Z–A. LB cycles dynamically discovered sources.
Headings never receive keyboard/controller focus. Source and Sort value text
updates independently with reduced-motion-aware 170ms crossfade/settle. The
changed gallery content receives a restrained fade-in; there is no sliding
focus frame.

The generic activity repository exposes read-only, per-GameId Library sort
metrics. A single query retrieves first-import time, last persisted session,
observed duration, and best source-reported lifetime duration. Use Steam/Heroic
reported lifetime **instead of** Horizon observation when available for
Time played, not their sum. Multiple provider lifetime totals are not summed.
A–Z and GameId break ties. Sorting never changes database contents or the
selected game's durable identity.

## Non-goals

No source adapter changes, database migration, Activity page redesign,
Flatpak permission changes, new graphical sort picker, or focus redesign.
