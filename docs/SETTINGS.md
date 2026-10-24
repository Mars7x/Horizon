# Settings

[SettingsController](../src/presentation/settings.rs) and
[the page](../ui/pages/settings.slint) expose Appearance, Controller,
Third-Party and Cores. A fresh visit selects Appearance. Back from a sub-page restores its
parent row; full departure/re-entry resets the root selection.

## Appearance and controller

Theme is System/Light/Dark/Black. Accent is independent: System plus nine color
presets. UI Sounds is independent of both. Choices apply immediately and persist
atomically; a failed save must not silently replace the previous preference.
Focus is distinct from the selected value. See [UI standards](UI_STANDARDS.md).

Controller has Swap Select and Back, plus Swap Hints. The first changes controller
menu actions and gameplay A/B and X/Y; the second determines whether hints follow the swap. Keyboard
Enter/Escape remains unchanged. Button mapping lists the cores (mGBA, melonDS);
each opens its mapping, and Back returns to the list, then to Controller. mGBA's
page covers A, B, L, R, Start and Select, shared by Game Boy, Game Boy Color and GBA
games (L and R are GBA only). Each row pairs a named game control with
its assigned controller input in a keycap, using controller-family labels;
selecting either side changes the same
assignment. Select a row, release held controls, then
press a face button, shoulder, trigger, Start or Select. A binding already used by
another action swaps those two assignments. D-pad/stick directions and the
Guide's quick-menu control stays fixed (in DS games L3 swaps the screens). Stick click, Guide, Escape or
the dialog's Cancel button cancels capture. Capture times out after ten seconds;
disconnect, device changes and focus loss cancel it without saving.
melonDS's page is the same layout for the DS's A, B, X, Y, L, R, Start,
Select and Microphone (hold to blow; left trigger by default), stored
separately. Older eight-control files keep their buttons and gain Microphone's
default, or the first free button. When its rows don't fit the window, the list follows
focus.

Shortcuts is the same page for in-game shortcuts shared by every system:
Fast-forward (hold; both triggers together by default), Quick save, Quick load,
Screenshot and Rewind (not set by default). Each takes one button or two pressed
together: hold them, then let go (more than two asks again). X clears a row. A
one-button shortcut's button is held back from games; a pair's buttons still
reach the game alone and are held back only while the whole pair is down, so
the DS microphone (left trigger) keeps working. A button serves one shortcut:
giving it to another clears the first. Restore defaults restores the triggers.
Earlier single-button/null shortcut files retain their assignments; the next
settings save writes them in the current format that also supports pairs.
Four-row files retain their shortcuts and gain an unset Rewind row.

Mappings apply to all controllers after family normalization, independently of
keyboard gameplay. Swap Select and Back exchanges mapped gameplay A/B and X/Y; the rows
show the effective assignment. Restore defaults asks for confirmation with Cancel
focused and preserves the two menu/hint switches and the other system's mapping.
Values live in `controller.json`.
Loads/writes use a serial worker and the shared atomic settings store. Old files
without mappings get the existing defaults; invalid files remain intact and
report an error. Values and live input publish only after a successful write.
See [ADR 0208](adr/0208-controller-remapping-and-playback-speed.md).

## Cores

The Cores page contains the mGBA, melonDS and Azahar entries and shared Freeze when unfocused and
Resume last state switches. Resume defaults on and loads the newest compatible
manual or automatic state at launch. Off starts normally; exit still updates the
automatic point. Manual slots remain separate.
Freezing defaults on: the game and audio freeze without opening the quick menu,
and resume automatically on return. Off allows background playback; input is
always released/blocked while inactive. Explicit pause remains paused.

Cores → mGBA contains Sunlight level (0–10), Video filter, Colour correction,
Game Boy palette and Super Game Boy border.
Sunlight is a manual value for solar-sensor games, default zero, without hardware
sensor access. Video filter cycles between None (sharp pixels), LCD (subtle
subpixel/grid effect), Smooth (bilinear scaling) and Smooth pixel art (xBRZ
resized to the screen). Colour correction independently approximates the original
GBA and Game Boy Color screens using the credited sRGB profile. Game Boy palette
cycles Grey (default), Original green, Pocket, Light and Game Boy Color, the GBC's
own colours for the games it has them for (others stay grey); it applies to
original Game Boy games only, as Super Game Boy and colour games bring their own.
Super Game Boy border frames Super Game Boy games with their border (256×224).
Filter, correction and border default off. These are Horizon presets;
RetroArch shader files, audio smoothing controls, BIOS, core download, advanced
timing and developer controls are not exposed.

Cores → melonDS (the melonDS DS libretro core) contains Screen layout (Side by side, Stacked, Large and
small; default Side by side), Video filter (the same four presets, default
None) and Internal resolution (Native, then 2× to 8× with their DS line counts;
default Native). Higher resolutions draw 3D on the graphics card. LCD preserves
that higher-resolution image at every scale. Smooth pixel art is native-only;
above Native the row explains that Smooth is used to preserve sharper 3D. Swapping the screens is a quick-menu action, not a setting. Layout and swap
changed from the quick menu are remembered for that game, which then ignores
this default.

Cores → Azahar contains Screen layout (Stacked, Side by side, Large and small;
default Stacked) and Internal resolution (Native, then 2× to 6× with their line
counts; default Native). Higher values render the 3DS's 3D sharper on the
graphics card. As on the DS, a layout or swap changed from the quick menu is
remembered for that game.

All preferences apply on the next game launch; the pages don't repeat this
under their rows, which show only save errors. Shared focus freezing and resume are stored
in `playback.json`; mGBA sunlight, filtering, colour correction, palette and border are stored in
`cores/mgba.json`; the DS layout and filter in `cores/melonds-ds.json`; the 3DS layout and resolution in `cores/azahar.json`. Old `cores.json` files are not loaded or migrated. Invalid
files and failed writes preserve prior data and remain visible as errors.

Left/Right turns switches off/on or adjusts sunlight. Select toggles a switch
or increases sunlight; the sunlight row also has a pointer decrement button.
On Video filter, Left/Right cycles presets and Select/click chooses the next one.
Back restores the core's row in Cores, then Cores' row in Settings. Preferences load
and save on a serial worker using independent atomic files. Each action commits
only the file that owns the changed preference.
Changes publish only after a successful save. Pending writes preserve row styling,
focus and hints instead of disabling/repainting the whole page. Failed writes retain prior values;
unreadable/invalid files are preserved and reported, not replaced with defaults.
See [GBA playback](GBA_PLAYBACK.md) and [ADR 0199](adr/0199-curated-core-preferences.md).

## Third-party services

SteamGridDB and Steam Account have separate service pages, credentials and
files under `config/integrations/`. SteamGridDB offers its API key, artwork preference and cover refresh.
With no active games there are no cover jobs. Refresh runs off-thread and does
not overwrite explicit user choices. See [Artwork](ARTWORK.md).

Steam Account stores a validated SteamID64 and personal Web API key. It supplies
profile pictures and read-only friends only; no Steam screenshots or achievements
remain. This is manual account configuration, not OAuth or interactive login.
Disconnecting removes the saved account and updates retained account features;
inherited developer environment credentials may still exist.

Keys are masked during editing and never placed in general Slint state, logs,
request-URL errors or documentation. Private permissions are not encryption.
Service workers receive only short-lived copies needed for requests. Replacing an
account must reject stale results from the old account.

Remove/Disconnect is a trailing secondary action, with X as its controller
alternative, not a separate settings row. Confirm with Cancel initially focused.
Editors use the normal text field and [on-screen keyboard](INPUT.md). Invalid
values, failed saves and network errors must preserve useful prior state and
other preferences. See [Storage](STORAGE.md).
