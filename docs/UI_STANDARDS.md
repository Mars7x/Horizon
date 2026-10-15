# UI standards

Home keeps its own shell. The approved [first-run Welcome](ONBOARDING.md) is a
minimal introduction without a header, hint bar or preference controls. Every other full-screen page uses the shared frame;
feature work must not replace Home's focus geometry, selected scale or marquee.
See [Home](HOME_UI.md) and [navigation](NAVIGATION.md).

## Shared components

| Element | Contract | Implementation |
| --- | --- | --- |
| Header | Stable section title, shared title baseline and margins | [PageHeader](../ui/components/page-header.slint) |
| Header values | Right-aligned read-outs/pickers with shared value transitions | [HeaderValue](../ui/components/page-header.slint) |
| Sub-page | Breadcrumb in the content area; click means Back | [Breadcrumb](../ui/components/page-header.slint) |
| Focusable rows | One exclusive animated outline; hover is not selection | [ListRow](../ui/components/list-row.slint), [SettingsRow](../ui/components/settings-row.slint) |
| Read-only panels/history | Never focusable; scrolling is independent of selection | [Surface](../ui/components/list-row.slint) |
| Covers/media | Home corner brackets follow animated selected geometry | [FocusFrame](../ui/components/focus-frame.slint) |
| Hints | Bottom-bar baseline shared with Home; show working actions only | [HintBar](../ui/components/hint-bar.slint) |
| Long text | Selected text may marquee; other text clips with soft edges | [MarqueeText](../ui/components/marquee-text.slint) |

[Theme, Metrics, Motion and Typography](../ui/theme/theme.slint) own colors,
spacing, sizes and animation timing. No ad-hoc layout constants or local font
families. Authored SVG geometry/colors stay intact; generated chrome uses
semantic tokens. Do not set hand cursors; hover color communicates clickability.

## Motion and scrolling

Use shared TransitionDriver/TransitionLayer for forward/back zoom and crossfade.
Section headers stay fixed while content transitions. Incoming pages prewarm
before timing starts. Only the active and immediate outgoing layers are needed;
rapid navigation must not expose stale focus. Item-to-item Album viewing slides
rather than changing the page transition contract.

Reduced Motion makes shared transitions immediate and stops marquee/pulsing.
High Contrast suppresses idle focus pulses. Cover focus must follow the animated
selected cover rather than its fixed grid slot. Do not revive segmented text
masks or runtime gradient-percent coercions; validate against the installed
Slint syntax and real rendering.

Scroll-edge shadows follow the animated camera: no top shadow at offset zero,
no bottom shadow once nothing remains below. Wheel/touchpad panning does not
change selected items. Read-only history scrolls without manufacturing row focus.
Inactive/outgoing pages must not accept pointer actions or retain bottom-bar
hints. Clicks play the same navigation/OK/Back cue as equivalent input actions.

## Appearance, typography and sound

Theme choices are System, Light, Dark and Black; System follows light/dark only.
Accent choice is independent: System plus nine presets, including White. Portal
preferences are color-scheme, accent-color, contrast and reduced-motion. Missing
preferences use deterministic fallbacks; UI colors are resolved in Rust.
Focus/accent contrast is at least 4.5:1 on relevant surfaces, 7:1 in High Contrast.
The stored choice and current focus have distinct visual states.

LINE Seed JP is the shared family: Regular 400 for interface text, Bold 700 for
emphasis/game titles, ExtraBold 800 for clock digits. Flatpak bundles four
unmodified static faces including Thin 100. Font packaging/provenance is in
[Licensing](LICENSING.md) and the manifest. UI sounds are existing WAV assets
played through PipeWire and controlled by the appearance preference.

## Text and dialogs

Use short, plain labels without trailing full stops or visible ellipses. Do
not expose implementation details, credentials, URLs containing secrets or
unavailable data as zero. Missing achievement artwork uses a neutral fallback,
never an invented badge. Match marquee edge color to the actual background.

Secondary Remove/Disconnect actions live beside the item, with X as the
controller equivalent. Confirm destructive actions with Cancel initially focused.
Do not add a success message for a change already visible; show failures where
they help recovery. Hints are ordered X, B, A, with A rightmost. Dialogs and the
on-screen keyboard own input before the page beneath them.
