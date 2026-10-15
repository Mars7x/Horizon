# ADR 0104 — Opt-in Heroic command-runtime bridge

**Status:** Superseded by ADR 0105 in Phase 9.5.44.48; retained as historical rationale

## Context

Heroic's `timestamp.json` is written when a session ends, not while a game
runs. Heroic invokes `beforeLaunchScriptPath` before attempting to start the
game; a before-launch event cannot be trusted as a Playing signal. Heroic's
per-game `wrapperOptions` runs **around the actual game command** with
`HEROIC_APP_NAME` and `HEROIC_APP_RUNNER` environment identity. This is usable
without replacing Heroic as the owner of Wine/Proton, authentication, arguments,
or exit handling.

## Decision

Provide an **explicitly opt-in**, versioned shell wrapper installed into the
native or Flatpak Heroic config tree. The user adds it to each game's Heroic
wrapper list. It runs Heroic's original command (`"$@"`) and publishes atomic,
per-session heartbeats only while waiting for that command. Identity is exact,
UTF-8-hex encoded and Legendary-only. The wrapper writes nothing outside
Heroic's own config tree and does not modify Heroic's game settings.

The Heroic adapter reads that tree through existing read-only Flatpak grants.
When the version marker is absent, observation is unavailable and external
launch works normally. Fresh heartbeat => `Running`; explicit cleanup or stale
heartbeat older than five seconds => `Stopped`. Existing source-neutral
`SourceRuntimeObservationExecutor`, Activity and per-card Playing UI consume
those states. No `/proc` scanning, broad permissions, subprocesses launched by
Horizon, added dependencies, or changes to game launch dispatch.

## Limitations

- Per-game Heroic wrapper configuration is required; merely installing Horizon
  does not enable observing Heroic.
- Command wrappers follow the lifetime of the command handed to them. A game
  that detaches from that command cannot be tracked beyond its parent's exit.
- The heartbeat begins after a one-second grace interval, so very brief
  sessions may not be recorded. Observation start/end times are poll timestamps,
  not exact provider timestamps.
- A killed wrapper may leave a heartbeat; it expires within five seconds.
- Multiple simultaneous session files for the same identity are supported.
- Heroic's provider-reported lifetime is kept **separate** from Horizon's
  observed session total to prevent double counting.

## External references

- Heroic command launch, wrapper variables, and before/after-script boundaries:
  https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/launcher.ts
- Heroic Epic/Legendary commands and wrapper options:
  https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher
