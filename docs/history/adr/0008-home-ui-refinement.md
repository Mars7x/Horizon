# ADR 0008: Refine the home shell before expanding navigation

## Status
Accepted

## Context
The Phase 2/3 home screen proved the Slint layout and Rust-owned input/state
architecture, but the first running build exposed visual quality issues: square
focus corners, placeholder Unicode navigation/status glyphs, a controller icon
shown while disconnected, and several elements that were mathematically placed
without being optically balanced.

## Decision
Treat these as design-system defects rather than future page-specific patches.
The home shell now uses:

- rounded Path-based focus brackets;
- a centralized carousel shelf and selected-tile axis;
- custom vector Path icons instead of font glyphs for navigation and status;
- independent left, centered-navigation, and right-status alignment regions;
- a composite, optically centered clock readout;
- conditional controller chrome that is not instantiated while disconnected.

All geometry remains centralized in `ui/theme/theme.slint` where it is shared
across the home-shell components.

## Consequences
Phase 4 can build navigation on top of a visually stable shell instead of
carrying Phase 2 placeholders into additional pages. Future visual changes
should modify theme metrics or reusable components rather than adding local
coordinate exceptions.
