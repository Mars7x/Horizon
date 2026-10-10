# Activity and playtime

> **Current-state guide:** The sections below include chronological implementation notes beginning in Phase 8. The present Activity overview shows Horizon-observed totals, most-played imported games, a weekly chart, and an unfocusable read-only per-game session history. Its **Milestones** area now previews up to three recent **real provider-attributed achievement unlocks**, currently from Steam, with Steam badge artwork when available and a neutral fallback, not artificial Horizon playtime challenges; see [ACHIEVEMENTS.md](ACHIEVEMENTS.md). Steam lifetime and Horizon-observed session time must never be added.


Phase 8 adds Horizon's first durable play-history model and replaces the Activity placeholder with persisted data.

## Two different kinds of playtime

Horizon deliberately keeps these concepts separate:

1. **Horizon-observed sessions** — time Horizon can directly observe through a documented tracking method.
2. **Source-reported lifetime playtime** — a provider's own cumulative value, when a source can supply one reliably.

They must never be added together. A source lifetime value may already include play that occurred outside Horizon, while Horizon-observed sessions describe only activity Horizon itself witnessed.

## Foreground-handoff sessions

Steam Phase 7 launches through XDG OpenURI, so Horizon does not own the game process and cannot truthfully claim exact process lifetime. The first tracking method is therefore explicitly approximate:

```text
Home Accept
   ↓
launch target dispatched
   ↓
pending launch
   ↓
Horizon loses OS window activation
   ↓
ForegroundHandoff session starts
   ↓
Horizon becomes active again
   ↓
candidate return (not complete yet)
   ↓
short grace / startup stabilization
   ↓
├─ Horizon loses activation again → cancel candidate, keep session open
└─ Horizon stays active → complete at the original return timestamp
```

Only a launch dispatched by Horizon arms this handoff. Ordinary alt-tabbing away from Horizon does not fabricate a session.

A URI launcher may temporarily give focus back to Horizon while it is still
starting the real game. Phase 9.5.26 therefore treats reactivation as a
**candidate return**, not an immediate session end. During the first 30 seconds
a startup return must remain stable through the launch-stabilization window
(with at least a 10-second grace from the bounce). Once a session is mature, a
1.5-second stable return is enough. If Horizon loses activation again before
confirmation, the candidate is discarded and the same session continues.

The grace period never inflates playtime: when a return is confirmed, SQLite is
completed using the timestamp at which Horizon originally became active, not
the later confirmation time. This remains approximate foreground ownership; it
does not claim Steam process lifetime.

The same activation boundary already owns controller-input suspension. Session tracking consumes the semantic activation event at the application/service boundary; Steam does not toggle tracking directly.

## Interrupted sessions

An open session can survive in SQLite if Horizon exits or crashes while the game owns the foreground. On the next startup, Horizon marks any such row `interrupted` without inventing an end timestamp or duration.

Interrupted sessions do not contribute to observed playtime totals. Unknown is preferable to fabricated time.

## Activity page (Phase 10.3.0)

The Activity route is a controller-first, full-shell playtime overview using the
same background, square-cover pipeline, typography, and tile-local FocusFrame
as Home and Library. It has **one** focusable region: a horizontal selection row
of up to 12 most-played games, ranked by persisted Horizon-observed seconds.
Left/Right moves within that row and clamps at the ends; Home and Back retain
normal shell navigation. Pointer selection is supported. Since Phase 10.3.2,
Accept opens that game's full Activity history, without an in-page game
launch action (Phase 10.3.2.1).

The header shows observed time during the rolling **last seven local calendar
days** and the **current local calendar month**. The Playtime panel displays
seven real calendar-day buckets, with completed/recovered session durations
clipped to each day's local-midnight boundaries (including DST changes). Open
sessions do not enter these finalized period totals before persistence; ongoing
activity is reflected after it is committed. This deliberately avoids inventing
or extrapolating playtime. The other lower panel is a muted **Milestones / Coming
later** placeholder, with no focus, achievements, progress logic, or fake data.

The cover row reuses Home's locally cached, normalized square artwork rather
than launching another source-specific artwork pipeline. A cover's caption is
**Horizon-observed playtime** only, not an unlabelled sum of source lifetime and
observed sessions. Reported Steam/Heroic lifetime time remains persisted and
independent for future game detail views. The page uses fixed, adaptive regions
rather than a large dashboard or a secondary navigation sidebar. Since Phase
10.3.2 the row spans the full shell width, with covers aligned to the page
margins at rest and a single smoothly moved strip camera. It no longer clips
covers within a narrower inset centered viewport. Offscreen movement clips
only at the actual window boundary.

The existing source-neutral Activity service and SQLite session schema remain
unchanged, except for one **read-only** interval aggregation query. No new
permissions, authentication, D-Bus contract, or database migration are needed.

## Individual-game Activity details (Phase 10.3.2)

Accept/Enter on the selected cover opens a nested Activity details page without
changing the Activity shell route. It shows the existing square artwork,
source-reported lifetime total when available, separately measured Horizon
sessions, last played date, and longest/average finished session. The
Phase 10.3.2.1 interface removes the original Play Game action; review is
strictly read-only and cannot launch a game.

Session history is **complete**, newest-first and scoped by the durable
`GameId`, with a date, start/end local clock times, source/recovered/active
status, and recorded duration for every persisted session. The repository
returns the complete list; Rust retains it and Slint renders a bounded slice
that follows controller Up/Down navigation. This keeps hundreds of historic
sessions browseable without mounting hundreds of simultaneous UI rows. An open
session is shown as *In progress* until it has a persisted end/checkpoint;
no missing duration is guessed. Back returns to the overview with the same
cover selected. Home retains its existing global semantics.

No per-game graph or milestones are added. Reported lifetime snapshots are
not decomposed into artificial sessions and are never summed with the
Horizon-observed sessions. The main overview's Milestones panel remains an
inert placeholder.

This phase adds a source-neutral, read-only repository query and a nested
presentation state only. No migration, network permission, source adapter,
or new runtime dependency is required.

## Persistence

Schema version 2 adds:

- `play_sessions` for Horizon-observed session history;
- `source_lifetime_playtime` for provider-reported cumulative values.

Historical activity retains the logical `games` row even after the final active source reference disappears. Such a game is no longer part of the active library because `list_games()` requires a current `game_sources` row, but its historical title/session records remain available to Activity.

## Future precision

Future adapters may support more precise process/session tracking. Add those as explicit `SessionTrackingMethod` variants rather than changing the meaning of `ForegroundHandoff`.

Heroic now reports lifetime playtime through the source-neutral Activity service and the separate lifetime table. Never back-fill that provider value into observed sessions or combine the counters; the two totals can refer to the same gameplay.


## Managed sessions (Phase 9.5)

Schema v3 adds `managed_session` to the persisted tracking-method vocabulary.

A managed session begins only after the host helper successfully starts a
Gamescope session. It remains open across Horizon focus changes and is completed
when the helper reports the managed session exited.

If the helper reports failure or loses the session identity, Horizon marks that
activity row interrupted with no fabricated end timestamp/duration.

`ForegroundHandoff` remains the fallback method for ordinary URI launches. Its
Phase 9.5.26 focus-bounce tolerance changes only *when a return is considered
stable*; it still measures Horizon-observed foreground handoff rather than exact
process lifetime. Activity totals can contain both methods because both are
Horizon-observed sessions; provider-reported lifetime values remain in the
separate `source_lifetime_playtime` table and are still never added to observed
totals.

## Source-runtime sessions (Phase 9.5.27)

Schema v4 adds `source_runtime` as a third explicit tracking method.

A source-runtime session is used when a registered source can identify the
lifecycle of one exact source-owned game on the host. The Flatpak does not scan
host processes itself. Instead it sends only `(source_id, external_id)` to the
same-user Horizon helper, and the helper asks the registered source adapter for
provider-specific runtime state.

Steam is the first provider. Steam launches a per-game `reaper` process whose
command line includes `SteamLaunch AppId=<appid>`. Horizon's Steam adapter checks
only for that exact numeric AppID in the host helper. The helper records the
first observed running timestamp and the timestamp at which that exact reaper
is no longer present.

This means Horizon focus changes do not affect an observed Steam session:

```text
Horizon dispatches steam://rungameid/<appid>
   ↓
host observer armed for that exact AppID
   ↓
Steam reaper appears → SourceRuntime session starts
   ↓
Alt-Tab to Horizon / another app → no session transition
   ↓
return to game → same session remains open
   ↓
Steam reaper exits → SourceRuntime session completes
```

If the host helper/runtime observer is unavailable, launch still succeeds and
Horizon falls back to the existing `ForegroundHandoff` behavior. If an observer
is lost after a source-runtime row has started, that row is interrupted rather
than assigned a fabricated end time.

This works whether Horizon is being used as a normal Flatpak window or from a
Gamescope-oriented shell. Runtime observation is not conditional on Gamescope
being installed; Gamescope is only required for the separate managed-session
launch capability.

## Phase 9.5.32 normal-Flatpak Steam lifecycle

Steam `SourceRuntime` tracking no longer depends on the optional host helper.
The ordinary Horizon Flatpak reads Steam's provider-owned
`logs/gameprocess_log.txt` through the same read-only Steam filesystem grants
already used for discovery/artwork. `AppID <id> adding PID ...` marks that AppID
running; `Remove <id> from running list` marks it stopped. Alt-Tab/focus changes
therefore do not complete a Steam `SourceRuntime` session.

The host helper remains optional and is still used for genuine managed
Gamescope sessions. `ForegroundHandoff` remains the fallback when a provider
cannot expose a reliable runtime signal from within the normal app.


## Phase 9.5.33 live in-progress Activity

Open Horizon-observed sessions are now surfaced separately from completed
history. While a source-runtime, managed, or foreground session is open, the
Activity page shows it at the top of Recent sessions as `Playing now` and its
elapsed duration refreshes once per second. The Observed playtime summary also
adds the current in-memory/open-session elapsed time for display.

This live value is presentation-only until Horizon receives a real terminal
signal and completes the session. Completed-session count, durable historical
totals, and Most played remain based on completed rows so an unfinished session
is never silently converted into permanent history.

Schema v6 supersedes the old all-or-nothing crash rule with persisted
observation checkpoints. Live sessions remain `open`, but Horizon periodically
records the last instant at which the session was definitely still being
observed. Startup recovery never uses reboot/startup time as a fabricated exit.


## Phase 9.5.34 crash-resilient checkpoints

Every live Horizon-observed session now receives a durable checkpoint every
five seconds. The checkpoint is conservative: it means Horizon definitely knew
the session was still live at that timestamp; it does not claim the game ended
there. The session-start transaction itself provides the initial checkpoint.

On a clean exit, the normal source-runtime/managed/foreground terminal signal
continues to provide the final `ended_at`, so no precision is lost. On a hard
Horizon crash or full computer power loss, the next startup converts any
leftover `open` row to `interrupted` and recovers duration only through its last
committed checkpoint. That recovered duration is included in Observed playtime,
Recent sessions labels it `Recovered`, and Most played may include the confirmed
recovered seconds. The completed-session count still excludes interrupted rows.

This intentionally loses the small uncheckpointed tail rather than inventing
playtime after Horizon's last proof of life. With a five-second cadence the
normal loss window is less than one interval, subject to filesystem/storage
durability. SQLite synchronous mode is explicitly `FULL` for these commits.

If a session is interrupted before any checkpoint after its start (for example
the game or launcher is lost within the first interval), its duration is
**unknown** (`ended_at` stays NULL), not zero. Game history shows it as
*Unknown duration*; it adds nothing to Observed playtime, Most played, played
game counts or Recent sessions. Earlier builds recorded such rows as 0 seconds;
those existing rows are left as they are (no migration).


## Phase 9.5.44.46 — Heroic source lifetime

Heroic's `timestamp.json` supplies cumulative time in **minutes**, including
play outside Horizon. Horizon stores this data separately from its observed
session timeline. The Activity page now shows both totals independently; it
never adds them. Provider-reported totals only update after Heroic completes a
launch and writes its store, so the approximately 20-second polling interval
applies **after that write**, not as live process/session detection.

The right-side Activity panel distinguishes Most played (observed) from
Source-reported lifetime. A provider lifetime row cannot reconstruct individual
historic Heroic sessions, nor prove a game is currently playing.


## Phase 10.3.1.1 — Immediate Activity covers and complete-card navigation

The Activity showcase is constructed from the persisted overview and Home's
already resolved artwork before the UI's first route activation. On each entry
into Activity, a synchronous refresh updates the overview *before* the Activity
page becomes the active route; returning to Activity never waits for the
three-second Home recents timer to populate the game row.

Activity retains a single `VecModel<ActivityCoverData>` for its lifetime.
When the observed game ordering is unchanged, the refresh updates only changed
playtime labels instead of remounting the model. A Home artwork resolution is
forwarded directly to the corresponding Activity row, without waiting for the
periodic refresh. If the ordering changes, the selection is restored by
`GameId`, not by the old visual index.

The horizontal cover strip renders **complete game covers only** at rest, with
no permanent partially clipped cover, chevron, or scrollbar. Its up-to-12
cards remain mounted inside one clipped camera viewport, and the entire strip
animates horizontally with Home's existing carousel easing. This avoids the
previous immediate hide/show of cards and flashing left/right arrows under
rapid controller repeats. The strip stays centered within the Activity page;
Left/Right controller navigation and tile-local FocusFrame remain unchanged.
Movement alone communicates additional offscreen games. Under Reduced Motion,
the established carousel animation duration collapses to an immediate camera
update. No new route, extra panel, or Milestones functionality is introduced.

## Activity selection lifetime (Phase 10.3.2)

Opening Activity from another route selects the first displayed game, rather
than restoring the last selection from a previous Activity visit. This reset
occurs before the entry refresh publishes the game row. Opening a game's detail
view and backing out does **not** reset the selection: the same cover remains
selected for the current visit. This also applies to Global Home and Back
leaving Activity; a subsequent entry starts at the first game.

## Phase 10.3.2.1 — Gallery and history view polish

The overview carousel intentionally reveals about one fifth of the next cover
at the physical shell edge, so additional games are discoverable without
arrows. The camera remains stationary for all initially fully visible tiles;
when focus advances beyond the last fully visible cover, the complete strip
scrolls with the existing Home camera duration and easing. The preview is an
actual partial card, not a separate mask or navigation target. Left/Right
clamps at the first/last game, while leaving and reopening Activity resets to
its first game as before.

A selected cover's long title uses Home's established marquee pacing: two
seconds of rest, distance-sensitive linear movement, a 2.4-second end hold,
then an invisible reset and repeat. The clipped label fades at its viewport
edges without segmented glyph slices. Unselected titles use the ordinary
ellipsis. Reduced Motion prevents automatic text movement.

Per-game Activity details use Horizon's normal solid background and a balanced
two-column design when space permits: cover, game information, separately
labelled observed/source playtime and session summary on the left; complete
Horizon-recorded session history on the right. At small window sizes the
summary compresses above the history. The page uses Settings-style 220 ms
crossfade and 12 px settling motion; the selected session row animates its
outline and surface as focus moves. Back restores the same cover in Activity.

There is **no Play Game action**, per-game graph, milestone tracking, or extra
focusable controls. The history list itself is the sole controller focus
region within details, with Up/Down scrolling through every persisted record.
The session model remains bounded to a visible window rather than mounting the
full history in Slint. No stored data or tracking semantics change.

The Phase 10.3.2 description above is retained as a historical record. Its
Play Game control and complete-cover-at-rest cue have been superseded by this
phase; the current interface intentionally has neither the launch action nor
an all-whole-covers carousel layout.

## Phase 10.3.2.2 — Cover focus parity and Activity activation feedback

The Activity overview uses Home’s existing `Metrics.game-selected-scale`
for the selected game cover, rather than leaving the artwork shell static. The
original Home `FocusFrame` is reused unchanged. Its origin and dimensions
are now anchored to the *animated cover shell*, with about six logical pixels
of clearance, instead of a frame attached to an unscaled card box. This
keeps the corner brackets close to the artwork even at the smallest part of
the press feedback, while preserving Library’s compact focus-gap language.
The carousel’s vertical clipping allowance expands to contain the larger
selected artwork and focus corners without moving the row itself.

Accept/A on a selected cover or pointer activation now uses the same short
press-in rhythm as Home: the selected cover contracts from Home’s 1.10 scale
toward 1.025 for 125 ms, then releases into the existing Settings-style
details page transition. Activation requests are ignored while a press is
pending, and moving focus, leaving Activity, or exiting the route cancels
stale requests. Under Reduced Motion, the details view opens immediately
with no delayed press feedback. This is a **details-open animation**, not a
game launch: the Activity detail screen still has no Play Game control.
Home, Library, source data, session history and Milestones are unchanged.

## Phase 10.3.2.3 — Static clipped titles and read-only scrolling history

Activity overview game titles **never** show an ellipsis. Every long title
rests at its beginning with a clipped, softly faded trailing edge, including
unselected games. The selected game's title retains the original Home-timed
marquee (2-second initial hold, distance-sensitive travel, 2.4-second end
hold and invisible reset); Reduced Motion keeps it stationary. Short titles
remain fully visible and never auto-scroll.

In individual-game Activity details, the session history is **read-only**:
there is no selected session, active border, or clickable session row. Up and
Down scroll its virtualized row window directly, one session at a time, and
clamp at the top and bottom. If all recorded sessions fit, Up/Down does nothing.
Resizing clamps the current viewport rather than choosing a focused row.
No session records or lifetime playtime calculations are changed.

On wide screens the `Session history` and `Playtime` headings share the same
y coordinate, as do the first history row and first Playtime stat panel.
At compact widths the two sections stack to avoid overlapping content.
The rest of Activity and the inert Milestones placeholder are unchanged.


## Phase 10.3.2.4 — Cover/caption spacing

Activity captions are positioned against the maximum Home-selected cover
footprint, *including* the actual animated focus frame, not against the
unselected artwork slot. At every focus state a 22px baseline clearance below
the bracket bounds separates the cover from the title, with a further 11px
between the title's 25px line box and the recorded-time caption. Labels share
one fixed baseline through selection/press transitions; they never move into
or beneath the focused frame. The heading and `Tracked by Horizon` legend are also moved above the
expanded focus bracket top edge to prevent upper-caption collisions. The
carousel's clip area includes both caption lines. The chart and inert Milestones panels follow the caption stack with
28px breathing room instead of being laid out at a conflicting fixed Y value.
No artwork, animation timing, session data, or controller behavior changes.


## Phase 10.3.3 — Activity audit and hardening

A fresh Activity visit always starts with the first game, whether the route is
opened directly or restored through Back from another route. A new visit
also closes any nested details screen retained by an earlier Home/Menu route
change. Back from nested game details within the same Activity visit is
different: it retains the selected cover.

The most-played cover query excludes games that no longer have an installed
source **before** applying the cover limit. Historical session records and
observed playtime totals are retained and are not deleted or rewritten. The
SQLite test helpers and regression coverage are kept runnable under `cargo test`.

No changes to Activity's UI, visual animation, session tracking, milestone
placeholder, Home, or Library behavior are introduced by this hardening pass.

## Phase 10.4.0 — Real Steam achievement preview

The `Milestones` heading remains part of Activity's existing non-focusable
information panel, but it no longer contains the inert `Coming later` text or
Horizon-generated playtime challenges. Instead it lists up to **three most
recent unlocked Steam achievements**, sorted by actual unlock timestamps from
the read-only Steam achievements importer. An unconfigured or inaccessible
Steam profile produces a truthful message instead of fake zero progress.

The detailed achievements collection lives in the seventh top-level Home
utility, **Achievements**, not inside per-game Activity session details.
Horizon-observed session history, provider lifetime totals, charts, cover
focus, and controllers are unchanged. See `docs/ACHIEVEMENTS.md`.

## Phase 10.4.2 — Achievement badge preview

The Milestones panel shows up to three recent real achievement unlocks. Each row reserves a 54 px badge slot and displays achievement name, game/source, and unlock date. Steam's official achievement schema provides artwork URLs where available; images are bounded/allowlisted and loaded asynchronously after the unlock records, so a missing/slow icon never prevents actual activity from appearing. Activity session history, observed playtime, provider lifetime values, and controller focus remain unchanged.

## Phase 10.4.3.3 — Read-only session history mouse wheel (pending build verification)

The individual-game Session history viewport now accepts mouse-wheel scrolling as well as controller/keyboard Up/Down. Neither path focuses or activates a session row. Bounds and virtualization remain managed by the existing Rust Activity details controller. The main Activity overview and Home are unchanged.
