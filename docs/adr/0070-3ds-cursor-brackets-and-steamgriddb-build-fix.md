# ADR 0070 — 3DS-inspired cursor brackets and build correction

Status: experimental visual revision (Horizon 9.5.44.13).

## Visual reference

Nintendo 3DS HOME Menu theme metadata documents cursor expansion/contraction and expanded-glow colour. The two user-provided frames show the selected icon staying in place while the cursor appearance changes. This is an interpretation of that behaviour, not pixel-identical reverse engineering of Nintendo assets.

## Implementation

- Remove the shared `GameTile.visual` vertical idle movement completely. The game's artwork, selected shell, layout, connector and carousel camera never move from the new idle effect.
- Remove the eight previously drawn halo/inner-glow `Path` elements. Preserve **exactly four** original accent-coloured rounded corner paths with identical SVG path commands, viewboxes, stroke widths, line caps and joins.
- Move only the corner paths smoothly and symmetrically up to 1.3 logical pixels diagonally outward from the selected game's centre, then return. The cycle takes 3.8 seconds. The brackets do not scale or change thickness.
- Gently brighten the single existing `Theme.focus` stroke by at most 12% during outward movement. This gives a restrained illuminated appearance without duplicate hard-edged strokes or white highlights. There is no separate blurred-glow renderer effect.
- When motion is reduced, high contrast is active, or game focus is inactive, return to solid accent brackets with no added motion. On activation/deactivation, fade movement strength over 220 ms.

## Compile fix

- Fix variable shadowing in `steamgriddb_artwork.rs`: `cached` counted hits, but a later `let cached = Option<...>` shadowed it, causing E0368 on `cached += 1`. Name the latter `cached_artwork` instead.
- Scope `PermissionsExt` to settings persistence tests, where `.mode()` is actually used, so non-test builds no longer warn about the unused import.

## Verification

Compile in GNOME Builder, then inspect selected cover immobility, bracket motion, source art and fallback covers, scaling and carousel selection, dark/light/custom accent, Reduced Motion, High Contrast, and SteamGridDB worker statuses. The visual settings are intentionally conservative, and neither external artwork nor native Wayland clipboard behavior is modified in this phase.
