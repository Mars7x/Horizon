# ADR 0103 — Import Heroic-reported lifetime playtime

**Status:** Accepted in Phase 9.5.44.46.

## Decision

Use the existing source-neutral `LifetimePlaytime` capability and separate
`source_lifetime_playtime` table to ingest Heroic's `timestamp.json` store,
without modifying Heroic or mimicking its launch stack. The Heroic adapter
owns the store's format and native/Flatpak paths. A generic sync service
associates normalized provider IDs only with already-imported library IDs,
then caches last-written values to avoid unnecessary SQLite writes.

Missing and corrupt provider data never reset existing totals. Imported totals
are reported distinctly in Activity and are never added to observed session
times because both can refer to the same gameplay.

## Non-goals

- Heroic current-running process detection from logs, URI success or focus
- Installing or overwriting Heroic before/after scripts
- Reconstructing historical sessions from a cumulative value
- Automatic GOG/Amazon import (Legendary remains the only supported runner)

The Playing pill will only reflect provider-confirmed runtime lifecycle
when a reliable signal becomes available. No new permission is required.
