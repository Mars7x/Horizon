# ADR 0191: Limit Steam to profile pictures and friends

Date: 2026-10-14

## Status

Accepted. Supersedes ADR 0190's preservation of Steam screenshots,
achievements and automatic local controller-file lookup. Earlier Album and
achievement ADRs remain historical; their shared UI/media decisions still apply.

## Context

After retiring external game launchers, Horizon still read Steam screenshots,
loaded Steam achievement caches and fetched unlocks/badges. These integrations
continued to require local Steam access and achievement-specific workers.
The user now wants only Steam profile pictures, friends and the independent
SteamGridDB artwork service. RetroAchievements is the future achievement
direction, but its implementation is outside this change.

## Decision

- Remove the Steam capture adapter and its screenshot listing, thumbnails and
  deletion code. Album continues to read Horizon-owned screenshots/videos with
  its existing viewer, selection, deletion confirmation and playback behavior.
- Remove the Steam achievement service/controller, API requests, badge
  downloads, cache reader/writer and all startup, route and account-change
  fetch hooks. Preserve the Achievements destination, shared list/detail UI
  and Activity Milestones layout with “Achievements unavailable”. Do not show
  an active source selector, an actionable Open hint or a fabricated zero.
- Preserve account credentials/settings and account switching/disconnect for
  profile pictures (including animated avatars) and read-only friends. Keep
  the existing image-host validation used by those features. Manual
  SteamID64/Web API-key setup remains; no interactive login is introduced.
- Preserve SteamGridDB keys, preferences, requests, artwork caches and refresh.
  SteamGridDB is independent of the Steam achievement and capture providers.
- Remove automatic local Steam controller-file discovery and all Steam
  filesystem grants. Retain generic SDL device recognition, glyphs and
  navigation, including compatibility with virtual input devices; no local
  Steam installation is required or searched by Horizon.
- Remove the unused VDF parser dependency and its now-unreferenced locked
  package. Keep historical attribution and license texts. No new dependency
  or permission is introduced.
- Leave existing screenshots, old achievement cache files, user artwork,
  account files, database identities, sessions and lifetime counters untouched.
  Old achievement caches are ignored. Schema stays at 0010; no migration or
  data deletion is needed. Archived identities can still name Horizon-owned
  captures that carry legacy game references.

## Consequences

Steam account changes refresh only the retained profile/friends integration.
Opening Activity or Achievements causes no Steam achievement request or cache
read. Home, its four utilities, navigation, theme and reduced-motion behavior
are preserved; Back from the empty Achievements page uses normal route history.

No RetroAchievements authentication, game identification, ROM hashing,
achievement unlocking or cache is implemented. Those need a separate design
and implementation tied to the future embedded-emulator architecture.

Rust/Slint compilation, tests and Flatpak builds still require a usable
Rust/SDK environment. Static inspection and SQLite checks do not establish
UI or per-architecture build correctness.
