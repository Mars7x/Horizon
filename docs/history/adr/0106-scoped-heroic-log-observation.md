# ADR 0106 — Launch-scoped Heroic runtime observation

**Status:** Accepted implementation for Phase 9.5.44.49; build and end-to-end runtime validation pending.

## Problem

ADR 0105 introduced no-setup Heroic log observation, but treated **any**
unfinished Heroic per-game log as a current running game. A log orphaned by
an earlier crash could falsely start a newly requested Horizon observation,
and a closed Flatpak session could be masked by an old open native log of the
same Epic identity. These produce incorrect Playing badges and observed
playtime even when Heroic did not actually launch a new game.

## Decision

1. Add `GameSource::runtime_state_for_observation(external_id, armed_at)` as an
   optional source-owned method. It defaults to the established `runtime_state`
   contract; the generic host runtime observer passes its arming wall-clock
   time and does not contain Heroic-specific knowledge.
2. Heroic accepts an unfinished log as a possible running session only when its
   last modification occurred no earlier than 45 seconds before the observer
   was armed. The grace period allows the OpenURI handoff to begin asynchronously
   before the host helper records the observation.
3. When both native and Flatpak Heroic installations have game logs for the
   same Legendary ID, inspect the **newest log modification**, with an ended
   log preferred on an exact timestamp tie. Never treat older incomplete logs
   as overriding newer completion records.
4. Continue checking `timestamp.json` for Heroic's `lastPlayed` completion,
   and retain the 18-hour maximum age for silent unfinished logs.
5. Preserve all existing source-neutral activity/session handling, original
   Heroic URI launch target, no-wrapper behaviour, and narrowly scoped
   read-only filesystem access.

## Limits and deliberately rejected alternatives

- A recent unfinished log is **not** authoritative evidence that a game
  process is alive. The 45-second grace window can admit a fresh orphaned log,
  and a very slow handoff may be missed. This is a targeted false-positive
  reduction, not Steam-equivalent process tracking.
- No app-specific `/proc` probing, broad process scanning, arbitrary host
  command execution, game wrapper injection, changes to Heroic settings,
  modified Heroic binaries, or permanent daemon are added.
- Crashes and detached game processes can still result in inaccurate observed
  sessions. Do not conflate observed playtime with Heroic-reported lifetime.

## Upstream references

- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/launcher.ts
- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/logger/paths.ts
- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/logger/log_writer.ts
