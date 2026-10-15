# ADR 0054: Explicit Steam multi-resolution icon extraction

Status: Accepted

## Context

Horizon requires native 1:1 primary game artwork. Steam can expose square
artwork through container formats:

- `common.clienticon` points to an ICO, which may contain multiple resolutions
  and color depths;
- `common.linuxclienticon` points to a ZIP containing PNG representations.

Passing an ICO to a generic image loader leaves frame selection implicit. That
can cause Horizon to display a lower-resolution frame even when the same local
container contains a better square representation.

## Decision

The Steam adapter owns container extraction.

For ICO:
1. Parse the ICO directory with `ico`.
2. Enumerate every entry.
3. Reject zero-sized, non-square, or unreasonably large frames.
4. Decode usable frames.
5. Choose the largest square; use color depth as the tie breaker.
6. Convert only that selected frame to PNG bytes for the generic artwork
   boundary.

For Linux icon ZIPs:
1. Enumerate PNG entries within the existing per-entry size bound.
2. Decode enough to validate dimensions.
3. Reject non-square entries.
4. Keep only the largest valid square PNG from the archive.

The generic `ArtworkService` continues to validate the 1:1 invariant and then
chooses the largest candidate across provider-owned square sources.

A 184×184 App Icon remains valid when Steam has no larger square representation.

Cache version `square-v5` invalidates pre-9.5.37 normalized artwork so the new
selection policy takes effect immediately.

## Consequences

- Horizon can use a higher-resolution frame already present in the user's local
  Steam icon container without adding network access.
- ZIP/ICO extraction remains provider-specific and does not leak into Slint.
- Non-square Steam artwork remains ineligible.
- Games for which Steam truly supplies only a 184×184 square icon remain
  limited by that source resolution.
