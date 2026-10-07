# ADR 0002: Flatpak-first architecture

Status: Accepted

## Decision

Treat Flatpak sandbox constraints as application architecture from the beginning.

## Consequences

- The Freedesktop runtime is the primary distribution target.
- Portals are preferred for sandbox-safe host integration.
- Broad host-filesystem permissions are not an acceptable default workaround.
- Native development builds are useful, but may not define behavior that cannot work from Flatpak.
