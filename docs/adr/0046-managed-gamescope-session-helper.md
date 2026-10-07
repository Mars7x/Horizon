# ADR 0046: Optional host Gamescope session helper

- Status: Accepted
- Phase: 9.5

## Context

Horizon is Flatpak-first, while Gamescope may already be installed by the host
or immutable system image. A Flatpak cannot safely assume it can execute the
host's `/usr/bin/gamescope`.

Granting access to `org.freedesktop.Flatpak` and using `flatpak-spawn --host`
would provide a much broader host-execution capability than Horizon needs.
Passing arbitrary commands through a custom D-Bus helper would recreate the
same architectural problem under another name.

Normal source URI launching also cannot provide exact session lifecycle. A URI
may be handled by an already-running launcher, so the resulting game is not a
child Horizon can observe.

## Decision

Introduce an optional same-user host helper named
`io.github.Mars7x.Horizon.Session1`.

The application sends only durable source/game identity to the helper. The
helper reconstructs Horizon's source registry on the host and asks the source
adapter for a `SourceManagedLaunchTarget`. The D-Bus API never accepts an
arbitrary executable path or command line.

Add `SourceCapability::ManagedSession` independently of normal `Launch`.
`GameLaunchService` prefers a managed session when the source and helper support
it and otherwise falls back to the existing portal launch target.

The initial helper launches:

```text
gamescope -f -- <adapter-owned managed launch target>
```

and reports the resulting Gamescope child/session lifecycle.

Bottles is the Phase 9.5 pilot because its explicit saved-program metadata can
be mapped to a direct CLI launch. Steam, Lutris, and Heroic retain external URI
launching until their real game lifecycle can be placed under the managed
compositor without pretending that launcher URI handling is process ownership.

Managed activity uses a new persisted tracking method,
`managed_session`, and is completed from helper session state rather than window
focus.

## Consequences

### Positive

- Horizon can use host-installed Gamescope on immutable systems.
- The Flatpak keeps a narrow D-Bus permission instead of generic host execution.
- Managed sessions provide a stronger lifecycle signal for playtime.
- Sources can adopt managed sessions incrementally.
- Normal launch remains available when the helper/Gamescope is absent.

### Negative

- Horizon gains an optional host component in addition to the Flatpak.
- Development installation currently requires separately building/installing
  the helper.
- Source adapters that want managed launching must expose a validated host-side
  recipe.
- The helper and Flatpak protocol must remain compatible across upgrades.

## Rejected alternatives

### `flatpak-spawn --host`

Rejected because it requires broad host execution through the Flatpak D-Bus
interface and is unnecessary for Horizon's narrow use case.

### Generic `RunCommand(program, args)` D-Bus method

Rejected because it would make the helper an arbitrary host-command bridge.

### Wrap every provider URI in Gamescope

Rejected because an already-running provider may handle the URI outside that
Gamescope process tree, falsely claiming a managed game session.

### Write a compositor inside Horizon

Rejected for this phase. Gamescope already supplies the nested compositor,
Xwayland, input, and Vulkan presentation machinery. A true embedded compositor
would be a substantially different project.
