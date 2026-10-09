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

Steam now provides local-only lifetime playtime and artwork through the generic `LifetimePlaytime` and `Artwork` capabilities. Steam account authentication, Steam Web API calls, cloud data, and achievements are not implemented; no general runtime network access is needed.

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

Phase 7 removed the hard-coded demo catalog. Since Phase 9.5.29, Home asks the generic `ArtworkService` for normalized 1:1 artwork and uses `FallbackCover` only when no usable source-owned image is available. Steam IDs/paths still never enter Slint.

If no source-backed games exist, Home shows an explicit empty-library state and does not instantiate the carousel. Horizon does not reinsert fake demo rows into the production database or UI, and zero games must never leave title/shelf/focus geometry stranded on screen.

## Startup behavior

The Phase 7 vertical slice performs registered-source discovery during startup before building the Home model. Expected Steam absence is logged at info level. A Steam discovery failure is logged and Horizon continues with the already-persisted library. A persistence failure remains fatal for the import pass because partially trusted durable state must not be presented as a successful refresh.

Source-removal reconciliation is intentionally conservative in this phase: discovery refreshes/upserts games that Steam reports, but does not delete previously persisted source references merely because a current metadata pass omitted them. A future reconciliation policy must distinguish a complete authoritative snapshot from a partial/inaccessible source before deleting durable entries.


## Installed-only reconciliation

Steam's successful complete scan publishes authoritative source membership. `SourceImportService` passes that through the generic Phase 6/7 repository contract; SQLite removes Steam references that are no longer in the installed membership and deletes a logical game only when it has no remaining source references. This also cleans stale Steam rows created by earlier builds without requiring the user to recreate the database.

If any detected Steam root fails, the merged snapshot is deliberately partial and persistence remains additive for that pass. This prevents a permission or parse problem from being mistaken for an uninstall. See ADR 0042.

## Phase 9.5 managed-session status

Steam remains an external URI launch in Phase 9.5. The adapter does not advertise `ManagedSession`: invoking `steam://` from inside Gamescope would not prove that an already-running Steam client launches the actual game into that compositor. Steam will opt in only when Horizon can guarantee the real game session is managed rather than merely wrapping URI dispatch.

## Phase 9.5.27 host runtime observation

Steam now advertises `SourceCapability::RuntimeObservation` in addition to
normal URI launch. This is separate from `ManagedSession`: Steam still does not
claim that Horizon owns the actual game inside Gamescope.

The same-user Horizon host helper resolves the Steam source/game identity and
checks host `/proc` for Steam's per-game `reaper` command line containing the
exact marker:

```text
SteamLaunch AppId=<appid>
```

Matching is numeric-AppID scoped and checks the token boundary, so AppID `440`
will not match `4400`. The Flatpak never sends a PID, executable path, process
name, or arbitrary search expression over D-Bus.

The observer is armed only after the normal OpenURI launch dispatch succeeds.
When the exact Steam reaper appears, Activity starts a `source_runtime` session;
when it disappears, that session ends. Window focus is irrelevant, so an
Alt-Tab into Horizon does not end Steam playtime.

If the helper is absent, incompatible, or cannot provide the runtime observer,
Steam remains launchable and Activity falls back to `ForegroundHandoff`.

Implementation provenance: the lifecycle marker is visible in Steam launch
logs and is also used by Lutris' Steam runner to follow native, Proton/Wine,
and Flatpak Steam launches. Horizon's Rust implementation is independent and
does not copy Lutris source code.

References:

- https://github.com/lutris/lutris/blob/master/lutris/runners/steam.py
- https://github.com/ValveSoftware/steam-for-linux/issues/8308


## Phase 9.5.29 square artwork

Steam now advertises `SourceCapability::Artwork`. The adapter exposes only local
Steam-owned icon candidates for the requested numeric AppID; generic presentation
code never inspects Steam paths or hashes.

Horizon checks `common.linuxclienticon` first for Steam's local
`steam/games/<hash>.zip` container and exposes each contained PNG as a candidate.
It also checks `common.clienticon` for `steam/games/<hash>.ico` and `common.icon`
for modern `appcache/librarycache/<appid>/<hash>.jpg`/PNG entries. Steam's legacy
`<appid>_icon.jpg`/PNG and newer per-AppID `icon.jpg`/PNG cache names remain
fallbacks.

The original Phase 9.5.29 implementation decoded the local icon candidates and
kept the largest source. Its temporary behavior of adapting unexpectedly
non-square inputs has been superseded by Phase 9.5.36: current Horizon rejects
any decoded candidate whose width and height differ.

Steam documents the compact App Icon as 184×184 JPG and the submitted Shortcut
Icon as 256×256 or 512×512 PNG/ICO. Horizon therefore treats 184px as a fallback
quality level, not a fixed ceiling, and uses whichever higher-resolution local
square representation Steam has actually cached.

No Steam artwork is bundled or redistributed by Horizon, and this phase does not
add general network access to fetch missing CDN assets. See `ARTWORK.md` and ADR
0049.

## Phase 9.5.36 square-only artwork

Horizon only uses Steam artwork whose decoded source dimensions are already
1:1. The Steam adapter exposes icon-oriented local cache candidates and does
not expose `library_600x900`, `library_capsule`, hero, or header assets as
primary game artwork.

Candidate locations include the Linux client icon ZIP, client ICO, hashed App
Icon cache entries, and legacy/per-AppID icon paths. `ArtworkService` decodes
them, rejects any candidate where `width != height`, and chooses the
highest-resolution remaining square.

A low-resolution 184×184 square icon is still valid. It is never replaced with
a sharper portrait image simply to improve resolution. If Steam has no usable
true-square source, Horizon falls back to its procedural cover.

The normalized Horizon cache is `square-v4`, which invalidates older cached
entries derived from non-square artwork.

## Phase 9.5.37 multi-resolution square icon extraction

Horizon now resolves Steam icon containers explicitly rather than handing the
container to a generic image decoder and accepting whichever representation it
chooses.

For `common.clienticon`, Horizon reads the local ICO directory, enumerates every
frame, rejects non-square or unreasonable representations, decodes every usable
frame, and selects the largest square. Higher bit depth breaks ties at the same
resolution. The selected frame is converted to PNG bytes before entering the
generic source-artwork boundary.

For `common.linuxclienticon`, Horizon enumerates every PNG in the local ZIP,
validates its decoded dimensions, rejects non-1:1 entries, and retains only the
largest valid square PNG from that archive.

The ordinary 184×184 App Icon remains a valid fallback. Horizon does not replace
it with portrait/landscape artwork when no larger square representation exists.

Artwork cache version `square-v5` forces an immediate re-resolution after this
change so a lower-resolution square chosen by an older decoder cannot remain
hidden behind a fresh cache entry.

## Phase 9.5.32 runtime tracking without the host helper

Steam playtime tracking in the ordinary Flatpak uses Steam's own
`logs/gameprocess_log.txt`, not host `/proc` and not the Horizon session helper.
The Steam adapter derives running state for the exact AppID from Steam's
tracked-process/running-list transitions. Horizon already has read-only access
to the supported Steam data roots, so this requires no additional filesystem
or D-Bus permission.

The optional host helper remains unrelated to normal Steam runtime tracking; it
is reserved for launch/session capabilities that genuinely require host-side
execution such as managed Gamescope sessions.

## Steam-reported lifetime playtime

Steam advertises `LifetimePlaytime`. The adapter reads the cumulative `Playtime` (minutes) per app from the active account's local `userdata/<account>/config/localconfig.vdf` and normalizes it to seconds. No Steam login, Web API, or network access is involved.

- The account is the one uniquely marked `MostRecent` in `config/loginusers.vdf` (SteamID64 minus `76561197960265728` gives the `userdata` folder name). Without a unique marker, a lone `userdata` account is accepted; with several accounts and no unambiguous marker, nothing is reported rather than guessed.
- Key lookup is case-insensitive because Steam clients have varied key casing.
- Zero, missing, malformed, or overflowing per-game entries are skipped; an unreadable or incomplete file is rejected without importing partial values, preserving previously persisted values.
- Equivalent native/Flatpak paths to the same account file are read only once. If multiple clients have different active Steam accounts, only the first successfully read account contributes a snapshot; unrelated accounts are not merged. For repeated apps within that account, the first discovered root wins. Values are never summed.
- The value is stored in `source_lifetime_playtime` through the existing `SourcePlaytimeSync` and is never added to Horizon-observed sessions (ADR 0044).
- `localconfig.vdf` is written by Steam periodically and on exit, so the total can lag a just-finished session. The periodic sync picks up later writes.

