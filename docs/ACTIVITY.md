# Activity and playtime

Phase 8 adds Horizon's first durable play-history model and replaces the Activity placeholder with persisted data.

## Two different kinds of playtime

Horizon deliberately keeps these concepts separate:

1. **Horizon-observed sessions** — time Horizon can directly observe through a documented tracking method.
2. **Source-reported lifetime playtime** — a provider's own cumulative value, when a source can supply one reliably.

They must never be added together. A source lifetime value may already include play that occurred outside Horizon, while Horizon-observed sessions describe only activity Horizon itself witnessed.

## Foreground-handoff sessions

Steam Phase 7 launches through XDG OpenURI, so Horizon does not own the game process and cannot truthfully claim exact process lifetime. The first tracking method is therefore explicitly approximate:

```text
Home Accept
   ↓
launch target dispatched
   ↓
pending launch
   ↓
Horizon loses OS window activation
   ↓
ForegroundHandoff session starts
   ↓
Horizon becomes active again
   ↓
session completes
```

Only a launch dispatched by Horizon arms this handoff. Ordinary alt-tabbing away from Horizon does not fabricate a session.

The same activation boundary already owns controller-input suspension. Session tracking consumes the semantic activation event at the application/service boundary; Steam does not toggle tracking directly.

## Interrupted sessions

An open session can survive in SQLite if Horizon exits or crashes while the game owns the foreground. On the next startup, Horizon marks any such row `interrupted` without inventing an end timestamp or duration.

Interrupted sessions do not contribute to observed playtime totals. Unknown is preferable to fabricated time.

## Activity page

The Activity route is now a real full-shell page. It shows only completed Horizon-observed sessions:

- total observed playtime;
- completed session count;
- number of games with completed sessions;
- recent sessions;
- most-played games by observed time.

The page labels the data as Horizon-observed. It must not silently present approximate foreground time as exact source lifetime playtime.

## Persistence

Schema version 2 adds:

- `play_sessions` for Horizon-observed session history;
- `source_lifetime_playtime` for provider-reported cumulative values.

Historical activity retains the logical `games` row even after the final active source reference disappears. Such a game is no longer part of the active library because `list_games()` requires a current `game_sources` row, but its historical title/session records remain available to Activity.

## Future precision

Future adapters may support more precise process/session tracking. Add those as explicit `SessionTrackingMethod` variants rather than changing the meaning of `ForegroundHandoff`.

A future source that reports lifetime playtime should write that value through the source-neutral Activity service and the separate lifetime table. Do not back-fill it into observed sessions or combine the counters.


## Managed sessions (Phase 9.5)

Schema v3 adds `managed_session` to the persisted tracking-method vocabulary.

A managed session begins only after the host helper successfully starts a
Gamescope session. It remains open across Horizon focus changes and is completed
when the helper reports the managed session exited.

If the helper reports failure or loses the session identity, Horizon marks that
activity row interrupted with no fabricated end timestamp/duration.

`ForegroundHandoff` remains the fallback method for ordinary URI launches and
its meaning is unchanged. Activity totals can contain both methods because both
are Horizon-observed sessions; provider-reported lifetime values remain in the
separate `source_lifetime_playtime` table and are still never added to observed
totals.
