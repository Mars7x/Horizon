# ADR 0027: Pin LINE Seed JP binaries and use a subtle focus accent sweep

## Status
Accepted. Supersedes the Fontsource archive-selection portion of ADR 0026.

## Context
The Phase 3.14.15 development installer downloaded Fontsource's complete-family
archive and heuristically selected TTF entries by weight. Fontsource's archive
also contains subset-oriented TTFs; the selector could choose a small subset
instead of the canonical desktop face. The build then correctly rejected that
unexpectedly small payload.

The home focus indicator is already a single layer of four rounded corner paths.
A restrained animated accent highlight can add motion without changing the
geometry or introducing a second halo layer.

## Decision
Development Flatpak builds download the four canonical static LINE Seed JP TTF
files directly from Google Fonts commit
`874ec71eac706dd23900d1305449abed6767b7df`. The installer validates that every
payload is a large SFNT font before placing it under `/app/share/fonts`.

The focus paths share one linear-gradient brush. A narrow highlight, derived
from the current system accent, rotates calmly through that brush on a 4.2 s
cycle. The animation is active only for the selected tile. Reduced-motion and
high-contrast modes use the original solid `Theme.focus` brush.

## Consequences
- Font installation no longer depends on archive path heuristics or subset names.
- The four bundled faces are the same canonical Google Fonts desktop binaries.
- Development still uses network access; release packaging must use SHA-256
  pinned flatpak-builder sources.
- Focus remains one geometric layer with no glow/halo.
- Phase 3.14.17 increases the accent-derived highlight contrast and width slightly so the sweep is easier to perceive without changing the focus geometry.
- Phase 4.1.1 replaces the generic brightness adjustment with a 62% white mix, widens the visible sweep plateau, and shortens the cycle to 3.6 seconds so saturated accents such as red retain an obvious animation.
- The sweep follows the desktop accent and automatically disables for reduced
  motion and high contrast.

- Phase 4.1.2 tones the effect back to a 42% white mix, narrows the highlight plateau, and uses a 4.0-second cycle. This is the preferred balance: still visible on red and other saturated accents, but less dominant than the Phase 4.1.1 contrast pass.
- Phase 4.1.3 reduces only the white mix from 42% to 38%; highlight width and the 4.0-second cycle remain unchanged for a slightly calmer result.

- Phase 4.1.5 reduces the white mix to 34% while retaining the narrow highlight plateau and 4.0-second cycle. This is the current focus-sweep baseline.
