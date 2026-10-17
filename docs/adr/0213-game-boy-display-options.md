# ADR 0213 — Game Boy display options

Status: Accepted. Follows ADR 0209, which left the Game Boy palette and Super Game
Boy border for later; the user asked for this Game Boy polish.

Cores → mGBA gains two curated settings, both read at launch like the others.
Game Boy palette offers Grey (default), Original green, Pocket, Light and Game Boy
Color, a few of mGBA's own presets; Game Boy Color uses the GBC's built-in colours
for games it knows and grey otherwise. It affects original Game Boy games only:
Super Game Boy and colour games bring their own colours. Super Game Boy border
(default off) passes mGBA's border option, making those games 256×224; state
previews accept that size. Existing `cores/mgba.json` files keep working.

Colour correction now also applies to Game Boy Color games. Upstream's gbc-color
sRGB profile, by the same public-domain authors, is identical to the gba-color
profile Horizon already adapts, so no new matrix or licence is added.

The Library's Source filter lists the cartridge systems in release order (Game Boy,
Game Boy Color, Game Boy Advance), and button mapping labels L and R as GBA only.
No dependency, permission, migration or core change is added.
