# Artwork

Primary game tiles use native square artwork. Do not crop, pad, blur-fill or
convert landscape/portrait art into a square substitute. Decode and verify
width equals height before presentation; fallback covers are procedural and do
not imply downloaded content.

[ArtworkService](../src/services/artwork.rs) selects/decodes candidates behind
the generic source boundary. No production game adapters currently supply local
art or launch cover jobs. User-selected/cached files and provenance remain;
cleanup must not discard them or reintroduce Steam installation scanning.

## SteamGridDB

[The client](../src/services/steamgriddb.rs) uses its own optional key from
[Settings](SETTINGS.md). [Background workers](../src/services/steamgriddb_artwork.rs)
perform lookup, downloading and refresh off the UI thread. Registered adapters
may provide authoritative external catalog IDs; otherwise lookup uses conservative
title matching. Do not infer Steam AppIDs from another provider's numeric IDs.
Eligible square grids rank by score with deterministic ties, not upload recency.

Requests and public-image URLs use explicit HTTPS host validation, bounded
responses and no credential-bearing UI/log errors. Missing/ambiguous matches,
network failures or invalid art use a neutral fallback and allow later retry;
never fabricate a catalog identity. SteamGridDB is independent of Steam Account.

## Rendering and cache

Canonical artwork/cache metadata retain the native-square contract and provenance.
Pixel-art-like sources use nearest-neighbor normalization and Slint pixelated
rendering; smooth imagery uses appropriate smooth filtering. Scale cover artwork
and focus as one visual unit. Cache-version changes invalidate only the relevant
disposable cache, not explicit user data. See [Storage](STORAGE.md).

Authored utility SVGs are separate from game artwork: preserve their paths,
colors, filters and scale-aware renderer. [Third-party notices](../THIRD_PARTY_NOTICES.md)
and [Licensing](LICENSING.md) govern bundled assets; downloaded game artwork is
not bundled or relicensed as Horizon content.
