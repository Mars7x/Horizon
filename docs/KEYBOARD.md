# On-screen keyboard — current state

Horizon draws its own keyboard for typing with a controller or a touch screen (ADR 0179). It is part of Horizon's window, not the desktop's keyboard, so it works the same on a desktop and inside gamescope.

## When it appears

- A text editor opens (the Settings key and Steam ID editor, the Profile name editor, the Shop's search) and the last input was a **controller** or a **touch screen**. A physical keyboard or a mouse doesn't bring it up.
- **A on the text field** brings it back after it was closed.
- The field's **keyboard button** (right of the text) brings it up by pointer. Under gamescope a touch screen can arrive as a mouse, so this button, not touch detection, is what guarantees a touch user can always get the keyboard.

It closes with B (the editor stays open), when the editor closes, or with Home, Menu or Forward.

## Using it

| Input | Does |
| --- | --- |
| D-pad / stick | Move between keys. Left and Right follow the shared edge rule (a held press stops at the row's end, a fresh one wraps); Up and Down go to the nearest key in the next row, and stop at the top and bottom rows. |
| A, or a click or tap | Press the key |
| X | Delete the character before the caret |
| LB / RB | Move the caret left / right |
| L3 or R3 (press either stick) | Shift |
| + (Start) | Done |
| B | Close the keyboard |

It shows no hints: the hint bar is empty while it is open.

It plays the UI sounds (and is silent when they are off in Settings → Appearance): the navigation sound when focus moves to another key, OK for a key, Shift or Done, and Back for Delete (X or the key) and Close. A click or tap on a key sounds the same as A.

## Layout

Five rows, each 12 units wide, centred and docked above the hint bar (`components/on-screen-keyboard.slint`, at most 940 px wide):

- digits and Backspace;
- `q`–`p` and `-` `_`; `a`–`l` and `'` `.` `@`; Shift, `z`–`m` and `,` `?` `!`;
- `?123` (symbols), Space, Done.

The symbols layer swaps the letter rows for punctuation. **Shift** (the key, L3 or R3): one tap capitalises the next letter only; a quick double tap (within 400 ms) locks capitals; a tap while locked turns them off, as does a slow second tap after a single one. Done saves the editor, which closes it and the keyboard.

Keys type at the caret and write straight into the editor's draft, so the field, Save and the editor's own rules see the same text. The keyboard keeps the caret (in characters) and the field shows it there: `keyboard-caret` (a byte offset) is applied with `set-selection-offsets` whenever `keyboard-edit` changes. It starts at the end of the text. A field that opens with its text selected (the name) is replaced by the first key typed, as a physical keyboard would. The name stops at 32 characters.

The editor's dialog moves up to stay above the keyboard (`ModalDialog.lift`).

## Code

- `navigation/keyboard.rs`: the layers, key geometry and focus moves (pure, tested).
- `presentation/keyboard.rs`: `KeyboardController`: when it shows, the text target, Shift and layer, what keys do. Navigation routes input to it first while it is open.
- `input::InputDevice`: the device behind the last input (controller and keyboard from `InputManager`; touch and mouse from winit window events).
