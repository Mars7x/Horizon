# UI standards

Home keeps its own shell. The approved [first-run flow](ONBOARDING.md) omits the
app header; Welcome also omits the hint bar and preference controls. Other full-screen pages use the shared frame;
feature work must not replace Home's focus geometry, selected scale or marquee.
See [Home](HOME_UI.md) and [navigation](NAVIGATION.md).

The embedded game surface is content: aspect-preserving software frames on a
black background, without shell chrome. Loading/errors use shared ModalDialog and
ActionButton components. Pause uses a full-height side panel over a dimmed frame,
flush with the left edge and square at the outer corners. It slides fully from
and back behind that edge using the shared dialog duration, with no panel fade
or floating margins; Reduced Motion makes this immediate. It uses the shared
compact ListRow variant, button glyphs and action buttons.
The right edge uses the shared header-bar shadow turned sideways, with strength
following the slide; the panel has no edge outline.
The panel uses Theme.background (true black in Black), shared typography/marquee,
spacing and focus animation; hints share Home's footer baseline. Pointer and
keyboard/controller actions use the same UI sound boundary. Pending actions preserve row styling, focus and footer hints. Speed changes update
only the pause-panel value with the shared value-swap duration and a directional
slide/fade, with no gameplay badge or progress/success text; only failures need feedback. Save/load state browsers use shared ListRows with native-pixel previews and
timestamps. Internal quick-menu views use shared TransitionDriver/TransitionLayer
forward/back zoom/crossfade, clipped inside the stationary drawer; header, shadow
and footer stay fixed. Reduced Motion makes view/value changes immediate. Empty/unavailable load slots never take focus. Save replacement and
restart confirmations default to Cancel. Screenshot success uses a small native-pixel
thumbnail in its action row: shared 180 ms fade/90%→100% scale, one-second hold,
then fade out. The label, focus, menu and paused game stay still; no flash or bottom
success message. Reduced Motion shows a static thumbnail for the same hold. A quiet
shutter follows UI Sounds and plays only after the file is published. Rust owns player mode, handoff
phases and focus. Launch/return use card-origin zooms with shared Motion tokens;
keep the card press/rebound, preserve video aspect ratio, and hold the last frame
until saving succeeds. The returning game picture crossfades into the existing
card during its shrink; never replace a fully opaque frame at the endpoint.
Reduced Motion bypasses these handoffs. Background freezing is independent of
the quick menu: deactivation must not open it or change manual menu selection.

Controller button mapping uses the shared Settings header, breadcrumb,
forward/back transition and compact ListRows. Game controls are named on the left;
assigned controller inputs appear in keycaps beneath their own column label. Each pair is one focus/click target. Restore defaults is separated
from the mapping rows by a section gap. Capture names the game control and uses ModalDialog with
a single Cancel action and suppresses page hints/navigation underneath it;
Restore defaults uses ConfirmDialog with Cancel initially focused. Pending
writes retain the existing row instances, focus and previous values until saved.

## Shared components

| Element | Contract | Implementation |
| --- | --- | --- |
| Header | Stable section title, shared title baseline and margins | [PageHeader](../ui/components/page-header.slint) |
| Changing values | Shared directional slide/fade; stationary row and focus | [ValueText](../ui/components/value-text.slint) |
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

Changing text values use shared ValueText: a 180 ms horizontal slide/fade,
matching quick-menu speed, header pickers, core settings and controller bindings.
Outgoing text fades before incoming text appears, avoiding doubled long labels.
The row, label, focus and click target remain still. Core settings and quick-menu speed pass
the actual change direction from Rust. Initial/empty values appear immediately;
rapid changes replace the current transition without queuing stale values.
Reduced Motion presents the new value immediately. Toggles retain their animated
track/thumb; row focus keeps its shared 140 ms fade.


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
[Licensing](LICENSING.md) and the manifest. UI sounds play through PipeWire and follow the appearance preference.
Quick-menu opening and closing use distinct short latch cues: two tiny soft
clicks, brighter when opening and duller when closing, level with the
navigation/OK/Back cues. The panel slides in over 120 ms (ease-out-quart) and out
over the shared 180 ms dialog duration. Continue/Back, successful restart and successful Return to Horizon share
the closing cue; nested confirmation cancellation keeps the normal Back cue.
Repeat/ignored input and background freezing stay silent. The UI sound stream
stays warm during embedded gameplay in the active Horizon window, allowing the
close sound to finish instead of replaying when the panel next opens. Focus
movement plays a warm, soft-edged snap (its ring kept below the 3-5 kHz band
ears are most sensitive to); a game starting plays a crisp onset over a low
swelling tone as its card zooms in (not for a failed load). Every cue plays at
one standard loudness, applied when it loads: its loudest 30 ms at about
-26 dBFS, its peak at most -10 dBFS. Navigation, heard on every focus move,
then sits about 7 dB below the rest. The owner-supplied OK/Back WAVs stay
byte-for-byte unmodified; the other cues are original and reproducible with
[the generator](../scripts/generate-quick-menu-sounds.py).

## Text and dialogs

Use short, plain labels without trailing full stops or visible ellipses.
Use plain action verbs (Open, Edit, Choose), without decorative arrow suffixes
or chevrons on editable rows. Keep arrows for actual directional navigation,
such as breadcrumb Back and Album previous/next. Keyboard Enter remains a
button glyph in the hint bar, where it explains the active action. Do
not expose implementation details, credentials, URLs containing secrets or
unavailable data as zero. Missing achievement artwork uses a neutral fallback,
never an invented badge. Match marquee edge color to the actual background.

Secondary Remove/Disconnect actions live beside the item, with X as the
controller equivalent. Confirm destructive actions with Cancel initially focused.
Do not add a success message for a change already visible; show failures where
they help recovery. Hints are ordered X, B, A, with A rightmost. Dialogs and the
on-screen keyboard own input before the page beneath them.
