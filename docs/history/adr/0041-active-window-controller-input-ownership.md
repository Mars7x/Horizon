# ADR 0041: Active-window controller input ownership

## Status

Accepted for Phase 7.0.4.

## Context

Horizon polls SDL3 gamepad events independently of Slint/Winit keyboard focus. After launching a game through a source-neutral URI, the game can take foreground focus while Horizon remains running in the background. SDL continues reporting the same controller, so gameplay input can unintentionally navigate Horizon at the same time.

The launch executor cannot reliably own session lifetime: XDG OpenURI hands the request to the desktop/source launcher and returns before the launched game exits. Adding Steam-specific process checks to presentation or input would also violate Horizon's source abstraction.

## Decision

Controller UI ownership follows Horizon's OS window activation state, not launcher/process state.

The root Slint `FocusScope` reports focus gain/loss only when the `FocusReason` is `window-activation`. `InputManager` forwards this active/inactive state to the SDL adapter. While inactive, SDL continues pumping events and maintaining gamepad topology, but axis/button events do not emit semantic `UiActionEvent` values. Switching ownership resets analog and D-pad held/repeat state.

Keyboard mapping uses the same manager gate defensively, although normal keyboard events are already delivered only to the focused window.

## Consequences

- A foreground game can use the controller without simultaneously moving Horizon's UI.
- Returning to Horizon immediately restores controller navigation even if the game is still running.
- Controller connect/disconnect status remains accurate while Horizon is in the background.
- No source-specific process monitoring or launcher special case is required.
- Future activity/session tracking may observe game lifetime separately; it must not replace active-window input ownership.
