# ADR 0047: Source-owned host runtime observation

## Status

Accepted for Phase 9.5.27.

## Context

`ForegroundHandoff` cannot distinguish a game closing from a user Alt-Tabbing
back to Horizon. For Steam URI launches this can truncate playtime even though
the game is still running. The Flatpak cannot reliably inspect host process
lifecycle directly, and generic host process scanning would violate Horizon's
narrow helper boundary.

Steam exposes a stronger provider-specific lifecycle signal on Linux: the
per-game reaper process includes `SteamLaunch AppId=<appid>` in its command
line and persists for the launched game lifecycle.

## Decision

Add a source capability named `RuntimeObservation` and a source-owned
`runtime_state(external_id)` hook. Execute that hook only in Horizon's same-user
host helper.

The D-Bus client sends only `(SourceId, ExternalGameId)` and receives a
helper-issued observation ID. The helper owns waiting/running/exited state and
timestamps. The app polls only observation IDs it created.

Steam implements runtime observation by checking host `/proc/*/cmdline` for a
reaper containing the exact numeric `SteamLaunch AppId=<appid>` marker. No
process pattern, path, PID, or command is supplied by the sandbox.

Persist these sessions as the distinct `source_runtime` tracking method.
`ForegroundHandoff` remains the fallback when the helper is unavailable.
`ManagedSession` remains separate and continues to mean Horizon owns the
managed compositor/session lifecycle.

## Consequences

- Steam playtime survives Alt-Tabbing to Horizon or other applications.
- The mechanism works for normal Flatpak use and is independent of Gamescope.
- The host helper gains provider-specific observation capability without
  becoming a generic process-inspection API.
- Steam runtime observation depends on a Linux Steam implementation detail and
  must fail conservatively if that signal changes.
- An observation that is lost after timing starts is interrupted rather than
  assigned a fabricated duration.
