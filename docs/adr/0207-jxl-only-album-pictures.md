# ADR 0207 — Canonical JPEG XL image storage

Status: Accepted. The user requested JXL screenshots, followed by JXL for
save-state previews, profiles, artwork caches and animated pictures. Videos and
authored SVGs keep their existing formats.

All newly stored raster images use lossless sRGB RGBA8 JPEG XL. This covers native
game screenshots, state previews, normalized profile stills/animations, normalized
local/SteamGridDB artwork and Album picture/video-poster thumbnails. The existing
runtime libjxl encoder is called through an original C shim on workers. Stills
and animations share validation, standard effort 7 and chunked output. Animations
use full composited frames, millisecond delays and the existing looping profile
policy. New persisted files retain pixels/timing, not original GIF/APNG metadata.
No external tool, host package, new Rust dependency or sandbox grant is needed.

Shared import decoders remain for images supplied by users, Steam and SteamGridDB.
JXL animations use the underlying jxl-oxide frame API, with bounded input,
decoder allocations and prepared frame memory. Profiles fit/crop images on their
workers; Album renders frames on a cancellable worker with generation rejection,
background pause and Reduced Motion. Errors are reported rather than silently
converting broken animations into stills. Album lists JXL pictures only; existing
PNG/JPEG/GIF Album files remain untouched and hidden. Video support remains
MP4, M4V, MKV, WebM and MOV.

Profile paths use `.jxl` for stills and animations; readers retain compatibility
with old PNG/extensionless copies. New save-state metadata is version 2 with JXL
previews; version-1 PNG bundles remain loadable and are preserved. Slot contents,
state checksums, exact core compatibility and rollback rules remain unchanged.
Artwork cache versions move to 7, SteamGridDB provenance schema to 3, and Album
thumbnail cache version 5 stores lossless JXL. Only disposable caches rebuild;
no old durable media or user originals are deleted or bulk-migrated.
