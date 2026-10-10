# ADR 0159 — Vertical achievement browsing and a private Steam cache

Status: Proposed for Phase 10.4.3, pending GNOME Builder compilation and visual verification.

## Decision

Use a single vertical list of games across the full Achievements utility width. Each row shows a readable game name, source and the unlocked/total achievement count right-aligned, supported by a restrained progress bar that fills strictly left-to-right. A/Enter or pointer selection opens that game's full-width achievement view. Left/Back returns to the previous game and its scroll position. Achievement rows are read-only, without per-row focus. Continue using global Settings focus styling and Library-like scroll-edge elevation. Scroll movement, pane entry, focus surfaces and new rows use the shared `Motion` timings; Reduced Motion removes all movement. Actual badge imagery fills a clipped, round-corner square rather than sitting over an additional dark tile.

## Data boundaries

The page works on `AchievementGameData` and `AchievementEntryData` and must never infer the source from a game name or convert Heroic IDs to Steam AppIDs. Steam remains the only provider; All sources and Steam currently resolve to the same verified rows. The detailed list may have hundreds of entries, so preserve the bounded visible range and source-owned scroll index rather than constructing interactive per-achievement focus state.

## Cache policy

The Steam importer writes one account-scoped, non-secret catalog snapshot containing completion statuses and timestamps and separate bounded 56x56 badge pixels to a private XDG cache directory. Fresh results (up to 30 minutes) are served locally without rescanning; older saved results up to 7 days may be displayed while a background scan runs. New results are published before supplementary artwork requests. Cache writes are best-effort atomic replacement with restricted file permissions; malformed/expired data falls through to normal retrieval. Clear the previous account's cache when Steam credentials change or are removed, and never persist a Web API key or password in the cache. Distinguish historical cached results from current server availability; never synthesize a zero unlock count for inaccessible/private profiles.

## Non-goals

No change to Home, Library, Friends, Steam sign-in, achievements unlocking, or other provider adapters. No extra Rust dependencies or database migration.

## Verification

Test controller/pointer navigation, the left-origin progress bar, visible partial next row, source switching, empty/large lists, Reduced Motion, offline/cache freshness, changed Steam account, private results, and GNOME Builder aarch64/x86_64 compilation.
