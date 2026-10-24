# ADR 0233 — Per-game mods

Status: Accepted, for 3DS games only. Players wanted translations, fixes and
texture packs without editing their game files.

Each game folder has a `mods/` folder the user fills, and `mods.json` records
which mods are on; new mods start off. Manage mods in game options lists them,
switches them and opens the folder. It appears only for 3DS games; no other
system takes mods. There is no download, catalog or archive extraction.

3DS mods use Azahar's own LayeredFS. Horizon merges enabled folders into
Azahar's per-title mod folder inside the game's own `core-data`, as hard links
(copies across filesystems), and marks the folder it built. It rebuilds only a
marked folder; one made by hand is left as it is. Links and special files are
never followed. Azahar's source stays unmodified (ADR 0231).
