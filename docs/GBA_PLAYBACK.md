# Playback

Game Boy, Game Boy Color and Game Boy Advance games all play in the bundled mGBA
core. Select an imported game in Home or Library and press Accept. Horizon validates
the ROM's content hash and opens the bundled mGBA player inside the same window.
Missing folder access or changed bytes require selecting the game folder again.
Playback does not depend on hash-catalog recognition or SteamGridDB availability.

The game uses its original aspect ratio and black bars as needed. The default
None filter preserves sharp pixels. Software frames are paced at the core's reported rate. The
core worker hands each new frame to a separate filter thread, so filtering never slows the game.
That thread filters the newest frame straight into Slint's pixel buffer only when the player has
taken the last one, and wakes the UI, which shows it without copying it. The player takes at most
one frame per display refresh (read from the monitor, 60 Hz if unknown), so on a high-refresh
display fast-forward shows more frames, up to the refresh rate; audio goes through
the existing native PipeWire boundary. mGBA's built-in BIOS implementation allows
the first version to run without a BIOS setup screen or distributed BIOS file.

mGBA picks the hardware from the cartridge header: colour games and dual-mode
games run as a Game Boy Color, Super Game Boy enhanced games as a Super Game Boy
(with its colours) and others as a Game Boy, in the chosen Game Boy palette.
The Super Game Boy border is off by default, so every Game Boy picture stays
160×144; turned on, Super Game Boy games show at 256×224 with their border.

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
| L / R (GBA only) | A / S | Left / right shoulder |
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

Playback speed offers 1×, 2×, 3× and 4× for this loaded game. Left/Right steps
between the limits; Select cycles through all four. The worker multiplies its
frame pace and plays audio at the corresponding faster, higher pitch. Only the newest frame is
filtered, once per frame shown, so frames fast-forward skips over are never filtered. On a 175 Hz
display, 4× shows about 175 frames a second with Smooth pixel art (`real_player_frame_rate_report`). A stereo
low-pass filter matched to the factor prevents aliasing before integer decimation
back to the output sample rate. Buffered audio is cleared on speed changes and
kept to at most 50 ms in the application queue. Speed is shown only in the pause
panel, with a directional 180 ms slide/crossfade when the value changes
(immediate with Reduced Motion). Returning to 1× clears queued audio before playback resumes.

The game worker publishes atomic stereo frames to a bounded queue; PipeWire does
not lock against core filtering or allocate. Playback builds about 31 ms of
headroom on startup and after a real underrun (anything queued beyond that before
the first sample is heard is trimmed), with short ramps at audio boundaries.
The core is paced by the system clock and the output device by its own, which
differ slightly (about 15 ppm measured on a Ryzen desktop). The reader keeps the
queue's average in a steady band by skipping or repeating a single sample now and
then (at most 16 a second, enough for about 490 ppm), so long sessions never drop
samples at the queue limit or open a refill gap. `real_core_audio_drift_at_normal_speed`
measures this against real PipeWire. Ordinary pauses, including focus freeze, keep the PipeWire stream
connected and render silence without consuming the bounded queued audio. Resume
continues that timeline without restarting the output or rebuilding headroom;
speed changes, restart and state loads still discard obsolete audio. Brief frame
jitter no longer repeatedly drains an empty queue or
produces silence because the worker holds a mutex. Long core/storage/device stalls
can still exhaust a bounded buffer.
Restart retains the current speed; launching another game starts at 1×.

Take screenshot writes the native unfiltered frame to Horizon's Album, without
the pause panel, HUD or shader/grid treatment. Only after lossless JXL publication, a native-pixel
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
(None, LCD, Smooth or Smooth pixel art), Colour correction, Game Boy palette and
Super Game Boy border. Defaults keep filter/correction/border off, sunlight zero
and the grey palette. Saved choices are read on the game worker
at launch; restart the game to apply changes. Invalid/unreadable settings are
reported without overwriting the file. No hardware sensors or BIOS configuration
are added. Audio smoothing is no longer a user preference.

LCD applies an original subtle RGB subpixel/grid treatment at 3× on the core
worker, then uses smooth scaling. Smooth uses native bilinear scaling without
changing source pixels. Smooth pixel art uses the pinned xbrz-rs implementation
to round pixel-art diagonals, delivered at the screen's own size and shown 1:1.
The player tells the filter its size in physical pixels; xBRZ runs at the fitted
scale rounded up (at most 6×), and the result is resized on the CPU to exactly
fill the screen's height by area averaging: each screen pixel averages the xBRZ
pixels it covers, so an edge blends across at most one screen pixel and flat
areas are untouched (neither the blur of a GPU stretch nor visible steps from
repeating pixels). On a 3440×1440 ultrawide GBA is 2160×1440 and Game Boy
1600×1440; 4K GBA fills its height at 13.5×. xBRZ runs in horizontal bands on up
to four threads (with overlapping rows, so the result is identical to one pass)
and the resize is split the same way: about 4.4 ms a frame at 3440×1440 and 8 ms
at 4K on a Ryzen 7 5800X3D (`xbrz_screen_size_cost`). xBRZ's own corner rounding
against the frame's outside is made opaque. When the window cannot fit twice the
game, xBRZ 6× is fitted smoothly by the GPU. Window resizes apply from the next
frame shown.
Colour correction
independently applies the credited sRGB display profile before any filter, for
GBA and Game Boy Color games (upstream's GBA and GBC profiles are identical);
original Game Boy games keep their palette;
it approximates the original screen rather than calibrating the user's display.
All presets preserve aspect ratio. Native screenshots exclude these display
treatments. No RetroArch shader runtime or imported presets are supported.

## Saves and sessions

Battery saves belong to Horizon under `saves/<system>/<game>/<game name>.sav`
(`gb`, `gbc` or `gba`), with full ROM identity in `game.json`. A Game Boy
cartridge clock (MBC3, as in Pokémon Gold, Silver and Crystal) is saved after the
battery RAM in the same file, the layout standalone mGBA uses, so the clock keeps
real time between sessions. Changing a
filename does not change the save identity. Changed RAM is saved every five
seconds and on quit. The previous valid save is retained as `backups/<game name>.previous.sav`; invalid
existing saves are preserved and reported. Save errors pause the game so the
user can fix storage and retry Return to Horizon. Closing the window waits for normal
saving. A forced process/core loss can lose progress since the last checkpoint.
See [storage](STORAGE.md).

Manual states and the automatic resume point are separate from battery saves.
Every normal game exit or application close writes an automatic point before
return/close completes. Resume last state loads the newest compatible automatic
or manual point at launch, before the first displayed frame. Metadata orders the
candidates so older slots can skip state-data reads/hashing and preview decoding
on the launch path; each candidate that could win still undergoes complete integrity
and compatibility checks, including backup recovery. Turning this option
off starts normally but continues saving the automatic point on exit. A missing
state starts normally; if states exist but none can be read compatibly, launch
reports the problem and preserves them. Disable the option to start normally.
States match the full ROM hash, metadata format and mGBA release (for example
`mGBA@0.10.5`); a rebuild of the same release keeps them, and mGBA rejects any
state it cannot read with the session rolled back. A new mGBA release makes
states unavailable; battery saves stay independent. Previews keep the game's
native size (240×160, or 160×144 for Game Boy and Game Boy Color).
There is no automatic state checkpoint during gameplay, so a forced exit can lose
progress since the latest manual/automatic state. See [ADR 0214](adr/0214-screen-size-xbrz-and-display-paced-frames.md) for filtering and pacing,
[ADR 0206](adr/0206-save-states-and-resume.md) and
[ADR 0211](adr/0211-states-follow-the-core-release.md).

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
test needs the built core; `real_core_plays_game_boy_games_at_their_own_size_and_keeps_their_clock`
checks Game Boy picture size, Super Game Boy borders and the cartridge clock. `real_player_window_input_pause_quit_and_relaunch`
also needs Wayland/PipeWire and a fresh `HORIZON_GBA_TEST_DIR`. Set
`HORIZON_TEST_VIDEO_FILTER` to `none`, `lcd`, `smooth` or `xbrz` to review each preset. `HORIZON_TEST_MENU_AUDIO=1` enables real UI cue playback
alongside the runtime test's semantic sound trace. Both use generated
original homebrew, never personal ROMs or the production database. See
[ADR 0198](adr/0198-first-gba-playback.md) and [licensing](LICENSING.md).

The ignored `real_core_pipewire_audio_soak` test runs generated tone homebrew
through the actual core, audio queue and PipeWire callbacks for ten minutes.
It cycles through 1×/2×/3×/4× each minute and pauses/resumes every fifteen seconds.
`HORIZON_AUDIO_SOAK_SECONDS` can set a shorter diagnostic run (10–3600 seconds).
It reports queue size, callback gaps, unexpected silence/underruns, invalid
samples and pause/resume failures. Its quiet test tone and callback measurements
do not verify physical speaker/headphone quality or game-specific music.
