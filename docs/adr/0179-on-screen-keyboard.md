# ADR 0179: On-screen keyboard

**Status:** Implemented. The build, the gate and new tests pass (row widths, nearest-key Up/Down, the edge rule, layer switches). Rendered offscreen over the Profile name editor, light and dark. Not yet used in the running app, with a controller, a touch screen or gamescope.

## Context

Text fields (API keys, the Steam ID, the profile name) needed a physical keyboard. Horizon is controller-first and will run inside gamescope, where there is no desktop on-screen keyboard to rely on and a touch screen can be delivered as plain mouse input.

## Decision

- **Horizon draws it**, in its own window, above every page (`OnScreenKeyboard`). No system keyboard, input-method protocol, separate surface or portal: none of those can be counted on in a gamescope session, and one window is what gamescope composites.
- **Rust owns it.** Layout and focus moves live in `navigation::keyboard` (pure); `presentation::keyboard::KeyboardController` decides when it shows and what keys do, and `NavigationController` routes input to it first while it is open. Slint only draws keys and reports clicks.
- **When it shows.** `InputDevice` records the last device: SDL actions set Gamepad, keyboard actions Keyboard, winit `Touch` and `MouseInput` events Touch and Pointer. An editor opening (`text-entry-changed`) shows the keyboard for Gamepad or Touch. A on the field, or the field's keyboard button, shows it on request, so a touch screen that arrives as a mouse is never stuck.
- **Controls.** D-pad moves (shared edge rule across, nearest key up/down), A presses, X deletes before the caret, LB/RB move the caret, L3 or R3 is Shift, + (Start) is Done, B closes. No hints are shown while it is open. Shift: one tap is for one letter, a double tap (400 ms) locks capitals, the next tap unlocks (`navigation::keyboard::Shift::tap`, tested). L3 and R3 are a new `UiAction::StickPress`, unused elsewhere; Menu (Start) means Done only while the keyboard is open. Done is the editor's own save.
- **Writes the draft, at a caret.** The keyboard keeps a caret (characters) and tells the field where it is (`keyboard-caret`, `keyboard-edit`, applied with `set-selection-offsets`); a first version only appended, and the field's caret stayed at the start. Keys edit the editor's draft property directly, so validation, Save's enabled state and saving are the editor's, unchanged. A on the text field now opens the keyboard instead of saving (Enter on a physical keyboard still saves, through the field).
- **Layout around it.** `ModalDialog.lift` keeps a dialog's card above the keyboard while its backdrop still covers the page.

## Consequences

- Moving the caret by pointer in the field isn't seen by the keyboard; its next key types at the keyboard's own caret.
- Long keys (32-character API keys) are slow to type by controller. Paste remains for a physical keyboard.
- Whether gamescope or Steam also raises its own keyboard when a Horizon text field takes focus is untested; if it does, Horizon's field may need to stop requesting the platform's text input while the on-screen keyboard is up.
