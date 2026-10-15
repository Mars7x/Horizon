# GBA playback

Select an imported GBA game in Home or Library and press Accept. Horizon validates
the ROM's content hash and opens the bundled mGBA player inside the same window.
Missing folder access or changed bytes require selecting the game folder again.
Playback does not depend on hash-catalog recognition or SteamGridDB availability.

The game uses its original aspect ratio with pixelated scaling and black bars as
needed. Software frames are paced at the core's reported rate; audio goes through
the existing native PipeWire boundary. mGBA's built-in BIOS implementation allows
the first version to run without a BIOS setup screen or distributed BIOS file.

## Controls

| Game control | Keyboard | Controller |
| --- | --- | --- |
| Direction | Arrows | D-pad or left stick |
| A / B | Z / X | Device-family A/B equivalents |
| L / R | A / S | Left / right shoulder |
| Start / Select | Enter / Backspace | Start / Back |
| Pause | Escape or F10 | Guide or stick click |

Pause offers Resume and Save and quit, using normal keyboard/controller focus.
Back closes Pause. Window deactivation pauses the core and releases gameplay
input; returning does not automatically resume. Menu confirm/back swapping is
separate from game button mapping. Home/Library focus and route are retained.

## Saves and sessions

Battery saves belong to Horizon under `saves/gba/<ROM-SHA-256>.sav`. Changing a
filename does not change the save identity. Changed RAM is saved every five
seconds and on quit. The previous valid save is retained as `.sav.bak`; invalid
existing saves are preserved and reported. Save errors pause the game so the
user can fix storage and retry Save and quit. Closing the window waits for normal
saving. A forced process/core loss can lose progress since the last checkpoint.
See [storage](STORAGE.md).

Observed Activity sessions begin after the core produces a frame, include time
while the loaded game is paused, and finish after normal shutdown. Failed sessions
are interrupted; no session is invented for a failed load. No source lifetime
counter is added. Save states, rewind, custom cores and RetroAchievements remain
future work.

## Development

The Flatpak manifest builds and installs a checksum-pinned mGBA core at
`/app/lib/horizon/mgba_libretro.so`. No host emulator package is required. Native
development can set `HORIZON_GBA_CORE` to that core built in a development prefix;
the production Flatpak always uses its packaged path.

The ignored `real_core_draws_responds_to_held_input_and_restores_battery_memory`
test needs the built core. `real_player_window_input_pause_quit_and_relaunch`
also needs Wayland/PipeWire and a fresh `HORIZON_GBA_TEST_DIR`. Both use generated
original homebrew, never personal ROMs or the production database. See
[ADR 0198](adr/0198-first-gba-playback.md) and [licensing](LICENSING.md).
