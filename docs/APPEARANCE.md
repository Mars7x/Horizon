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

Phase 9.5.44.55 implements **Settings → Appearance** as the first Settings category. Theme is **System, Light, Dark**. **System** follows only the desktop light/dark preference; it is not a third visual theme. Accent colour independently offers **System, Red, Orange, Yellow, Green, Teal, Blue, Purple, Pink, White**. The nine accessible preset choices run from warm red through cool blue/purple to pink, with neutral white last. Appearance choices take effect immediately, can be changed with mouse or controller, and persist in `$XDG_CONFIG_HOME/io.github.Mars7x.Horizon/appearance.json` (0600 under private 0700 config dir). Saving is atomic and a failed write leaves the previous selection active.

Phase 9.5.44.57 simplifies the settings UI: independent category rows cannot
physically overlap, Theme choices show the selected value with accent text (no
checkmark/selected border), and accent presets appear as unlabeled circular
swatches in spectrum order, retaining accessible names for screen readers.
Keyboard and controller navigation still use a single focus indication distinct
from the stored selection. The swatches remain the original hues except **White**:
the White swatch displays the exact effective white/grey focus colour derived
from the normal Rust palette resolver for the current Light/Dark/High Contrast
mode, even if another accent is selected. Its background animates over 260 ms on
system or manual theme changes; Reduced Motion makes this instantaneous.

The preset colours are shown in the picker. The UI focus/accent token is adjusted toward dark in Light mode and toward light in Dark mode until it maintains at least 4.5:1 contrast against the app background and elevated surfaces (7:1 in High Contrast). This means Yellow and White can appear darker as UI outlines on a light surface and lighter on a dark surface. Readable foreground is independently chosen for tinted buttons. System accent falls back to `#3584E4` if the portal does not supply a colour.

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

## UI navigation sound (Phase 9.5.44.56)

**Settings → Appearance → UI Sounds** is an independent, default-on toggle. It is
written atomically to the same private `appearance.json` as the theme and accent.
Older preference files without this field default to **On**. Off clears queued
audio immediately; failure to initialize a playback device is non-fatal.

For this initial sound phase, Horizon plays the user-provided
`ui/assets/horizon-navigation.wav` **only when keyboard/gamepad directional
navigation changes the selected item or region**. There is no sound on ignored
keys, pointer clicks, initial page load, launching games or opening dialogs.
Rapid movement replaces pending samples instead of layering sounds. Playback
uses the already-linked SDL3 audio API, 48 kHz mono PCM16, with a lazily opened
audio device. The Flatpak adds `--socket=pulseaudio` for the ordinary host audio
server (including PipeWire's PulseAudio compatibility).

Phase 9.5.44.58 replaces the initial bundled navigation cue with the owner-supplied
`horizon-navigation.wav` without re-encoding or gain changes. The retired
`horizon-navigation-soft.wav` is removed from the package. This is an asset-only
change to playback: semantic focus gating, queue clearing, enable/disable
preference, accessibility, and Flatpak audio permissions remain unchanged.


## Appearance directional focus (Phase 9.5.44.59)

The keyboard/controller focus graph matches the rendered layout rather than
artificially separating System accent from the adjacent swatches. Theme
(System, Light, Dark) is one horizontal row; accent (System, Red, Orange,
Yellow, Green, Teal, Blue, Purple, Pink, White) is another. Left/Right travel
through every option in each row, with wraparound at the ends. Confirm activates
only the focused option; moving focus alone never changes a saved preference.

Up from an accent moves to the nearest Theme column (System/Red/Orange → Theme
System; Yellow/Green/Teal/Blue → Theme Light; Purple/Pink/White → Theme Dark).
Down from that Theme column restores the most recently focused accent when it
belongs to that column, rather than unexpectedly jumping to a different swatch.
The defaults for columns with no focus history are accent System, Teal, and
White respectively. Down from an accent enters UI Sounds; Up restores the last
focused accent. On UI Sounds, Left sets Off and Right sets On without moving
focus. The Third-Party layout and editor grid remain unchanged. Semantic focus
changes continue to trigger exactly one navigation sound when enabled.

## Universal focus and edge behavior (Phase 9.5.44.60)

Keyboard/controller focus is now signalled by the same single-layer animated
accent stroke used by Settings. A selected theme/accent remains a persisted
choice and never adds a competing focus ring; all menu focus follows the
140 ms shared animation token (instant with Reduced Motion).

**Holding Left/Right across Theme or the accent swatches stops at the final
control**, even if repeated input reaches it during the hold. Release, then
press again to wrap. The same applies in reverse. For vertical moves, only a
fresh Up from the top Theme row wraps to UI Sounds, and only a fresh Down from
UI Sounds wraps to Theme. The visual column mapping and swatch memory remain
unchanged; see ADR 0117.


### Phase 9.5.44.62 — focus consistency

All navigable Appearance choices use the Settings-style single-ring focus
with the same gentle full-perimeter accent brightness pulse used by utilities
and Home game brackets. Focus must not be confused with a saved choice;
selected values continue to use their existing nonfocus styling.
Reduced Motion and High Contrast disable the idle pulse.

## Phase 9.5.44.63 — focus and audio timing

The original Home focus geometry and idle animation are preserved. Every
non-Home menu focus outline breathes through the shared accent-derived
focus/highlight colours on the same 3200 ms cycle. Focus reveal remains 140 ms;
Reduced Motion and High Contrast use a stable full-strength outline.

The existing navigation, OK and Back WAVs are not altered in gain, length,
format, or bytes. When UI Sounds is enabled, SDL playback is prepared once
shortly after the interface starts rather than on first interaction; a brief
silent device primer avoids sending an audible attack as the backend's first
packet. Short cues are explicitly flushed after enqueueing. Successful
OK/Back callbacks are dispatched before route repaint when their success is
known, but invalid or ignored actions still stay silent. Runtime audio
latency remains dependent on the system's PipeWire/PulseAudio configuration.

### Settings focus correction (Phase 9.5.44.65)

The appearance picker retains the selected value independently of navigation
focus. Only the currently focused option receives the breathing accent border;
other controls show their normal divider/selected swatch treatment. Outgoing
Appearance controls cannot receive clicks after navigating to a different
Settings page, even while the page-exit fade runs.

### Phase 9.5.44.66 — gapped swatch focus and accurate UI Sounds description

Accent discs are 36 px when selected or focused and 31 px otherwise, within
a 48 px concentric focus/selection ring. The 2 px stroke leaves a 4 px
transparent radial gap at full size (3 px with High Contrast's 3 px stroke).
Focus continues breathing with the shared accent cycle; the selected but
unfocused accent keeps its neutral ring. The White preview still
animates to the palette-resolved grey in Light mode.

UI Sounds now displays “Play sounds when navigating, confirming, and going
back”, matching the existing Navigation/OK/Back cues and shared toggle.
