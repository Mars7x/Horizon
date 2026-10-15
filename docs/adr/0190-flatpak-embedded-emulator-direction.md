# ADR 0190: Flatpak-only Horizon with an embedded-emulator direction

Date: 2026-10-14

## Status

Accepted direction; recovery implementation prepared, pending Rust/Slint and
Flatpak build verification. Supersedes the game-launcher direction and ADR 0189.
ADRs 0046, 0181 and 0183 were already retired in earlier commits.

## Context

Horizon developed as a Rust/Slint frontend for external game sources. Steam and
Heroic supplied discovery, launch URIs, runtime observations and lifetime data;
Flatpak games and the Flathub Shop were subsequently removed. A Gamescope
session experiment was implemented and then removed in `bf87675`; the older
managed-session helper was removed in `06756c5`. Shop/Flatpak retirement is
committed as `a4f7774`. The interrupted Steam rewrite retained screenshots and
controller naming but left registry and test references to its removed adapter.

The user has chosen a standalone gaming application around embedded emulator
cores. External launchers and a dedicated compositor/session no longer serve
that goal. Existing UI polish, user data, artwork and independent Steam account
features remain valuable and must survive the transition.

## Decision

- Keep Rust, Slint, the normal Flatpak application and its ID
  `io.github.Mars7x.Horizon`. Linux x86_64 and aarch64 remain targets; each
  future core needs its own architecture compatibility assessment.
- Retire Steam and Heroic game adapters, discovery, URI launch implementations,
  runtime observers, source-owned artwork scanning and lifetime ingestion.
  Register no production game sources until later work provides one.
- Preserve generic domain, import, library reload, artwork and activity
  boundaries rather than designing a speculative emulator API now. The generic
  URI/portal executor remains dormant, with no registered production game
  targets; normal browser links continue to use desktop portals.
- Preserve Steam Account configuration and Web API services, profile/avatar,
  friends, achievements/cache/badges, active-account screenshots and controller
  naming. Existing account support is manual SteamID64/API-key configuration,
  not interactive Steam authentication. Keep SteamGridDB keys, preferences,
  lookup/download/cache/refresh and all existing user artwork/provenance.
- Add immutable migrations 0009 and 0010. Archive Steam/Heroic source references
  in `retired_game_sources` while preserving every game row, identity,
  timestamp, observed session and lifetime counter. Active membership stays in
  `game_sources`; no user database is opened or rewritten during development.
  Archive reads supply Album names and the existing Steam achievement catalog.
  Reconciliation must not delete games owned by the archive.
- Remove only Heroic/Legendary filesystem grants. Keep existing Steam grants
  for screenshots and controller naming, including Album's existing bounded
  deletion behavior. Keep network, PipeWire, input, graphics, portals and
  read-only status integration. Add no host execution or broader access.
- Preserve Home's visual/interaction design and four utility destinations:
  Album, Activity, Achievements, Settings. Retain full-shell Library. Replace
  only the obsolete Library install hint with “No games yet”.
- Keep Gamescope plans and ADRs as superseded history. Do not resume spikes,
  install host packages, or add compositor/session support. Normal Wayland,
  fullscreen, focus, signals and input handling remain.

## Consequences and tradeoffs

Home/Library have no active games, and no new play sessions or cover fetch jobs
are produced by a game source. Existing totals/recent history remain, while
installed-most-played slots exclude archived games. Historical details remain
stored; retired games are not added to the launch catalog to expose them.

Previously imported Steam identities continue to drive achievement reads and
Milestones. A fresh install has no such catalog; independent account-library
retrieval requires later work. No unlocks, playtime or provider identities are
invented. SteamGridDB remains available but has no active-game work yet.

An explicit archive adds a small schema/read boundary but preserves more data
than copying earlier removal migrations. Metadata and cached files remain
without masquerading as installed games. Prior deletions by older immutable
migrations cannot be recovered by this change.

The recovery environment has no usable Cargo/rustc/rustfmt or Flatpak builder.
SQLite checks and static inspection do not establish compilation, UI parity
or ARM compatibility. Actual Rust/Slint and per-architecture Flatpak validation
remain required before release. No commit or push is authorized by this ADR.

## Intentionally deferred

No emulator core, framework, permanent core API, ROM/BIOS manager, save-state
system, new achievement backend or emulation settings are implemented.

A separate decision must compare in-process hosting with isolated child
processes inside the sandbox, including crash containment, lifecycle, video
and audio integration, controller ownership, save-data permissions and
architecture-specific core licensing/compatibility. Start with a narrow
validated prototype only after that decision; do not constrain it with a
premature hosting abstraction during recovery.
