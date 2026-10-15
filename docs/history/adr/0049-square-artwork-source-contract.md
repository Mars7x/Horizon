# ADR 0049: Square source-artwork contract

## Status

Accepted for Phase 9.5.29.

## Context

Horizon is about to grow a production Library UX. If each provider feeds raw
artwork directly into Slint, Home and Library would accumulate provider-specific
paths, aspect-ratio rules, and quality fallbacks. Steam also exposes several
local icon representations whose actual resolution can differ.

## Decision

Primary game artwork is a generic source capability and the presentation
contract is always 1:1.

`GameSource::artwork_candidates` exposes provider-owned candidates. A generic
`ArtworkService` decodes those candidates, chooses the highest-resolution usable
local representation, and normalizes it to a square RGBA buffer. The service
pads non-square input transparently, never stretches it, never upscales small
sources as preprocessing, and caps oversized Home artwork at 512×512.

Steam is the first production provider. It supplies local Linux-client-icon,
client-icon, and App-Icon cache candidates using AppID plus `appinfo.vdf` hashes
and known Steam cache layouts. `linuxclienticon` ZIP containers are read in
memory and their PNG representations enter the same generic quality selection;
Steam archive paths never reach Slint. No Steam-specific path or AppID reaches
presentation code.

Phase 9.5.29 remains local-only. It does not add general Flatpak network access
just to fetch missing images from a CDN. Missing source art uses Horizon's
existing procedural fallback.

## Consequences

- Home and the future Library page share one square-artwork model.
- Steam may use a larger local Linux/client icon when one exists instead of
  being permanently limited to the compact 184×184 App Icon.
- Bottles/Heroic can add artwork later by implementing the same source method.
- A future persistent artwork cache can sit behind `ArtworkService` without
  changing Slint or provider identity rules.
- Source artwork remains optional and cannot make source import fail.
