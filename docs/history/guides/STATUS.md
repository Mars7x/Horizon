# Live top-bar system indicators

## Glyphs (current)

The battery and network glyphs are drawn natively in one style
(`ui/components/status-glyphs.slint`, ADR 0171): a 1.8px stroked outline with
round caps in the foreground ink, solid parts for what is filled, a faint ink
for what isn't, discrete steps only, on one 24x20 grid drawn 22px wide, so the network glyphs stand about as tall as the battery (16px against 14px) instead of towering over it.

| State | Glyph |
| --- | --- |
| Wi-Fi | A dot and three arcs; arcs light up at above 5, 30 and 55 percent strength (NetworkManager's bar thresholds), the rest stay faint |
| Wi-Fi, limited (no internet or a captive portal) | Faint arcs with a `!` beside the dot |
| Ethernet | A network port with three pins |
| Ethernet, limited | The port with a `!` in place of its pins |
| Another kind of connection | A globe (with `!` when limited) |
| Offline | The faint Wi-Fi glyph, struck through |
| Battery | A grey casing (never coloured) with a charge in five steps: red at 20% and below (charging or not), green while charging, the foreground ink otherwise. While charging, a bolt in the foreground ink (white in dark) sits centred, cut out of the charge by a thin clearance |

The earlier GNOME status icons are retired; the sections below record their
history.

## Power updates (current)

Host and controller battery follow UPower's and BlueZ's change signals: a
signal wakes the power worker, which waits 100 ms for the burst to settle and
re-reads once. A 60-second recheck covers missed signals and bus restarts
(5 s if no signal stream is available at all). Reads fetch only the properties
they need (no property cache or change subscription per proxy), and only a
changed reading reaches the UI. SDL3 controller power is still read every 2 s
on the input path while a controller is connected.

Phase 9.5.44.67 replaces the static Wi-Fi and battery drawings.

## Network

The small right-hand network icon shows one active physical network connection
as reported by `org.freedesktop.NetworkManager` on the *system* D-Bus. A primary
Ethernet or Wi-Fi connection wins; if a VPN is primary, the preferred underlying
physical connection is used instead. When several physical connections are active
and none is primary, Ethernet takes precedence. Wi-Fi draws three signal arcs
based on the associated access point's `Strength`: above 5, 30 and 55 percent, NetworkManager's own bar thresholds.
The small dot is always present for an associated Wi-Fi network. When
NetworkManager confirms no active physical link, a crossed-out Wi-Fi
symbol appears. A one-off D-Bus failure or transitional connection snapshot
retains the last verified link instead of briefly showing disconnected. Limited or
captive portal connectivity is marked by `!` where NetworkManager reports it.

Status refreshes on a five-second background-worker interval. Two consecutive
confirmed disconnected readings remove an established link; three consecutive
failed readings expire a previously cached link, preventing indefinite stale
status. A network connectivity assessment of "none" while a physical link is
still active keeps the Ethernet/Wi-Fi glyph with a limited-connectivity mark.
The controller and network update paths share the same latest status cache, so
controller hotplug or power updates cannot reset the link to offline. The
observer does not test Internet reachability with extra network requests, read
`/proc`, or poll on the render thread. NetworkManager's connectivity check may be unavailable; in that
case the icon reflects connection state, not verified Internet connectivity.

## Battery

Priority is intentionally strict:

1. UPower's composite host display battery, if present and typed as a system
   battery (laptop/handheld), always wins even if a controller is connected.
2. Otherwise SDL3's power report from an already-open connected gamepad is used.
   Power reports without a known percentage, or devices reporting no battery,
   are not represented as full or empty. Controllers are refreshed every two
   seconds and hotplug is reflected immediately.
3. If neither is available, the icon and its spacing disappear completely.

The five-step fill indicates the battery percentage and a lightning bolt
appears while charging. Fully charged does not imply still charging.
UPower's device battery estimate and SDL3 hardware reports may be inaccurate.

## Flatpak and failure behaviour

Horizon grants only `--system-talk-name=org.freedesktop.NetworkManager` and
`--system-talk-name=org.freedesktop.UPower` in addition to existing permissions.
All blocking system D-Bus calls are confined to a dedicated worker thread; Slint
receives immutable status snapshots. Sustained D-Bus absence or denial degrades
to offline after the brief grace period and leaves the UI usable. The existing
SDL3 controller subsystem continues to work without status services.

Host laptop display battery may not be available on systems without UPower or
when system-bus access is denied. Some controllers or wireless receivers do not
report accurate battery data to SDL3, so the icon can legitimately be absent.


### Controller battery fallback (Phase 9.5.44.71)

SDL3 `Gamepad::power_info` is the preferred controller battery source. A
valid percentage is now accepted even when SDL labels the power state
`Unknown`; no battery is shown if the percentage is missing (`-1`), the
controller reports no battery, or an error occurs.

If SDL does not report a percentage, Horizon queries UPower `Gaming Input`
(12) devices and connected BlueZ gamepads with `org.bluez.Battery1`. The
fallback requires exactly one SDL-connected gamepad; ambiguous multiple peripheral
readings are not arbitrarily assigned to a controller. BlueZ does not supply
charging state, so Horizon does not infer charging from a Bluetooth percentage.

Priority is host battery, SDL controller battery, then reported UPower/BlueZ
controller battery. A desktop with no reported controller charge hides the
battery indicator. No percentage is inferred for a 2.4 GHz dongle if SDL/
Linux does not expose one.

For the 8BitDo Ultimate 2 Wireless on a compatible receiver, SDL's HIDAPI
DInput mode can expose battery charge; Xbox/XInput emulation may not. This
may depend on 8BitDo firmware and HID access in the Flatpak. Horizon does
not change controller firmware, write udev rules, or broaden device grants.

### GNOME symbolic network icons (Phase 9.5.44.72, retired by ADR 0171)

The status cluster renders eight original SVG symbols supplied from GNOME's
Symbolic/Adwaita icon sets. Slint tints the SVG alpha mask with the current
foreground colour; real-time NetworkManager/UPower/SDL readings are unchanged.

- **Wi-Fi:** `radiowaves-1` for >=75%, `-2` for 50–74%, `-3` for
  25–49%, and `-4` for <25% access-point strength. These correspond to
  GNOME's excellent/OK/weak/very-weak designs rather than raw bars drawn by
  Horizon. Limited or portal connectivity uses `radiowaves-question`.
- **Ethernet:** `network-wired` for an active wired link and
  `network-wired-no-route` when NetworkManager reports limited connectivity.
- **Offline:** `radiowaves-x`; unknown/other falls back to
  `radiowaves-question`. The absent wired interface is *not* guessed to be
  a disconnected cable.

`radiowaves-dots`, `network-wired-acquiring`, and
`network-wired-disconnected` are intentionally not bundled because the status
model does not currently distinguish those states. Network policy and
five-second sampling/filtering remain unchanged. The network glyph remains
independent of the conditional host/controller battery display.

### Responsive status and animated appearance (Phase 9.5.44.73)

The network and power observers now run independently on background system-D-Bus
connections. NetworkManager retains its five-second cadence and connection
stability filter. UPower and BlueZ host/controller battery checks begin immediately
and repeat about every two seconds, without waiting for network queries
(superseded: signal-driven, see "Power updates" above). SDL3
controller power is also refreshed every two seconds, not on the 8 ms input path.
The main-thread timer merges partial updates so that a network-only observation
cannot overwrite a fresh battery or vice versa. Only changes are published.

A battery indicator and footer controller icon remain instantiated so that both
appearance **and disappearance** can animate. The battery slides and fades in
from the right and retracts on loss, while the controller SVG enters from the
left with a subtle 7px slide. Their separators follow the visibility state.
Animation durations are 220 ms in / 155 ms out; the shell retains its correct
battery precedence and hides unknown percentages. Reduced Motion disables
travel and duration. These animations never change focus or input ownership.

**8BitDo Ultimate 2 2.4GHz:** An actual percentage depends on the controller,
receiver and SDL HIDAPI driver's reporting mode. SDL DInput mode can supply
it on compatible firmware; a receiver exposing only Xbox/XInput emulation may
not. No replacement driver, custom HID commands, udev changes, or broader
Flatpak device permissions are introduced. If SDL, BlueZ and UPower all lack a
battery report, Horizon continues hiding the icon instead of inventing one.

## Phase 9.5.44.74 — Immediate network changes and crisp GNOME symbols

The network observer subscribes to system-bus signals sent by NetworkManager
(connection, device, and access-point property changes). Signals wake an
independent status worker to re-query the current active network immediately.
A five-second periodic query remains only for missed notifications and bus
reconnections; it is **not** the normal UI update cadence. Confirmed link-loss
is rechecked after 150 ms before displaying offline to avoid handoff flicker.
Each update wakes the UI thread through a `services::worker` channel, which
drains already-computed updates (no polling timer) and never performs
blocking system-bus operations. Actual system D-Bus/renderer
latency can still vary. Power polling stays independent at two seconds.

GNOME's eight original 16×16 SVG assets are retained unmodified under
`ui/assets/status/` for provenance. The visible versions are rendered with
Slint `Path` elements transcribed directly from their original path commands,
including the original opacity details. This prevents SVG image-cache
upscaling blur on 4K/ultrawide fullscreen UI and preserves theme colourization.
See ADR 0129 and THIRD_PARTY_NOTICES.md.

### Charging and low-battery display (Phase 9.5.44.75)

The battery frame, fill and charging bolt are red when the currently displayed
battery is **20% or below** (including exactly 20%). Dark mode uses a brighter
red than Light mode so it remains visible on both backgrounds. At 21% and
above, the icon returns to the theme foreground. The battery icon retains its
existing animations and host-over-controller priority. No charge estimate is
invented for devices which report no percentage.

The charging bolt is now filled with the indicator colour and separated from
the battery fill by a narrow background-coloured outline for legibility. Host
charging comes from UPower; known SDL `Charging` states remain authoritative.
For controller fallbacks, UPower Gaming Input is preferred to BlueZ because it
exposes charging state whereas BlueZ `Battery1` does not. If SDL reports a
percentage with **Unknown** power state, an unambiguous UPower charging report
may supplement it only when the two reported percentages differ by at most 10
points. SDL remains the displayed percentage source. A known SDL discharging
state is never replaced by a conflicting fallback report. Unknown charging
state is not proof that a device is charging; where neither SDL nor UPower
exposes charging state, no charging icon is displayed.


## Phase 9.5.44.76 — Scale-aware symbolic SVG rendering

The GNOME network glyphs now use the **same adaptive SVG sampling policy as
the utility icons**, instead of the fixed native-Path transcription added in
Phase 9.5.44.74. When Horizon's global UI scale is 1.0 or lower, each SVG is
rasterized at the visible logical icon size without unnecessary oversized
intermediates. When the scene enlarges, the raster target tracks `ui-scale`
(up to 4×), then is transformed back to its original layout size.
The GNOME SVG source files and icon state mappings remain unchanged, and
source alpha is retained through theme colorization. The bottom-left
controller indicator uses the same adaptive strategy with its original GNOME
SVG (since ADR 0186, Kenney's controller drawings instead). Battery geometry, status update timing and connection detection do not
change. This is a rendering-quality optimization, not an additional status
source or a guarantee of identical antialiasing on all compositors/scales.

## Phase 9.5.44.77 — Ethernet optical-size adjustment

The GNOME `network-wired` and `network-wired-no-route` glyphs use a
22 × 22 logical-pixel viewport, rather than the shared 26 × 26 Wi-Fi
viewport. The wired shapes fill more of their original 16-unit SVG
viewboxes, so this subtle reduction makes the header symbols more balanced.
Both wired glyphs stay centred within the unchanged 30 × 28 status slot.
Wi-Fi, offline/unknown, system scale-aware sampling, tinting, network
status updates, battery and utility icons remain unchanged.


## Phase 9.5.44.78 — Pixel-aligned wired glyph and charging-state audit

The normal Ethernet glyph preserves the GNOME network-wired topology but draws
its solid bar, stems and rounded nodes on whole logical-pixel coordinates within
the Phase 77 22 × 22px slot. It is an attributed, pixel-snapped Slint adaptation
made to avoid the multiple resampling steps of a 16px rasterized SVG at 22px;
the unmodified source SVG remains bundled. Wired limited/no-route continues to
use the original detailed SVG, as do all Wi-Fi states. Other icon dimensions,
network events, SVG scale policy, batteries and focus are unchanged. The
appearance at fractional compositor/desktop scales still requires visual review.

Charging-state audit: the Rust→Slint presentation binding and conditional
lightning bolt were already connected correctly. SDL `Charging` state and UPower
`State=1` mean actively charging. Previously the UPower parser also counted
`State=5` (**PendingCharge**) as active charging, which is incorrect: charging
may be paused at a configured threshold. State 5 is now displayed without the
bolt; 0/2/3/4/6 likewise do not assert charging. BlueZ Battery1 publishes no
charging state, and the 2.4GHz receiver may withhold it; no bolt is invented.
Host-battery priority and the red <=20% warning remain unchanged.

## Footer controller icon (ADR 0186)

The bottom-left icon draws the connected controller's model: Xbox 360, Xbox
One/Series, PS3, PS4, PS5, Switch Pro, a left or right Joy-Con, a Joy-Con
pair, the Steam Deck, the AYN Odin 3, or a generic gamepad for a pad SDL can't
name. Art is Kenney's Input Prompts (CC0), unmodified, in
`ui/assets/controllers/`; the Odin 3 (`horizon_odin3.svg`) is Horizon's own in
the same style, as Kenney has none (ADR 0187).

With several controllers it shows the one used last (else the first
connected). `SdlState` keeps each pad's `ControllerShape` from SDL's gamepad
type when it connects (`sdl::controller_shape`) and reports it in
`ControllerStatus.shape`; a press on a different controller republishes the
status. `StatusController` sets `controller-art`, and keeps the last drawing
while the icon fades out after a disconnect.

