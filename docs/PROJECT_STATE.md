# Current project state — source snapshot through 10.4.2

This page describes the **prepared source** assembled from the cleaned repository and the Phase 10.3.3 → 10.4.0 (Steam Achievements) → 10.4.1 → 10.4.1.1 → 10.4.2 patches. It is an onboarding snapshot, **not** a release claim or a substitute for inspecting the checked-out branch. The crate's `Cargo.toml` version remains `0.1.0`.

**Verification boundary:** source and documentation can be audited here; a successful full GNOME Builder Rust/Slint build of the new 10.4.2 changes has **not** been established in this snapshot. Test on the user's actual `x86_64`/`aarch64` environments before claiming compatibility.

## Implemented surfaces

- **Home:** game covers, controller/pointer navigation, game launching, utility row, original focus geometry, title marquee and shell status.
- **Library:** browse imported titles with source/sort controls, responsive artwork grid and Home-matched selected scaling/focus geometry. Library is a full-shell route; Home's original presentation is not replaced.
- **Activity:** Horizon-observed totals, most-played imported games, recent playtime chart, and a per-game read-only session history; per-game screen Up/Down scrolls without focusing rows. The **Milestones** preview shows up to three recent *real Steam achievements*, not synthetic playtime goals.
- **Achievements:** seventh Home utility after Activity; read-only Steam Web API achievement unlock states for eligible imported Steam games; list browsing and recent-unlocks preview. Now has source-aware game/achievement models, optional bounded Steam badge thumbnails, and a source selector; no other achievement provider, full offline cache, or comprehensive account sign-in flow yet.
- **Settings:** appearance and third-party service configuration, including SteamGridDB and **Steam Account** under Third-Party. SteamID64 + personal Web API key can be stored in an app-private file with owner-only permissions; not encrypted or OAuth.
- **Album:** Horizon's own captures (`data/album/`, written by the future screenshot/recording features) and the active Steam account's screenshots, newest first, in a grid with Game and Type filters. A full-screen viewer slides between captures and plays videos through GStreamer. See [Album](ALBUM.md).
- **Friends, Web, Shop:** retained utility routes; do **not** infer production social, browser, or commerce integrations from their presence. Steam Friends is a **future intention**, not implemented.

## Production source and data boundaries

- Production game sources: **Steam** and **Heroic**. Lutris and Bottles were retired; keep their existing removal migrations.
- Steam local game discovery and source-reported lifetime playtime are separate from opt-in **Steam Web API** achievement retrieval.
- Existing SQLite schema through migration **0007**. Observed sessions and source lifetime are distinct; never add them together. Historic activity can remain after a game is no longer imported.
- Artwork may come from source adapters or the optional SteamGridDB integration. Preserve local user choices and provenance.
- Routing, focus decisions and semantic controller input are Rust-owned. Slint is presentation only.

## Current limitations and planned work (not implemented)

- Phase 10.4.1.1 fixes a reported 14-element Rust tuple comparison (`E0369`) and replaces the achievement utility SVG; confirmation requires a real rebuild.
- Steam Web API key configuration is **manual** and should be treated as sensitive; saved credentials are protected by file permissions, not encryption. Environment variables can provide a fallback for developer runs.
- Achievement data can be inaccessible due to privacy/API/network conditions; missing is **not** zero. The achievement browser uses an account-scoped disk cache. Phase 10.4.3.3 adds on-demand badge hydration for opened games; images unavailable from Steam/CDN still use a neutral fallback. Rich Steam sign-in is not implemented.
- Future Steam Friends support should use the shared Steam account and HTTP client services. Do not implement it until expressly requested.
- Existing historical guides/ADRs may refer to retired providers or superseded UI experiments. Read the code for the present behavior.

## Quick links

Start with [INDEX.md](INDEX.md) for navigation, [DEVELOPMENT.md](DEVELOPMENT.md) for validation, [ACHIEVEMENTS.md](ACHIEVEMENTS.md) for credential/data behavior, [ACTIVITY.md](ACTIVITY.md) for session semantics, and [NAVIGATION.md](NAVIGATION.md) for route policy.

## Phase 10.4.3 (pending GNOME Builder build verification)

Achievements now presents a full-width vertical game list with A/Enter to open full-width per-game read-only achievements, Back to preserve selection, smooth camera movement, genuine square badge artwork, and an optional per-account achievement snapshot/badge cache (30-minute freshness, 7-day stale display while refreshing). Steam remains the only achievement provider; Steam Friends is deferred.

## Phase 10.4.3.1 (prepared, pending GNOME Builder build verification)

Achievements' header and scroll boundary now match Library's full-width edge treatment; games and details crossfade in the Settings style; list content reaches the physical bottom edge. Rapid navigation no longer culls intermediary cards before the animated camera reaches them, and navigating upward brings selection back into view. Source is an unboxed, non-focusable text grouping, and all game titles have stable bold weight. Desktop directional repeat is globally capped at the controller D-pad's 300ms/115ms timing with release and deactivation reset. See [ADR 0160](adr/0160-achievement-list-transition-and-input-parity.md).

## Phase 10.4.3.2 (prepared, pending GNOME Builder build verification)

Achievements games now have a single exclusive focused-row border, avoiding teal focus trails during rapid navigation. The top full-width scroll-edge shadow is absent at scroll offset zero and fades in/out with the already-animated camera in both the game and per-game achievements lists. Library uses the same rule for its header elevation, without changing Home, Library's bottom shadow, or source/focus behavior. See [ADR 0161](adr/0161-scroll-driven-header-elevation-and-exclusive-game-focus.md).

## Prepared Phase 10.4.3.3 (awaiting GNOME Builder verification)

Corrects held Achievements detail scrolling, adds camera-only mouse-wheel navigation in Achievements/Library and read-only Activity session details, resets Achievements selection/viewport on every fresh utility visit, and lazily fills authentic per-game Steam achievement badges from the existing account-scoped cache/API. Home, Friends, other source adapters, and the database schema are unchanged. See [ADR 0162](adr/0162-achievement-input-and-lazy-badge-parity.md).

Settings intra-page Back now restores the parent row (Appearance, Third-Party,
or Steam Account), while completely leaving and revisiting Settings resets to
Appearance. This change is prepared pending GNOME Builder verification.
