# AGENTS.md — Horizon

This file defines the working rules for coding agents contributing to Horizon.

## Project intent

Horizon is a controller-first, console-style Linux game library. It imports games from existing launchers/sources, presents them in a unified library, launches them through their owning application, and records activity/playtime where that can be measured reliably.

Horizon is **not** an emulator manager. Do not add emulator configuration, firmware/key management, graphics settings, save management, or other source-owned functionality unless the project direction is explicitly changed.

## Primary stack

- Rust for application/backend logic
- Slint for presentation only
- SDL3 for controller input beginning in Phase 3
- SQLite + `rusqlite` for persistence when the database phase begins
- `ashpd` for XDG Desktop Portal integration
- `zbus` only where portal APIs are insufficient
- Flatpak as the primary distribution target
- Wayland first, X11 fallback

Do not introduce Qt, GTK, Electron, Tauri, web views, or another UI framework without an explicit architecture decision.

## Architecture rule

Dependency direction is one-way:

```text
Slint UI
   ↓
Presentation
   ↓
Application services
   ↓
Domain

Infrastructure adapters implement boundaries used by services/domain.
```

The domain must not depend on Slint, SDL, SQLite, Flatpak, DBus, or source-specific file formats.

### Layer responsibilities

**`ui/`**
- Render presentation state.
- Emit user intent through callbacks.
- Use design-system tokens and reusable components.
- Never parse launcher data, access SQL, inspect the filesystem, call portals, or implement business rules.

**`src/presentation/`**
- Convert Rust application state into Slint-facing models/properties.
- Receive UI callbacks and route them to application controllers/services.
- Do not contain source-specific parsing or persistence logic.

**`src/domain/`**
- Pure Horizon concepts and rules.
- No UI/platform/storage dependencies.

**`src/services/`**
- Coordinate use cases such as importing, launching, library management, and activity.
- Depend on abstractions, not concrete source implementations.

**`src/sources/`**
- Source-specific adapters such as Steam, Heroic, Lutris, and Bottles.
- Source-specific quirks must remain here.

**`src/platform/`**
- Flatpak/portal/desktop integration and platform-specific behavior.
- Hard-coded platform paths belong here or in the owning source adapter, nowhere else.

**`src/persistence/`**
- SQLite access, repositories, and numbered schema migrations.
- SQL must not escape this layer.

**`src/input/`**
- Raw SDL3/keyboard/controller events.
- Convert raw device events into semantic actions before they reach presentation code.

## No monkey patching

Do not fix architectural problems with local exceptions.

Before adding a special case, ask whether the abstraction is missing a legitimate concept. If it is, improve the abstraction and document the change. Examples of disallowed patterns include:

- source-name checks scattered outside `src/sources/`
- UI components directly handling Steam/Heroic/Lutris behavior
- hard-coded filesystem paths in presentation/services
- broad Flatpak permissions added simply to make a feature work
- duplicating slightly different copies of the same UI component
- unexplained magic numbers for layout/animation values
- bypassing a service/repository because direct access is faster to implement
- swallowing an error to keep the UI moving
- adding temporary compatibility branches with no removal plan

If a clean implementation requires restructuring existing code, restructure it before adding the feature.

## Source adapters

External game providers must conform to a common source abstraction. A new source should normally be implementable without modifying unrelated presentation or library code.

Do not write application-wide chains such as:

```rust
if source == "steam" {
    // ...
} else if source == "heroic" {
    // ...
}
```

Source capabilities should be represented explicitly rather than inferred from source names.

## Input architecture

Beginning in Phase 3, raw input must be normalized into semantic actions such as:

```rust
UiAction::Up
UiAction::Down
UiAction::Left
UiAction::Right
UiAction::Accept
UiAction::Back
UiAction::Menu
UiAction::Home
```

Slint must not know about Xbox/PlayStation/Switch button names, SDL button constants, or device-specific mappings.

Keyboard and controller navigation should drive the same Rust-owned navigation state.

## Appearance and design system

Horizon supports:

- System / Light / Dark theme preference
- system accent color with optional user override
- live XDG portal updates
- higher-contrast preference
- reduced-motion preference

Use semantic tokens from `ui/theme/`. Do not scatter literal colors, spacing, radii, font sizes, or animation timings through components.

Accent color is for interactive emphasis such as focus, selection, toggles, and activity highlights. Do not indiscriminately tint the full interface.

All new motion must respect reduced-motion state.

## Flatpak-first constraints

Horizon is designed as a Flatpak from the beginning.

- Prefer XDG Desktop Portals for desktop integration.
- Do not assume unrestricted host filesystem access.
- Do not assume `/usr/bin/<app>` exists or is visible in the sandbox.
- Do not add `--filesystem=host` or similarly broad permissions as a shortcut.
- Support native and Flatpak-installed game sources through explicit adapters where feasible.
- Keep Wayland as the primary display path; X11 is fallback only.

Any requested permission expansion must be justified in documentation.

## Persistence rules

When persistence is introduced:

- Use SQLite through the persistence layer.
- Use numbered migrations from the first schema version.
- Never mutate a released schema ad hoc.
- Preserve user libraries across upgrades.
- Keep source-reported lifetime playtime separate from Horizon-observed session data so values are not double-counted.
- Record the tracking method when playtime precision differs by source.

## Error handling

- Do not use production `unwrap()`/`expect()` for recoverable failures.
- Use typed errors at subsystem boundaries.
- Add context before propagating infrastructure failures.
- A source failure should not corrupt or make the entire library unusable.
- Do not silently fabricate data when a source cannot provide it. Unknown is preferable to a false value.

## Logging

Use `tracing` for diagnostic output.

- Prefer structured fields over formatted blobs.
- Never log secrets, tokens, private file contents, or other sensitive data.
- Expected absence of an optional source is not an error.

## Quality gate

Before a change is considered complete, run:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

The repository helper is:

```bash
./scripts/check.sh
```

For changes that affect packaging or platform integration, also build/run the Flatpak.

Do not weaken linting or tests merely to merge a feature.

## Documentation

Keep these documents current when their area changes:

- `docs/ARCHITECTURE.md`
- `docs/APPEARANCE.md`
- `docs/HOME_UI.md`
- `docs/INPUT.md`
- `docs/NAVIGATION.md`
- `docs/CLOCK.md`
- `docs/adr/`

Use an Architecture Decision Record when a change introduces or reverses a significant architectural choice. An ADR should explain the decision, why it was made, and its consequences.

## Current phase boundary

The repository currently contains work through **Phase 4.1**.

Implemented:

- Rust + Slint foundation
- Flatpak packaging skeleton
- XDG portal appearance integration
- system light/dark and accent color
- reduced motion / higher contrast
- design tokens
- Rust-owned demo home state
- top navigation
- selected-game label
- game carousel and reusable game tiles
- accent focus brackets
- pointer-driven demo selection routed through Rust
- SDL3 gamepad adapter
- semantic `UiAction` layer
- keyboard and controller normalization into the same actions
- controller startup enumeration and hotplug/disconnect handling
- left analog-stick navigation normalized into directional `UiAction` values
- analog deadzone hysteresis and controlled hold-repeat
- one app-wide bundled LINE Seed JP Slint default font family through `Typography.family`
- selected-game title-only pill and unified top/footer alignment axes
- single-layer rounded focus brackets hugging the scaled game shell
- responsive filled logical viewport with no ultrawide letterboxing
- footer chrome returned near the true vertical center
- Flatpak `input` device permission rather than `--device=all`
- pure Rust `AppRoute`/`Navigator` for Home, Library, Activity, and Settings
- explicit back-stack and global Home reset semantics
- `NavigationController` between semantic input and page controllers
- active route published to Slint as presentation state

Not yet implemented:

- visible multi-page shell/page switching
- focus-region navigation
- page transitions
- SQLite library
- real game importers
- launching
- playtime/session tracking
- Activity/Settings production pages

Do not prematurely implement later-phase behavior as a shortcut while working on the current phase.

## Phase 4 target

Phase 4 should establish the application navigation shell without adding persistence or real game sources. **Phase 4.1 is complete for items 1 and the route/action boundary of item 3; later Phase 4 passes continue the remaining work:**

1. Define Rust-owned navigation state for Home, Library, Activity, and Settings.
2. Define focus regions so Up/Down/Left/Right have deterministic screen-level behavior.
3. Route existing `UiAction` values into that navigation state; do not change SDL/keyboard adapters to understand screens.
4. Add page transitions that use the design-system motion tokens and respect reduced motion.
5. Make Back/Home/Menu behavior explicit and testable.
6. Keep page components presentation-only; do not put navigation policy in Slint.
7. Add tests for navigation transitions without requiring Slint rendering or controller hardware.
8. Preserve the current home carousel and pointer behavior through the same Rust controllers.

If implementing Phase 4 requires SQLite, source importing, or game launching, stop: those belong to later phases.

## Coding style

- Prefer small cohesive modules over large catch-all files.
- Prefer explicit domain types over strings/booleans with hidden meaning.
- Prefer composition over inheritance-like abstractions.
- Keep public APIs minimal.
- Avoid premature generic frameworks: abstract after a real boundary is understood, but do not duplicate known concepts.
- Keep comments focused on *why*, not restating obvious code.
- Delete dead code instead of leaving disabled alternatives around.

## Definition of done

A feature is done only when:

1. It behaves correctly.
2. It belongs in the correct architectural layer.
3. It does not introduce source/platform/UI special cases elsewhere.
4. It respects Flatpak constraints.
5. It respects theme, accent, contrast, and reduced-motion behavior where applicable.
6. Tests/lints pass.
7. Relevant documentation is updated.

"It works" is necessary, but not sufficient.

## Phase 3 Flatpak dependency note

The Phase 3 development manifest currently permits **build-time only** network
access so Cargo can resolve crates in fresh GNOME Builder environments. Do not
reintroduce `CARGO_NET_OFFLINE=true` or `cargo --offline` until both `Cargo.lock`
and `flatpak/cargo-sources.json` are current and committed. Runtime network
access is still not granted. Before public/Flathub release, restore a fully
offline, locked Cargo build as described in `flatpak/README.md` and ADR 0006.

## Slint backend invariant

Horizon intentionally does not enable Slint's `backend-default` feature. The
application must call `platform::slint_backend::initialize()` before
`slint::set_xdg_app_id()`, creating `AppWindow`, or invoking another
platform-dependent Slint API. The selected stack is Winit + FemtoVG. Do not fix
backend failures by enabling Qt or setting a Flatpak-only `SLINT_BACKEND`
environment variable. See ADR 0007.

## Home carousel invariant

The home library carousel is one world-space scene viewed through a camera.
`selected-index` is Rust-owned, while transient visual `camera-offset` belongs
to the Slint `GameCarousel`. Title pill, connector, rounded backdrop, game row,
and selected focus geometry must share that same camera transform. Do not
restore shelf-local clipping, a fixed selection anchor, or independent backdrop
positioning. Focus moves inside the camera safe zone; the complete scene pans
only when focus crosses a zone edge. See ADR 0011 (which supersedes the shelf-
clipping portion of ADR 0009).


## Analog navigation and typography invariant

SDL left-stick axes are device-specific input and must terminate in
`src/input/sdl.rs`. They map to existing directional `UiAction` values using
hysteresis/repeat logic; pages must never read raw axes. All Slint text should
inherit the app-wide `Typography.family` through `AppWindow.default-font-family`
unless a documented design requirement calls for another family. Do not scatter
per-component font-family declarations or independent top/footer alignment
offsets. See ADR 0010.

D-pad directions are also held navigation state inside `src/input/sdl.rs` and
must repeat after the shared controller repeat delay. Do not depend on SDL to
synthesize repeated button-down events, and do not implement D-pad repeat in
pages/controllers. See ADR 0014.

## Focus indicator invariant

The home focus indicator is reference-matched geometry, not a generic border.
Use exactly one layer of four thick, strongly rounded **stroked** corner paths
driven by `Theme.focus`; do not add a halo/duplicate under-stroke, rectangular
outline, or blocky filled L-shapes. The frame must hug the actual game shell and
scale with the selected card. Selection uses a restrained 1.10 scale-up while
preserving the fixed row slot, plus elevation and the focus brackets. See ADR
0015 (and ADR 0014 for the scale-up behavior).

## Responsive shell and clock invariant

The root `Window` must remain resizable: use preferred/minimum constraints, not
fixed root width/height. F11 is the presentation-level fullscreen toggle and
must be handled before forwarding keys into semantic application navigation. The 1280×720 metrics are the baseline for one uniform visual scale, but the
logical root viewport must expand to `window_size / ui_scale` so the native
window is completely filled on 16:10, 21:9, fullscreen, and other aspect ratios.
Do not reintroduce letterboxing or stretch individual game tiles/icons
non-uniformly; responsive shell regions should consume the expanded parent
width/height. See ADR 0015. All Slint text inherits LINE Seed JP
from `Typography.family`. The footer clock is live Rust presentation state and
reads GNOME's `org.gnome.desktop.interface/clock-format` through the XDG
Settings portal; do not query GSettings directly from the Flatpak or hard-code
12/24-hour mode in Slint. Profile and system-status edge placement derives from
`Metrics.top-edge-inset`. See ADR 0013.

## Phase 3.13 shell/footer/artwork invariant

The top chrome height is intentionally unchanged. The footer baseline height is
92 px and its controller, separators, clock, and action hint must all derive
from the footer's true vertical midpoint; do not reintroduce independent
vertical lifts. Game shells use the centralized thicker-frame metrics
(`game-tile-size`, `game-art-inset`, `game-shell-radius`). The focus frame must
remain a single layer whose elbow curvature matches the shell radius family.
Selected cards scale as one coherent visual unit so the shell, fallback artwork,
labels, and focus treatment animate together without independent text/layout
resizing. High-resolution bitmap artwork must follow `docs/ARTWORK.md`: prefer
>=1024×1024 sources and never reuse a low-resolution UI thumbnail for
fullscreen/HiDPI rendering. ADR 0020 supersedes ADR 0016's earlier native-size
selection-animation rule.

## Phase 3.14 focus/header invariant

The home focus treatment remains exactly one rounded stroked bracket layer with
no halo. It must sit just outside the game shell and hug it closely without a visually obvious gap; do not move the
focus centerline back onto or inside the shell. `Metrics.focus-offset` owns that
separation centrally. The top utility row intentionally has no dedicated Home
applet and no selected-home pill; do not reintroduce one unless the product
direction explicitly changes. Keep the remaining utility row centered as a
whole. See ADR 0017.



Phase 3.14.1 updates:
- Tightened focus offset so the focus frame hugs the shell more closely without a visible gap.
- Replaced the header utility placeholders with the supplied SVG placeholders for Friends, Album, Web, and Settings.
- Removed the previous placeholder utility icon set.

- Updated placeholder utility icon colors: Friends orange, Album blue, Web blue, Settings gray.

- Utility SVGs remain unmodified; their colors are applied at render time from semantic Theme tokens.


## Phase 3.14.4 utility icon invariant

The Friends, Album, Web, and Settings placeholder utility icons are rendered as
Slint `Path` geometry in `ui/components/utility-icons.slint`, derived from the
user-supplied symbolic SVGs. Keep the SVG source assets untouched. Do not render
these utilities through `Image.colorize`, bitmap exports, or hard-coded colors.
All icon colors come from semantic `Theme.nav-*` tokens. This ensures crisp
vector rendering at every responsive/fullscreen scale. See ADR 0019.


Phase 3.14.5 updates:
- Footer controller indicator now uses the supplied applications-games symbolic geometry, rendered as a Slint Path for crisp scaling.
- Added Activity placeholder using the supplied dictionary symbolic geometry; Activity is teal.
- Browser uses the semantic blue browser token; Album remains blue; Friends remains orange; Settings remains adaptive gray.
- Supplied SVG files are preserved unchanged in ui/assets; colors are applied from Theme at render time.


## Phase 3.14.6 placeholder-transition invariant

Fallback/demo artwork is part of the selected card's single visual scale. Do not
independently animate its font sizes, radii, or artwork detail scale during
selection; that causes visible breathing/re-layout. Browser utility color is
semantic `Theme.nav-browser` and is green. See ADR 0020.

## Third-party attribution invariant

Third-party provenance is part of correctness. Before adding any external asset or copied/derived code fragment, record its source, copyright/author information, SPDX license, and modification status in `THIRD_PARTY_NOTICES.md` and `docs/LICENSING.md`; include the required license text in `LICENSES/`. Never transcribe SVG/path geometry without preserving attribution to the source artwork. If licensing is uncertain, stop and treat it as a release blocker instead of guessing. Run `scripts/check-third-party.sh` as part of the normal quality gate.


## Phase 3.14.8 invariants
- A game-row boundary wrap is permitted only for a fresh directional input event. Repeated/held directional events clamp at the current boundary.
- Input adapters must preserve whether a semantic action is fresh or repeated; do not collapse this information before presentation navigation.
- Procedural/fallback game artwork must be rendered at native target geometry for the selected endpoint; do not reintroduce subtree transform-scaling that softens fallback typography at large UI scales.
- Utility icon colors remain semantic Theme tokens; current Browser green and Settings gray are intentionally lighter for dark-mode legibility.


### Phase 3.14.9 rendering invariants
- Procedural/fallback game artwork must remain crisp when Horizon is fullscreen or HiDPI. Horizon now relies on the Skia renderer for correctly scaled glyph rasterization; do not reintroduce the removed DemoCover render-scale/density supersampling workaround.
- Real cover art must use appropriately high-resolution source images; do not "fix" soft real covers by supersampling low-resolution files.
- The supplied Web and Settings SVG assets remain unchanged. Their smaller optical presentation size is intentional because their filled geometry is visually heavier than the outline utility icons.


Phase 3.14.10 rendering update:
- Slint renderer is now Winit + Skia instead of FemtoVG because FemtoVG scales cached glyph bitmaps and made fullscreen placeholder text visibly soft.
- Removed the high-density DemoCover workaround that caused breathing during selection.
- Web and Settings keep the supplied GNOME assets for provenance but render lighter outline derivatives to match the visual weight of the other utilities.


### Typography-specific rule
- LINE Seed JP is the default UI family for all text and is bundled with the Flatpak.
- Clock and selected-game title use centralized LINE Seed JP weight tokens.
- Keep LINE Seed JP pinned to the documented upstream revision and preserve its OFL-1.1 notice/license when bundled.
- Clock intent: Bold main digits / DemiBold suffix. Selected game title intent: DemiBold/Semibold.

### Selected-title connector invariant
- The shelf/backdrop must render below the connector and endpoint dot.
- The connector and dot use one continuous `Theme.divider` color; the shelf must never visually tint the lower segment.


## Phase 3.14.12 typography/icon invariants
- LINE Seed JP is the application-wide font family. Do not reintroduce UD Shin Go NT or Inter as the default without a later ADR explicitly superseding ADR 0025.
- The Flatpak must bundle LINE Seed JP under OFL-1.1 and make it available through fontconfig from `/app/share/fonts`.
- Web and Settings utility glyphs are presentation derivatives of the credited GNOME assets. Keep the original SVG files untouched in `ui/assets/` and keep provenance in `THIRD_PARTY_NOTICES.md`.
- Settings should read clearly as an eight-tooth gear at the same optical scale as neighboring utilities; Web should retain a clean globe outline with lighter internal lines.


Phase 3.14.13 home sizing:
- Removed the divider between the main view and footer.
- Kept top chrome and footer heights unchanged.
- Moved the title/carousel scene upward and enlarged the shelf/game cards to better match the reference composition.


## Bundled font invariant (ADR 0025)

- Horizon uses LINE Seed JP for all UI text.
- Flatpak builds must install the pinned upstream Thin, Regular, Bold, and ExtraBold faces into `/app/share/fonts/truetype/line-seed-jp`.
- General UI uses Regular 400; game titles/emphasis use Bold 700; the clock uses ExtraBold 800.
- Do not silently fall back to another family in packaged builds. A packaged build missing LINE Seed JP is a build/packaging defect.
- Preserve `© LY Corporation`, OFL-1.1, upstream provenance, and the exact pinned revision in third-party notices.

## LINE Seed JP development packaging

Do not reintroduce the upstream LINE Seed JP `pip install -r requirements.txt` / `make build`
step into the Freedesktop 26.08 Flatpak. Its legacy font-build dependencies currently fail
under Python 3.14. Development builds intentionally install the published prebuilt TTFs via
`scripts/install-line-seed-jp.py`. Release packaging must later replace this network fetch
with checksum-pinned immutable sources rather than restoring the incompatible source build.

## Phase 3.14.16 font/focus invariants
- LINE Seed JP development packaging downloads the four canonical static TTFs from Google Fonts commit `874ec71eac706dd23900d1305449abed6767b7df`; do not return to heuristic selection from a multi-subset archive.
- Validate downloaded font payloads as SFNT data before installing them. Release packaging must eventually promote the same immutable files to SHA-256-pinned flatpak-builder sources.
- The focus accent sweep is a brush change on the existing single bracket layer, not an extra glow/halo. Keep it calm and restrained, derive the highlight from the current accent, and disable it for reduced-motion and high-contrast modes. The Phase 3.14.17 baseline is a 4.2 s cycle with a modestly brighter/wider highlight; do not turn it into a rainbow, glow, or second layer.


## Phase 4.1 navigation invariants

- Top-level route history is Rust-owned in `src/navigation/`; Slint must never push/pop history.
- `AppRoute` currently contains Home, Library, Activity, and Settings only. Do not force Friends/Album/Web into top-level routes before their later utility-navigation design is decided.
- `InputManager`, keyboard mapping, and SDL mapping remain screen-agnostic and emit only semantic `UiActionEvent` values.
- `presentation::NavigationController` is the screen-level input boundary: global Back/Home are handled there and remaining actions are forwarded to the active page controller.
- Global Home clears the back stack and establishes Home as the root; Back after Home must not return to the abandoned page.
- Phase 4.1 publishes `AppWindow.current-route` but intentionally leaves Home as the only rendered page. Page switching belongs to Phase 4.2.
- Keep route-state tests independent of Slint rendering and controller hardware. See `docs/NAVIGATION.md` and ADR 0028.


## Focus sweep contrast

- Keep the selected-game focus indicator as one layer only; do not add a halo or duplicate under-stroke.
- The animated sweep must remain visibly distinct across saturated desktop accent colors, including red.
- Reduced Motion and High Contrast must use the static solid focus color.


## Phase 4.1.3 focus sweep baseline

- Keep the focus sweep visible but restrained: 38% white mix, narrow central highlight, 4.0 s cycle.
- Do not increase the highlight back to the Phase 4.1.1 62% white mix without explicit visual direction.
- Preserve the single-layer/no-halo invariant and static Reduced Motion / High Contrast behavior.
