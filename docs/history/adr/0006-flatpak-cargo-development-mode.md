# ADR 0006: Online Cargo resolution during development

## Status

Accepted for development; must be replaced before public release.

## Context

GNOME Builder prepares Flatpak dependency modules and then invokes Cargo inside
the SDK. Horizon's Phase 3 repository did not yet contain a generated
`cargo-sources.json`, while the manifest forced `CARGO_NET_OFFLINE=true` and
`cargo --offline`. A fresh build therefore had no registry/cache entry for
crates such as `ashpd` and could not resolve the dependency graph.

## Decision

During active development, the Flatpak **build sandbox** may use network access
and Cargo runs without `--offline`. Runtime network permission remains absent.

Once Horizon reaches release packaging, generate and commit `Cargo.lock` and
`flatpak/cargo-sources.json`, remove build-time network access, and restore
`cargo --offline ... --locked`.

## Consequences

- Fresh GNOME Builder development environments can resolve Rust dependencies.
- The development manifest is not yet suitable for Flathub submission.
- Runtime sandbox permissions remain unchanged.
- Reproducible offline dependency packaging remains an explicit release gate.
