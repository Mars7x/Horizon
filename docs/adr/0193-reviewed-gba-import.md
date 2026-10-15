# ADR 0193 — Reviewed local GBA imports

Status: Accepted for identity/retention. Manual catalog/review interaction is
superseded by [ADR 0195](0195-automatic-folder-import.md). First-run integration
is covered by [ADR 0194](0194-first-run-onboarding.md).
The catalog supply policy is superseded by [ADR 0197](0197-offline-hash-title-recognition.md).

Start with raw Game Boy Advance cartridges before integrating an emulator. Users
choose a folder, inspect matches, then confirm registration. Exact title/region
recognition uses a user-supplied GBA XML DAT, full-file SHA-1 and size. Filename
fallbacks remain explicitly unverified; ambiguous hashes never choose a title.
No catalog is redistributed or silently fetched.

Use GBA plus SHA-256 for Horizon identity, retain multiple file locations and
provenance, and commit selected imports atomically. Do not infer identity from a
title or conflate catalog hashes with future RetroAchievements identifiers.
Unavailable folders and incomplete scans cannot remove games. Portal grants
replace broad filesystem access. File work stays on workers; SQL remains in
persistence. Reuse the shared catalog/page refresh after confirmation.

Register GBA without launch capabilities. Preserve Home's established UI and
show playback as unavailable until a separately designed core integration exists.
The reusable import dialog is available from Library and first-run setup.
