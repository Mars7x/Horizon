# Bottles source adapter

Phase 9 adds a local-only Bottles `GameSource` using the program entries persisted by Bottles itself.

## Discovery

Horizon reads each standard bottle's `bottle.yml` and imports entries from `External_Programs`. Those entries have a stable program ID/name pair suitable for durable rediscovery and launching. Automatically discovered Windows shortcuts that Bottles has not persisted as external programs are not guessed or scraped in this phase.

Horizon checks standard native and Flatpak bottle collections:

```text
$HOST_XDG_DATA_HOME/bottles/bottles
~/.var/app/com.usebottles.bottles/data/bottles/bottles
```

Outside Flatpak, the host XDG data location resolves from `XDG_DATA_HOME` or `~/.local/share`.

Durable external IDs include the installation scope, bottle directory, and Bottles program ID:

```text
native:<bottle-directory>/<program-id>
flatpak:<bottle-directory>/<program-id>
```

The bottle's display `Name` and program `name` are looked up again at launch time instead of being embedded into identity, so display-name changes do not rewrite the persisted source key.

Bottles can represent bottles stored outside its standard app-data root through `placeholder.yml`. Horizon intentionally does not grant arbitrary host filesystem access to follow those paths. Detecting such a placeholder makes the scan partial/non-destructive rather than pretending the inaccessible bottle does not exist.

## Launching

The adapter resolves the current bottle/program names and returns the documented Bottles URI:

```text
bottles:run/<encoded-bottle>/<encoded-program>
```

The generic OpenURI launcher handles dispatch. The sandboxed Horizon application does not execute `bottles-cli`, Wine, Flatpak commands, or bottle runners directly. Phase 9.5 may ask the optional same-user host helper to execute an adapter-owned managed target under Gamescope; that path is documented below.

## Flatpak permissions

Only Bottles-owned standard metadata trees are exposed read-only:

```text
xdg-data/bottles:ro
~/.var/app/com.usebottles.bottles:ro
```

External bottle locations remain outside the sandbox unless a user independently grants access; Phase 9 does not broaden Horizon's manifest for them.

## Upstream references

- Bottles documents the `bottles:run/<bottle>/<program>` XDG-open protocol and defines the components from `bottle.yml`: https://github.com/bottlesdevs/documentation/blob/master/advanced/xdg-open.md


## Phase 9.5 managed sessions

Bottles is Horizon's first `ManagedSession` source. The adapter already has the
durable bottle/program identity needed to re-resolve an explicit saved program
on the host.

For a native Bottles installation the host helper resolves the managed target to:

```text
bottles-cli run -b <bottle-name> -p <program-name>
```

For Flatpak Bottles it uses the equivalent host Flatpak CLI invocation. The
helper then starts that adapter-owned target under host Gamescope.

This logic remains in the Bottles adapter. `GameLaunchService`, Activity, Slint,
and the D-Bus transport never check for the string `"bottles"`.

If the helper or Gamescope is unavailable, Bottles falls back to its existing
`bottles:run/...` OpenURI launch. See `MANAGED_SESSIONS.md` and ADR 0046.
