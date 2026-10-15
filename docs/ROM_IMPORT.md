# ROM import

Library → Add games (X / keyboard Delete / clickable hint) opens the GBA review
dialog. The same importer is available from [first-run setup](ONBOARDING.md).
Choose an optional Game Boy Advance Logiqx XML DAT, choose a ROM folder through
the desktop portal, then Review games. Confirm only the files you want.

Exact recognition compares full-file SHA-1 and byte size with the selected
catalog. Release titles retain the catalog's region/revision tags. Multiple
catalog titles for the same hash remain ambiguous. No match means filename-only,
unverified metadata; these files require explicit inclusion. A DAT for a different
system, malformed DAT or missing catalog is an error rather than a silent match.
No catalog or ROM data is downloaded, bundled or uploaded.

The [No-Intro DAT guide](https://wiki.no-intro.org/index.php?title=DAT-o-MATIC_Guide)
describes obtaining catalogs. Select a GBA XML DAT; binary RDB, text ClrMamePro,
ZIP archives and other consoles are outside this first implementation.

Scans recurse through regular folders, skip symlinks and unsupported extensions,
and bound file size (192 bytes–32 MiB), depth (16) and directory entries (20,000).
Per-file read/size failures appear in review; directory failures abort the scan.
An unverified extension/size is not proof of a valid GBA image or safe execution.

Confirmation rehashes selected files on a worker before committing the batch.
GBA plus SHA-256 is the durable content identity, independent of title/path and
future RetroAchievements identification. Identical copies share a GameId; equal
titles with different bytes do not. Unverified filenames cannot replace an earlier
catalog title. ROMs stay in place and are never modified, moved or deleted.

Portal paths and catalog provenance are saved in SQLite, with raw Unix path bytes
preserved. Missing/revoked access requires selecting the folder/catalog again;
it never prunes existing games. A later core must verify the selected content at
launch. No emulator/BIOS configuration, achievement hash or launch is implemented.
See [database](DATABASE.md), [storage](STORAGE.md) and [ADR 0193](adr/0193-reviewed-gba-import.md).
