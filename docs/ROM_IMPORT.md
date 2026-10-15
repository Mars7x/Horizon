# ROM import

Choose game folder → scan → library. Library → Add games (X / keyboard Delete /
clickable hint) and [first-run setup](ONBOARDING.md) open the system folder picker
directly. Selecting a folder starts the scan and imports readable supported games
automatically. There is no catalog setup, per-file selection, review screen or
second confirmation. Cancelling the picker leaves the library/setup unchanged.

The current backend supports raw GBA files. Scans recurse through regular folders,
skip symlinks and unsupported extensions, and bound file size (192 bytes–32 MiB),
depth (16) and directory entries (20,000). Readable files are included automatically.
Empty/unsupported folders and directory errors offer only retry or Back. If some
files cannot be read, readable games are imported and one brief warning reports
the skipped count. No supported readable files means no database write.

Recognition remains behind the source boundary. A bundled offline No-Intro/Libretro
GBA catalog matches full-file SHA-1 plus byte size. Exactly one matching title is
required, regardless of the ROM's filename. Catalog release names retain their
region/revision tags. Ambiguous/unmatched files use filename metadata marked
unverified internally; no catalog title is guessed. SteamGridDB supplies artwork
only and cannot name or identify a ROM.

An existing local GBA XML DAT configuration takes precedence. If it is unavailable,
the failure is logged and the bundled catalog is used. There is no extra setup,
catalog picker, runtime catalog download or ROM/hash upload. On startup, a worker
also recognizes earlier filename imports using their saved SHA-1/size records.
It preserves GameIds, paths, history and previously verified catalog titles.

Selected bytes are rehashed on a worker before the batch commits. GBA plus SHA-256
is the durable identity, independent of title/path and future RetroAchievements
identification. Copies share a GameId; equal titles with different bytes do not.
A filename fallback never replaces an existing verified catalog title. ROMs stay
in place and are never modified, moved or deleted.

Portal paths and catalog provenance remain in SQLite, preserving raw Unix path
bytes. Missing/revoked access requires selecting the folder again; failed or
incomplete scans never prune existing games. A later core must verify content at
launch. [GBA playback](GBA_PLAYBACK.md) performs this check and runs the bundled
core. BIOS setup and achievement hashing remain unimplemented.
See [database](DATABASE.md), [storage](STORAGE.md) and
[ADR 0195](adr/0195-automatic-folder-import.md).

The bundled catalog contains metadata only, pinned to an upstream revision under
`CC-BY-SA-4.0`; see [third-party notices](../THIRD_PARTY_NOTICES.md) and
[ADR 0197](adr/0197-offline-hash-title-recognition.md). To regenerate, obtain the
pinned DAT linked in `scripts/generate-gba-catalog.py` and run
`python3 scripts/generate-gba-catalog.py INPUT.dat data/catalogs/gba.xml`.
The generator verifies the source checksum. Catalog updates require reviewing
the new revision, checksum, attribution and recognition fixtures together.
