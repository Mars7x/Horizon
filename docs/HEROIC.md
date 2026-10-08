# Heroic source adapter

Phase 9 adds Heroic as a normal `GameSource`. The adapter is local-only and does not authenticate with Epic, GOG, Amazon, or Heroic services.

## Current scope

The first Heroic slice imports **installed Epic/Legendary titles** only. Horizon reads Heroic/Legendary's local `installed.json` membership and local Legendary metadata for display titles. The durable Heroic external identity is namespaced by runner:

```text
heroic + legendary:<app-name>
```

Keeping the runner in `ExternalGameId` leaves room for later `gog:`, `nile:`, or other Heroic-managed identities without changing the generic source framework or conflating store-owned IDs.

GOG, Amazon/Nile, and sideloaded Heroic entries are deliberately not fabricated from incomplete metadata in this phase. They can be added as additional Heroic runner namespaces when their installed-membership contracts are implemented and tested.

## Metadata roots

Horizon checks the host XDG config tree and the Heroic Flatpak app-data tree for current and legacy Legendary config layouts, including:

```text
$HOST_XDG_CONFIG_HOME/heroic/legendaryConfig/legendary
$HOST_XDG_CONFIG_HOME/legendary
~/.var/app/com.heroicgameslauncher.hgl/config/heroic/legendaryConfig/legendary
~/.var/app/com.heroicgameslauncher.hgl/config/legendary
```

Outside Flatpak, the host XDG config location resolves from `XDG_CONFIG_HOME` or `~/.config`.

For each native/Flatpak installation, Horizon prefers Heroic's current `heroic/legendaryConfig/legendary` layout and falls back to the older `legendary` layout only when the current directory is absent. This avoids merging a stale migrated Legendary cache with the active one.

The selected Legendary root is authoritative for installed Epic membership. A readable `installed.json` supplies that membership; if the selected root exists but `installed.json` is absent, Heroic itself treats that as no installed Legendary games, so Horizon publishes an authoritative empty membership for that root. If a title cannot currently be resolved from local metadata, its installed ID remains in authoritative membership so synchronization does not delete an already-known game merely because display metadata is temporarily incomplete.

## Launching

The adapter advertises `SourceCapability::Launch` and returns Heroic's URI form with its upstream-supported no-window override:

```text
heroic://launch?appName=<encoded-app-name>&runner=legendary&gui=false
```

The URI is still dispatched by the source-neutral `GameLaunchService` through XDG OpenURI. Horizon does not spawn Heroic, call `flatpak run`, or reproduce Heroic's Wine/Proton configuration. The `gui=false` parameter is independent of Horizon's Playing indicator and runtime observation.

## Flatpak permissions

Horizon requests read-only access to the native Heroic/Legendary config trees and to:

```text
~/.var/app/com.heroicgameslauncher.hgl:ro
```

No credentials are interpreted by Horizon and no broad home/host permission is required.

## Upstream references

- Heroic's current Legendary library manager reads installed membership from `installed.json`: https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/storeManagers/legendary/library.ts
- Heroic creates Linux shortcuts with `heroic://launch?appName=<app>&runner=<runner>`: https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/shortcuts/shortcuts/shortcuts.ts
- Heroic's troubleshooting documentation records the native and Flatpak config locations used by its stores: https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/wiki/Troubleshooting

## Phase 9.5 managed-session status

Heroic remains an external URI launch in Phase 9.5. Horizon does not advertise `ManagedSession` merely by wrapping `heroic:` URI dispatch; the capability will be added only when the actual game lifecycle can be placed reliably under the managed compositor.


## Phase 9.5.44.46 — Lifetime playtime import

Heroic writes `appName.totalPlayed` (integer minutes) to an electron-store file
at `heroic/store/timestamp.json` after the game launch command finishes. Horizon
reads the native and Flatpak Heroic stores using its **existing read-only**
filesystem grants and normalizes these entries into `SourceLifetimePlaytime`
seconds, keyed only by Heroic's `legendary:<appName>` game identity. Native
stores have precedence over Flatpak for duplicate app names; values are never
summed across installations. Bad JSON, invalid numeric values, or missing files
are skipped without deleting previously stored totals.

On startup and approximately every 20 seconds while Horizon runs, the generic
`SourcePlaytimeSync` service refreshes changed totals into the existing
`source_lifetime_playtime` SQLite table. Heroic's totals also include time from
sessions launched outside Horizon. They are **not** added to Horizon-observed
sessions, and Heroic's file does not contain a complete session timeline.

Activity displays a separate **Reported lifetime** summary and a short ranked
list with provider-labelled game totals. A Heroic game must already exist in the
imported Horizon library for its lifetime value to be associated with a game.

### Live Playing indicator: not implemented in this slice

`timestamp.json` changes **after** a launch ends and is not a live running-state
source. Heroic's pre/post launch scripts fire at launch boundaries, but the
pre-launch hook is invoked *before* the game is confirmed running. Horizon does
not use script timestamps or launch URI dispatch as evidence that the actual
game is running. Therefore `RuntimeObservation` is still not advertised for
Heroic and its Playing pill requires future verified lifecycle integration.
No broad process scanning, Flatpak permissions, changes to Heroic configuration,
or hidden background helpers were added in this phase.

Upstream evidence:
- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/constants/key_value_stores.ts
- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/launcher.ts

## Phase 9.5.44.48 — Automatic Heroic session observation

The optional command-wrapper integration from Phase 9.5.44.47 is **retired**.
Heroic games require **no scripts, manual wrapper settings, external helpers,
or changes to Heroic configuration**. At the time, Horizon used the normal
`heroic://` URI without GUI flags; Horizon reads Heroic's local game-launch log and last-played store
through scoped read-only filesystem access. There is no process scan.

Heroic's current Linux game launch logs live at:

```text
$HOST_XDG_STATE_HOME/Heroic/logs/games/<appName>_legendary/launch.log
~/.var/app/com.heroicgameslauncher.hgl/.local/state/Heroic/logs/games/<appName>_legendary/launch.log
```

Horizon also checks the older Heroic Flatpak `state/` layout. It reads only a
bounded log tail (16 KiB) and interprets Heroic's built-in end-of-log marker.
With game logging disabled, Heroic may not write that marker; Horizon then
checks `appName.lastPlayed` in the matching installation's `timestamp.json`,
which Heroic updates after its launch promise completes. Completed sessions do
not count as Playing; an old log cannot revive them. The source-neutral
runtime observer drives the existing Playing card and observed Activity sessions.

**Important accuracy limits:** This is **best-effort Heroic launch-lifecycle
observation**, not direct proof that a game process is alive. Heroic writes the
first log when launch preparation begins, potentially before the actual game
starts; a failed launch can briefly show Playing until Heroic finishes its
launch attempt. A Heroic crash can leave an unfinished log, and without an
external status API it is impossible to distinguish that from a long-running
game. Horizon expires log observations after 18 hours without a log update,
trading off very long sessions against indefinitely stuck Playing badges.
Games that detach from Heroic's launch promise may finish tracking early.
Source-reported lifetime playtime remains the more authoritative overall Heroic
total and is never added to Horizon-observed time.

**Flatpak permission:** The existing Heroic app-data grant covers Flatpak logs.
Native Heroic uses one additional narrow grant,
`--filesystem=~/.local/state/Heroic/logs:ro`. There is no `xdg-state`
filesystem permission alias, so installations with a custom host
`XDG_STATE_HOME` outside the default home location might need a matching
user-provided read-only override. On conventional Fedora/Flathub installs,
no setup is needed.

Users who installed the old Phase 9.5.44.47 wrapper can remove it from Heroic's
per-game Advanced settings when convenient. Horizon no longer reads its
heartbeat files. It does not modify Heroic configuration or remove any
user-installed wrappers automatically.

Upstream references:
- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/logger/paths.ts
- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/logger/log_writer.ts
- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/launcher.ts
- ADR 0105

## Phase 9.5.44.49 — Launch-scoped session evidence

Heroic's per-game log is not a game-process API. A log may be created during
preparation, and a killed Heroic can leave a log without the terminal marker.
An unrelated prior unfinished log must not become a new Horizon session when a
user launches the same game again.

The source-neutral runtime observer now passes its **wall-clock arming time**
to the owning source adapter. Steam's authoritative source observation retains
its previous semantics. Heroic accepts open launch-log evidence only when the
log's most recent modification falls within **45 seconds before the observer
was armed**, or is newer. This accommodates the asynchronous OpenURI handoff,
while rejecting older incomplete log files. The same log remains valid for the
rest of the observed session until a completion signal or the pre-existing
18-hour no-log-write safeguard, even if the game runs silently.

If native and Flatpak Heroic both have files for the same Legendary app name,
Horizon uses the **newest log generation**, not the first open log found. A
completed log wins an exact modification-time tie. A matching `lastPlayed`
completion is still checked in the corresponding installation's timestamp
store. No heuristic log from one installation can keep another installation's
newer completed launch in the Playing state.

**Limitations:** The 45-second handoff tolerance cannot distinguish a very
recent orphan log from an actual new session, nor does an open Heroic log
prove an executable was spawned. Exceptionally slow Heroic handoffs may fail
the scope check. Crashes, log-disabled sessions, detached child processes,
and very long (>18 hour) games remain best-effort; observed playtime must not
be represented as guaranteed process time. Provider-reported lifetime totals
remain separate. No process scanning, wrapper, Heroic modification, additional
Flatpak access, or new dependencies are introduced.

See ADR 0106.

## Phase 9.5.44.53 — Hide Heroic during game launches

Heroic 2.22.0 (May 2026) introduced `gui=false` on `heroic://launch` URLs.
Horizon now adds `&gui=false` for each installed Legendary/Epic game launch;
Heroic itself hides its main window at startup, on repeated instance invocation,
and on URL dispatch to an already-running Heroic instance. Heroic remains the
owning launcher, with its normal game settings and cloud saves. The Heroic
backend may continue running; this is a **window-hiding** feature, not a
replacement or bypass of Heroic's launcher process.

**Compatibility:** Heroic installations older than 2.22.0 can ignore the option
and may still display the Heroic application. Users should update Heroic rather
than adding new Horizon permissions or installing an external wrapper. If a
user tries to launch a game that is no longer installed, Heroic may show the
install confirmation dialog. This doesn't affect Horizon's launch-time
observation, Steam behaviour, or separate playtime accounting.

Horizon needs no new Flatpak permissions, dependencies, services, URI handler,
or manual configuration for this behaviour. See ADR 0110.

Upstream evidence:
- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/pull/5501
- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/releases/tag/v2.22.0
