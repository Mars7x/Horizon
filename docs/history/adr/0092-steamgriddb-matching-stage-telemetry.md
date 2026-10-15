# ADR 0092: Stage-level diagnostics for SteamGridDB artwork

## Status
Accepted as a diagnostic phase, pending validation against GNOME Builder and actual Flatpak runtime.

## Context
The host-side API key probe succeeded for Team Fortress 2; a read-only probe of Horizon's actual library confirmed exact Steam AppID matching and returned square grid metadata for A Short Hike, Black Mesa, and Cyberpunk 2077. Yet Horizon's cache contained zero positive artwork entries and 36 negative v3 entries. This evidence does not prove the Flatpak worker succeeded at any stage.

## Decision
The worker reports stage-specific counts and one representative failure (with game title, but no credential). It logs match rejection and failed CDN decoding as info-level diagnostics. Existing v2/v3 miss markers are ignored by advancing to v4 with the same five-minute TTL. A failure to contact the API or CDN never produces a negative-cache marker.

## Follow-up
Capture the new Settings → Third-Party status after a fresh run. If matched=0, investigate the source registry/platform ID plan or request deserialization. If matches are positive but missing-art dominates, investigate the returned API candidate metadata. If rejected-download dominates, inspect the image validation or Flatpak CDN connection. Treat host and Flatpak network access as distinct. Do not ship permanent verbose logs if they are no longer necessary.
