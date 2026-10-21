# Input

[Input adapters](../src/input/) normalize device events into UiAction values.
[The manager](../src/input/manager.rs) owns repeat/held state and last-device
tracking; Rust navigation decides route and focus changes. SDL's event pump stays
on its required thread. Controller arrival/removal must not require a restart.

## Controls

| Intent | Keyboard | Controller |
| --- | --- | --- |
| Navigate | Arrows | D-pad or stick |
| Accept / Back | Enter / Escape | Platform confirm/back buttons |
| Bumpers | Q / E | LB / RB |
| Library | Activate Home’s Library tile | Activate Home’s Library tile |
| Home | Home | Guide/Home when delivered |

Text fields retain normal editing behavior. Directional repeat is bounded across
keyboard/controller paths and resets on release/deactivation/reconnection.
Shared edge policy stops held input at an edge; only a fresh press can wrap where
the page permits wrapping. Grids keep their own no-wrap/boundary rules.

Glyphs follow the last used keyboard/controller family. Xbox, PlayStation,
Nintendo, Steam Deck and Odin recognition remain; Nintendo uses its right-hand A
convention. Controller select/back swapping is a preference independent of
keyboard input; optional hint swapping follows the logical action.
SDL may identify underlying virtual devices itself. Horizon does not search a
Steam installation or set Steam controller-file paths. Generic hardware
recognition is separate from Steam account services.

During [GBA playback](GBA_PLAYBACK.md), held buttons and key releases drive the
core independently of menu repeats. Keyboard: arrows move, Z/X are A/B, A/S are
L/R, Enter is Start and Backspace is Select. Escape/F10, Guide or a stick click
opens Pause. Its vertical menu includes save/load state browsers; Left/Right
adjusts the focused speed choice. Back continues, or cancels restart confirmation. Focus loss clears and blocks
gameplay input without opening this menu; optional background freezing resumes
automatically, while an explicit pause remains paused.
Controller face-button conventions follow the device family;
Start/Back are game Start/Select. Swap Select and Back also exchanges controller gameplay A/B after device-family
mapping. [Button mapping](SETTINGS.md) can reassign controller A/B/L/R/Start/Select
from face buttons, shoulders, triggers and Start/Select; conflicting bindings
swap assignments. The menu swap applies after that mapping. D-pad/stick directions,
quick-menu access and keyboard controls remain unchanged. Pause, window deactivation and player entry/exit release held state.

## Mouse and touch

Pointer activation must match controller activation and feedback. Hover is not
selection. Wheel/touchpad input pans list cameras without moving selection.
Mouse side Back/Forward use the same multi-page history across routes, Settings
subpages, Activity details and Album viewers. Temporary dialogs/editors keep
input priority. New navigation clears Forward. Home game clicks activate the
clicked game; a recognized drag pans without launching. Window scaling,
fullscreen and ultrawide do not change semantic navigation.
Touch can be reported as mouse input; provide the existing explicit keyboard
button rather than relying only on device detection.

## On-screen keyboard

[Pure keyboard geometry](../src/navigation/keyboard.rs) has five rows, each 12
units wide. [The controller](../src/presentation/keyboard.rs) supplies target,
caret, shift/layer and editor actions to [the component](../ui/components/on-screen-keyboard.slint).

It opens for controller/touch editing, the field's A action or its keyboard
button. B closes it without closing the editor; editor close and global route
shortcuts dismiss it. The hint bar is empty while it is open.

D-pad/stick moves keys; A/click/tap types; X deletes; LB/RB move the caret;
stick-click shifts; Start means Done. Shift can capitalize once or lock on a
quick double tap. Keys edit the same draft as physical typing, replace selected
text, preserve Unicode caret offsets and obey the editor's validation. The
profile name, including [first-run creation](ONBOARDING.md), is normalized and
capped at 32 characters. Dialog lift keeps fields
above the keyboard. Sound and reduced-motion preferences still apply.

Nintendo DS adds keyboard C/V for X/Y and its own controller mapping for all
four face buttons, still following the shared A/B swap. Its touchscreen accepts
mouse press/drag, or right-stick cursor movement with R2 to touch. During a DS
game R3 swaps the screens and L3 alone opens the quick menu; other games keep
either stick click for the quick menu. See [playback](GBA_PLAYBACK.md).
