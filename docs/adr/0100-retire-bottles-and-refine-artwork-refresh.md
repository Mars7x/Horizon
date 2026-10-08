# ADR 0100: Retire Bottles and refine manual artwork progress

## Status
Accepted — Phase 9.5.44.43

## Decision
- The modal refresh progress bar is anchored at x=0, y=0 and clipped to the
  rounded progress track, filling from left to right. The heading is
  `Refreshing artwork` (no ellipsis); actual completed/total counters remain.
- Retire Bottles from Horizon's **production source registry**, drop the adapter
  and its now-unneeded YAML dependency, and revoke its two Flatpak metadata grants.
  Neither the app nor the session helper will resolve Bottles launch targets.
- Schema migration 0007 removes active `game_sources` membership for Bottles.
  Unreferenced games without Activity or lifetime history are removed; games
  with historical play sessions or playtime remain in history but not Library.
- Retain historical ADRs and the host D-Bus helper architecture rather than
  rewriting past decisions. Current providers are Steam and Heroic.

## Heroic artwork research (not implemented in this patch)
Horizon currently only advertises `Launch` in the Heroic source. Heroic's
Legendary game metadata includes `keyImages` entries for `DieselGameBox`,
`OfferImageWide`, `DieselGameBoxTall`, `OfferImageTall`, and possibly game-specific
other types. Heroic's `art_square` field often refers to **tall cover art**,
not a natively square image. Source details:
- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/storeManagers/legendary/library.ts
- https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/images_cache.ts

For a future keyless Heroic `Artwork` capability: associate game ID with
Legendary local metadata and Heroic's local URL-hashed image cache. Inspect
actual decoded image dimensions, rank true 1:1 assets by native resolution,
normalize the best to 512x512, and preserve the strict no-crop/no-stretch
contract. Heroic's `art_square` name alone must not qualify a portrait image.
If no genuine 1:1 is locally available, keep Horizon's neutral fallback cover.
Never assume the Heroic app's cache has high-res square artwork for every game.
