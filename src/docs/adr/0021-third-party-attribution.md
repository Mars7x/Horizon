# ADR 0021: Third-party attribution is part of the architecture

## Status
Accepted

## Decision
Every third-party asset or copied/derived fragment used by Horizon must have recorded provenance, license, modification status, and a corresponding human-readable notice before it is considered complete.

Horizon keeps original supplied visual assets in `ui/assets/`, records their use in `THIRD_PARTY_NOTICES.md`, includes relevant license texts in `LICENSES/`, and installs those notices in the Flatpak.

Derived vector geometry remains attributed to its original artwork even when rewritten as Slint `Path` commands.

## Consequences
- External artwork cannot be added as an untracked convenience copy.
- Uncertain licensing becomes a release blocker rather than an assumption.
- Any future About/Credits UI must source its content from the same attribution records instead of maintaining a second incompatible list.
- The resolved Rust dependency graph must receive an automated license audit before a public release.
