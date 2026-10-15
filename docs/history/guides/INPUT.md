# Phase 3 input architecture

Phase 3 introduces Horizon's controller-first input boundary.

## Goals

- Normalize controller and keyboard input into the same semantic `UiAction` type.
- Keep device-specific button/key knowledge out of Slint presentation components.
- Keep home-screen selection state owned by Rust.
- Support controller connection and removal without restarting the application.
- Preserve Flatpak sandboxing with the narrow `input` device permission rather than `--device=all`.
- Respect SDL's main-thread event-pump requirement instead of moving SDL to a worker thread as a workaround.

## Data flow

```text
Keyboard event from Slint/Winit        SDL3 gamepad event
            │                                  │
            ▼                                  ▼
     keyboard adapter                    SDL adapter
            │                                  │
            └──────────► UiAction ◄────────────┘
                            │
                            ▼
                     InputManager
                            │
                            ▼
               NavigationController
                    │               │
          global Back/Menu/Home          └────► active page controller
                    │                            │
                    ▼                            ▼
                 Navigator                   HomeController
                    │                            │
                    └──────────────► Slint state ◄┘
```

Slint forwards logical keyboard text and repeat state but does not compare keys or decide their meaning. All key mapping occurs under `src/input/`.

## Semantic actions

Phase 3 defines:

```text
Up
Down
Left
Right
Accept
Back
Menu
Home
LeftBumper  (gamepad LB, keyboard Q)
RightBumper (gamepad RB, keyboard E)
Secondary   (gamepad X: west, or north on Nintendo pads; keyboard Delete; Album: delete)
StickPress  (gamepad L3 or R3; the on-screen keyboard's Shift)
```

Directional keyboard actions may auto-repeat. Activation/navigation actions such as Accept and Back require a fresh press.

## SDL3 integration

SDL3 is initialized on the same thread that runs Slint. A lightweight Slint `Timer` polls SDL's event pump: every 8 ms while a controller can navigate Horizon, every 100 ms while none is connected (only hotplug matters), and every 250 ms while another app (a game) has the foreground, so Horizon stays out of the game's way. This is deliberate: SDL's event pump is main-thread-bound, so moving it to a background thread would violate SDL's threading contract.

The SDL adapter:

- enumerates already-connected gamepads at startup
- opens gamepads so SDL continues to deliver mapped gamepad events
- handles `GamepadAdded`
- handles `GamepadRemoved`
- converts mapped buttons into `UiAction`
- publishes the connected-controller count

Phase 3 maps the physical south face button to Accept and east to Back. Controller-layout preferences/remapping are a later settings concern; source/presentation code must not special-case controller brands.

## Keyboard adapter

The Slint `FocusScope` forwards a logical key string and repeat flag to Rust. Rust maps:

- arrows → directional navigation
- Return / Space → Accept
- Escape / Back → Back
- Menu → Menu
- Home → Home

Unrecognized keys are rejected so they are not silently claimed by Horizon.

## Mouse side buttons

`src/input/mouse.rs` maps a mouse's back button to Back and its forward button to Forward (see NAVIGATION.md), wherever the pointer is. `app.rs` reads them from winit (`on_winit_window_event`), claims only those two buttons, and dispatches the action on the next event-loop turn, through the same action sink as the keyboard, controller and hint clicks.

## Presentation behavior after Phase 4.2

The Rust `NavigationController` remains above page controllers while `AppWindow` now renders the published active route inside a persistent shell:

- Left / Right forwarded to `HomeController` move the selected game when Home is active
- pointer clicks still select through the same `HomeController`
- Accept is recognized but intentionally does not launch anything yet
- Back is interpreted at the navigation layer and restores route history when available
- Home resets route history to the Home root
- Library, Activity, Achievements, Album and Settings have page-local controllers (see their feature guides); Profile's is in [PROFILE.md](PROFILE.md); the Shop is gone
- Up / Down / Menu / bumpers remain semantic and gain additional screen-level behavior in later Phase 4 passes

The input adapters themselves are unchanged: they never inspect or choose application routes. See `NAVIGATION.md`.

## Flatpak

The manifest adds:

```text
--device=input
```

This exposes `/dev/input`, including game controllers, on Flatpak versions that support the input-device permission. Horizon does not request raw USB or `--device=all` for basic navigation.

SDL 3.4.18 is built as its own Flatpak module and the Rust `sdl3` crate links through `pkg-config`. Native development therefore requires an SDL3 development package available to `pkg-config`.

## Left analog stick navigation

The left analog stick maps into the same directional `UiAction` values as the
D-pad and keyboard. SDL axis values never leave `src/input/sdl.rs`.

Navigation uses hysteresis rather than a single threshold:

- enter a direction at a deliberate stick deflection;
- release it at a lower threshold to prevent drift/chatter;
- choose the dominant axis for diagonal input;
- emit one action immediately on entry;
- after a short initial delay, repeat at a controlled interval while held.

This behavior is implemented by `AnalogNavigation` and has hardware-independent
unit tests. Screen/page code must not inspect analog values directly.


## D-pad hold repeat

SDL does not need to synthesize keyboard-like repeat events for Horizon. The
SDL adapter records directional button-down/button-up state and emits semantic
`UiAction` repeats after the same kind of initial delay used for the analog
stick. Non-directional buttons remain single-fire.


## Fresh vs repeated navigation
Input adapters preserve a `repeated` bit alongside each semantic action. A fresh Left/Right event at the first or last game may wrap to the opposite end. Auto-repeat from a held keyboard key, D-pad, or analog stick never wraps; it stops at the boundary until the user releases and makes a new directional input.

## Phase 4.3 shell focus routing

Directional input remains semantic before it reaches screen logic. On Home/Library, `NavigationController` interprets Up/Down when crossing between page content and the persistent top utility row, while Left/Right move the selected utility whenever that row owns focus. Every routed utility submenu is full-shell with no visible utility row, so Up remains page-local there and Rust normalizes those routes to content focus. On Home, fresh vertical transfers use the shortest rendered center-to-center horizontal distance. The resulting utility↔game pair is sticky in both directions: immediately reversing direction returns to the exact element the user came from, while horizontal movement in the destination region invalidates that pair and restores nearest-center behavior. Slint reports geometry only; Rust owns the focus decision. SDL3 and keyboard adapters remain unaware of focus regions, routes, utility names, or geometry math. Held Left/Right repeats naturally through the existing semantic repeat pipeline and clamps at the ends of the utility strip.

## Phase 4.4 route-local focus restoration

Focus ownership is remembered per route above the input-adapter layer. SDL3 and keyboard still emit only `UiActionEvent`. When navigation changes routes, Rust saves the source route's shell focus and restores the destination route's saved focus. Home/Library may restore top-utility focus; every `Utility(...)` route is normalized to content focus because its shell chrome is hidden. Back therefore restores a focus region that is valid for the destination page, while global Home always returns Home to content focus and the first Home game. Temporary spatial pairing anchors are route-local and are discarded during route restoration.

## Phase 4.5 canonical global actions

The shell now treats `Back`, `Forward`, `Menu`, and `Home` as an explicit global-action set. They are handled before focus-region or page-local input and never auto-repeat.

Canonical mappings:

| Semantic action | Keyboard | SDL3 gamepad |
| --- | --- | --- |
| Accept | Return / Space | South face button (East on Nintendo pads) |
| Back | Escape / Back | East face button (South on Nintendo pads) |
| Menu | Menu key | Start |
| Home | Home key | Guide |
| Forward | (mouse forward button) | — |

The adapters only produce `UiAction`; `NavigationController` owns meaning. Back closes the global shell Menu when open, then falls through to normal route history. All four top utilities are real routed submenu destinations, so there is no separate utility-overlay modal priority. Menu toggles Horizon's shell Menu without mutating route history. Home closes the shell Menu and resets navigation to Home content focus with the first Home game selected. Unit tests cover both keyboard and SDL mappings so the two input paths cannot silently drift.



## Phase 4.7 controller-topology hardening

SDL device topology changes invalidate held navigation state. On any successful gamepad addition or any gamepad removal event, the SDL adapter resets both analog and D-pad direction/repeat latches before subsequent input is processed. This prevents a disconnected controller from leaving a phantom held direction when another controller remains connected or when the device reconnects without delivering the matching release/center event.

The connected-controller count changes only when Horizon actually opens or removes a gamepad handle. Screen and presentation code remain unaware of SDL device IDs. Hardware-independent tests verify that topology reset returns both repeat engines to neutral state.

## Phase 7 Home activation and launch

`UiAction::Accept` on Home now has its first production action. A fresh Accept on the selected source-backed game asks `GameLaunchService` to choose a registered source reference that advertises `SourceCapability::Launch`. Repeated Accept events are ignored to avoid duplicate launch requests.

The input adapters remain completely source-agnostic. They do not know Steam app IDs or URI schemes. The Steam adapter prepares a generic `SourceLaunchTarget::Uri`, while `PortalLaunchExecutor` dispatches the URI through XDG OpenURI. Pointer selection still only changes selection; launching remains the semantic Accept action path.


## Phase 7.0.4 active-window input ownership

SDL gamepad events are not scoped to the foreground Slint window. Horizon therefore treats OS window activation as an explicit input-ownership boundary. The root `FocusScope` reports only `FocusReason.window-activation` gain/loss events to Rust; ordinary pointer/programmatic focus changes inside Horizon do not toggle controller ownership.

`InputManager` enables semantic UI input only while Horizon owns the active window. When Horizon deactivates (for example because a launched game takes focus), `SdlGamepadInput` continues polling SDL and processing controller add/remove events so device status stays correct, but it emits no `UiActionEvent`. The analog and D-pad held/repeat latches are reset on every enable/disable transition so a button or stick held while playing cannot produce a stale navigation action when Horizon becomes active again.

This rule is source-neutral and game-neutral. Launch services do not guess process lifetime, and Steam-specific code does not control the input layer. If the user intentionally returns focus to Horizon while a game remains running, Horizon owns input again because it is the active application.

## Phase 7.0.5 launch-pending input policy

The input layer does not learn about launch state. It continues to emit semantic actions while Horizon owns the active window. `HomeController` alone suppresses duplicate Accept and Left/Right carousel actions while a source-neutral launch handoff is pending. This keeps device normalization independent from launch/services and avoids a Steam-specific input mode. See ADR 0043.


## Phase 8 activation sharing

The root window-activation signal now serves two independent application concerns: controller input ownership and approximate launch-session observation. `InputManager` still owns only controller enable/disable. `ActivityService` separately consumes the activation transition after a source-neutral launch dispatch to start/end a `ForegroundHandoff` session.

Do not merge these into one input/session state machine. Activity tracking must not teach SDL about games, and controller ownership must not depend on SQLite success. See ADR 0044.

## Phase 9.5 managed-session input ownership

> **Retired (2026-10-12):** the managed-session helper was removed (ADR 0046 superseded).

Managed Gamescope sessions do not introduce a second controller-routing path. Horizon still consumes SDL navigation only while its own window is active. When Gamescope/the game owns foreground activation, Horizon suppresses semantic controller actions exactly as it does for an external launch. Bringing Horizon intentionally back to the foreground restores Horizon UI input even if the managed session is still running.

Session lifecycle and input ownership remain independent: the host helper reports managed-session state to the launch/activity services, while `InputManager` continues to depend only on Horizon window activation.

## Phase 10.4.3.1 — Keyboard/controller repeat consistency

The Slint input scope forwards logical directional presses **and releases** to `InputManager`. The manager caps OS auto-repeat events to the same timing constants used by SDL's D-pad: an immediate first step, no repeat until **300ms**, then at most one additional step every **115ms**. This avoids the desktop environment's much faster keyboard repeat accelerating Horizon's menus. Keyboard input remains device-independent `UiAction` by the time it reaches controllers. A release or loss of window activation clears the held-direction state, and orphan repeats are ignored. `Accept`, `Back`, `Menu` and `Home` remain fresh-press only. A slower OS repeat setting may still produce a slower rate; the cap does not synthesize repeated events.

## Phase 10.4.3.3 — Mouse wheel is camera input, not navigation

On scrollable lists, pointer-wheel deltas pan the camera without emitting controller/keyboard Up or Down events or changing selection. The Achievements game catalog and Library grid now explicitly decouple viewport position from focus. Read-only Achievements and Activity session details wheel-scroll their existing history windows without focusing any rows. Directional keys and gamepad inputs continue using the established navigation actions and repeat caps. See [ADR 0162](../adr/0162-achievement-input-and-lazy-badge-parity.md).

Wheel direction follows the desktop's scrolling setting (natural/reverse or traditional). The compositor applies that setting before Horizon sees the event, so Horizon must map Slint's sign faithfully: a positive `delta-y` moves the camera **up**. Every wheel handler uses the shared `Wheel.rows()` helper in `ui/components/wheel.slint`; never hand-write the mapping.

## Last input device

`input::InputDevice` records what the user last used: `InputManager` sets Gamepad for SDL actions and Keyboard for keyboard actions, and the winit window hook in `app.rs` sets Touch for touch events and Pointer for mouse buttons. The on-screen keyboard reads it to appear for a controller or a touch screen (see [KEYBOARD.md](KEYBOARD.md)). Under gamescope a touch screen may arrive as Pointer, so nothing may depend on telling the two apart.

## Controller families and glyphs

Each opened SDL gamepad gets a `ControllerFamily` (`sdl::controller_family`):
PlayStation for SDL's PS3/PS4/PS5 types, Nintendo for the Switch Pro and
Joy-Cons, Steam Deck for Valve's Deck controller (vendor `0x28de`, product
`0x1205`), Odin for the AYN Odin 3's own controls, and Xbox for everything
else.

SDL may report a virtual controller's underlying IDs/name/type when that
information is available from its own input backend. Horizon no longer searches
Steam roots or sets `SteamVirtualGamepadInfo`; all local Steam filesystem grants
are removed (ADR 0191). An unresolved virtual pad (`28de:11ff`) keeps the existing
handheld-aware fallback: Deck on DMI `Jupiter`/`Galileo`, or the Odin 3's own
controls when that handheld is detected. Controller navigation and SDL hardware
recognition remain independent of Steam account services.

The Odin 3 is recognised by its device tree (model "AYN Odin 3", which sysfs
shows in `/sys/devices/soc0/machine` inside Flatpak; compatible `ayn,odin3`
outside it). On an Odin 3, its controls are the pad its kernel driver makes
(`2020:3001`, "AYN Odin3 Gamepad"), the Xbox Elite 2 pad InputPlumber turns
them into (`045e:0b00`, ArmadaOS's default), and Steam's virtual pad for
either.
InputPlumber's Elite 2 is told from a real one by being virtual: SDL's
device path is an evdev node whose sysfs device is under
`/sys/devices/virtual/input` (uinput), where a USB pad is on its bus.

Roles are decided for all open pads together (`sdl::pad_roles`), again on
every connect and disconnect:

- Behind Steam Input both Elite 2s are virtual pads with Elite 2 IDs. Sysfs,
  which Steam doesn't hide, says which Elite 2s exist: with no real one it's
  the Odin's; with no InputPlumber pad it's the real one; with both, the
  Odin's is the one in Steam's first slot (its controls exist from boot).
  Steam's pads keep `28de:11ff` in sysfs and are numbered by name ("… pad
  N"), as SDL reads them.
- When the raw pad and InputPlumber's (or Steam's) are both open, as
  ArmadaOS's raw-input sharing can allow, the raw one is ignored so a press
  counts once; it's left out of the controller count too.

The "gamepad connected" and "gamepad role" log lines record each pad's IDs,
path, whether it's virtual, its Steam slot and what Horizon made of it.
Untested on the device (ADR 0187).

Nintendo pads and the Odin 3 (whose buttons wear Nintendo's labels) follow
Nintendo's convention: the right button (A) confirms,
the bottom (B) goes back and the top (X) is the secondary action. SDL's
buttons are positional, so `action_for_button` maps them per family.

`input::GlyphStyle` is the keyboard or the family used last. `InputManager`
sets it on every keyboard or controller action and publishes it to the UI only
when it changes; a mouse or touch screen leaves it as it was. At startup it is
the first connected controller's family, else the keyboard. `app.rs` writes
it to the Slint global `InputGlyphs.family`, and `ButtonGlyph`
(`ui/components/hint-bar.slint`) draws the logical button for that device:

| Logical | Keyboard | Xbox | PlayStation | Nintendo | Steam Deck | Odin 3 |
| --- | --- | --- | --- | --- | --- | --- |
| A (Accept) | Enter | A | ✕ | A | A | A (right) |
| B (Back) | Esc | B | ○ | B | B | B (bottom) |
| X (Secondary) | Del | X | □ | X | X | X (top) |
| LB / RB | Q / E | LB / RB | L1 / R1 | L / R | L1 / R1 | L1 / R1 |

Hints and header values keep naming logical buttons ("A", "LB"), so a click
still sends the same action whatever is drawn. See ADR 0185.

## Swapping select and back

**Settings → Controller → Swap Select and Back** makes every controller
select with the button that normally goes back and go back with the one that
normally selects (Xbox: B selects; Nintendo and the Odin 3: B, the bottom,
selects; PlayStation: ○ selects). It is saved in `controller.json`
(`services::controller_settings`). `InputManager` exchanges Accept and Back on
controller actions only (`UiAction::with_confirm_swapped`); the keyboard
keeps Enter and Esc, and clicked hints send their logical action as before.
With **Swap Hints** on (the default), `app.rs` also sets
`InputGlyphs.swap-confirm`, and `ButtonGlyph` then draws a controller's
logical A as its B and B as its A, so the hints name the button that does it.
With it off, hints keep the controller's usual buttons. See ADR 0188.

