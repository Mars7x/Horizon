# Steam achievements and Activity Milestones

**Current implementation:** Phase 10.4.3 presents a full-width vertical game list with per-game details, consistent motion and a private account-scoped cache. Phase 10.4.2 introduced source-neutral presentation, separate Steam account settings and bounded genuine badges. Steam remains the **only implemented achievement provider**. See [PROJECT_STATE.md](PROJECT_STATE.md) for verification status.

## Feature contract

- Achievements is the **seventh Home utility**, after Activity, not a replacement for the Activity page. `TopUtility` ordering is maintained in `src/navigation/mod.rs`, navigation presentation and Slint.
- The full-shell **Achievements utility** is a **single-column vertical game list**. Each full-width row shows the game title, real provider, and the count of unlocked versus total achievements at the right, with a left-origin completion bar. Up/Down navigates through games; A opens the selected game's full-width, read-only achievement list with the OK sound (Right does not open it); Up/Down scrolls that list; only B (Back) returns to the same game selection; Left does nothing. Pointer-clicking a game also opens details, with the same OK sound; clicking the details breadcrumb returns to games. Achievement rows are not focusable. Scroll-edge elevation and a partially visible next row reveal continuation; the scrolling camera is animated and respects Reduced Motion.
- The **source selector** is designed to admit additional providers without reworking the screen. The initial options are All sources and Steam, which necessarily show the same results until another provider exists. LB/RB or pointer cycles the selector. Do not imply achievements from Heroic or any other provider are already available.
- **Activity → Milestones** is a compact preview of up to **three most recently unlocked real Steam achievements**. It does not create artificial Horizon time-played goals.
- The importer is read-only. It never unlocks, edits or resets a user's Steam achievements.

## Sources, privacy and truthfulness

- `src/services/achievements.rs` consumes eligible `steam` source references with numeric AppIDs. **Heroic game IDs are not assumed to be Steam AppIDs.**
- Valve's `ISteamUserStats/GetPlayerAchievements/v1` supplies per-game unlock state and timestamp when accessible. The supplementary `GetSchemaForGame/v2` can provide named image URLs; download only a small bounded number of achievement badges per game from explicit allowlisted HTTPS Steam CDN hosts. Missing icons use a neutral placeholder; never fabricate unlocks, descriptions, locked dates, or unavailable completion.
- Inaccessible, private, offline and malformed responses are **unavailable**, not `0 / 0`. No achievement is invented when credentials are absent.
- Fetches occur on Activity or Achievements entry on a background thread. Completion records are published before image fetching; at most **7 badge images per accessible game** are requested in a later background pass, prioritizing latest unlocks. Other achievements retain the fallback icon.
- **Phase 10.4.3 cache:** each SteamID has its own private folder under `$XDG_CACHE_HOME/io.github.Mars7x.Horizon/achievements/steam-<SteamID64>/` (or `~/.cache/…`). It holds an API-key-free JSON completion snapshot and bounded 56×56 RGBA badge thumbnails in restricted files. A matching snapshot newer than **30 minutes** suppresses the normal achievement rescan; up to **7 days** of older saved results can appear immediately while a background refresh attempts to update them. The cache is best-effort: a missing, malformed or inaccessible cache never blocks normal import. A Steam account change, key replacement or disconnect removes the prior account's cache. This is **not encrypted storage**, and an offline snapshot is not proof that Steam data is currently accessible or unchanged. No login token/API key is cached.
- This phase does not introduce periodic polling, manual refresh, or a cache for another provider (on-demand badges for an opened game were added in Phase 10.4.3.3). Those require their own verification and design.
- The feature is independent of both Horizon-observed play sessions and source-reported Steam lifetime playtime; these quantities must never be combined.

## Account configuration — not on the Achievements page

- Use **Settings → Third-Party → Steam Account** to set/replace a 17-digit SteamID64 and personal Steam Web API key or disconnect the saved account. No Steam username/password is collected and this is not OAuth or Steam browser login.
- `src/services/steam_account.rs` owns the shared `SteamAccountService` and `SteamWebApiClient` so a **future** Steam Friends implementation can reuse them. **Friends integration is not currently implemented**.
- Credentials are written as an atomic replacement to `$XDG_CONFIG_HOME/io.github.Mars7x.Horizon/steam-account.json`, using `0600` file permissions inside a `0700` configuration directory. This is **not encrypted storage**; other processes under the same user may still read them. Never log, publish into Slint or commit the key.
- As a developer fallback, if no saved account exists, both `HORIZON_STEAM_WEB_API_KEY` and `HORIZON_STEAM_ID64` may be inherited from the process environment. Removing a saved account does **not** unset inherited environment variables.
- An account replacement/disconnect invalidates previously published achievement results; stale background imports for the previous account must not repopulate the UI.

## Not yet present

- Direct Steam sign-in/account linking without manually entering a Web API key.
- Rarity metadata, and multi-provider aggregation. The source selector currently offers All sources and Steam only.
- Steam Friends browsing and presence; that is intentionally deferred.

## Testing expectations

Test unconfigured mode, invalid ID/key, inaccessible/private data, zero returned games, account switching during a fetch, route re-entry, controller and pointer behavior, long/short lists, and Reduced Motion. No live Steam credential should be placed in a test fixture, screenshot or commit. For data privacy and account safety, prefer deterministic mocked API responses in tests.

See [SETTINGS.md](SETTINGS.md), [ACTIVITY.md](ACTIVITY.md), ADR 0156 and ADR 0157.

## Phase 10.4.2 — UI standardization and provider boundaries

The game list now follows Settings-style focus outlines and progress bars; the achievements detail list follows Activity's read-only scroll convention. Each displayed game and unlock identifies its real provider. Presentation structs in `ui/models/achievements.slint` are source-neutral; Steam translation happens in `src/presentation/achievements.rs`. Future Epic/GOG/etc. providers need their **own authenticated service adapter and explicit provenance**, not conversion of Heroic external IDs to Steam AppIDs. Source identity and unreadable/private/unavailable states must remain distinct from a legitimate zero-unlock count.

See [ADR 0158](adr/0158-source-neutral-achievements-ui-and-badge-pipeline.md).

## Phase 10.4.3 — Full-width game browsing and cache

See [ADR 0159](adr/0159-vertical-achievement-browsing-and-private-cache.md). The former two-column game/details pane is retired. No source adapter other than Steam is added. Cache validity is account-bound and time-bound, with credential data deliberately excluded. Page navigation uses Settings-style focus and Library-style scroll-edge shading; lists are read-only in their detail view.

## Phase 10.4.3.1 — Navigation and header parity (pending build verification)

- Game browsing and read-only achievement details remain **two separate full-width views**. Both stay mounted during the 220ms Settings-family crossfade/12px settle in both directions; Reduced Motion bypasses it. Back preserves game selection and scroll context.
- The full-width, stationary heading stays on `Theme.background`, with an **11px full-shell Library-style top-edge elevation** at the start of the moving list. Do not introduce a bordered/tinted header or a partial-width shadow. *(Superseded by [UI standards](UI_STANDARDS.md): lists end at the shared bottom bar and show a bottom shadow while more rows remain.)*
- `Source` follows Library's unboxed, non-focusable text grouping. LB/RB and pointer can still change the source; it is **not** a controller-focusable settings button. Games are bold even when unselected, and the count label reserves a full text line.
- Both scroll cameras retain intermediate records based on their **animated viewport position**, not their final target index. This prevents blank gaps under rapid held input. Game scrolling remembers its first visible row separately from the selected row so reverse navigation stays in view. Background/cache updates do not reset the layout.
- Keyboard directional auto-repeat is rate-limited in `src/input/manager.rs` to the SDL D-pad **300ms initial delay** and **115ms repeat interval**. It is a maximum cadence imposed on OS repeat events; platforms with slower OS repeat cannot be accelerated by this cap. Physical key-up and app deactivation clear the repeat latch. All other pages share this input behavior. See [INPUT.md](INPUT.md).

## Phase 10.4.3.2 — Exclusive focus and scroll-aware header elevation (pending build verification)

- *(Superseded in 10.4.3.3 by the Settings focus reveal.)* **At most one game row carries the teal selected outline.** Selection is an immediate, index-owned state, not an independent animated border on every game. This prevents fast navigation leaving behind apparent selections on previous rows; hover/background motion and the animated camera remain.
- The **Games** and **individual game Achievements** lists have a fully transparent top scroll-edge shadow when their respective animated camera is at position zero. An 11px, full-shell-width penumbra progressively appears during the first row of downward movement, stays present while scrolled, and fades out on returning to the first row. *(Bottom shadow added by [UI standards](UI_STANDARDS.md).)*
- Both views continue to use their existing Settings-family crossfade and 12px settle. Achievement rows remain read-only and unfocusable; selection/focus is only for choosing a game in the games view. Reduced Motion makes camera transitions immediate rather than creating extra animation.

See [ADR 0161](adr/0161-scroll-driven-header-elevation-and-exclusive-game-focus.md).

## Phase 10.4.3.3 — Input, entry reset, and on-demand badge hydration (pending build verification)

- A **fresh entry** to the Achievements top-level utility (direct navigation or navigation-history Back) resets to All sources, the first game, first visible row, and the Games view. Back from a game's details during the **same visit** still preserves its selection and scroll context. Activity's independent achievements preview fetch does not reset Achievements navigation.
- Keyboard/controller Up/Down moves game focus or scrolls the unfocusable achievement history; the mouse wheel **only pans the list viewport** and never changes the focused game. The games list camera is independent of the Rust-owned game selection for wheel input.
- Held D-pad/keyboard scrolling of achievement details uses **110ms linear camera steps** instead of repeatedly restarting a slower ease-in-out transition, matching the shared approximately 115ms repeat cadence. Reduced Motion remains immediate.
- Game-row focus uses the **Settings row focus reveal**: the outline fades in on the new row and out on the old one over `Motion.focus-duration` (140ms; immediate under Reduced Motion). At the 115ms repeat cadence only the previous row can still be partly faded, exactly as in Settings. This supersedes the Phase 10.4.3.2 "immediate, index-owned outline" rule above.
- Steam's initial import prefetches a **bounded seven badges per game**, explaining why later records previously showed a generic star. Opening a game's achievements now starts a separate nonblocking background fetch for its remaining badges using the documented Steam schema, storing verified thumbnails in the existing account-scoped cache. Badge cache files are keyed by **locked/unlocked state** as well as AppID and API name, because Steam serves different artwork for each; an unlock therefore fetches the unlocked icon instead of reusing a cached grey one. Thumbnails cached under the earlier state-less names are ignored and re-fetched on demand. Cached thumbnails display immediately; missing/private/broken artwork remains a neutral fallback (never a fabricated badge). This does not add an achievement source or new authentication.

See [ADR 0162](adr/0162-achievement-input-and-lazy-badge-parity.md).
