# ADR 0112 — Settings Appearance preferences (Phase 9.5.44.55)

## Decision

Appearance is the first Settings category, before Third-Party. Theme is System / Light / Dark, where System exclusively resolves to the host's Light or Dark preference from the existing XDG Settings portal. Accent is independent: System, Red, Orange, Yellow, Green, Teal, Blue, Purple, Pink and White. The named presets are ordered warm-to-cool on a single row, with neutral White last. No new theme or background style is introduced.

The domain owns palette resolution and hue presets. The Settings controller owns keyboard/controller and pointer selection, calls `AppearanceController`, which commits a private JSON preferences file atomically before applying the new palette. Portal events continue to update System choices without overriding explicit settings. The existing High Contrast and Reduced Motion behavior is preserved.

The visible swatch is the unmodified chosen colour, while text, focus and control accents are dynamically shaded toward black (Light) or white (Dark) to meet 4.5:1 contrast on primary/elevated surfaces, 7:1 in High Contrast. Foreground text over accent uses the better of white and black. These neutral adjustments are particularly important for Yellow and White. Do not recolour authored utility icons or artwork.

This is intentionally only a UI/settings addition: no new dependency, Flatpak permission, database schema migration, or changes to game launching/activity.

## Validation

- Verify appearance is first, Third-Party retains settings and navigation, and controller/pointer choices match.
- Verify System theme follows live desktop light/dark changes; Light and Dark overrides remain fixed.
- Verify System accent follows portal colour changes; named accent overrides remain fixed.
- Verify preferences survive relaunch; corrupt/unwritable settings produce an error rather than silently discarding user choices.
- Verify focus/text contrast for all presets in both themes and High Contrast; Reduced Motion still removes focus/page animation.
