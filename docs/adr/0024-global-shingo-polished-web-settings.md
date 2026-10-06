# ADR 0024: Global UD Shin Go NT and polished Web/Settings glyphs

## Status
Accepted. Supersedes the active Inter-default portions of ADR 0010, ADR 0013, and ADR 0023.

## Decision
Horizon requests `UD Shin Go NT` as the application-wide Slint font family. Inter is no longer bundled by the Flatpak manifest. The font itself is not redistributed; Horizon relies on the user-installed/fontconfig-visible copy.

The Web and Settings utility icons remain credited derivatives of the supplied GNOME SVG assets, but use Horizon-specific outline presentation geometry for consistent optical weight. Web uses a clean globe ring with lighter meridian/latitude detail. Settings uses a larger eight-tooth gear with a clear center hub. Original SVG assets remain unchanged.

## Consequences
- All UI typography has one coherent family.
- The Flatpak no longer downloads or installs Inter.
- Systems without UD Shin Go NT fall back according to the platform font stack.
- Third-party provenance for the GNOME-derived icons remains mandatory.
