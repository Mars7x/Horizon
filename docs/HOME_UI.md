# Phase 2 home UI

> **Current-state guide:** Home’s original focus geometry, selected-card scale, and marquee are protected reference behavior. Older phase sections below include intentionally rejected/reverted title masks and focus experiments; inspect the present `ui/pages/home.slint` and its components before copying any implementation. See [PROJECT_STATE.md](PROJECT_STATE.md).


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

A fresh Accept on a source-backed Home game triggers a brief 125 ms source-neutral press animation before the selected cover springs back over 230 ms. A successful `GameLaunchService` dispatch shows a compact, separate `Playing` pill beside the unmodified game-title label instead of `Launching…` as a second title line. `Playing` denotes a successful launcher dispatch, not independently verified process state for external URI launches. The press uses real width/height geometry (`Metrics.game-launch-pressed-scale`), not a bitmap transform.

While launch handoff is pending, Home suppresses duplicate Accept and Left/Right carousel actions. A synchronous dispatch error continues to show `Launch failed`. The Playing pill clears when Horizon loses OS activation, when navigation resets it, or when a managed session terminates. Both `Motion.launch-press-duration` and `Motion.launch-release-duration` respect Reduced Motion. See ADR 0043 (historical), ADR 0101.


## Phase 8 launch-to-activity handoff

A successful generic launch dispatch now also arms the source-neutral Activity service with the selected Horizon `GameId` and owning `SourceId`. This does not start playtime immediately. The observed session begins only if Horizon subsequently loses OS window activation.

If launch feedback is cancelled before that handoff (for example by abandoning the pending Home launch state), the pending activity handoff is cancelled as well so an unrelated later alt-tab cannot fabricate a game session. Steam-specific state is never exposed to Home.

## Phase 9.5 managed launch feedback

Home uses the separate source-neutral `Playing` pill after successful external or managed launch dispatch. A managed receipt adds only an opaque `ManagedSessionId` to Rust feedback so unusually early helper/session failure can clear or fail that acknowledgement; no helper, Gamescope, or provider command detail enters Slint.

## Phase 9.5.5 selected-title marquee polish

The selected-game title pill keeps its existing fixed geometry. Titles that fit remain centered and completely static. When the selected title is wider than the pill's text viewport, Horizon clips it without an ellipsis, holds the beginning in place for two seconds, then scrolls left to reveal the hidden end. After a short end pause, the title fades out, its position resets while invisible, and the beginning fades back in before the next two-second rest. The cycle repeats for as long as the same game remains selected. There is no animated rightward return and no visible snap back to the beginning.

Marquee travel remains distance-sensitive and linear, but slight overflow now uses a gentler minimum travel window and only a small end gutter. A narrow right-edge gradient masks the clipped text while additional characters remain hidden, making the continuation obvious before and during the reveal. Once the title has moved away from its beginning, a weaker left-edge gradient masks the clipped leading text. Both fades stay inside the title viewport and never change pill geometry.

Changing the selected title, changing launch-feedback state, or moving Home focus away from/back to the carousel resets the marquee immediately. Reduced Motion disables title movement and leaves overflowing text statically clipped with the continuation edge cue. The motion is presentation-only and does not alter carousel camera, title-pill, connector, or selection geometry.

Once the game takes foreground activation, Home clears the transient launch acknowledgement as before. The managed session itself continues independently in `GameLaunchService`/`ActivityService` until the host helper reports it terminal.


## Phase 9.5.6 title-pill fade masking

The long-title marquee edge fades are inset from the pill's rounded endcaps and clipped to a smaller band around the text line. This preserves the continuation cue without introducing visible flat spots or odd shapes near the capsule corners.


## Phase 9.5.7 title fade edge coverage

The selected-title continuation fades now extend fully to the left/right edges of the title viewport so clipped glyphs never remain visibly hard-cut. The masks remain vertically inset and rounded, preserving the Phase 9.5.6 capsule-corner fix.


## Phase 9.5.8 title fade selection timing

The continuation edge fade appears immediately when an overflowing title becomes selected or the marquee resets. Its 120 ms opacity transition is enabled only while the marquee is actively travelling, where smoothly revealing/hiding the edge masks is useful. This prevents a newly selected short-overflow title from rendering hard-clipped for a frame or brief fade-in before the continuation cue becomes visible.


## Phase 9.5.9 marquee descender coverage

The selected-title edge fades remain vertically inset from the capsule border, but the fade surfaces themselves are rectangular rather than capsule-rounded. Rounded fade surfaces narrowed near their top/bottom corners and could leave descenders such as `g`, `y`, `p`, `q`, or `j` visibly hard-clipped at the viewport edge. The safe vertical inset preserves the pill silhouette while the rectangular fade covers the complete glyph area.


## Phase 9.5.10 marquee end hold

After a long selected title finishes its one-way reveal, Horizon now holds the fully revealed end for 1.2 seconds before starting the fade-through reset. Scroll speed, reset timing, edge fades, and reduced-motion behavior are unchanged.


## Phase 9.5.11 softer marquee edge clipping

Phase 9.5.11 widened and softened the temporary surface-color edge masks. Phase 9.5.12 supersedes that masking implementation.

## Phase 9.5.12 background-independent title fading

The selected title now fades its own glyph alpha at the clipped viewport edges. `Text.color` uses a dynamic horizontal brush whose stops track the animated title's visible interval. Text reaches zero alpha immediately before each active clipping boundary, so partial glyphs do not expose the hard cut. The right fade exists while text remains hidden to the right, the left fade appears as leading text leaves the viewport, and each disappears naturally when that edge no longer clips title content.

No pill-colored rectangles are drawn over the title. This makes the marquee edge treatment independent of `Theme.surface-raised` and compatible with future translucent, blurred, gradient, or wallpaper-tinted pill surfaces. The 2-second initial hold, distance-sensitive linear travel, 1.2-second end hold, fade-through reset, repeat behavior, and Reduced Motion semantics are unchanged.


## Phase 9.5.13 marquee end hold

The fully revealed end of an overflowing selected title now remains visible for 2.4 seconds before the existing fade-through reset. This supersedes the 1.2-second end hold from Phase 9.5.10. Scroll speed, text-alpha edge fades, reset timing, repeat behavior, and Reduced Motion behavior are unchanged.


## Phase 9.5.14 Slint gradient-stop unit fix

The title-local marquee gradient percentage helper keeps its arithmetic in Slint unit space: it multiplies the input length by `100%` before dividing by the natural title width, yielding a `percent` directly. This fixes the Slint compile error caused by attempting to convert a plain float to `percent`. Visual marquee behavior is unchanged.


## Phase 9.5.15 Slint percentage coercion fix

Dynamic title-gradient stop positions are derived from a clamped unitless 0..1 length ratio and converted to Slint's `percent` type by adding that plain number to `0%`. This avoids both unsupported float-to-percent return conversion and the `length * percent / length` expression that Slint 1.18.1 reduces back to a float. Visual marquee behavior is unchanged.


## Phase 9.5.16 segmented alpha marquee mask

Slint 1.18.1 rejects the runtime float-to-`percent` conversion required to keep dynamic linear-gradient stops anchored to the moving title viewport. Phase 9.5.16 therefore supersedes the Phase 9.5.12–9.5.15 dynamic-gradient implementation.

Overflowing titles now render through a solid center region and 16 narrow clipped text slices at each viewport edge. The edge slices change the opacity of the title glyphs themselves using a smoothstep falloff, reaching zero alpha at the physical clip boundary. This preserves the background-independent behavior needed for future translucent, blurred, gradient, or wallpaper-tinted pill surfaces without painting `Theme.surface-raised` over the text and without using dynamic percentage-valued gradient stops. The 2-second initial hold, distance-sensitive linear travel, 2.4-second end hold, fade-through reset, repeat behavior, and Reduced Motion semantics are unchanged.


## Phase 9.5.17 marquee fade implementation rollback

The segmented text-slice alpha mask from Phase 9.5.16 was reverted. In practice it introduced visible vertical seam artifacts while the title moved because separately clipped text slices were anti-aliased independently. Horizon now returns to the cleaner surface-overlay fade treatment for marquee edges, while keeping the refined marquee timing, including the 2-second initial hold and 2.4-second end hold. A background-independent alpha-mask implementation may be revisited later only if Slint exposes a clean, build-safe mechanism that does not introduce slice seams.


## Phase 9.5.18 selected-title image mask

Long selected titles now render from a Rust-generated SVG alpha mask, displayed through Slint's `Image` element and colorized with a fixed viewport-anchored gradient. Short titles still use normal centered `Text`. This removes the previous surface-overlay fade dependency for overflowing titles, keeps the one-way marquee timing/behavior unchanged, and remains compatible with future non-opaque pill treatments because clipped glyph alpha is handled inside the title image rather than by painting the pill background over the text. Rust regenerates the title mask only when the selected title or launch-feedback compact state changes. The title width still comes from Slint's own hidden `Text` measurement so the mask source width stays aligned with the UI typography.


## Phase 9.5.19 image source-clip unit fix

Slint 1.18.1 defines `Image.source-clip-x`, `source-clip-y`, `source-clip-width`, and `source-clip-height` as integer source-image coordinates. Horizon therefore converts logical `length` values to unitless pixel numbers by dividing by `1px`, then uses `Math.round(...)` to produce the required `int`. This is a compile-time type correction only; marquee timing and image-mask behavior are unchanged.


## Phase 9.5.20 title-mask width ownership fix

The image-mask marquee no longer reads the hidden title measurement back through `HomePage`/`GameCarousel` child IDs. Slint remains the sole owner of the actual title width, overflow decision, and marquee travel distance. Rust only creates a conservatively oversized transparent SVG source canvas for the selected title; the moving `source-clip-x` viewport reveals the required portion. This removes an unnecessary UI-to-Rust telemetry path and fixes the Slint child-ID scope build failure without changing marquee behavior.


## Phase 9.5.21 integration repair

The selected-title image-mask work must be layered onto the current Phase 8+ application shell rather than replacing `ui/app.slint` with an older shell snapshot. The production Activity route, `ActivitySessionData`/`ActivityGameData` exports, Activity summary/model properties, and `ActivityPage` bindings remain intact. The title-mask addition is limited to the `selected-title-mask-image` property passed into Home. The Rust-generated SVG uses a two-hash raw string delimiter because the SVG color literal `#ffffff` would terminate a one-hash raw string.

## Phase 9.5.22 marquee rendering regression fix

The temporary image-backed title-mask experiment introduced two visual problems
for long selected titles:

- the title glyphs were rasterized into an intermediate image, so scrolling
  text became visibly soft compared with native Slint text rendering;
- `source-clip-*` scrolling operates on integer image coordinates, which made
  the marquee advance in quantized steps and appear choppy.

Phase 9.5.22 intentionally removes the image-backed marquee path and returns
`SelectedGameLabel` to native text rendering with the previously tuned
surface-overlay fades. This restores crisp glyphs, smooth motion, and the
existing delayed one-way marquee behavior while preserving the current 2.4 s
end hold.

## Phase 9.5.44.45 Playing badge lifecycle and placement

The Playing badge belongs to the game card, centered just inside the lower
artwork edge rather than beside the selected-title pill. It follows card scale
without depending on game selection, Home focus, or window activation.

The badge now appears only for runtime-observed games after the source emits a
confirmed Started event or for a genuinely managed session after launch. The
exact matching terminal event clears it, including an unknown/lost lifecycle.
Overlapping game sessions are tracked independently. Steam runtime observation
can therefore keep a badge visible while a game is running even when Horizon
is not the foreground window. Heroic's currently external-only dispatch does
not claim a confirmed Playing state; launch dispatch alone is insufficient.

Transient launch handoff/press feedback is separate from this lifecycle state.
Artwork refreshes preserve the badge. This supersedes ADR 0101's decision to
display Playing immediately on dispatch and clear it on window deactivation.

## Phase 9.5.44.50 — Animated Playing badge transitions

The Playing badge remains anchored just above the game card's lower artwork edge,
independent of selection and Home focus. The card keeps the badge component in
its Slint tree even when `is-playing` is false; binding its existence to `if`
would discard it before an exit animation can run. Instead, only `opacity` and
`y` respond to the existing source-driven session state.

The pill enters with a 210 ms fade and gentle 7 px rise. It exits with a
155 ms fade and sink, remaining in place and fully readable while the game runs.
An interrupted transition reverses to the new target rather than restarting a
separate timer. Both motions use `Motion` tokens and resolve instantly when
Reduced Motion is enabled. The pill uses a Switch-inspired dark neutral fill,
white lettering, and no border or accent-coloured outline; only the small dot
uses the selected accent. The capsule stays legible in High Contrast. Card
geometry, launch/session semantics, and source permissions remain unchanged.

## Phase 9.5.44.51 — Reduce Playing badge drift during focus scaling

The active Playing capsule should not noticeably jump when its cover is
selected or deselected. The game art still grows smoothly from 220 px to
242 px at 1.10× selection, but a layout-only badge anchor compensates for
40% of that growth. As a result the pill moves approximately 2.2 px rather
than 11 px vertically during this focus transition. The badge remains centered
and comfortably inset inside the cover, keeps its constant 90×30 px size and
borderless neutral appearance, and does not obstruct focus brackets.

The Playing lifecycle animation now occurs inside that anchor, not on the
anchor itself. Its separate 210 ms enter/155 ms exit fade-and-slide continues
to respond only to `is-playing` and honours Reduced Motion. Shell geometry,
selection animation, title pill, Steam/Heroic observation, and persistence are
unchanged. See ADR 0108.

## Phase 9.5.44.52 — Eliminate Playing-label end-of-animation popping

The pill's background still rises 7 px/fades in over 210 ms and sinks/fades
over 155 ms. Its white `Playing` label and accent dot now live in a sibling,
stationary layer instead of following the background's animated `y` coordinate.
The content opacity animates over 160 ms on entry and 120 ms on exit, allowing
the text to settle before the capsule's final movement frames. This prevents
subpixel-baseline text re-rasterization at the end of the transition.

The card-growth compensation is now exactly 50%, keeping the label's baseline
stationary while the art expands or contracts on focus; the earlier residual
2.2 px drift is removed. The pill remains borderless, bottom-centred, and
Switch-inspired. All four animation durations honor Reduced Motion. No changes
to game lifecycle, Heroic/Steam runtime observation, or playtime recording.
See ADR 0109.


## Phase 9.5.44.54 — Keep Playing badge anchored to the artwork

Selection expands a game card symmetrically around its centre. The earlier
50% growth compensation from Phase 9.5.44.52 kept the Playing label fixed in
screen space, but unintentionally made the badge travel **relative to the
artwork** as its height changed. Phase 9.5.44.54 reverses that policy.

The persistent Playing overlay is now a child of `artwork-frame`, clipped
within the same rounded artwork viewport. Its horizontal centre and 12 px
bottom inset are defined directly against the artwork's actual animated
bounds. Consequently, the cover and badge travel together while focus or
launch-press changes card size, without any counter-transform or extra
focus-induced `y` animation. The badge remains a fixed 90×30 px capsule.

The independent Playing-state transitions from Phase 9.5.44.52 remain intact:
the background fades/slides on session start/stop, and the white text and
accent dot only fade within the badge's artwork-local coordinates. The text
no longer moves *within the pill* during its entrance/exit, although both the
text and pill follow their parent artwork when the artwork itself resizes.
The dark borderless styling, Reduced Motion, and Steam/Heroic tracking are
unchanged. See ADR 0111.

## Focus and boundary consistency (Phase 9.5.44.60)

The authored single-layer Home game focus bracket remains visually distinct
for the game carousel, but uses the same `Theme.focus`, 140 ms focus transition
and Reduced Motion behaviour as menu focus. Header utilities now use the same
outline-only focus treatment as Settings (rounded to each icon's geometry).
Directional repeat NEVER wraps. A new discrete Left/Right press when already
at the game/header row boundary MAY wrap to the opposite end; the same press
that reaches an edge does not wrap. Keyboard, controller D-pad and analog share
one Rust policy. See ADR 0117.


### Phase 9.5.44.61 utility focus alignment

The top utility outline uses the same stroke-and-colour animation as Settings
root, with a circular 48px radius-matched frame. Each 40px utility icon
remains precisely centred in its 40px focus cell through focus and defocus.
The previous quiet perimeter-highlight sweep remains, without an upward lift or background tint. The existing navigation sound,
edge-wrap rules and game artwork-specific focus treatment are unchanged.


### Phase 9.5.44.62 — Coupled utility focus lift

Phase 9.5.44.61 kept both utility icons and their circular focus strokes
stationary. The preferred earlier effect was a subtle **2px upward lift** of
the focused utility; the defect was that the outline did not follow the icon.

Each utility cell now exports one focus-owned lift offset applied to both the
40px authored icon and its 48px, transparent, single-stroke selection outline.
Their `y` transitions use identical `Motion.focus-duration` and `ease-in-out`
so the icon remains centered **inside its moving focus ring** on focus and
defocus, not necessarily at the stationary cell centre. The layout cell and
pointer hit target stay fixed; no utility tint, icon scale, or recolouring is
introduced. Reduced Motion disables the displacement. No other menu focus
style, navigation policy, or OK/Back sound is changed. See ADR 0119.


### Phase 9.5.44.62 — uniform pulse + coupled utility lift

The focus reference is **one stationary, full-perimeter accent stroke** whose
brightness rises and falls uniformly (3200ms cycle, 86%-100%). Neither the
utility outline nor the Home game's authored corner brackets use a rotating
highlight or sweeping gradient. The utilities and their matching rings rise
**together by 2px** on focus, keeping their centres aligned throughout the
140ms transition. The background remains transparent, icon SVG colours stay
unchanged, and the pointer hit area does not move. Reduced Motion/High Contrast
suppress the idle brightness pulse; Reduced Motion also suppresses the lift.

## Phase 9.5.44.63 — Home focus preservation

Phase 9.5.44.62 incorrectly replaced the Home game-card focus with an
alpha-only pulse. The *exact* native vector focus frame from Phase 9.5.44.38
is restored (with the Phase 9.5.44.39 3200 ms shared cycle). It retains the
four corner paths, their subtle size motion, the accent-derived highlight, and
the soft Gaussian emission. No Home focus geometry or animation style was
intended to change; other menus now adopt its brightness rhythm rather than
Home adopting their border style. See ADR 0120.

## Live system status (Phase 9.5.44.67)

The top-right network and battery glyphs are data-driven, not decoration.
NetworkManager reports active wired/Wi-Fi connections and the strength of the
associated access point. When disconnected or inaccessible, the indicator shows
an offline slash rather than fictional full Wi-Fi. Limited/portal connectivity
is distinguished with an exclamation point. See `docs/STATUS.md`.

UPower's composite *host* battery has priority (laptop and handheld hardware),
followed by the first SDL-connected gamepad that actually reports power.
The battery icon disappears when neither has valid information, without leaving
an empty battery-shaped slot. Charging adds a bolt. Neither source is inferred
from window focus or program launch; all readings may be hardware estimates.

### Peripheral presence motion (Phase 9.5.44.73)

Top-right battery and bottom-left controller symbol use the same restrained
status-motion family: 7px or less directional slide with alpha fade, 220ms on
appearance and 155ms on disappearance. Indicator elements are persistent, not
conditionally removed at the end of a connection; this preserves animated exit.
They remain independent of the GameCard focus-bracket design. With Reduced
Motion both the opacity and position change immediately.

### Phase 10.1.0 — Recent games and Library tile

Home's original world-space carousel now shows at most fifteen distinct installed games with Horizon-observed play sessions, newest started session first. The final item is a permanent Library destination with the same card footprint, selected scaling, and original Home `FocusFrame`; there is no fake LibraryGame. The tile remains even when there is no observed game history. The full Library keeps its complete source-backed catalogue and card updates, independently of the Home subset. Main Menu/Start is a direct Library shortcut instead of opening the old modal menu.

### Phase 10.1.1 — Alphabetical initial Home and unplayed fallback

Home now fills up to **15 game cards** from currently imported games even when no
Horizon-observed play sessions exist. The ordered, distinct games with actual
play history remain first (newest session first). Remaining slots are filled
from games **without** recorded play history, ordered case-insensitively A–Z
(full title; no removal of articles). If no games have yet been played, all
visible Home game cards are alphabetical. If at least 15 games have been played,
only the 15 most recently played appear. With fewer than 15 imported games,
every imported game appears. The permanent Library tile follows those cards;
it is the only tile when the catalogue is empty.

This affects only the Home presentation order. Neither the full Library data nor
the source-backed launch/artwork/Playing identities change. As play sessions
arrive, Home promotes their titles ahead of the alphabetical fallback, retaining
focused game identity where still present. The original carousel and its
`FocusFrame` animations are unchanged. See ADR 0136.

### Phase 10.1.2 — Minimal system Library tile

The final Home Library destination keeps exactly the same square shell footprint,
selected-size animation, camera participation, pointer target, and original game
`FocusFrame` as Phase 10.1.1. Inside its neutral art frame, the prominent 2×2
foreground grid is now centered on both axes. The duplicate word “Library”
was removed from inside the tile: the existing selected-title pill above the
carousel remains its only visible name. There is no small caption, fake cover
art, background pattern, gradient or extra focus layer. Other game-card
components, recent ordering and input semantics remain unchanged. See ADR 0137.
