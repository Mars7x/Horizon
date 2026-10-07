# Appearance architecture

Horizon follows host appearance preferences through the standardized XDG Settings portal.

## Supported host preferences

Horizon reads and listens for changes under `org.freedesktop.appearance`:

- `color-scheme`
- `accent-color`
- `contrast`
- `reduced-motion`

The portal is optional. If a setting or the entire portal is unavailable, Horizon uses deterministic fallback values instead of reaching into desktop-specific configuration databases.

## Default behavior

- Theme preference: **System**
- Accent preference: **System**
- No system color-scheme preference: light fallback
- No system accent preference: `#3584E4` fallback
- No contrast preference: normal contrast
- No reduced-motion preference: normal motion

Manual Light/Dark and custom-accent overrides are already represented by the Rust domain model. Persistence and the Settings UI arrive in later phases.

## Dependency direction

```text
XDG Settings portal
        |
        v
appearance::portal
        |
        v
SystemAppearance
        |
        v
AppearanceState + preferences
        |
        v
ResolvedAppearance
        |
        v
presentation::appearance
        |
        v
Slint Theme / Motion globals
```

Portal/DBus types never enter Slint. Slint only consumes resolved semantic properties.

## Semantic colors

Generated UI chrome must use `Theme` properties rather than literal colors. Authored artwork with intrinsic colors is exempt when its asset contract explicitly requires direct, unmodified rendering. The current semantic palette contains:

- `background`
- `surface`
- `surface-raised`
- `foreground`
- `secondary-foreground`
- `divider`
- `accent`
- `accent-hover`
- `accent-pressed`
- `accent-subtle`
- `accent-foreground`
- `focus`

The six top-utility SVGs are authored artwork, not theme glyph masks. Slint renders those source files directly with no `colorize` property and no semantic tint token; their orange/blue/teal/gray/red colors and white outline/shadow are part of the assets themselves. The shared authored-icon renderer is scale-aware: at UI scales at or below 1× it renders directly at the logical 40×40 size, while enlarged layouts supersample by the actual app UI scale (capped at 4×). This avoids the old 4×→40px→downscaled-window resampling path at small sizes while preserving crisp fullscreen rendering. The policy never changes SVG bytes or the visible 40×40 layout size.

The Rust resolver derives hover, pressed, subtle, and readable accent-foreground values from the effective accent.

## Motion

Components use durations from the `Motion` global. When the desktop requests reduced motion, those shared durations resolve to zero. Individual components must not bypass the motion tokens with hard-coded animation durations.

Phase 4.6 applies this contract to route changes. `PageTransitionLayer` animates opacity and a small vertical settle using `Motion.page-duration`; when Reduced Motion is active, the duration is `0ms` and the same route bindings resolve immediately. Persistent shell chrome and the global shell Menu are intentionally not included in page motion. Phase 4.7 bounds rapid transition overlap to the active page plus the immediate outgoing page; this hardening does not introduce another timing path.

## Typography

`Typography.family` is the single app-wide font-family token and currently
resolves to LINE Seed JP. ADR 0025 supersedes the older Inter packaging described below; the Flatpak now builds and bundles the pinned LINE Seed JP family instead of the Inter variable
font into `/app/share/fonts`, while native builds use the system fontconfig
installation. `AppWindow.default-font-family` applies it to all Slint text;
components must not introduce local font-family overrides without a documented
design requirement. Clock-format integration is intentionally separate from
appearance and is documented in `docs/CLOCK.md`.

## Phase 9.5.24 utility SVG scaling and launch-token restoration

Utility SVG supersampling now follows the actual `AppWindow` UI scale instead of
always rasterizing at 4×. Window scales at or below 1× render the SVG directly
at its logical size; larger scales supersample only enough to match the outer
scene enlargement, capped at 4×. This removes unnecessary intermediate
resampling at reduced window sizes while retaining the fullscreen sharpness
workaround.

The Shop utility theme edit also inadvertently dropped the existing
`Metrics.game-launch-pressed-scale` and `Motion.launch-press-duration` tokens.
Phase 9.5.24 restores their Phase 7.0.5 values (`1.055` and `90ms`, with Reduced
Motion resolving the latter to `0ms`) so `GameTile` launch feedback compiles and
retains its established behavior.
