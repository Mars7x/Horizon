# ADR 0158 — Source-neutral achievements UI and bounded Steam badges

Status: Prepared for verification (Phase 10.4.2)

## Context

The first Achievements utility displayed a Steam-only two-column text list, while Activity's recent unlocks lacked visual badges. Steam Account appeared directly below the SteamGridDB artwork actions without its own section header, suggesting a false relationship. Future achievement providers must not require another rewrite of the page.

## Decision

1. Keep **Steam** as the only implemented achievement adapter, with real SteamID64/Web API credentials under the existing shared account service. Do not infer Heroic achievement support from the Heroic game source.
2. Treat provider identification as part of each achievement game and badge presentation model. The source selector begins with All sources and Steam; other providers must register explicit adapters and availability states before being displayed. Never show a provider's unavailable/private results as zero unlocks.
3. Keep two logical areas: game collection with Settings-style focus and actual completion fractions; read-only achievement history/details with no per-row focus. On compact screens show one pane at a time. LB/RB (and pointer selection) controls the provider selector.
4. Milestones remains a compact three-item preview. Show a badge thumbnail when the service delivers one; otherwise use a neutral glyph, not made-up artwork. Display the source and unlock timestamp.
5. Steam game achievement state is loaded first, then official schema metadata and at most seven trusted-CDN badges per game on the worker thread. The importer never makes UI-thread HTTP requests; uses no arbitrary third-party URLs, credentials in artwork fetches, or fabricated values. Full badge coverage/lazy cache are deferred.
6. Visually separate Steam account configuration from SteamGridDB in Third-Party settings; preserve all existing Settings navigation and credential storage logic.

## Non-goals

No new game source adapters, Steam Friends implementation, account OAuth, SQLite migrations, filesystem credential changes, new dependencies, changes to Home/Library, or new achievement unlock logic. No persistent downloaded-badge cache.

## Validation

Verify Slint 1.18.1/Rust 1.92 compilation, controller/pointer interactions, compact and ultrawide geometry, unconfigured/private/offline Steam modes, long lists, image failures, Reduced Motion, and account replacement mid-fetch. Preserve Third-Party attribution and licensing records.
