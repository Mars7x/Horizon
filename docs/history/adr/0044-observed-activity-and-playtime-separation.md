# ADR 0044: Observed activity and playtime separation

## Status

Accepted — Phase 8.0.0

## Context

Horizon needs activity history and playtime, but the Phase 7 Steam launch path uses XDG OpenURI. Portal acceptance confirms dispatch; it does not give Horizon a child process whose lifetime can be measured exactly.

Providers may also expose their own lifetime playtime. That value can include launches outside Horizon and therefore cannot be safely combined with Horizon-observed session time.

## Decision

Horizon stores observed sessions and source-reported lifetime playtime as separate concepts.

The initial observed tracking method is `ForegroundHandoff`:

- a successful Horizon launch arms one pending game/source identity;
- loss of Horizon window activation starts the session;
- return of Horizon window activation completes it;
- unrelated deactivation without a pending Horizon launch creates nothing;
- startup converts leftover open sessions to `interrupted` without assigning an end timestamp.

`play_sessions` stores the tracking method and state with every session. Only completed sessions contribute to observed totals.

`source_lifetime_playtime` stores provider-reported cumulative values independently. Those values are never added to observed totals.

Logical game rows with historical sessions or source-lifetime records are retained after their final current source membership is removed. Active-library queries still require a current source reference, so uninstalled games do not reappear on Home.

The Activity route receives formatted presentation models from Rust; Slint does not query SQLite or infer session durations.

## Consequences

- Phase 8 can provide useful activity immediately without pretending Steam URI launches are exact process tracking.
- Crash recovery does not fabricate duration.
- A future managed-session/process-tracking implementation can add a more precise tracking-method variant without rewriting historical meaning.
- Provider lifetime values remain available for future UI without double counting.
- Historical game identity may outlive active source membership, so `games` is no longer equivalent to the currently installed library; active library queries must join `game_sources`.
