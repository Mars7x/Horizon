# ADR 0223 — Image-only Album

Status: Accepted. The user explicitly retired Album clip playback and requested
removal of its references throughout the app.

Album lists JPEG XL screenshots and animated images only. Remove the media-type
filter, clip badges/durations, play/pause and seeking UI, decoder contracts,
FFmpeg/PipeWire movie backend and native FFmpeg shim/build linkage. Game
rendering and audio, screenshot capture and animated image viewing remain.
The Game filter and controller/pointer image navigation retain their shared
layout, focus, transitions and deletion confirmation behavior.

Scans visit screenshots folders only. New capture paths reject unsupported
extensions. Existing recording folders/files are ignored and preserved; no
migration deletes or converts user data. Album deletion cannot select or delete
those files. Thumbnail image cache keys and format stay compatible; clip
metadata is no longer produced or consumed. Historical ADRs retain the decisions
that applied when written and do not authorize reintroducing clip playback.

Regression coverage checks ignored recording files, rejection of new clip paths,
retained images, image thumbnail caching and viewer navigation. Full native,
packaged runtime and supported-architecture verification are separate checks.
