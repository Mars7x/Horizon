# AGENTS.md — Horizon

This file defines the working rules for coding agents contributing to Horizon.

## Project intent

Horizon is a controller-first, console-style Linux game library. It imports games from existing launchers/sources, presents them in a unified library, launches them through their owning application, and records activity/playtime where that can be measured reliably.

Horizon is **not** an emulator manager. Do not add emulator configuration, firmware/key management, graphics settings, save management, or other source-owned functionality unless the project direction is explicitly changed.

## Primary stack

- Rust for application/backend logic
- Slint for presentation only
- SDL3 for controller input beginning in Phase 3
- SQLite + `rusqlite` for persistence
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
- `docs/SOURCES.md`
- `docs/CLOCK.md`
- `docs/adr/`

Use an Architecture Decision Record when a change introduces or reverses a significant architectural choice. An ADR should explain the decision, why it was made, and its consequences.

## Current phase boundary

The repository currently contains work through **Phase 6.0**.

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
- pure Rust `AppRoute`/`Navigator` with Home, Library, and first-class `UtilityPage` submenu routes
- explicit back-stack and global Home reset semantics
- `NavigationController` between semantic input and page controllers
- active route published to Slint as presentation state
- app-level top/footer chrome on Home and Library, suppressed by full-shell utility destinations
- retained route layers with central Home/Library bounds and full-shell Friends/Album/Activity/Web/Settings bounds
- presentation-only placeholder pages for Library and all five utility submenus
- route-local shell-focus restoration for Home, Library, and all five utility submenu routes
- explicit global Back/Menu/Home policy with modal shell Menu state
- tested keyboard/controller mappings for global shell actions
- restrained routed-page crossfade/settle transitions driven by design-system motion tokens
- Reduced Motion disables route animation through `Motion.page-duration` without changing navigation semantics
- controller topology changes clear held analog/D-pad navigation state to prevent phantom repeat after disconnect/reconnect
- rapid route retargeting permits only the active route and one immediate outgoing transition layer
- route focus memory enforces route-valid focus snapshots at its own boundary
- responsive shell math guards transient zero-sized surfaces while preserving windowed/fullscreen/ultrawide fill behavior
- deep route/back-stack, focus-normalization, spatial-boundary, and input-reset stress tests
- pure domain library identity types for games and source-owned identities
- source-neutral `LibraryRepository` and `LibraryService` boundaries
- SQLite persistence through `rusqlite` with numbered schema migrations
- XDG application-data database path resolution without broad Flatpak filesystem access
- schema v1 `games` + `game_sources` tables with stable `(source_id, external_id)` rediscovery identity
- startup database initialization/migration and pure/in-memory persistence tests
- generic `GameSource` adapter boundary and deterministic `SourceRegistry`
- explicit source descriptors/capabilities without provider-name branching
- normalized `SourceGame`/`SourceSnapshot` discovery contract with duplicate external-ID rejection
- unavailable-vs-failed source discovery outcomes
- `SourceImportService` with per-source failure isolation and source-owned identity attachment
- atomic per-source snapshot persistence through repository batch writes

Not yet implemented:

- concrete provider adapters (Steam, Heroic, Lutris, Bottles)
- launching
- playtime/session tracking
- utility submenu production pages

Do not prematurely implement later-phase behavior as a shortcut while working on the current phase.

## Phase 4 target

Phase 4 establishes the application navigation shell without adding persistence or real game sources. **Phase 4.7 completes the shell with hardening for controller topology changes, responsive-window edge cases, rapid route retargeting, focus validity, and stress-tested Back/Home semantics:**

1. Define Rust-owned navigation state for Home, Library, and the five routed utility submenus.
2. Define focus regions so Up/Down/Left/Right have deterministic screen-level behavior.
3. Route existing `UiAction` values into that navigation state; do not change SDL/keyboard adapters to understand screens.
4. Add page transitions that use the design-system motion tokens and respect reduced motion.
5. Make Back/Home/Menu behavior explicit and testable.
6. Keep page components presentation-only; do not put navigation policy in Slint.
7. Add tests for navigation transitions without requiring Slint rendering or controller hardware.
8. Preserve the current home carousel and pointer behavior through the same Rust controllers.

If implementing Phase 4 requires SQLite, source importing, or game launching, stop: those belong to later phases.


## Phase 5 target

Phase 5 establishes the durable library/domain foundation while deliberately keeping source adapters and production library UI out of scope:

1. Keep game/source identity pure in `src/domain/`; no SQLite, Slint, SDL, portal, or provider-specific types may leak in.
2. Future source adapters feed normalized `DiscoveredGame` values through the service boundary; they never issue SQL.
3. `src/persistence/` is the only layer allowed to use `rusqlite` or SQL.
4. Every schema change is a new numbered migration. Released migrations are immutable.
5. Rediscovery identity is `(SourceId, ExternalGameId)`. Never merge games merely because titles match.
6. Platform-specific XDG path resolution stays in `src/platform/`; persistence receives a resolved path.
7. Corrupt/invalid persisted values produce typed errors rather than fabricated domain objects.
8. Keep source-reported lifetime playtime separate from future Horizon-observed sessions; Phase 5 does not add either yet.
9. The existing demo Home cards remain presentation-only until the Phase 7 source-backed vertical slice.

If Phase 5 work requires Steam parsing, launch commands, activity/session rows, or production Library/Home replacement, stop: those belong to later phases. See `docs/DOMAIN.md`, `docs/DATABASE.md`, and ADR 0037.


## Phase 6 target

Phase 6 establishes the generic source/import framework while keeping concrete provider parsing for the vertical slices:

1. Provider-specific paths, formats, and quirks terminate behind `GameSource` implementations in `src/sources/`.
2. Every adapter exposes one stable `SourceDescriptor`; duplicate `SourceId` registration is rejected.
3. Provider capabilities are explicit metadata. Never branch on source-name strings outside the owning adapter.
4. Discovery distinguishes normal unavailability from genuine failure; one failing optional source must not block unrelated sources.
5. Adapters emit normalized `SourceGame` values without choosing their own registered `SourceId`; the import service attaches registry identity.
6. A source snapshot must not contain duplicate `ExternalGameId` values.
7. Successful snapshots flow through `LibraryService`/`LibraryRepository`; source adapters never issue SQL.
8. Persist one successful source snapshot atomically. A persistence failure aborts the import pass instead of pretending the durable state is healthy.
9. Keep launch commands, source playtime, artwork retrieval, and production Home/Library wiring out of the framework until a real provider exercises those boundaries.
10. Phase 7 is the first concrete adapter: Steam.

If Phase 6 work requires provider-name conditionals in generic services, direct SQL from an adapter, or fake launch/playtime abstractions with no concrete consumer, stop and fix the boundary instead. See `docs/SOURCES.md` and ADR 0038.

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
platform-dependent Slint API. The selected stack is Winit + Skia. Do not fix
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
- The focus accent sweep is a brush change on the existing single bracket layer, not an extra glow/halo. Keep it calm and restrained, derive the highlight from the current accent, and disable it for reduced-motion and high-contrast modes. The current Phase 4.1.5 baseline is a 4.0 s cycle with a 34% white mix and narrow highlight plateau; do not turn it into a rainbow, glow, or second layer.


## Phase 4.2 navigation/shell invariants

- Route history is Rust-owned in `src/navigation/`; Slint must never push/pop history.
- `AppRoute` is structured as Home, Library, or `Utility(UtilityPage)`. `UtilityPage` contains Friends, Album, Activity, Web, and Settings. This is the canonical route model; do not reintroduce a separate utility-overlay navigation path. See ADR 0035.
- `InputManager`, keyboard mapping, and SDL mapping remain screen-agnostic and emit only semantic `UiActionEvent` values.
- `presentation::NavigationController` is the screen-level input boundary: global Back/Home are handled there and remaining actions are forwarded to the active page controller.
- Global Home clears the back stack and establishes Home as the root; Back after Home must not return to the abandoned page.
- `AppWindow.current-route` selects the active retained route layer; route history/policy still belongs to Rust. Home/Library use central shell bounds while every utility submenu uses the full logical surface.
- `TopNavigation` and `Footer` are app-shell chrome and must not be duplicated back into route pages. They are visible on Home/Library and intentionally hidden for every utility submenu route.
- Home subtracts `Metrics.top-chrome-height` from its internal scene origin so moving it into the central host does not change established screen-space geometry.
- Library and utility submenu content are presentation-only placeholders during Phase 4; do not add persistence/source behavior merely to make them look complete.
- Keep route-state tests independent of Slint rendering and controller hardware. See `docs/NAVIGATION.md`, ADR 0028, and ADR 0035.


## Focus sweep contrast

- Keep the selected-game focus indicator as one layer only; do not add a halo or duplicate under-stroke.
- The animated sweep must remain visibly distinct across saturated desktop accent colors, including red.
- Reduced Motion and High Contrast must use the static solid focus color.


## Phase 4.1.5 focus sweep baseline

- Keep the focus sweep visible but restrained: 34% white mix, narrow central highlight, 4.0 s cycle.
- Do not increase the highlight back to the Phase 4.1.1 62% white mix without explicit visual direction.
- Preserve the single-layer/no-halo invariant and static Reduced Motion / High Contrast behavior.

## Phase 4.3 top-utility invariants

- Shell focus is Rust-owned. Slint may render the current focus region and selected utility, but it must not decide Up/Down/Left/Right navigation policy.
- When top utilities own focus on Home, the selected game remains logical state only: its focus brackets, title/connector, scale-up, raised z-order, and selected shadow must all be hidden. Returning focus to content restores the selection visuals.
- Fresh Home↔utility transfers choose the shortest rendered center-to-center path. The resulting game↔utility pair is reciprocal: reversing vertical direction without horizontal movement returns to the exact source element. Horizontal movement in the destination region invalidates that pair and the next transfer uses nearest-center geometry. Other pages may preserve the last utility.
- Left/Right within the top utility row clamp at Friends/Settings; they do not wrap. Held directional repeat still comes only from the input adapters.
- Every utility icon activates a first-class `AppRoute::Utility(UtilityPage)` submenu route. Friends, Album, Activity, Web, and Settings must share this route/history abstraction rather than splitting into route and modal-overlay special cases.
- Every utility submenu owns the full visual shell while active, hides top/footer chrome, and restores as content focus because the utility row is not visible there.
- Pointer activation of a utility must enter the same Rust `NavigationController` path as Accept; do not duplicate route policy in Slint.
- The utility focus treatment is a compact rounded surface around the existing icon. Do not alter the credited utility SVG source assets or their semantic colors to indicate focus.
- Phase 4 utility submenus are presentation placeholders only; do not add social, screenshot, browser, persistence, or launching behavior yet. See ADR 0035.

- Exactly one focus region should present selection chrome at a time. When top utilities own focus, Home game focus brackets, selected-game title pill, connector line, and connector dot must all be hidden. Slint may report rendered geometry, but Rust owns the shortest-distance/return-anchor focus decision. See ADR 0031.

- Utility-row focus uses a compact fixed-size accent orb/ring treatment with a restrained 2px icon lift. Utility icons never scale on focus. Do not add utility text labels. The 40x40 utility layout cells stay fixed so spatial center calculations remain stable; only the visual treatment may extend outside them.

## Phase 4.4 focus restoration invariants

- Every route remembers a durable focus snapshot. Home/Library may restore content vs top utilities; every full-shell `Utility(...)` route is always restored as content focus while active.
- Route changes must save the source route's shell focus before switching and restore the destination route's saved focus afterward.
- First visits default to content focus. Each utility route retains the identity of the utility that owns it, but no utility route may focus the hidden top row while active.
- Temporary reciprocal Home↔utility anchors are never persisted across route changes.
- Back restores the destination route's remembered focus. Global Home always lands on Home content focus and the first Home game while preserving Home's remembered utility identity for future use.

## Phase 4.5 global action invariants

- `Back`, `Menu`, and `Home` are global shell actions. Input adapters only map hardware/keys to these semantic actions; they never implement navigation policy.
- Global actions are fresh-press only. Repeated global actions must be rejected by adapters and defensively ignored by `NavigationController`.
- Back priority is: shell Menu → route history → no-op at the root. Utility submenus are routes, not a second modal stack.
- Menu toggles the global shell Menu from any route. Opening/closing it must not mutate route history or route-local focus memory.
- Home closes the shell Menu, clears route history, returns Home to content focus, and selects the first Home game. Global Home never restores the previously selected Home game.
- Controller mapping remains South=Accept, East=Back, Start=Menu, Guide=Home. Keyboard mapping remains Return/Space=Accept, Escape/Back=Back, Menu=Menu, Home=Home.


## Phase 4.6 page-transition invariants

- Route history and route selection remain Rust-owned. Slint may animate the already-published `AppRouteView`, but it must not infer or mutate navigation history.
- Route pages use one reusable `PageTransitionLayer` per retained route. Home/Library keep the central shell content bounds; Friends/Album/Activity/Web/Settings use the full logical surface. The active page crossfades to full opacity and settles upward by the centralized `Metrics.page-transition-offset`; inactive pages perform the inverse.
- All five utility routes share the reusable presentation-only `UtilitySubmenuPage` component during Phase 4; do not fork five slightly different placeholder shells.
- Page transition timing must use `Motion.page-duration`. Do not add route-specific hard-coded durations or offsets.
- Reduced Motion makes `Motion.page-duration` zero, so route changes become immediate while preserving the exact same route/focus/history semantics.
- Top navigation, footer chrome, and the global shell Menu do not participate in route motion. Route content alone transitions. Every utility submenu suppresses top/footer chrome while active; chrome visibility changes immediately and is not part of the route animation.
- The active route layer owns pointer input during overlap. A fading inactive page must never receive a click.
- Retained inactive pages must not render active-route selection chrome. In particular, outgoing Home must hide its game focus brackets/title treatment as soon as `current-route` leaves Home, even while the page is still fading.
- Route components stay instantiated across navigation so crossfades can overlap and route-local presentation state can survive navigation. Do not move durable application state out of Rust into those retained Slint components.
- Keep the transition restrained: no zoom, blur, bounce, parallax, or long travel. See ADR 0034 and ADR 0035.


## Phase 4.7 shell-hardening invariants

- SDL gamepad add/remove is a topology boundary. Reset analog-stick and D-pad held/repeat state on topology changes so disconnect/reconnect cannot carry phantom input into the new device set. Do not move device lifecycle handling above `src/input/sdl.rs`.
- `RouteFocusMemory` is responsible for route-valid snapshots. Full-shell `Utility(...)` routes must normalize to content focus inside the navigation abstraction; callers must not reintroduce per-route focus fixups.
- Rust publishes both the active route and exactly one immediate transition source route. `PageTransitionLayer` may keep only that source visible while fading. Rapid navigation must immediately drop any older outgoing retained route so partially faded pages cannot accumulate.
- The back stack remains independent of animation state. Rapid navigation and Back must be deterministic even when visual transitions are still in flight. Global Home remains a hard reset of history, Home content focus, and first-title selection.
- Responsive sizing keeps one uniform visual scale and an expanded logical viewport. Guard transient zero-sized compositor surfaces and non-negative content bounds; do not solve resize/fullscreen issues with fixed root dimensions, letterboxing, or non-uniform tile scaling.
- Phase 4.7 stress behavior is documented in `docs/SHELL_HARDENING.md` and ADR 0036.


## Phase 5 domain/database invariants

- SQL terminates in `src/persistence/`; UI, presentation, services, domain, and future source adapters must not issue SQL directly.
- The source-owned identity key is `(SourceId, ExternalGameId)`. Equal titles never imply equal games.
- Schema files under `src/persistence/migrations/` are append-only after release. Add a new numbered migration rather than rewriting a shipped migration.
- `game_sources` is intentionally relational so a logical game can support multiple source references later without source-name branching in the domain.
- XDG data path resolution belongs to `src/platform/data_paths.rs`; no source or presentation code should construct host paths.
- Phase 5 startup may initialize/migrate the database, but the demo Home model remains until the source-backed vertical slice. Do not read SQLite directly into Slint.
- Source-reported lifetime playtime and future Horizon-observed sessions must remain separate persistence concepts.
- `rusqlite` uses the `bundled` feature in the current development baseline; preserve the dependency provenance in `THIRD_PARTY_NOTICES.md` and include it in the final lockfile-derived license report. See ADR 0037.


## Phase 6 source-framework invariants

- `src/sources/` is the only layer allowed to understand provider-specific discovery formats and quirks.
- `SourceRegistry` owns adapter uniqueness. Generic services must consume `GameSource`/descriptor/capability abstractions rather than matching `SourceId` strings.
- Adapter discovery output is source-neutral: `SourceGame` carries external identity/title; `SourceImportService` attaches the registry-owned `SourceId`.
- `Unavailable` is expected optional-source state; `Failed` is a real discovery error. A failed source must not suppress discovery of later registered sources.
- One successful `SourceSnapshot` has unique `ExternalGameId` values and is persisted as one atomic repository batch.
- Persistence failure aborts the import pass and reports the source whose snapshot was being committed. Do not swallow it into a per-source discovery report.
- Phase 6 adds no concrete provider adapter and no launch/playtime/UI shortcut. Steam consumes this framework in Phase 7. See ADR 0038.
