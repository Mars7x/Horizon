# ADR 0156 — Steam Achievements utility and recent Activity milestones

**Status:** Phase 10.4.0 foundation (requires GNOME Builder build verification).

## Decision

Retire the unshipped custom playtime-milestones proposal. Add Achievements as
a **seventh** Home utility after Activity, keeping all six existing utilities
and maintaining Rust-owned focus/history. The Activity Milestones panel is a
small, read-only recent-unlocks preview of the dedicated utility.

Use Valve's documented, key-authenticated read-only Steam Web API rather than
scraping undocumented user stats caches or impersonating Steam clients. Keep
the credential mechanism opt-in for Phase 10.4.0; do not log or persist keys.
Perform network requests off Slint's UI thread, publish progressive updates
through a channel, and distinguish inaccessible games from zero unlocked.
Only `steam` source AppIDs are eligible. Provider achievements and observed
playtime remain separate. No SQLite changes, new Cargo dependencies, or
additional Flatpak permissions.

Achievements are cached in memory for the active app process. On the first Activity or Achievements entry, the game list and recent unlocks
populate as the network
requests succeed. Missing key/SteamID yields an explicit unconfigured state,
not fictional entries.

## Follow-ups

Build-tested Slint/Rust fixes; account setup through a secure user-facing
workflow; durable cache and error handling; achievement icon fetching; clearer
privacy and offline states; richer per-game browsing, filtering and refresh.
