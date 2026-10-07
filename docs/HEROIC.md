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

The adapter advertises `SourceCapability::Launch` and returns the current Heroic URI form:

```text
heroic://launch?appName=<encoded-app-name>&runner=legendary
```

The URI is still dispatched by the source-neutral `GameLaunchService` through XDG OpenURI. Horizon does not spawn Heroic, call `flatpak run`, or reproduce Heroic's Wine/Proton configuration.

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
