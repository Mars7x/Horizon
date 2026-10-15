# ADR 0197 — Name imported games from exact ROM hashes

Status: Accepted. Supersedes the user-supplied-only catalog supply policy of
[ADR 0193](0193-reviewed-gba-import.md). Identity/retention rules remain.

The user wants names derived from ROM hashes. Filename normalization and
SteamGridDB title searches cannot establish ROM identity. Bundle a pinned,
attributed No-Intro/Libretro GBA hash/title snapshot so the existing minimal
folder import recognizes known bytes offline with no additional setup.

Generate only SHA-1, byte size and unchanged release titles from the pinned
upstream DAT; retain the data's CC-BY-SA-4.0 license and attribution separately
from Horizon's GPL code. The generator validates the upstream checksum. An
existing configured XML DAT keeps precedence; missing configuration uses the
bundled snapshot. There is no runtime catalog fetch or ROM/hash upload.

A unique SHA-1 plus size match supplies the catalog title, independent of filename.
Unknown/ambiguous files keep unverified fallback names; do not guess an identity
from a cartridge header, fuzzy title or artwork provider. The snapshot is finite,
so homebrew, hacks and later releases may remain unrecognized.

Before UI creation, a startup worker upgrades prior unverified imports from saved
hashes in an atomic transaction. Preserve verified catalog titles, unknown or
conflicting hashes, GameIds, source identities, paths and session/history data.
GBA plus SHA-256 remains Horizon's durable identity; achievement hashing remains
future work. SteamGridDB remains artwork only. No emulator API, migration,
dependency or sandbox permission is added.
