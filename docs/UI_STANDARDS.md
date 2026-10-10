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
| Route changes | The page body crossfades. Bottom-bar chrome (hints, bottom-bar context, the bottom scroll shadow) switches with the route instantly, so a leaving page never ghosts into Home's footer. | `focus-visible` / `active` on each page |
| Scroll edges | Top shadow only once content has scrolled under the header; bottom shadow only while more content is below. Driven by the animated camera where there is one. | `ScrollEdgeShadow` |

## Controller hints

`page-hints` in `ui/app.slint` is the single mapping from page state to hints.
It is display-only: it must mirror what the Rust handlers actually do
(`NavigationController::handle_action`, `LibraryController::handle_action`,
`SettingsController::handle_action`). Change both together. Buttons: A (Accept),
B (Back), LB/RB (bumpers). A shared action can show two glyphs (`button2`). While a dialog is open the bar fades out with the page behind the backdrop; dialogs show their own focusable buttons.

## Lists and surfaces

- **Focusable rows** (Settings, Achievements games) are `ListRow`: one height
  (`Metrics.list-row-height`, gap `list-row-gap`), `Theme.surface`,
  `radius-lg`, 1px divider border, hover to `surface-raised`, and the Settings
  focus outline that fades in and out over `Motion.focus-duration`. Titles use
  `heading-size`, subtitles `body-size`. Page-specific trailing content is passed
  as children.
- **Panels and read-only rows** (Activity panels, session history, achievement
  entries) are `Surface`: same colour, radius and border, never focusable.
- **Game covers** get the Home corner-bracket `FocusFrame`; rows and buttons
  get the outline. Never mix them.

## Text

- No ellipsis anywhere. Long text uses `MarqueeText`: the focused item scrolls
  with Home's pacing (2s hold, distance-based travel, 2.4s end hold, fade
  reset); everything else is clipped with a soft edge fade. Reduced Motion keeps
  text still.
- `MarqueeText.edge-color` must match the colour behind the text
  (`Theme.background`, `Theme.surface`, or `Theme.surface-raised`).
- Missing achievement artwork is the neutral "◇", never a star or fake badge.

## Motion

Use `Motion` tokens, not literal durations: `focus-duration` (focus reveal),
`hover-duration`, `page-duration` (route and sub-view crossfade with
`page-transition-offset` settle), `value-swap-duration` (changing values),
`list-camera-duration` (held-direction list scrolling, linear), and
`carousel-duration`. Page-specific choreography (Library's re-sort animation,
the text caret) may keep local timings with a comment. Everything respects
Reduced Motion.

## Mouse wheel

Wheel input moves the camera only, never selection or focus, and always goes
through `Wheel.rows()` (`ui/components/wheel.slint`), which follows the
desktop's natural/traditional scrolling setting.

## Checking a change

Render pages offscreen and compare before/after screenshots in light and dark
themes. Keep Home pixel-identical unless the change is meant for Home.
