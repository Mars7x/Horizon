# ADR 0233 — Per-game mods

Status: Accepted. Players wanted translations, fixes and romhacks without
editing their game files.

Each game folder has a `mods/` folder the user fills, and `mods.json` records
which mods are on; new mods start off. Manage mods in the game menu lists them,
toggles them and opens the folder. There is no download, catalog or archive
extraction.

Cartridge systems (GB, GBC, GBA, DS) take IPS, BPS and UPS patches, applied in
name order to an in-memory copy after the file is read. The imported SHA-256
check stays on the unpatched file, so identity, saves, sessions and artwork are
unchanged. BPS and UPS checksums reject a patch for another version, and the
launch stops with the patch's name rather than running a broken game.

3DS mods use Azahar's own LayeredFS. Horizon merges enabled folders into
Azahar's per-title mod folder inside the game's own `core-data`, as hard links
(copies across filesystems), and marks the folder it built. It rebuilds only a
marked folder; one made by hand is left as it is. Links and special files are
never followed. Azahar's source stays unmodified (ADR 0231).
