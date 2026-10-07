# Managed game sessions

Phase 9.5 introduces an optional host-managed launch path for games that can be
started inside a Gamescope session without weakening Horizon's Flatpak sandbox.

## Goals

- keep the normal URI/OpenURI launch path working when no helper is installed;
- use the host's existing Gamescope installation on immutable systems;
- give Horizon a real session lifecycle for sources that can be wrapped safely;
- improve playtime precision for managed sessions;
- avoid `flatpak-spawn --host`, broad filesystem permissions, or an arbitrary
  host-command API.

## Architecture

```text
Horizon Flatpak
    |
    | io.github.Mars7x.Horizon.Session1 (user D-Bus)
    v
horizon-session-helper
    |
    | source/game identity is resolved again on the host
    v
Gamescope
    |
    v
provider launch command / game
```

The Flatpak sends only a registered `SourceId` and `ExternalGameId`. It does not
send an executable path, shell command, or arbitrary argument vector. The host
helper reconstructs the same source registry and asks the owning adapter for a
`SourceManagedLaunchTarget`.

That duplicated validation boundary is deliberate: exposing a D-Bus method such
as `RunCommand(program, args)` would effectively turn the helper into a generic
sandbox escape.

## Capability model

`SourceCapability::ManagedSession` is separate from ordinary
`SourceCapability::Launch`.

A provider may support both:

- **Launch** returns the normal `SourceLaunchTarget`, currently an URI dispatched
  through XDG OpenURI.
- **ManagedSession** returns a `SourceManagedLaunchTarget` that is meaningful only
  to the trusted host helper.

`GameLaunchService` prefers a managed session only when all of these are true:

1. the source advertises `ManagedSession`;
2. the optional D-Bus helper is reachable;
3. the helper can resolve that source/game on the host;
4. Gamescope is available;
5. the managed process can be started.

Otherwise it falls back to the existing normal launch path.

## Phase 9.5 provider coverage

Bottles is the first managed-session provider. Its local metadata already gives
Horizon the bottle and explicit saved program identity needed to reconstruct a
direct Bottles CLI launch on the host.

Native Bottles uses:

```text
bottles-cli run -b <bottle> -p <program>
```

Flatpak Bottles uses the equivalent host-side Flatpak command. Those exact
provider mechanics remain inside `src/sources/bottles.rs`.

Steam and Heroic remain normal external launches in Phase 9.5. Their
existing URI handoff may be serviced by an already-running launcher, which means
wrapping the URI dispatcher in Gamescope would not prove that the actual game
joined the managed compositor. Do not advertise `ManagedSession` for those
sources until that lifecycle is real.

## Helper lifecycle

The helper owns a small table of Gamescope child processes. The D-Bus contract
supports:

- helper capability/protocol probing;
- start by `(source_id, external_id)`;
- query running/exited/failed state;
- stop a known managed session;
- forget a terminal managed session.

The current application polls active managed sessions every 500 ms. This is
bounded to active managed IDs and does not scan host processes. Local D-Bus
method calls use a short timeout so an unhealthy optional helper cannot block
the Horizon UI indefinitely.

The helper runs as the logged-in user. It is not a privileged/root service.

## Activity tracking

Managed sessions use `SessionTrackingMethod::ManagedSession`.

The activity session begins after the helper successfully starts the Gamescope
session and ends when the helper reports that session terminal. Window
activation changes do not end managed-session activity.

This is stronger than `ForegroundHandoff` because it observes the lifecycle of
the managed compositor/session rather than Horizon's foreground state. Existing
foreground-handoff rows retain their original meaning.

If the helper loses knowledge of an active session or reports failure, Horizon
marks the play session interrupted rather than inventing a duration.

## Immutable-system development installation

Gamescope remains installed by the host/image. Horizon does not bundle or
replace it.

Build the helper in a compatible host or toolbox environment, then install the
result for the current user:

```bash
cargo build --release --bin horizon-session-helper
./scripts/install-session-helper-dev.sh
```

An explicit binary path may be supplied to the script if it was built elsewhere.

The development installer writes only:

```text
~/.local/libexec/horizon-session-helper
~/.config/systemd/user/horizon-session-helper.service
```

and enables the user service.

Production host packaging is intentionally deferred to Phase 12. Horizon must
remain usable when this helper is absent.

## Current limitations

- Bottles is the only source with managed-session capability.
- There is no Horizon in-game overlay or session menu yet.
- The UI has no explicit "quit game" command yet even though the service/helper
  boundary has a stop operation.
- Gamescope options are intentionally minimal (`-f`) in this first slice.
- HDR, VRR, nested resolution policy, overlay composition, and advanced
  Gamescope tuning belong to later console-polish work.

## Phase 9.5.27 helper runtime-observation role

The existing same-user D-Bus helper now has a second narrow responsibility:
provider-owned runtime observation. This is available even when Gamescope is
not installed and therefore also serves ordinary Flatpak launches.

The additional D-Bus surface still accepts only source/game identity and
helper-issued observation IDs. It does not expose generic `/proc` search,
process-name matching, arbitrary executable paths, or host command execution.
The helper rebuilds the production `SourceRegistry`, verifies that the source
advertises `RuntimeObservation`, and asks that source adapter for runtime state.

Steam is the first runtime-observed source. Bottles managed launching remains
unchanged. Steam still does not advertise `ManagedSession`.

The helper protocol version is now 2. Development installs must rebuild and
reinstall `horizon-session-helper` after applying this phase.


## Phase 9.5.28 provider set

Lutris is no longer part of the production source registry. The helper therefore
cannot resolve Lutris identities for managed launch or runtime observation. This
requires no helper-specific special case because both processes consume the same
`production_source_registry()`.

## Runtime observation is not a helper requirement

As of Phase 9.5.32, normal Steam `SourceRuntime` observation is performed inside
the Horizon Flatpak from Steam-owned `gameprocess_log.txt`. Installing the
session helper is not required for Steam playtime tracking. The helper remains
optional for managed Gamescope launch/session ownership.
