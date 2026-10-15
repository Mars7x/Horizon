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

Recognition remains behind the source boundary. An existing local GBA XML DAT
configuration is used automatically for full-file SHA-1 plus size matching. Its
release titles retain region/revision tags. Ambiguous/unmatched files use filename
metadata marked unverified internally; no catalog title is guessed. An unavailable
saved catalog logs the failure and falls back to unverified filenames rather than
blocking folder import. There is no catalog picker, bundled catalog or background
download in this flow. ROM data is never uploaded.

Selected bytes are rehashed on a worker before the batch commits. GBA plus SHA-256
is the durable identity, independent of title/path and future RetroAchievements
identification. Copies share a GameId; equal titles with different bytes do not.
A filename fallback never replaces an existing verified catalog title. ROMs stay
in place and are never modified, moved or deleted.

Portal paths and catalog provenance remain in SQLite, preserving raw Unix path
bytes. Missing/revoked access requires selecting the folder again; failed or
incomplete scans never prune existing games. A later core must verify content at
launch. No emulator/BIOS setup, achievement hash or launch is implemented.
See [database](DATABASE.md), [storage](STORAGE.md) and
[ADR 0195](adr/0195-automatic-folder-import.md).
