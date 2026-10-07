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

Components must use `Theme` properties rather than literal colors. The current semantic palette contains:

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

The Rust resolver derives hover, pressed, subtle, and readable accent-foreground values from the effective accent.

## Motion

Components use durations from the `Motion` global. When the desktop requests reduced motion, those shared durations resolve to zero. Individual components must not bypass the motion tokens with hard-coded animation durations.

Phase 4.6 applies this contract to route changes. `PageTransitionLayer` animates opacity and a small vertical settle using `Motion.page-duration`; when Reduced Motion is active, the duration is `0ms` and the same route bindings resolve immediately. Persistent shell chrome and the global shell Menu are intentionally not included in page motion.

## Typography

`Typography.family` is the single app-wide font-family token and currently
resolves to LINE Seed JP. ADR 0025 supersedes the older Inter packaging described below; the Flatpak now builds and bundles the pinned LINE Seed JP family instead of the Inter variable
font into `/app/share/fonts`, while native builds use the system fontconfig
installation. `AppWindow.default-font-family` applies it to all Slint text;
components must not introduce local font-family overrides without a documented
design requirement. Clock-format integration is intentionally separate from
appearance and is documented in `docs/CLOCK.md`.
