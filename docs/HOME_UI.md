# Phase 2 home UI

Phase 2 replaces the Phase 1 appearance verification screen with the first real Horizon home shell.

## Goals

- Match the proportions and visual rhythm of the supplied console-home reference without copying proprietary artwork or branding.
- Keep all application state in Rust.
- Keep layout, styling, and animation in Slint.
- Keep design constants centralized in `ui/theme/theme.slint`.
- Preserve the Phase 1 system theme, accent, higher-contrast, and reduced-motion behavior.
- Use generated demo cover art until real library import exists.

## Component tree

```text
AppWindow
└── HomePage
    ├── TopNavigation
    ├── SelectedGameLabel
    ├── GameCarousel
    │   └── GameTile × N
    │       ├── FallbackCover
    │       └── FocusFrame
    └── Footer
```

## State ownership

`HomeController` owns the Phase 2 demo model and selected game state.

```text
Rust HomeController
       │
       ├── GameCardData model ──────────────┐
       │                                    │
       └── selected index/title             │
                    │                       │
                    v                       v
                 AppWindow  ───────────> HomePage
                    ^
                    │ select-game(index)
                    │
                 Slint UI
```

Clicking a tile emits selection intent. Rust validates the requested index and updates the presentation state. The Slint carousel reacts declaratively.

Keyboard/controller navigation is intentionally deferred to Phase 3. Phase 2 only provides pointer selection so the carousel animation and state boundary can be exercised without pre-empting the input architecture.

## Demo artwork

`FallbackCover` renders original gradient artwork from presentation-model colors. It is not a filesystem-backed image loader; Phase 7 uses it for real source-backed games until the generic production artwork pipeline is introduced.

## Layout tokens

Home geometry lives in `Metrics`, including:

- top chrome height
- selected-game label position
- carousel position and height
- footer height
- carousel anchor
- tile size and stride
- selected scale
- artwork inset

The components do not independently tune these values.

## Motion

- focus scale: `Motion.focus-duration`
- focus-frame fade: `Motion.focus-duration`
- carousel translation: `Motion.carousel-duration`

All three resolve to zero when the desktop requests reduced motion.

## Phase 2 non-goals

- SDL3/controller support
- keyboard navigation architecture
- real game import
- SQLite
- launching games
- live clock
- actual network/battery status
- real cover artwork

## Phase 3 UI refinement pass

Before Phase 4, the running UI received a visual correction pass based on the
first real Flatpak build:

- focus corners are rounded vector paths rather than eight square rectangles;
- the carousel has a dedicated shelf/background region;
- the selected-game label, connector, and selected tile share one explicit
  horizontal axis;
- top navigation uses custom vector icons and three alignment regions;
- Wi-Fi and battery indicators use vector geometry rather than Unicode glyphs;
- the controller indicator is absent when no controller is connected;
- the clock is treated as one centered visual group, including its AM suffix.

This is intentionally a reusable-component/design-system correction, not a set
of one-off offsets for the current 1280x720 development window.

## Phase 3 carousel behavior refinement

The home carousel is a clipped viewport, not a fixed selection anchor.

- The selected game moves left/right across visible positions.
- The row remains stationary while the selected slot is fully inside the
  viewport.
- Once selection reaches an edge, the row scrolls only enough to reveal the
  next selected slot.
- The current scroll offset is preserved while navigating back through visible
  games, producing a real dead-zone/hysteresis behavior instead of snapping
  back toward the first game after every input.
- The shelf owns clipping, so games cannot render beyond its left/right bounds.
- The selected-game label and connector derive their horizontal axis from the
  actual selected tile position.
- No separate continuation arrow is rendered; carousel movement/clipping communicates additional games.

Scroll offset is presentation state local to `GameCarousel`; Rust continues to
own *which* game is selected. This keeps visual viewport policy out of the
application/domain layers while preserving the existing semantic input path.

## Phase 3.7 alignment and typography refinement

Before Phase 4, the home shell receives one more reusable refinement pass:

- the selected-game pill contains centered text only; the decorative leading
  cartridge glyph is removed;
- the right-edge continuation button is removed entirely;
- profile, centered navigation, Wi-Fi, battery, and separators use one shared
  top-bar vertical axis;
- footer content is lifted from the geometric center with one centralized
  metric so the clock and action hints match the reference's optical position;
- the application Window defines one default font family through
  `Typography.family`, so typography is consistent across every component;
- heavier placeholder weights are softened while preserving hierarchy.

These are shared design-system/layout changes, not page-specific coordinate
exceptions. See ADR 0010.

## Phase 3.8 world-space carousel camera

The carousel scene now matches the reference more closely by treating the
middle of the home screen like a camera over one continuous world-space strip.

`GameCarousel` owns one moving scene containing the title pill, connector,
rounded backdrop, tiles, and focus frame. Focus may move left/right inside a
central camera safe zone; when it reaches an edge, the entire scene pans.

The rounded backdrop is therefore intentionally *not* permanently centered in
screen space. Near the first game its left rounded end is visible. In the middle
of a long library both ends can be outside the viewport. Near the final game the
right rounded end becomes visible at the same inset. This is intentional camera
behavior rather than an alignment error.

The camera root, not the backdrop, performs clipping. This ensures that title,
connector, backdrop, and games enter and leave the visible screen together.

The top chrome was also normalized so `TopNavigation` spans the full chrome
height and profile/navigation/system-status controls derive from exactly the
same vertical center. Footer optical lift is centralized in `Metrics` and was
increased slightly so the clock reads vertically centered in the rendered
footer rather than merely in its text box.

See ADR 0011.

## Phase 3.10 responsive shell and edge alignment

The top-level `Window` is no longer fixed at 1280×720. That size is now the
logical design surface. The window may resize/maximize freely, while the entire
design surface is uniformly scaled and centered. This preserves the reference
composition and prevents component-by-component scaling drift.

Top chrome now spans the full design surface. The profile avatar is inset from
the true left edge by `Metrics.top-edge-inset`; the battery's right edge uses
the same inset on the opposite side. Wi-Fi and its separator are derived from
that right-side cluster, while the navigation group remains centered.

The footer clock is live rather than a mock value. Its complete visual group is
centered in both 12-hour and 24-hour modes and receives one shared optical lift
from `Metrics.footer-content-lift`.


## Focus treatment (Phase 3.11)

The selected tile scales to 1.10 within its fixed layout slot. Four thick,
rounded stroked brackets sit just outside the game shell and scale with it.
This supersedes the blocky filled-corner experiment from Phase 3.9.

## Phase 3.14.17 focus accent sweep

The selected focus frame still consists of exactly one layer of four rounded
stroked corner paths. Its stroke uses a calm 4.2-second linear-gradient sweep:
the base is the current system focus accent and a moderately wider, brighter highlight is derived
from that same accent rather than introducing another hue. The extra contrast is intentionally modest so the motion reads more clearly without becoming a glow. Only the selected
frame evaluates the animation. Reduced-motion and high-contrast modes render the
same geometry with a static solid focus color.


## Phase 4.1.1 focus sweep contrast

The selected focus frame still consists of exactly one layer of four rounded
stroked corner paths. Its stroke now uses a more visible 3.6-second linear-gradient
sweep. The highlight is derived by mixing the current system accent 62% toward white
instead of relying on a generic brightness adjustment; this keeps saturated accents
such as red visibly animated. The highlight also has a short plateau around the center
of the sweep so it reads clearly without adding a halo or a second focus layer. Reduced
Motion and High Contrast deliberately keep the same geometry with a static solid focus
color.


## Phase 4.1.2 focus sweep balance

The focus sweep keeps the accent-to-white approach so saturated accents remain readable, but the highlight is intentionally calmer than Phase 4.1.1: the mix is 42% toward white, the bright plateau is narrower, and the cycle is 4.0 seconds. This keeps the motion visible without making the focus indicator look like a flash or glow. Reduced Motion and High Contrast remain static.

## Phase 4.1.3 focus sweep balance

The white mix is reduced slightly from 42% to 38%. Highlight width and the 4.0-second cycle are unchanged. Reduced Motion and High Contrast remain static.


## Phase 4.1.5 focus sweep balance

The white mix is reduced to 34%. Highlight width and the 4.0-second cycle remain unchanged. This is the current focus-sweep baseline. Reduced Motion and High Contrast remain static.


## Phase 4.7 responsive-shell hardening

The existing responsive-fill model is unchanged: Horizon uses one uniform scale derived from the 1280×720 reference surface and expands the logical viewport to consume extra width/height. Phase 4.7 guards the scale denominator against compositor-reported zero-sized transient surfaces during minimize/fullscreen changes and clamps the central shell content region to a non-negative height. This is defensive only; normal windowed, fullscreen, 16:10, and ultrawide geometry remains the same.

## Phase 7 source-backed Home

Phase 7 replaces the original hard-coded demo card vector with the durable library returned by `LibraryService::games()`. `HomeController` owns the ordered `LibraryGame` values needed for launch identity and derives a Slint-facing `GameCardData` model from them. Selection is still Rust-owned and all established carousel/focus behavior remains unchanged.

The data flow is now:

```text
Steam GameSource
      ↓
SourceImportService
      ↓
SQLite / LibraryService
      ↓
Vec<LibraryGame>
      ↓
HomeController ──> GameCardData ──> Slint HomePage
      │
      └─ Accept ──> GameLaunchService
```

There is no source check in `HomeController`. Any later source that imports a `LibraryGame` and advertises the generic launch capability can use the same Home/launch path.

Games without production artwork use `FallbackCover`, a renamed source-neutral version of the earlier procedural demo artwork. Its palette is derived deterministically from durable game identity/title so the placeholder remains stable between launches. Horizon does not fabricate demo games when the durable library is empty.

A zero-game library uses a dedicated Home empty state. `GameCarousel` is instantiated only when at least one durable game exists, so its selected-title pill, connector, shelf, focus frame, and camera geometry can never render against an empty model.


## Phase 7.0.5 launch feedback

A fresh Accept on a source-backed Home game immediately enters source-neutral launch feedback before `GameLaunchService` dispatch. The selected title pill keeps the same fixed geometry and shows a second-line `Launching…` status. The selected card performs a restrained press-in by changing its real target width/height from the normal selected scale to `Metrics.game-launch-pressed-scale`; it does not transform-scale an already rasterized subtree.

While launch handoff is pending, Home suppresses duplicate Accept and Left/Right carousel actions. A synchronous dispatch error changes the status to `Launch failed`. Pending feedback clears when Horizon loses OS window activation to the launched game/another application. `Motion.launch-press-duration` respects Reduced Motion. See ADR 0043.


## Phase 8 launch-to-activity handoff

A successful generic launch dispatch now also arms the source-neutral Activity service with the selected Horizon `GameId` and owning `SourceId`. This does not start playtime immediately. The observed session begins only if Horizon subsequently loses OS window activation.

If launch feedback is cancelled before that handoff (for example by abandoning the pending Home launch state), the pending activity handoff is cancelled as well so an unrelated later alt-tab cannot fabricate a game session. Steam-specific state is never exposed to Home.

## Phase 9.5 managed launch feedback

Home keeps the existing source-neutral `Launching…` acknowledgement for both external and managed launches. A managed receipt adds only an opaque `ManagedSessionId` to Rust launch feedback so an unusually early helper/session failure can clear or fail that acknowledgement; no helper, Gamescope, or provider command detail enters Slint.

Once the game takes foreground activation, Home clears the transient launch acknowledgement as before. The managed session itself continues independently in `GameLaunchService`/`ActivityService` until the host helper reports it terminal.
