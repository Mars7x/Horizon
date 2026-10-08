# ADR 0075 — Retry artwork misses and use vector focus lighting

## Issue

The SteamGridDB progress line may report every game as a recent miss because
negative-cache markers survive for a full day, masking whether corrected
matching and artwork rules work. The previous 3DS-inspired selection used four
PNG blur masks that look like a diffuse neon glow rather than the handheld UI.

## Decision

- Version negative-cache markers from `miss-v2` to `miss-v3`, invalidate older
  entries without deleting source artwork, and shorten the retry duration from
  24 hours to five minutes. This is NOT a claim that the API always returns art.
- Preserve exact Steam AppID resolution first. When the platform does not map
  to any SteamGridDB game, attempt only an unambiguous, normalized exact-name
  match; never choose a fuzzy name or among conflicting platform IDs.
- Include SteamGridDB square PNG icons at 256, 512, 768, or 1024 native pixels
  when no 512/1024 square grid is available. Do not accept arbitrary aspect
  ratios or animated assets. Higher native resolution is preferred and the
  existing pixel-aware 512px normalization remains authoritative.
- Replace the PNG glow masks with very faint, broader native vector strokes
  directly behind the same original four sharp paths. Each bracket retains its
  accent colour and minor diagonal motion. The tile itself never floats.
- Keep Reduced Motion and High Contrast static with no extra lighting.

## Limitations and verification

No SteamGridDB API key was available for live integration testing. If fresh
lookups still find no usable artwork, read the new per-pass status rather than
assuming the cache is the sole cause. Test under GNOME Builder with Steam and
Heroic games, key changes, offline behavior, and a new Slint/Flatpak build.

The four older `ui/assets/focus/{top-left,top-right,bottom-left,bottom-right}.png`
files are obsolete and must be removed from the project; they are no longer
referenced by the UI. No new third-party assets are introduced.
