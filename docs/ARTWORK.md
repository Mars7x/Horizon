# Artwork policy

Horizon treats primary game artwork as a source-neutral **1:1 square asset**.
Home, Library, and future game-detail surfaces must consume the same normalized
artwork contract rather than knowing how Steam, Bottles, or Heroic store their
images.

## Phase 9.5.29 artwork foundation

The production path is now:

```text
GameSource
  └─ artwork_candidates(ExternalGameId)
          ↓
     SourceArtworkCandidate
          ↓
      ArtworkService
          ↓
  decode + choose best local candidate
          ↓
       normalize 1:1
          ↓
       GameCardData
          ↓
        Slint UI
```

Source adapters expose provider-owned candidates only. They do not decide how
Horizon crops, pads, scales, or renders those assets. Slint receives only the
normalized presentation image and a `has-artwork` flag.

## 1:1 invariant

- Primary game artwork is always represented as a square RGBA pixel buffer.
- Already-square source artwork is preserved without cropping or stretching.
- A non-square future source is centered on a transparent square canvas rather
  than stretched or silently cropped.
- Sources larger than 512×512 are downscaled to at most 512×512 for the current
  Home presentation path.
- Smaller authoritative artwork is **not upscaled in storage/memory merely to
  claim a higher resolution**. The UI may naturally scale it at render time.
- Missing/broken artwork always falls back to the procedural `FallbackCover`.

This keeps the source contract stable while allowing a later disk cache or
higher-resolution detail view without source-specific Slint code.

## Steam square artwork

Steam advertises `SourceCapability::Artwork`. Horizon reads artwork already
owned and cached by the local Steam installation; Phase 9.5.29 does not grant
Horizon general runtime network access.

For each installed AppID the Steam adapter exposes the available square icon
candidates it can prove belong to that AppID, including:

1. Steam's local `linuxclienticon` ZIP from `steam/games/<hash>.zip` when
   `common.linuxclienticon` is present; every PNG representation in the archive
   is offered to the generic decoder so the largest usable square can win;
2. Steam's local `clienticon` ICO container from `steam/games/<hash>.ico` when
   `common.clienticon` is present;
3. the AppID/hash image in modern `appcache/librarycache/<appid>/...` layouts;
4. Steam's legacy `<appid>_icon.jpg`/PNG cache names;
5. the newer per-AppID `icon.jpg`/PNG filename fallback.

`ArtworkService` decodes every available candidate and chooses the one with the
largest actual pixel area. This matters because the Linux ZIP or ICO may contain
a better square representation than Steam's compact 184×184 app icon, while
older games can still have small client icons. A lower-resolution client icon
therefore cannot displace a larger cached App Icon simply because it was listed
first.

Steam documents the compact App Icon as 184×184 JPG and its Shortcut Icon as a
256×256 or 512×512 PNG/ICO submission. Horizon prefers the best **locally
available** square representation rather than hard-coding 184×184 as the
artwork quality ceiling.

## Rendering

`GameTile` always reserves the same square artwork frame. If real artwork is
available it fills that 1:1 frame; otherwise `FallbackCover` renders in exactly
the same geometry. Selection/launch press animation therefore does not change
artwork aspect ratio or source policy.

The decoded source is retained at native resolution up to 512×512. This avoids
unnecessary 184→512 preprocessing while still preventing extremely large source
images from multiplying Home memory use.

## Fallback artwork

`FallbackCover` remains source-neutral procedural Slint geometry. Palette and
monogram values derive from Horizon's durable game identity/title, never from a
provider name. It is the required fallback for:

- sources that do not advertise artwork;
- games whose provider cache has no usable square art;
- unreadable or corrupt artwork files.

The fallback is resolution-independent and remains valid for Steam, Bottles,
Heroic, and future source adapters.

## Provenance and licensing

Horizon does not bundle or redistribute Steam game artwork. It displays files
already present in the user's Steam installation/cache. Those images remain the
property of their respective game publishers/developers.

The Rust `image` crate is used only for decoding and square normalization; its
license/provenance is recorded in `THIRD_PARTY_NOTICES.md`.
