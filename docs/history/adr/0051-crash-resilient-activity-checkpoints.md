# ADR 0051: Crash-resilient Activity checkpoints

## Status

Accepted for Phase 9.5.34.

## Context

A `play_sessions` row is opened as soon as Horizon has a reliable session-start
signal and normally receives its final `ended_at` from the owning lifecycle
signal. Before this change, a process crash or hard machine shutdown left the
row open. Startup recovery correctly refused to invent an exit timestamp, but
that also discarded all observed duration for the session.

Using the next application startup time as the exit would over-count an unknown
period and is therefore unacceptable.

## Decision

Add a durable `checkpoint_at` timestamp to each play session. New sessions begin
with `checkpoint_at = started_at`. While the session is live, Horizon advances
the checkpoint every five seconds. The write stays behind `ActivityRepository`;
source adapters and Slint do not know about SQLite.

A clean terminal signal still owns the exact `ended_at` and also advances the
checkpoint to that same value. If startup finds an old open row, recovery marks
it `interrupted` and sets `ended_at` to the already-persisted checkpoint. For an
interrupted row, that field is a conservative **confirmed observed-through**
boundary, not a claim about the real process exit.

Recovered interrupted duration is valid Horizon-observed time and contributes
to durable observed totals. It is surfaced as `Recovered` in Recent Activity.
It does not increment the completed-session counter.

## Consequences

- A hard shutdown normally loses only the uncheckpointed tail instead of the
  entire session.
- The five-second cadence adds one very small SQLite write while at least one
  session is live.
- SQLite synchronous mode is explicitly `FULL` to prioritize checkpoint
  durability over maximum write throughput.
- The design remains source-neutral and works for Steam SourceRuntime, managed
  sessions, and foreground-handoff fallback.
- Startup time is never used as a synthetic game end.
- An application-only crash while the game continues preserves time through the
  last checkpoint, but automatically re-attaching to a game that remained
  running across a Horizon restart is a separate lifecycle-reconciliation
  problem.
