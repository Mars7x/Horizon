# ROM import

Choose game folder → scan → library. [First-run setup](ONBOARDING.md) opens the
system folder picker directly. Library has no Add games action or importer shortcut. Selecting a folder starts the scan and imports readable supported games
automatically. There is no catalog setup, per-file selection, review screen or
second confirmation. Cancelling the picker leaves the library/setup unchanged.

The chosen folder stays live. Horizon checks it again at startup and every
10 seconds while running, on a worker, and adds new supported files silently.
Files already imported at the same size are not reread, and a file changed in the
last 5 seconds (still copying) waits for a later check.

A game whose files have all been deleted from the folder leaves Home and Library
after the next complete check. It is retired, not deleted: its GameId, title,
play history, saves, states and Album pictures stay, and if the same file comes
back under any name it returns as the same game. Only files under the chosen
folder are checked. A check that cannot read the folder, or finds none of the
folder's imported files (an unmounted drive, say), changes nothing; deleting
every game at once therefore keeps them until at least one imported file is seen
again. A refresh never changes the saved setup, and an unavailable folder only
logs a warning (once until a check succeeds again). See
[ADR 0210](adr/0210-live-game-folder.md) and
[ADR 0215](adr/0215-deleted-games-leave-the-library.md).

The current backend supports raw Game Boy (`.gb`), Game Boy Color (`.gbc`) and
Game Boy Advance (`.gba`), Nintendo DS (`.nds`) and decrypted Nintendo 3DS
(`.3ds`, `.cci`, `.cxi`, `.3dsx`) files, each its own Library source. Scans recurse through regular folders,
skip symlinks and unsupported extensions, and bound file size (192 bytes–32 MiB for GB/GBC/GBA; 4 KiB–512 MiB for DS),
depth (16) and directory entries (20,000). Readable files are included automatically.
Empty/unsupported folders and directory errors offer only retry or Back. If some
files cannot be read, readable games are imported and one brief warning reports
the skipped count. No supported readable files means no database write.

Recognition remains behind the source boundary. Bundled offline No-Intro/Libretro
catalogs, one per system, match full-file SHA-1 plus byte size. Exactly one matching title is
required, regardless of the ROM's filename. Stored catalog release names retain their
region/revision tags. Displayed titles omit recognized parenthesized region labels
(for example, `(USA, Europe)`), including for existing imports; edition, language
and revision labels remain intact. Ambiguous/unmatched files use filename metadata marked
unverified internally; no catalog title is guessed. SteamGridDB supplies artwork
only and cannot name or identify a ROM.

`.gb` and `.gbc` files share a format, so the extension does not decide their
system. A hash listed in exactly one of the Game Boy and Game Boy Color catalogs
decides; otherwise the cartridge header's colour flag (byte `0x143`, bit 7) does.
Dual-mode games, which run on both, are Game Boy Color games, as No-Intro lists
them, and play in colour.

An existing local XML DAT configuration takes precedence for the system its
header names. If it is unavailable, the failure is logged and the bundled catalogs are used. There is no extra setup,
catalog picker, runtime catalog download or ROM/hash upload. On startup, a worker
also recognizes earlier filename imports using their saved SHA-1/size records.
It preserves GameIds, paths, history and previously verified catalog titles.

Selected bytes are rehashed on a worker before the batch commits. The system's
source plus SHA-256 is the durable identity, independent of title/path and future RetroAchievements
identification. Copies share a GameId; equal titles with different bytes do not.
A filename fallback never replaces an existing verified catalog title. Import leaves ROMs in place without modifying or moving them. Only a separate,
explicitly confirmed [Uninstall action](GAME_OPTIONS.md#uninstall) deletes registered ROM files.

Portal paths and catalog provenance remain in SQLite, preserving raw Unix path
bytes. Missing/revoked access requires selecting the folder again; failed or
incomplete scans never prune existing games. A later core must verify content at
launch. [Playback](GBA_PLAYBACK.md) performs this check and runs the bundled
core. BIOS setup and achievement hashing remain unimplemented.
See [database](DATABASE.md), [storage](STORAGE.md) and
[ADR 0195](adr/0195-automatic-folder-import.md).

The bundled catalogs contain metadata only, pinned to one upstream revision under
`CC-BY-SA-4.0`; see [third-party notices](../THIRD_PARTY_NOTICES.md),
[ADR 0197](adr/0197-offline-hash-title-recognition.md) and
[ADR 0209](adr/0209-game-boy-and-game-boy-color.md). To regenerate one, obtain
the pinned DAT that `scripts/generate-hash-catalog.py` names for it and run
`python3 scripts/generate-hash-catalog.py gb INPUT.dat data/catalogs/gb.xml`
(or `gbc`/`gba` with `gbc.xml`/`gba.xml`).
The generator verifies the source checksum. Catalog updates require reviewing
the new revision, checksum, attribution and recognition fixtures together.

## Nintendo DS

`.nds` files use a separately pinned offline No-Intro catalog with the same full
hash and size matching. Filenames do not determine recognized titles. DS files
can be up to 512 MiB; both ARM executable sections must fit within the file.
DSi-exclusive headers are rejected. DS-enhanced games run in DS mode.
GB/GBC/GBA keep their 32 MiB limit. Launch verifies the complete snapshot again.
See [ADR 0216](adr/0216-nintendo-ds-playback.md).

Nintendo 3DS uses the separately approved Azahar Vulkan core; see
[3DS playback](THREE_DS_PLAYBACK.md) for import formats, controls, isolated saves and initial limits.
