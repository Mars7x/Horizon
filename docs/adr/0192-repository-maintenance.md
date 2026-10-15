# ADR 0192: Current contracts and isolated repository history

Date: 2026-10-14

## Status

Accepted. Establishes the documentation lifecycle and structural quality gate.
The application direction in ADR 0190 and Steam scope in ADR 0191 remain.

## Context

Current guides had accumulated phase logs, superseded experiments and retired
provider instructions. Repeated status paragraphs contradicted older sections.
Onboarding required interpreting thousands of lines and nearly two hundred ADRs.
The repository also retained unused direct dependencies, stale asset annotations
and packaging descriptions, without an automated documentation/structure gate.

The user requested a ground-up cleanup and chose to preserve historical ADRs and
supporting notes in a separate folder. User data, immutable schema history,
licenses, shared services and the established UI must survive cleanup.

## Decision

- Keep a small indexed set of current guides, one owner per concern, describing
  implemented contracts and explicit limitations without chronological appendices.
- Preserve pre-transition ADRs and previous guides under docs/history, with a
  clear entry point and relocation-adjusted links. Current onboarding links to
  that entry point, not to old feature instructions. Routine edit history stays
  in Git; new work updates current guides rather than appending to snapshots.
- Keep current accepted direction/maintenance ADRs in docs/adr. New decisions use
  the next identifier across current and historical records. Supersede decisions
  explicitly rather than erasing their evidence.
- Add an offline standard-library Python/Bash repository gate and regression
  tests, run in CI and before local Rust checks. Validate current links/indexes,
  documented versions/schema/utility order, module paths, obvious unused direct
  dependencies, lockfile references, immutable SQL checksums, asset annotations,
  generated-file hygiene, retired-provider boundaries and reviewed sandbox grants.
- Keep lockfile resolution stable during source generation and development
  packaging. Remove only verified unused direct crates and newly unreachable
  lock entries. Retain third-party attribution even for removed dependencies.
- Preserve dormant shared library/lifecycle interfaces for the separate future
  core-hosting decision; documentation must call out their inactive status.
  Do not redesign Home, delete history/data, install tools or remove ignored
  local build output as an incidental part of source cleanup.

## Consequences

Current docs are shorter and no longer require reconstructing development phases.
History remains available with its original context. Structural regressions get
fast feedback even without a working Rust/native toolchain. The gate does not
prove semantic documentation accuracy, compile Slint/Rust, validate credentials
or replace real UI and per-architecture Flatpak testing.

Automated rules remain maintained code: new legitimate capabilities may need
updated contracts and tests. A failing guard is not permission to weaken policy,
change a deployed migration or claim build success. Review feature changes and
source ownership alongside documentation in the same patch.

See [Maintenance](../MAINTENANCE.md) and [Development](../DEVELOPMENT.md).
