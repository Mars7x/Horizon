# ADR 0018: Theme symbolic utility icons at render time

## Decision

Horizon keeps the supplied Friends, Album, Web, and Settings SVG assets unchanged.
Their colors are applied at render time through Slint `Image.colorize` using semantic
Theme tokens (`nav-friends`, `nav-album`, `nav-web`, `nav-settings`).

## Rationale

- Preserves the original symbolic SVG assets.
- Keeps color choices centralized in the design system instead of scattered literals.
- Allows settings gray to remain legible in both light and dark modes.
- Makes future palette changes a one-line theme edit rather than an asset rewrite.
