# ADR 0057 — SteamGridDB v2 client and conservative matching

Status: Accepted for Phase 9.5.40

## Decision

Implement an on-demand SteamGridDB API v2 metadata client, and a service that
matches library games without checking `SourceId` strings in generic code.
Extend the source adapter interface with `external_artwork_id`, an optional
cross-catalog identity. Steam implements the authoritative Steam AppID variant.
Neither Heroic's Legendary app name nor Bottles' internal program ID is
assumed to be a SteamGridDB platform ID.

The matcher gives authoritative platform IDs precedence. A failed exact Steam
lookup must not silently select a similarly named but potentially different
game. In the absence of an external catalog ID, use autocomplete and accept
only a unique, exact, case/whitespace-normalized title. Ambiguous matches
require a later explicit user-selection UI.

Only metadata for static 512×512/1024×1024 PNG/JPEG grids is requested. Reject
insecure or off-domain returned image URLs. Preserve author and asset IDs for
future provenance. Actual image fetching, artwork source order, permanent match
choices, quality comparisons, and attribution presentation are deferred.

## Network and credentials

The saved personal key stays inside the SettingsService/SteamGridDbClient
boundary and is sent only as the `Authorization: Bearer` request header to the
fixed HTTPS API base. Do not include the key in URLs or logs. Disable redirects,
limit JSON metadata size and timeouts, and distinguish authorization failure,
rate limiting, missing records and temporary API failures. Retry one transient
502–504 response with short bounded delay; do not hammer 429 responses.

The Flatpak must have `--share=network` to reach the public API; Flatpak's
sandbox cannot confine this grant to a single host. Never make blocking HTTP
calls on the Slint event loop. Phase 9.5.40 deliberately does not initiate
background work or automatic API calls yet; the consumer is Phase 9.5.41.

## API and third-party attribution

Specification: https://www.steamgriddb.com/api/v2 (SteamGridDB API v2).
Metadata is not the artwork itself. Uploaded images have their own authors and
rights. Before locally caching downloaded images, preserve image identifiers,
source URLs, author records, and credit/licensing evidence where available.
The Rust HTTPS transport is `reqwest 0.12` (MIT OR Apache-2.0, upstream
https://github.com/seanmonstar/reqwest); it is used unmodified.

Source metadata is captured into an owned `SteamGridDbLookup` plan before
worker dispatch. The blocking client and matcher hold no `Rc<SourceRegistry>`,
Slint handle, or non-thread-safe source adapter; worker integration therefore
will not need to move the application registry off its owning thread.
