# ADR 0045: Multi-provider local-metadata adapters

- Status: Accepted
- Phase: 9.0.0

## Context

After Steam proved the generic source/import/launch pipeline, Horizon needs additional real providers without turning generic services into launcher-specific conditionals. Lutris, Bottles, and Heroic expose different local metadata formats and packaging layouts, but all can provide installed membership and launch intents without network APIs or host-command execution.

Phase 9 also needs to remain compatible with a later managed-session/Gamescope architecture. Baking provider executables, Wine commands, or `flatpak run` into `GameLaunchService` would make that future work harder and violate the existing source boundary.

## Decision

1. Lutris, Bottles, and Heroic are ordinary `GameSource` implementations under `src/sources/`.
2. Provider paths, schemas, package-layout distinctions, and URI shapes terminate inside those adapters.
3. Generic services continue to consume `SourceDescriptor`, `SourceSnapshot`, and `SourceLaunchTarget::Uri` only. There are no provider-name branches in import, persistence, presentation, or launch execution.
4. Discovery is local-only:
   - Lutris reads installed/configured rows from `pga.db` read-only.
   - Bottles reads persisted `External_Programs` from standard `bottle.yml` files.
   - Heroic Phase 9 reads installed Epic/Legendary membership and local metadata; its external IDs include the Heroic runner namespace so GOG/Amazon can be added later without changing identity semantics.
5. Native/Flatpak installation-local numeric/UUID identities are scoped where the upstream identifier is not globally portable. Lutris and Bottles therefore include an installation scope in `ExternalGameId`.
6. Complete scans may publish authoritative membership. Any detected unreadable provider root or inaccessible Bottles external placeholder degrades the whole provider snapshot to partial so stale entries are not destructively pruned from incomplete evidence.
7. Launch-capable adapters produce documented launcher URI handlers (`lutris:`, `bottles:`, `heroic:`). `PortalLaunchExecutor` remains the only runtime dispatcher.
8. Flatpak permissions remain provider-owned, read-only subtrees. No `home`, `host`, arbitrary mount, or host command execution permission is added.
9. Shared JSON/YAML parsing uses Serde-family crates instead of handwritten format parsers. Bottles YAML uses the actively maintained YAML Organization `yaml_serde` fork rather than the deprecated `serde_yaml` crate. Dependency provenance is recorded and remains subject to the final lockfile-derived release inventory.

## Consequences

- Phase 9 adds three providers without changing domain/persistence/UI source semantics.
- The same launch feedback, controller ownership, and observed activity flow automatically applies to these sources.
- Bottles external/custom locations are conservatively partial unless readable; Horizon does not trade sandbox scope for discovery completeness.
- Heroic initially covers Epic/Legendary installed games only. GOG/Amazon runner namespaces can be added inside the same adapter later.
- URI launch remains an external handoff and therefore continues to use `ForegroundHandoff` activity precision. Phase 9.5 can add a new managed-session launch target/capability without rewriting these adapters' discovery models.
