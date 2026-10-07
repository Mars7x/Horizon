# Typography

Horizon uses **LINE Seed JP** throughout the interface. Flatpak builds bundle the
published static TTF faces so rendering does not depend on fonts installed on the host.

## Weight mapping

- General interface: Regular (`400`)
- Game titles and emphasized labels: Bold (`700`)
- Clock digits: ExtraBold (`800`)
- Clock suffix: Bold (`700`)
- Placeholder artwork text: inherited from the same bundled family

LINE Seed JP provides Thin 100, Regular 400, Bold 700, and ExtraBold 800. Horizon
installs all four faces so later screens can use the complete family without synthesized
weights.

## Packaging

During development, Horizon downloads the four canonical static TTF faces directly
from Google Fonts commit `874ec71eac706dd23900d1305449abed6767b7df` and installs
them into `/app/share/fonts/truetype/line-seed-jp`. This avoids both the upstream
Python/fontmake build chain and subset-selection ambiguity in third-party family
archives. The installer verifies the SFNT header/table directory and rejects
truncated or unexpectedly small payloads.

Before Flathub/release packaging, these same immutable URLs should be moved into
checksum-pinned flatpak-builder sources so Builder can cache and verify them before
the module runs. See ADR 0027.

## Source and license

- Typeface: LINE Seed JP
- Copyright: © LY Corporation
- Designers/project credits: LY Corporation, RixFont, Fontworks
- Upstream: https://github.com/line/seed
- Published binaries: Google Fonts
- License: SIL Open Font License 1.1 (`OFL-1.1`)

Horizon does not modify or rename the font files. See `THIRD_PARTY_NOTICES.md` and
`LICENSES/OFL-1.1.txt`.
