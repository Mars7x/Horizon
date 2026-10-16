# GBA playback

Select an imported GBA game in Home or Library and press Accept. Horizon validates
the ROM's content hash and opens the bundled mGBA player inside the same window.
Missing folder access or changed bytes require selecting the game folder again.
Playback does not depend on hash-catalog recognition or SteamGridDB availability.

The game uses its original aspect ratio and black bars as needed. The default
None filter preserves sharp pixels. Software frames are paced at the core's reported rate; audio goes through
the existing native PipeWire boundary. mGBA's built-in BIOS implementation allows
the first version to run without a BIOS setup screen or distributed BIOS file.

## Launch and return motion

The selected Home or Library card keeps its press/rebound. Once a real frame is
ready, the rebound releases after a 125 ms press hold and the player expands
from that card to the full viewport over 230 ms. A slow load may expand a black
loading surface after 500 ms so progress/errors remain accessible. Content stays
in its original aspect ratio; black bars grow with its surface. The origin uses
the live carousel/grid camera and scales with the window.

Return to Horizon keeps the last frame visible while saving. Only a successful stop
starts the reverse 230 ms collapse; the card rebounds as the shell regains input.
A save error stays paused with retry available. Input is consumed throughout
both handoffs. Reduced Motion skips hold/zoom. Closing the application waits for
saving and skips the decorative return animation.

## Controls

| Game control | Keyboard | Controller |
| --- | --- | --- |
| Direction | Arrows | D-pad or left stick |
| A / B | Z / X | Device-family A/B equivalents |
| L / R | A / S | Left / right shoulder |
| Start / Select | Enter / Backspace | Start / Back |
| Pause | Escape or F10 | Guide or stick click |

Pause slides a square-edged, full-height panel in from the left edge over the
dimmed game, with no floating margins. Reduced Motion makes it immediate. Continue is initially
focused; Up/Down selects enabled actions, and Back continues. Opening and closing
use distinct short, smooth shuffle sounds under UI Sounds. Only actual transitions play
these cues; repeated opens, ignored inputs and focus freezing are silent. Closing
is heard on Continue/Back, successful restart or successful Return to Horizon;
cancelling a nested confirmation keeps the usual Back sound. Save state opens four
manual slots with native-pixel previews and local timestamps. Occupied or damaged
slots require confirmation, initially Cancel. Load state includes these slots and
the separate Automatic resume point; empty/unavailable entries do not take focus.
Back returns to the main panel. Save/load browsers and confirmations use the
shared 180 ms forward/back zoom and crossfade inside the stationary drawer;
Reduced Motion switches views immediately. Saving stays paused; a successful load continues
the game and clears queued audio/input, respecting background freeze.

Playback speed toggles 1×/2× for this loaded game (Left/Right also selects speed).
At 2× the worker doubles its frame pace and plays audio twice as fast, one octave
higher. A stereo low-pass filter prevents aliasing when reducing the doubled
sample flow to the output rate. Buffered audio is cleared on speed changes and
kept to at most 50 ms in the application queue. Speed is shown only in the pause
panel, with a directional 180 ms slide/crossfade when the value changes
(immediate with Reduced Motion). Returning to 1× clears queued audio before playback resumes.

The game worker publishes atomic stereo frames to a bounded queue; PipeWire does
not lock against core filtering or allocate. Playback builds about 40 ms of
headroom on startup/resume and after a real underrun, with short ramps at audio
boundaries. Brief frame jitter no longer repeatedly drains an empty queue or
produces silence because the worker holds a mutex. Long core/storage/device stalls
can still exhaust a bounded buffer.
Restart retains the current speed; launching another game starts at 1×.

Take screenshot writes the native unfiltered frame to Horizon's Album, without
the pause panel, HUD or shader/grid treatment. Only after PNG publication, a native-pixel
thumbnail appears beside the unchanged action label, scales from 90% to full size
and fades in over the shared 180 ms value duration. It holds for one second,
then fades away. A quiet 120 ms shutter cue follows UI Sounds. The paused frame,
menu layout and focus stay still, with no flash or bottom success text. Reduced
Motion shows/hides the thumbnail immediately and retains the one-second hold.
Repeated captures restart the confirmation; leaving the menu clears it. Failed
writes show an error without a thumbnail or shutter cue. Restart asks for confirmation with Cancel focused,
saves battery RAM before reset, restores it, then resumes. If storage fails it
stays paused without resetting. Restart remains within the same observed session.
Return to Horizon saves and closes the game through the card return animation.
The last game picture fades into the existing card as it shrinks, before the
card rebounds; there is no opaque picture swap at the end.
Pending actions retain the panel’s rows, focus styling and footer hints; only
the relevant value or message changes. Repeated actions are guarded in Rust.
The panel and hints support keyboard, controller and pointer, and respect Reduced
Motion and appearance preferences. Errors remain visible in the panel.

Window deactivation always releases gameplay input and never opens the quick
menu. Freeze when unfocused defaults on: the worker freezes video/audio silently
and automatically resumes on reactivation. Turning it off keeps the game and
audio running in the background, with gameplay input blocked. A manually opened
pause menu remains paused on return. Startup can produce its first frame before
freezing; restart follows the same background policy. Swap Select and Back also exchanges controller
gameplay A/B; keyboard bindings stay unchanged. Home/Library focus and route are retained.

## Core preferences

Settings → Cores contains shared Freeze when unfocused and Resume last state
switches, both default on.
Cores → mGBA contains Sunlight level (0–10) for solar-sensor games, Video filter
(None, LCD, Smooth or Smooth pixel art), and Colour correction. Defaults keep
filter/correction off and sunlight zero. Saved choices are read on the game worker
at launch; restart the game to apply changes. Invalid/unreadable settings are
reported without overwriting the file. No hardware sensors or BIOS configuration
are added. Audio smoothing is no longer a user preference.

LCD applies an original subtle RGB subpixel/grid treatment at 3× on the core
worker, then uses smooth scaling. Smooth uses native bilinear scaling without
changing source pixels. Smooth pixel art uses the pinned xbrz-rs implementation
at 3× to round pixel-art diagonals, then smoothly fits the filtered output to the
window. This avoids enlarging the filter's small steps with nearest-neighbour
scaling at fractional sizes; it does not remove intentional pixel-art details.
Colour correction
independently applies the credited sRGB GBA display profile before any filter;
it approximates the original screen rather than calibrating the user's display.
All presets preserve aspect ratio. Native screenshots exclude these display
treatments. No RetroArch shader runtime or imported presets are supported.

## Saves and sessions

Battery saves belong to Horizon under `saves/gba/<game>/<game name>.sav`,
with full ROM identity in `game.json`. Changing a
filename does not change the save identity. Changed RAM is saved every five
seconds and on quit. The previous valid save is retained as `backups/<game name>.previous.sav`; invalid
existing saves are preserved and reported. Save errors pause the game so the
user can fix storage and retry Return to Horizon. Closing the window waits for normal
saving. A forced process/core loss can lose progress since the last checkpoint.
See [storage](STORAGE.md).

Manual states and the automatic resume point are separate from battery saves.
Every normal game exit or application close writes an automatic point before
return/close completes. Resume last state loads the newest compatible automatic
or manual point at launch, before the first displayed frame. Turning this option
off starts normally but continues saving the automatic point on exit. A missing
state starts normally; if states exist but none can be read compatibly, launch
reports the problem and preserves them. Disable the option to start normally.
States match the full ROM hash, metadata format and exact mGBA version/build
hash. Core upgrades can make states unavailable; battery saves stay independent.
There is no automatic state checkpoint during gameplay, so a forced exit can lose
progress since the latest manual/automatic state. See [ADR 0206](adr/0206-save-states-and-resume.md).

Observed Activity sessions begin after the core produces a frame, include time
while the loaded game is paused, and finish after normal shutdown. Failed sessions
are interrupted; no session is invented for a failed load. No source lifetime
counter is added. Loading a state or restarting does not start another session.
Rewind, custom cores and RetroAchievements remain future work.

## Development

The Flatpak manifest builds and installs a checksum-pinned mGBA core at
`/app/lib/horizon/mgba_libretro.so`. No host emulator package is required. Native
development can set `HORIZON_GBA_CORE` to that core built in a development prefix;
the production Flatpak always uses its packaged path.

The ignored `real_core_draws_responds_to_held_input_and_restores_battery_memory`
test needs the built core. `real_player_window_input_pause_quit_and_relaunch`
also needs Wayland/PipeWire and a fresh `HORIZON_GBA_TEST_DIR`. Set
`HORIZON_TEST_VIDEO_FILTER` to `none`, `lcd`, `smooth` or `xbrz` to review each preset. `HORIZON_TEST_MENU_AUDIO=1` enables real UI cue playback
alongside the runtime test's semantic sound trace. Both use generated
original homebrew, never personal ROMs or the production database. See
[ADR 0198](adr/0198-first-gba-playback.md) and [licensing](LICENSING.md).
