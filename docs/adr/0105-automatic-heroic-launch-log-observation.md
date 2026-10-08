# ADR 0105 — Automatic Heroic launch-log observation

**Status:** Accepted as Phase 9.5.44.48 design; build and real Heroic validation pending.

## Context

Phase 9.5.44.47 required manually installing a versioned shell wrapper and
adding it to every Heroic game. This is contrary to Horizon's goal of importing
and launching source-owned games without configuration. Heroic does not expose a
supported external game-process status API. Its documented launch pipeline does,
however, create a per-game log and update `lastPlayed` after the launch promise
returns. The logger appends `============= End of log =============` on close
unless logs are disabled. A forced warning is emitted on typical non-verbose
game launches; the `lastPlayed` completion compensates for the missing footer.

## Decision

- Retire the manual wrapper installer and game-command wrapper. Do not change
  Heroic settings, inject game arguments, or launch its binaries from Horizon.
- The Heroic adapter pairs each installation's `launch.log` with that same
  installation's `timestamp.json` and observes a *best-effort* active session
  until Heroic writes the end marker or a newer last-played timestamp.
- Read at most the last 16 KiB of each game log, bound inactive log lifetimes to
  18 hours, validate the external ID as a safe path segment, and filter to the
  supported `legendary` namespace.
- Add only `~/.local/state/Heroic/logs:ro` for native default Heroic; existing
  `~/.var/app/com.heroicgameslauncher.hgl:ro` covers Flatpak Heroic state.
  A user-configured non-default XDG state location cannot be automatically
  granted by the manifest.
- Keep `SourceRuntimeState`, session persistence, and Playing presentation
  source-neutral. Provider-reported lifetime totals remain separate.

## Limitations

This is **not a guaranteed game process detector**. A log begins during launch
preparation and can show a short false Playing status if launch later fails;
no new log can be an API-level guarantee that the executable is alive. A
crashed Heroic can leave an unfinished log until expiry, while exceptionally
long-running sessions can expire while still playing. Detached processes can
outlive the Heroic launch promise. The observed playtime is therefore an
estimate; Heroic's own cumulative playtime remains separately reported.
Native installations with customized XDG_STATE_HOME paths may require the user
to grant read-only access to that alternative path.

## Supersedes

ADR 0104's opt-in wrapper design. Its history remains in the repository but
both wrapper scripts are deleted. Previously user-installed Heroic wrappers are
not altered or removed by Horizon.

## Upstream contracts

- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/logger/paths.ts
- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/logger/log_writer.ts
- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/launcher.ts
