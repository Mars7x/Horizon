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
LeftBumper
RightBumper
```

Directional keyboard actions may auto-repeat. Activation/navigation actions such as Accept and Back require a fresh press.

## SDL3 integration

SDL3 is initialized on the same thread that runs Slint. A lightweight Slint `Timer` polls SDL's event pump every 8 ms. This is deliberate: SDL's event pump is main-thread-bound, so moving it to a background thread would violate SDL's threading contract.

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

## Presentation behavior after Phase 4.2

The Rust `NavigationController` remains above page controllers while `AppWindow` now renders the published active route inside a persistent shell:

- Left / Right forwarded to `HomeController` move the selected game when Home is active
- pointer clicks still select through the same `HomeController`
- Accept is recognized but intentionally does not launch anything yet
- Back is interpreted at the navigation layer and restores route history when available
- Home resets route history to the Home root
- Library, Activity, and Settings currently render presentation-only placeholders and intentionally ignore page-local actions
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

The shell now treats `Back`, `Menu`, and `Home` as an explicit global-action set. They are handled before focus-region or page-local input and never auto-repeat.

Canonical mappings:

| Semantic action | Keyboard | SDL3 gamepad |
| --- | --- | --- |
| Accept | Return / Space | South face button |
| Back | Escape / Back | East face button |
| Menu | Menu key | Start |
| Home | Home key | Guide |

The adapters only produce `UiAction`; `NavigationController` owns meaning. Back closes the global shell Menu when open, then falls through to normal route history. All five top utilities are real routed submenu destinations, so there is no separate utility-overlay modal priority. Menu toggles Horizon's shell Menu without mutating route history. Home closes the shell Menu and resets navigation to Home content focus with the first Home game selected. Unit tests cover both keyboard and SDL mappings so the two input paths cannot silently drift.



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
