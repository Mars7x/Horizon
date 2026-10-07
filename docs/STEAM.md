# Steam source

Phase 7 is Horizon's first concrete source vertical slice. Steam is implemented as a normal `GameSource`; generic import, persistence, presentation, and launch services do not branch on the string `"steam"`.

## Scope

The Phase 7 Steam slice provides:

- local Steam installation detection for native and Flatpak Steam layouts;
- installed-game discovery from Steam's own local metadata;
- title/type normalization through the source framework;
- durable import through `LibraryService` and SQLite;
- source-backed Home cards instead of the old demo catalog;
- launch dispatch through the source-neutral launch service and XDG OpenURI portal.

It intentionally does **not** add Steam account authentication, Steam Web API calls, lifetime playtime, activity/session tracking, cloud data, achievements, or a production artwork cache. Those belong to later phases.

## Local metadata

`src/sources/steam.rs` owns all Steam-specific paths and VDF knowledge. The adapter checks the conventional Linux roots that can correspond to native Steam or the Flatpak Steam client. When Horizon itself is sandboxed, it also honors Flatpak's `HOST_XDG_DATA_HOME` so a host with a custom XDG data directory can still resolve native Steam metadata through the narrow `xdg-data` permission. Detection is local-only; Horizon does not need Steam credentials or runtime network access.

The adapter reads:

- `steamapps/libraryfolders.vdf` to identify configured libraries and installed app IDs;
- accessible `appmanifest_<appid>.acf` files for direct installed-title metadata;
- `appcache/appinfo.vdf` to enrich discovery with `common.name`/`common.type` and to name games whose external-library manifests are outside Horizon's sandbox.

Current binary `appinfo.vdf` v40/v41 data is parsed through `steam-vdf-parser` 0.1.2. The dependency and dual Apache-2.0/MIT license are recorded in `THIRD_PARTY_NOTICES.md`.

Modern `libraryfolders.vdf` includes an `apps` object for each library. When a library's `steamapps` directory is readable, Horizon treats the actual `appmanifest_<appid>.acf` files as the primary installed-membership record. This prevents stale/cache-only IDs from appearing as installed games. For an external library outside Horizon's narrow sandbox, the `apps` map is the installed-membership fallback, so broad read access to every game-storage mount is still unnecessary.

Readable manifests and `appinfo.vdf` are complementary metadata sources. A missing, unreadable, or temporarily unparsable `appinfo.vdf` must not erase games whose installed manifests are already readable. Installed membership and normalized title metadata are tracked separately, so a currently installed ID can remain protected during authoritative reconciliation even if fresh display metadata is temporarily unavailable.

## Filtering

An installed app is included when local appinfo identifies it as a Steam `game`. Entries explicitly typed as tools, DLC, applications, etc. are ignored. If the type field is absent but a usable local title exists, Horizon retains the entry rather than inventing a provider-specific denylist.

Numeric Steam app ID is the `ExternalGameId`. Durable identity therefore remains the Phase 5 key:

```text
(SourceId("steam"), ExternalGameId(<appid>))
```

Titles are presentation metadata and are never used as identity.

## Multiple Steam roots

Horizon may see more than one conventional Steam root (for example, stale native metadata plus an active Flatpak installation). Each usable root is discovered independently and results are merged by Steam app ID. Failure in one detected root does not discard a successful result from another root. If Steam is not detected at all, the adapter returns `Unavailable(NotInstalled)` rather than a failure.

## Launching

The Steam adapter advertises `SourceCapability::Launch` and converts a numeric Steam app ID into:

```text
steam://rungameid/<appid>
```

It does **not** spawn `/usr/bin/steam`, call `flatpak run`, or inspect how Steam itself is installed. `GameLaunchService` asks the registered source for a source-neutral `SourceLaunchTarget`, and `PortalLaunchExecutor` hands the URI to `org.freedesktop.portal.OpenURI` through `ashpd`. The desktop/session chooses the registered Steam URI handler outside Horizon's sandbox.

This keeps Steam installation details inside the adapter and desktop activation details inside `src/platform/`.

## Flatpak filesystem access

Horizon requests read-only access to conventional native Steam XDG roots plus Flatpak Steam's own app-data subtree:

```text
xdg-data/Steam
xdg-data/steam
~/.var/app/com.valvesoftware.Steam
```

The Flatpak Steam grant intentionally covers the complete `com.valvesoftware.Steam` app-data directory rather than several deeper child paths. Real Flatpak Steam layouts may resolve the canonical Steam directory through links or paths crossing `.local/share` and `data`; granting only the final child directories can therefore make metadata visible when manually overridden but unreachable with the packaged permissions. The broader parent remains read-only and is still scoped to Steam's app sandbox only.

No `--filesystem=home`, `--filesystem=host`, or general `/mnt`/`/run/media` permission is added. External library installation paths remain outside Horizon's sandbox unless a later feature has a concrete reason to access them.

The OpenURI launch path is a portal and requires no direct access to the Steam executable.

## Home presentation

After discovery/import, `LibraryService::games()` supplies durable `LibraryGame` values to `HomeController`. Slint receives only `GameCardData` presentation values; Steam IDs and source objects do not enter Slint.

Phase 7 removes the hard-coded demo catalog. Real imported games currently use Horizon's procedural `FallbackCover` when production artwork has not been imported. The fallback is deliberately source-neutral. Real source artwork/cache policy remains a later feature rather than a Steam-specific UI exception.

If no source-backed games exist, Home shows an explicit empty-library state and does not instantiate the carousel. Horizon does not reinsert fake demo rows into the production database or UI, and zero games must never leave title/shelf/focus geometry stranded on screen.

## Startup behavior

The Phase 7 vertical slice performs registered-source discovery during startup before building the Home model. Expected Steam absence is logged at info level. A Steam discovery failure is logged and Horizon continues with the already-persisted library. A persistence failure remains fatal for the import pass because partially trusted durable state must not be presented as a successful refresh.

Source-removal reconciliation is intentionally conservative in this phase: discovery refreshes/upserts games that Steam reports, but does not delete previously persisted source references merely because a current metadata pass omitted them. A future reconciliation policy must distinguish a complete authoritative snapshot from a partial/inaccessible source before deleting durable entries.


## Installed-only reconciliation

Steam's successful complete scan publishes authoritative source membership. `SourceImportService` passes that through the generic Phase 6/7 repository contract; SQLite removes Steam references that are no longer in the installed membership and deletes a logical game only when it has no remaining source references. This also cleans stale Steam rows created by earlier builds without requiring the user to recreate the database.

If any detected Steam root fails, the merged snapshot is deliberately partial and persistence remains additive for that pass. This prevents a permission or parse problem from being mistaken for an uninstall. See ADR 0042.

## Phase 9.5 managed-session status

Steam remains an external URI launch in Phase 9.5. The adapter does not advertise `ManagedSession`: invoking `steam://` from inside Gamescope would not prove that an already-running Steam client launches the actual game into that compositor. Steam will opt in only when Horizon can guarantee the real game session is managed rather than merely wrapping URI dispatch.
