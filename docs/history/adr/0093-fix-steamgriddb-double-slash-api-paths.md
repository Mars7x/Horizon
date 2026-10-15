# ADR 0093: Fix SteamGridDB API path assembly and retry old negative cache

## Status
Accepted

## Context

Phase 9.5.44.35 stage diagnostics reported 0/36 matched games, with the first
failure `not matched: A Short Hike (platform ID)`. Independent, read-only
requests using the saved key and the exact Steam AppID 1055540 correctly found
SteamGridDB game 2978157 and eligible square art. This ruled out an invalid
key, missing upstream metadata, and missing source-owned platform identity
for the sampled title.

The API URL builder initialized `Url` with a trailing slash (`/api/v2/`) and
used `path_segments_mut().extend(...)`. Per the `url` crate semantics,
`extend` appends a slash even when the existing path's last segment is empty.
The resulting double-slash paths are not the documented SteamGridDB endpoints.
A 404 to such a path is considered an absent game and written to the negative
cache, leading to blanket false misses.

## Decision

1. Call `pop_if_empty()` before `extend(...)` in the shared API URL builder.
2. Add regression assertions for the full Steam-ID, title search, grids, and
   icons URLs, including encoded titles.
3. Advance negative-cache suffix to `.miss-v5` so `.miss-v2`, `.miss-v3`, and
   `.miss-v4` entries cannot suppress the first correctly formed requests.
4. Preserve exact AppID matching, the existing quality filters, provenance,
   source-art fallback, and stage-specific diagnostics.

## Consequences

The change applies to all SteamGridDB metadata endpoint paths. It does not
add permissions, dependencies, or modify the image download host. This fixes
a verified request-path defect, but actual Flatpak runtime success must be
checked in GNOME Builder after rebuilding.
