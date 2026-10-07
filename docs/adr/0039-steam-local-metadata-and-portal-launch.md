# ADR 0039: Steam local metadata and portal launch

- Status: Accepted
- Date: 2026-10-06

## Context

Phase 7 needs the first end-to-end provider implementation while preserving the generic source architecture established in Phase 6. Steam can exist as a native installation or as `com.valvesoftware.Steam`, and Horizon itself is Flatpak-first. Directly spawning a host `steam` executable would couple launch behavior to an installation form that may not be visible inside the sandbox. Broad filesystem access would likewise defeat the project's Flatpak boundary.

Steam's local installation already exposes the metadata needed for an installed-library slice: `libraryfolders.vdf`, `appmanifest_*.acf`, and binary `appinfo.vdf`. Current `appinfo.vdf` variants are non-trivial binary VDF and should not be reimplemented casually inside Horizon.

## Decision

Steam is a first-class `GameSource` in `src/sources/steam.rs`.

Discovery uses Steam's local metadata only. As refined by ADR 0042, readable libraries use actual `appmanifest_<appid>.acf` membership first, while the `apps` maps in `libraryfolders.vdf` remain the installed-membership fallback for external libraries outside Horizon's sandbox. `appcache/appinfo.vdf` enriches that data with canonical names/type filtering and supplies names for libraries whose manifests are not sandbox-readable. A missing/unreadable/unparsable appinfo cache is therefore a degradable metadata loss, not permission to discard valid manifest-backed games. Binary/text VDF parsing uses the existing `steam-vdf-parser` crate, whose provenance and Apache-2.0 OR MIT licensing are recorded with the repository.

The adapter advertises the generic `Launch` capability and produces `SourceLaunchTarget::Uri("steam://rungameid/<appid>")`. A generic `GameLaunchService` chooses a launch-capable registered source from a `LibraryGame`. `src/platform/launcher.rs` executes URI targets through XDG OpenURI using `ashpd`. Generic services do not compare provider names.

The Flatpak receives narrowly scoped read-only permissions for native Steam's XDG data roots and the single Flatpak-Steam app-data subtree `~/.var/app/com.valvesoftware.Steam`. The Flatpak-Steam parent grant is required because real installations may resolve metadata across `.local/share` and `data`; granting only deeper child paths is not reliable. Horizon still does not receive `home`, `host`, or broad external-library filesystem access.

The hard-coded demo Home catalog is removed. Persisted `LibraryGame` values drive Home through `HomeController`; Slint still sees presentation-only card data. Missing real artwork uses a renamed source-neutral `FallbackCover` rather than a Steam-specific visual path.

## Consequences

- Native and Flatpak Steam can share one source adapter and one launch path. Flatpak Steam's app-data permission is one read-only provider-scoped subtree rather than a broad home/host grant.
- Horizon does not need Steam credentials, Steam Web API access, or runtime network access for Phase 7.
- Installed games in external Steam libraries can normally be identified without mounting those libraries because modern `libraryfolders.vdf` includes app IDs and the primary Steam root's appinfo cache can supply their names.
- A broken or unavailable appinfo cache no longer zeroes a readable manifest-backed library; discovery degrades to the subset Horizon can prove locally.
- Steam-specific VDF/path quirks remain in `src/sources/steam.rs`.
- Desktop URI activation remains in the platform layer and can serve later sources that expose URI launch targets.
- The presentation layer remains source-neutral.
- `steam-vdf-parser` becomes a runtime dependency with required Apache-2.0/MIT attribution.
- The original Phase 7 additive-only refresh is superseded by ADR 0042: complete Steam scans may reconcile authoritative installed membership, while partial/failed scans remain non-destructive.
- Steam artwork and playtime remain future work rather than hidden additions to this slice.
