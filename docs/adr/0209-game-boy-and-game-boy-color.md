# ADR 0209 — Game Boy and Game Boy Color through mGBA

Status: Accepted. The user directed Game Boy and Game Boy Color support through
the existing mGBA core, as two Library systems, with dual-mode games filed and
played as Game Boy Color games and mGBA's default Game Boy palette for now.

The pinned mGBA 0.10.5 build enables its Game Boy core alongside GBA. No other
core, process abstraction, BIOS policy, dependency or sandbox permission is added;
the player, worker, saves, states, speeds, filters and controls are shared.

`gb` ("Game Boy") and `gbc` ("Game Boy Color") are sources beside `gba`, each
with SHA-256 content identity. `.gb`/`.gbc` files are classified by an exact hash
in exactly one of the bundled No-Intro catalogs, else by the header colour flag,
never by extension alone. The catalogs come from the same pinned libretro-database
revision and checksum-verified generator as the GBA catalog, keep `CC-BY-SA-4.0`
attribution, and contain metadata only. No migration is needed; existing GBA
imports, saves and states are unchanged.

mGBA autodetects the hardware model. Super Game Boy colours are kept but its
border is off, so every Game Boy picture is 160×144. GBA colour correction stays
GBA-only. Battery RAM and any MBC3 cartridge clock are saved together in the
standalone mGBA `.sav` layout, restored before the first frame. A Game Boy
palette choice, Super Game Boy borders and other models may follow as curated
mGBA settings; SameBoy or another core would need its own decision.
