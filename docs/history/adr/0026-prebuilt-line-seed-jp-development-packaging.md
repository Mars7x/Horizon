# ADR 0026: Use prebuilt LINE Seed JP binaries in development Flatpak builds

## Status
Accepted historically; archive-selection details superseded by ADR 0027.

## Context
Horizon originally cloned the LINE Seed JP source repository and ran its upstream
Python/fontmake build. Freedesktop SDK 26.08 currently provides Python 3.14. The
font project's pinned build dependencies include `compreffor 0.5.6`, whose build
setup imports `pkg_resources` and fails while resolving its wheel build requirements
under this environment. This failure is unrelated to Horizon or Slint.

Google Fonts already publishes LINE Seed JP as static TTF binaries, and Fontsource
provides those published desktop files as a complete-family archive. Rebuilding the
font in every Horizon development build adds significant complexity without changing
the font files Horizon actually needs.

## Decision
Development Flatpak builds install the prebuilt LINE Seed JP family from Fontsource's
complete-family download using only Python's standard library. The installer validates
that Thin 100, Regular 400, Bold 700, and ExtraBold 800 TTF faces are present and
installs them into `/app/share/fonts/truetype/line-seed-jp`.

Horizon does not modify the fonts. The existing OFL-1.1 notice and attribution remain
part of the application package.

The download is intentionally a development-stage network fetch because Horizon's
current Flatpak build already permits network access for Cargo dependency resolution.
Before a Flathub/release build, the font binaries must be converted to immutable,
checksum-pinned flatpak-builder sources, alongside the planned Cargo offline source
work.

## Consequences
- No fontmake, virtualenv, pip, `compreffor`, or Python build-isolation dependency.
- Works with the Freedesktop 26.08 / Python 3.14 SDK.
- Faster and substantially simpler development builds.
- Font provenance/licensing remains explicit.
- Release packaging still has a tracked reproducibility task: pin the binary payload by checksum.
