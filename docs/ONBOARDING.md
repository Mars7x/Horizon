# First-run setup

Horizon opens a full-screen setup flow until it has been completed:
Welcome → Profile → Add your games → Ready. Titles remain system-neutral;
[ROM import](ROM_IMPORT.md) currently supports raw Game Boy, Game Boy Color and GBA files. The introduction
contains a welcome, one short line and one action. It has no sound/motion toggles,
progress rail or third-party connection forms. Preferences remain in Settings.

The [controller](../src/presentation/onboarding.rs) owns input before normal
route navigation. [Pure focus policy](../src/navigation/onboarding.rs) defines
step movement and the name/avatar/Back/Continue controls. Home/Menu/Forward
cannot bypass setup. Keyboard, mouse and controller share the same actions and
existing UI sounds; no additional sound assets are bundled.
The Library and Ready action rows follow their visible left-to-right order:
Left moves toward Back and Right returns toward the primary action. Up/Down
leave these single rows unchanged; directional movement stops at either edge.
Leaving Profile returns native keyboard focus to the app's input scope so the
hidden name editor cannot retain keyboard events on subsequent steps.

The profile name uses the existing normalization and 32-character limit. Empty
confirmation uses Player. Four text placeholders stand in for future profile
pictures. Selection is persisted as a placeholder source; it never deletes an
existing chosen/Steam picture. Existing profile metadata is prefilled and its
picture source survives unless the user chooses a placeholder.

A/Enter on the name field and its keyboard button open Horizon's shared on-screen
keyboard. D-pad/stick navigates keys; A types, X deletes, bumpers move the caret,
stick-click shifts, and Start/Done finishes name editing. Back first closes the
keyboard, leaving Profile open. Done returns focus to Continue. Physical editing
and the on-screen keyboard share one draft and Unicode-aware caret handling.
The form moves above the keyboard and suppresses nonessential content while typing.

Choose game folder opens the system picker directly. Selection automatically
scans and imports games, then reaches Ready; cancelling stays on the library setup
step. Do this later also reaches Ready. There is no catalog setup or review menu.
The desktop portal's folder picker requires mouse/keyboard input; this explicit exception
preserves narrow sandbox access. All Horizon setup screens accept controller input.

Profile writes and setup progress run on workers. Profile storage is committed
before advancing beyond Profile. SQLite migration 0012 records the last confirmed
step and completion; failed saves stay on the current screen with a retryable
error. Unconfirmed text is not a saved draft. Completion is recorded before the
flow reveals the existing Home. Restarting resumes an unfinished confirmed step;
completed setup is skipped. Existing installations get setup once without losing
profile pictures, library entries or historical sessions.

The UI uses shared hint/button/focus and transition components. All setup steps
omit the repeated app header; the minimal Welcome also omits the hint bar. Shared motion
preferences apply, including Reduced Motion. No Home layout is replaced.

Normal tests cover focus policy, persistence constraints, profile serialization
and existing keyboard geometry/editing. The ignored Wayland test
`controller_setup_keyboard_and_restart_smoke` exercises Slint keyboard-event routing
and switching to normalized controller actions, shared keyboard typing/Back/Done,
placeholder selection, import cancel,
skipping games, completion and saved state. It requires isolated XDG directories
and `HORIZON_SETUP_TEST_DIR`; it must never run against the user's real data.
A semantic input test does not establish physical gamepad reconnection or ARM
compatibility. See [Development](DEVELOPMENT.md).
