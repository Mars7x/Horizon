# ADR 0135 — Home recents and inline Library destination

## Status
Accepted for Phase 10.1.0; supersedes the Phase 10.0.0 modal Library entry.

## Decision

Home is the recent-activity shelf, not a second full-catalogue view. A dedicated SQLite/Activity query returns latest distinct session-start identities, restricted to games with active source membership, capped at 15. Imported provider lifetime totals must not be treated as last-played timestamps. Games without a known observed session are found through Library.

Keep immutable original catalogue indices and game IDs as the source of truth for launch, artwork updates, Playing lifecycle, and the Library. Build a second Home-only projected VecModel for the recent ordered subset; translate Home card selection to original catalogue indices before launch. Reorder without losing catalogue identity, and propagate artwork/running-state changes to both models.

Append an explicit Library destination tile after Home's recent game cards. It uses the unmodified FocusFrame geometry and same world-space carousel as game covers, is not a synthetic LibraryGame, and opens AppRoute::Library via Navigator. Keep Home shell geometry, input repeat edge-wrap policy and Playing pill untouched. Remove the old modal ShellMenuOverlay from the app and the unused component file; Menu/Start maps directly to Library.

## Deferred

Phase 10.2.0 will redesign the Library page as a centred, artwork-first scrolling collection, with the same Home game-focus treatment. Phase 10.1.0 deliberately does not change its grid and controls.

## Validation

Run cargo check/test, exercise empty history, 16+ played games, reordering after session completion, mouse/controller Library entry, artwork refresh and live Playing status, and returning to Home using Back. Unit tests cover recent identity ordering, distinctness and exclusion of uninstalled games.
