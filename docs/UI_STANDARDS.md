# UI standards

Every full-screen page (everything except Home) is built from the same shared
parts so the app reads as one product. Home keeps its own chrome (top
utilities, footer) and is not changed by these rules. See
[ADR 0163](adr/0163-shared-page-frame-and-ui-standards.md).

## Page frame

| Part | Rule | Code |
| --- | --- | --- |
| Header | Title only, at `Metrics.page-title-y`, same on every page. The title is the section name (Library, Activity, Achievements, Settings); it does not change in sub-pages. | `PageHeader` in `ui/components/page-header.slint` |
| Header values | Read-outs or pointer-clickable pickers sit on the title row, right-aligned: a small `label-size` caption over a bold value. Value changes slide and crossfade over `Motion.value-swap-duration`. | `HeaderValue` |
| Content | Starts at `Metrics.page-header-height` and ends at the bottom bar. | — |
| Bottom bar | `Metrics.page-bottom-bar-height`, equal to Home's footer height so the hint line is centred in the bar. Left: page context (Library's selected game). Keep it to the selection itself; no status text such as "Saved results" or "Updating…". Right: controller hints. Hints and context sit on one line, `Metrics.hint-row-center-from-bottom` (Home's footer midline), and end at the page margin, so Home's footer "A OK" (the same `HintBar`) lines up with every page. | `HintBar` in `ui/app.slint` and `ui/components/footer.slint` |
| Sub-pages | Open with a clickable `‹  Section  /  Page` breadcrumb at the content top. Clicking it is Back. | `Breadcrumb` |
| Route changes | The shared Zoom transition (below). Bottom-bar chrome (hints, bottom-bar context, the bottom scroll shadow) switches with the route instantly, so a leaving page never ghosts into Home's footer. | `focus-visible` / `active` on each page |
| Scroll edges | Top shadow only once content has scrolled under the header; bottom shadow only while more content is below. Strength is `Scroll.edge` of the hidden pixels, from the animated camera. | `ScrollEdgeShadow`, `Scroll.edge` |

## Controller hints

`page-hints` in `ui/app.slint` is the single mapping from page state to hints.
It is display-only: it must not show anything the Rust handlers don't do
(`NavigationController::handle_action`, `LibraryController::handle_action`,
`SettingsController::handle_action`); change both together. The bar lists
the page's actions (A, X, B), not D-pad movement, and not fullscreen
shortcuts (Album's Left/Right browse and LB/RB skip).

**Bumper pickers are not hints.** A value that LB/RB change (Library Source
and Sort, Achievements Source, Album Game and Type) shows a compact glyph
before its header label (`HeaderValue.button`, and `button2` for a value
stepped both ways), so the button sits next to what it changes. Hints are also buttons: clicking one sends that
button through the same Rust input path as a controller press (`hint-activated`
→ `UiAction::from_hint`), so it does exactly what the controller would, for
whatever is focused (Home's footer "A OK" included). A pair activates the
glyph clicked and its label the first. A faded-out bar takes no clicks.
Order, left to right: X, B, then A, always rightmost (as Home's footer "A OK"). Buttons: A (Accept), B (Back), X (`Secondary`; Album: Delete), LB/RB (only
in headers). A shared action can show two glyphs (`button2`). While a dialog is open the bar fades out with the page behind the backdrop; dialogs show their own focusable buttons.

## Lists and surfaces

- **Focusable rows** (Settings, Achievements games) are `ListRow`: one height
  (`Metrics.list-row-height`, gap `list-row-gap`), `Theme.surface`,
  `radius-lg`, 1px divider border, hover to `surface-raised`, and the Settings
  focus outline that fades in and out over `Motion.focus-duration`. Titles use
  `heading-size`, subtitles `body-size`. Page-specific trailing content is passed
  as children.
- **Panels and read-only rows** (Activity panels, session history, achievement
  entries) are `Surface`: same colour, radius and border, never focusable.
- **Game covers and media tiles** (Album captures) get the Home corner-bracket
  `FocusFrame`; rows and buttons get the outline. Never mix them.

## Text

- No ellipsis anywhere. Long text uses `MarqueeText`: the focused item scrolls
  with Home's pacing (2s hold, distance-based travel, 2.4s end hold, fade
  reset); everything else is clipped with a soft edge fade. Reduced Motion keeps
  text still.
- `MarqueeText.edge-color` must match the colour behind the text
  (`Theme.background`, `Theme.surface`, or `Theme.surface-raised`).
- Missing achievement artwork is the neutral "◇", never a star or fake badge.

## Transitions: Zoom

Every page and sub-page change uses one transition, built from
`TransitionDriver` and `TransitionLayer` in `ui/components/transition.slint`:

- **Forward** (opening a page, a Settings section, a game's achievements,
  Activity details): the new layer grows in from 94% as it fades in, while the
  current one grows past to 106% and fades out.
- **Back** mirrors it: the layer you return to settles in from 106%, while the
  one you leave shrinks to 94% and fades.
- Leaving Home is forward; returning to Home is Back. Inside a section, depth
  decides the direction (Settings 0, Appearance/Third-Party 1, their pages 2).
- Section headers stay still: sub-page layers are clear above
  `Metrics.page-header-height` (`opaque` + `cover-from`).
- Route changes first draw the incoming page once at near-zero opacity
  (`prewarm`) and start timing only after that frame. A slow first frame
  (artwork upload on a fresh start) delays the transition instead of
  swallowing it.
- Home's top bar and footer zoom and fade with Home, around the window centre;
  hints appear once a page settles.
- `TransitionDriver.phase` is derived from its `key`, so a transition is already in its first phase the instant the key changes. Bind `key`; never start transitions from `changed` handlers, which run a frame late and flash the settled state.
- Duration is `Motion.page-duration`; Reduced Motion makes it instant.
- Offscreen review renders use Slint's software renderer, which ignores
  `transform-scale`: they show the fade but not the zoom. Check scaling in the
  real app.

## Slide (between items of the same kind)

Stepping between siblings shown one at a time (Album's viewer) slides
instead of zooming. The next item enters from the side of the button
pressed, and the current one leaves the other way, a gap apart. It is driven
by `TransitionDriver` with the step count as its `key`, over
`Motion.page-duration`. Reduced Motion makes it instant. Opening and closing
such a viewer is not: Album's fullscreen viewer grows out of the selected
tile and shrinks back into it (a shared-element transition).

## Fullscreen media

Album's viewer is the one exception to the page frame: the capture fills the
window on black, with no header or bottom bar. Its title, details, timeline
and hints sit on top/bottom gradients that fade three seconds after the last
input (`overlay-revision`, bumped by Rust on every viewer action). The hint
bar draws in its `on-dark` style there.

## Motion

Every animation uses a `Motion` token (`ui/theme/theme.slint`), never a
literal duration. Pick the token by **what happened**, and the easing by
**what changes**:

| What happened | Token | Examples |
| --- | --- | --- |
| Pointer hover | `hover-duration` (100 ms) | Row and button hover fills, hint hover, viewer edge arrows |
| Selection moved | `focus-duration` (140 ms) | Tile lift, focus ring in and out, tick boxes, dimming while selecting |
| A value changed | `value-swap-duration` (180 ms) | Header values, toggles, swatches, progress bars, chart bars, list rows arriving |
| A grid's contents changed | `GridRefresh` (`components/grid-refresh.slint`) | Library Source and Sort, Album Game and Type: the old grid fades out (85 ms), then covers rise in column by column (145 ms each, 7 ms apart) |
| Carousel moved | `carousel-duration` (190 ms) | Home and Activity carousels |
| Page or view changed | `page-duration` (220 ms) | Zoom, Album slide and grow, hint bar and viewer overlay fades |
| Dialog opened or closed | `dialog-duration` (180 ms), `dialog-rise` | Every modal (`ModalDialog`) |
| Camera moved | `scroll-duration` (110 ms) | Every list and grid camera, the key field's text scroll |
| Status appeared or left | `status-enter/exit-duration` | Battery, controller |

Easing: **moves and resizes ease-out** (they answer input, so they start fast);
**fades and colour changes ease-in-out**; **cameras and timelines are
linear**. Home's launch release (`ease-out-back`) is the one spring. Loops
(`caret-blink-cycle`, `spinner-cycle`, focus breathing) and the marquee
rhythm (`marquee-*`) are tokens too.

Every token is 0 under Reduced Motion; loops and marquees stop.

**Dialogs** use `ModalDialog` (`components/modal-dialog.slint`): the
component is the card (children lay out against it), over a backdrop that
takes every click.

**Choreography** that one `animate` can't express (the grid refresh,
Library's title swap, Achievements' arriving rows) runs on a `Stopwatch`
(`components/stopwatch.slint`). Never read `animation-tick()` in a binding
that stays evaluated at rest: the window then redraws every frame forever.
`Stopwatch` reads it only while running. The deliberate continuous loops are
the focus breathing, the caret, the spinner and a playing video's clock.

Slint's `Timer.restart()` does nothing on a stopped timer: use `stop()` then
`start()`.

An element hidden with `visible: false` must also hold its hidden value for any
animated property (for example `opacity: 0`). Otherwise it animates from a stale
value the moment it becomes visible (the hint bar once flashed in, faded out,
then faded in again).

A property's `animate` duration is read when the animated value is next
evaluated (at draw time), not when its inputs change. To make one change
instant (a fresh visit resetting a list to the top), keep the duration at 0
until that change has been drawn, then restore it (Achievements uses a short
timer after `visit-revision` changes).

## Scrolling

Every vertical list and grid (Library, Album, Achievements' two lists,
Activity's session history) scrolls the same way, through
`ui/components/scroll.slint`:

- **Target:** the first whole row in view. Moving focus past an edge moves
  the camera the least distance that brings the focused row fully into view.
  The wheel steps it a row at a time.
- **Never past the end:** `Scroll.camera` clamps the camera to
  `[0, content − viewport]`. At the end, the last row rests on the
  viewport's bottom edge; there is never empty space under it. A short list
  doesn't scroll at all.
- **Motion:** `Motion.scroll-duration` (110 ms), linear, just under the
  115 ms held-input repeat, so held steps glide at a steady speed.
- **Fresh visits jump:** a `CameraSettle` keyed on the visit (or on opening
  a sub-view) makes the reset instant instead of scrolling back.
- **Peeking rows** are part of the content: clicking one selects it and
  scrolls it into view.
- **Long content** mounts only rows near the camera, plus a couple of
  overscan rows each side, so a glide never reveals unmounted space.

Horizontal carousels (Home, Activity covers) are not lists and keep their
own camera.

## Mouse wheel

Wheel input moves the camera only, never selection or focus, and always goes
through `Wheel.rows()` (`ui/components/wheel.slint`), which follows the
desktop's natural/traditional scrolling setting.

## Checking a change

Render pages offscreen and compare before/after screenshots in light and dark
themes. Keep Home pixel-identical unless the change is meant for Home.
