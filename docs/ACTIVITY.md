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
candidate return (not complete yet)
   ↓
short grace / startup stabilization
   ↓
├─ Horizon loses activation again → cancel candidate, keep session open
└─ Horizon stays active → complete at the original return timestamp
```

Only a launch dispatched by Horizon arms this handoff. Ordinary alt-tabbing away from Horizon does not fabricate a session.

A URI launcher may temporarily give focus back to Horizon while it is still
starting the real game. Phase 9.5.26 therefore treats reactivation as a
**candidate return**, not an immediate session end. During the first 30 seconds
a startup return must remain stable through the launch-stabilization window
(with at least a 10-second grace from the bounce). Once a session is mature, a
1.5-second stable return is enough. If Horizon loses activation again before
confirmation, the candidate is discarded and the same session continues.

The grace period never inflates playtime: when a return is confirmed, SQLite is
completed using the timestamp at which Horizon originally became active, not
the later confirmation time. This remains approximate foreground ownership; it
does not claim Steam process lifetime.

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

`ForegroundHandoff` remains the fallback method for ordinary URI launches. Its
Phase 9.5.26 focus-bounce tolerance changes only *when a return is considered
stable*; it still measures Horizon-observed foreground handoff rather than exact
process lifetime. Activity totals can contain both methods because both are
Horizon-observed sessions; provider-reported lifetime values remain in the
separate `source_lifetime_playtime` table and are still never added to observed
totals.

## Source-runtime sessions (Phase 9.5.27)

Schema v4 adds `source_runtime` as a third explicit tracking method.

A source-runtime session is used when a registered source can identify the
lifecycle of one exact source-owned game on the host. The Flatpak does not scan
host processes itself. Instead it sends only `(source_id, external_id)` to the
same-user Horizon helper, and the helper asks the registered source adapter for
provider-specific runtime state.

Steam is the first provider. Steam launches a per-game `reaper` process whose
command line includes `SteamLaunch AppId=<appid>`. Horizon's Steam adapter checks
only for that exact numeric AppID in the host helper. The helper records the
first observed running timestamp and the timestamp at which that exact reaper
is no longer present.

This means Horizon focus changes do not affect an observed Steam session:

```text
Horizon dispatches steam://rungameid/<appid>
   ↓
host observer armed for that exact AppID
   ↓
Steam reaper appears → SourceRuntime session starts
   ↓
Alt-Tab to Horizon / another app → no session transition
   ↓
return to game → same session remains open
   ↓
Steam reaper exits → SourceRuntime session completes
```

If the host helper/runtime observer is unavailable, launch still succeeds and
Horizon falls back to the existing `ForegroundHandoff` behavior. If an observer
is lost after a source-runtime row has started, that row is interrupted rather
than assigned a fabricated end time.

This works whether Horizon is being used as a normal Flatpak window or from a
Gamescope-oriented shell. Runtime observation is not conditional on Gamescope
being installed; Gamescope is only required for the separate managed-session
launch capability.

## Phase 9.5.32 normal-Flatpak Steam lifecycle

Steam `SourceRuntime` tracking no longer depends on the optional host helper.
The ordinary Horizon Flatpak reads Steam's provider-owned
`logs/gameprocess_log.txt` through the same read-only Steam filesystem grants
already used for discovery/artwork. `AppID <id> adding PID ...` marks that AppID
running; `Remove <id> from running list` marks it stopped. Alt-Tab/focus changes
therefore do not complete a Steam `SourceRuntime` session.

The host helper remains optional and is still used for genuine managed
Gamescope sessions. `ForegroundHandoff` remains the fallback when a provider
cannot expose a reliable runtime signal from within the normal app.


## Phase 9.5.33 live in-progress Activity

Open Horizon-observed sessions are now surfaced separately from completed
history. While a source-runtime, managed, or foreground session is open, the
Activity page shows it at the top of Recent sessions as `Playing now` and its
elapsed duration refreshes once per second. The Observed playtime summary also
adds the current in-memory/open-session elapsed time for display.

This live value is presentation-only until Horizon receives a real terminal
signal and completes the session. Completed-session count, durable historical
totals, and Most played remain based on completed rows so an unfinished session
is never silently converted into permanent history.

Schema v6 supersedes the old all-or-nothing crash rule with persisted
observation checkpoints. Live sessions remain `open`, but Horizon periodically
records the last instant at which the session was definitely still being
observed. Startup recovery never uses reboot/startup time as a fabricated exit.


## Phase 9.5.34 crash-resilient checkpoints

Every live Horizon-observed session now receives a durable checkpoint every
five seconds. The checkpoint is conservative: it means Horizon definitely knew
the session was still live at that timestamp; it does not claim the game ended
there. The session-start transaction itself provides the initial checkpoint.

On a clean exit, the normal source-runtime/managed/foreground terminal signal
continues to provide the final `ended_at`, so no precision is lost. On a hard
Horizon crash or full computer power loss, the next startup converts any
leftover `open` row to `interrupted` and recovers duration only through its last
committed checkpoint. That recovered duration is included in Observed playtime,
Recent sessions labels it `Recovered`, and Most played may include the confirmed
recovered seconds. The completed-session count still excludes interrupted rows.

This intentionally loses the small uncheckpointed tail rather than inventing
playtime after Horizon's last proof of life. With a five-second cadence the
normal loss window is less than one interval, subject to filesystem/storage
durability. SQLite synchronous mode is explicitly `FULL` for these commits.
