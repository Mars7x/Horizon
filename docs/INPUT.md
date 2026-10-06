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
          global Back/Home          └────► active page controller
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

## Presentation behavior after Phase 4.1

The Home game carousel remains the only fully rendered page target, but Phase 4.1 now inserts the Rust `NavigationController` above page controllers:

- Left / Right forwarded to `HomeController` move the selected game
- pointer clicks still select through the same `HomeController`
- Accept is recognized but intentionally does not launch anything yet
- Back is interpreted at the navigation layer and restores route history when available
- Home resets top-level history to the Home root
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
