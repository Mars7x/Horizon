# ADR 0003: Follow standardized desktop appearance portals

## Status

Accepted.

## Decision

Horizon uses the XDG Settings portal as the host-facing source for color scheme, accent color, contrast, and reduced-motion preferences.

The portal adapter translates desktop values into Horizon-owned domain types. A presentation controller resolves these values with user overrides and applies a semantic palette to Slint globals.

Horizon does not read GNOME gsettings, KDE configuration files, or other desktop-specific theme stores as a fallback.

## Consequences

- The Flatpak follows desktop appearance changes live without broad permissions.
- Desktop-specific details do not leak into UI components.
- Missing portal keys degrade to deterministic defaults.
- Manual overrides can be added without changing the portal adapter.
