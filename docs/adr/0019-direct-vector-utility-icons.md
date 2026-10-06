# ADR 0019: Render utility symbolic icons as Slint Paths

## Status
Accepted. Supersedes the runtime `Image.colorize` approach in ADR 0018.

## Context
The supplied 16×16 symbolic SVG files rendered through Slint `Image` appeared
uncolored and visibly soft when Horizon was scaled fullscreen. The source SVG
files themselves must remain untouched and icon colors must remain semantic
Theme state rather than embedded asset colors.

## Decision
Keep the supplied SVG files unchanged as source/reference assets. Transcribe
their vector path geometry into dedicated Slint `Path` components and bind each
component's stroke/fill directly to semantic `Theme.nav-*` tokens.

This avoids scaling a decoded `image` and guarantees vector geometry is fitted
directly to the requested logical icon bounds. The components live in
`ui/components/utility-icons.slint`.

## Consequences
- Icons stay crisp at fullscreen/HiDPI scales.
- Icon colors respond to Horizon's theme without editing SVG source files.
- Header code no longer depends on `Image.colorize` support for symbolic SVGs.
- The original supplied SVGs remain unchanged under `ui/assets/`.
