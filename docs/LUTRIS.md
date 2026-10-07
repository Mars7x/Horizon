# Lutris source adapter

Phase 9 adds a local-only Lutris `GameSource` backed by Lutris's own `pga.db` library database.

## Discovery

Lutris tracks its library and installation state in `pga.db`. Horizon opens that database **read-only** and imports rows that are both marked installed and have a non-empty game config ID. It reads only the normalized local fields needed by Horizon: Lutris game ID and name.

Horizon checks:

```text
$HOST_XDG_DATA_HOME/lutris/pga.db
~/.var/app/net.lutris.Lutris/data/lutris/pga.db
```

Outside Flatpak, the host XDG data location resolves from `XDG_DATA_HOME` or `~/.local/share`.

Native and Flatpak Lutris databases use separate external-ID scopes (`native:<id>` / `flatpak:<id>`) because Lutris database IDs are local to one installation and must not be assumed globally interchangeable.

A successfully read database is authoritative for that installation scope. If one detected Lutris database fails while another succeeds, the merged snapshot is partial so Horizon cannot prune persisted entries based on an incomplete scan.

## Launching

The adapter converts the scoped durable identity back to the owning Lutris numeric game ID and returns:

```text
lutris:rungameid/<id>
```

XDG OpenURI resolves the installed Lutris handler. Horizon never executes the Lutris binary directly and does not parse Lutris runner configuration outside the source adapter.

## Flatpak permissions

Only Lutris-owned metadata trees are exposed read-only:

```text
xdg-data/lutris:ro
~/.var/app/net.lutris.Lutris:ro
```

## Upstream references

- Lutris `settings.py` defines `DATA_DIR` from the user's XDG data directory and `DB_PATH` as `DATA_DIR/pga.db` unless overridden: https://github.com/lutris/lutris/blob/master/lutris/settings.py
- Lutris `game.py` persists `id`, `name`, `installed`, `configpath`, and related local game state into that database: https://github.com/lutris/lutris/blob/master/lutris/game.py

## Phase 9.5 managed-session status

Lutris remains an external URI launch in Phase 9.5. Horizon does not advertise `ManagedSession` merely by wrapping `lutris:` URI dispatch; the capability will be added only when the actual game lifecycle can be placed reliably under the managed compositor.
