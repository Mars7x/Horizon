# ADR 0081: Single Authority for API-Key Caret Blink

## Status
Accepted

## Context
The custom Settings API-key editor used a 540 ms repeating `Timer` to toggle `TextInput.text-cursor-width` between 2 px and 0 px. Slint's underlying text input already manages caret blinking. The separate, repeatedly restarted width timer competed with this behavior, causing unpredictable or apparently accelerated flashing. The timer was restarted on cursor-position and text-edit events, compounding the inconsistency.

## Decision
Remove the separate caret timer, wake/reset helper and all calls that restarted it. Use the native Slint `TextInput` caret with a fixed 2 px width when the input is the active controller target. Slint owns caret timing, visible/masked glyph positioning and the focus response. Set caret width to zero only while a non-input controller target is selected (a state change, not an animation).

Keep the existing animated horizontal scrolling, text-edge fades, input editing, native Wayland clipboard, and other Settings controls unchanged. The game focus brackets, utility selection focus, SteamGridDB networking and cache are also unaffected.

## Verification
Build under the project's pinned Slint toolchain. Test typing, moving the caret, revealing/masking the key, and repeated Ctrl+V while the input is selected, then navigate to Eye/Cancel/Save and back. Verify the caret blink is steady without speeding up. Runtime verification is required; these source edits have not been compiled in the patch environment.
