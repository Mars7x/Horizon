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
menu actions and gameplay A/B; the second determines whether hints follow the swap. Keyboard
Enter/Escape remains unchanged. Values live in `controller.json`.

## Cores

The Cores page contains the mGBA entry and a shared Freeze when unfocused switch.
Freezing defaults on: the game and audio freeze without opening the quick menu,
and resume automatically on return. Off allows background playback; input is
always released/blocked while inactive. Explicit pause remains paused.

Cores → mGBA contains Sunlight level (0–10), Video filter and Colour correction.
Sunlight is a manual value for solar-sensor games, default zero, without hardware
sensor access. Video filter cycles between None (sharp pixels), LCD (subtle
subpixel/grid effect), Smooth (bilinear scaling) and Smooth pixel art (3× xBRZ).
Colour correction independently approximates the original GBA display using the
credited sRGB profile. Filter/correction default off. These are Horizon presets;
RetroArch shader files, audio smoothing controls, BIOS, core download, advanced
timing and developer controls are not exposed.

All preferences apply on the next game launch. Existing files retain their saved
freeze value when the former mGBA-specific field moves to shared storage. An
explicit shared value takes precedence; the retired audio smoothing field is
ignored. Saving writes the current shape through the same atomic boundary.

Left/Right turns switches off/on or adjusts sunlight. Select toggles a switch
or increases sunlight; the sunlight row also has a pointer decrement button.
On Video filter, Left/Right cycles presets and Select/click chooses the next one.
Back restores mGBA's row in Cores, then Cores' row in Settings. Preferences load
and save on a serial worker using the shared atomic store in `cores.json`.
Changes publish only after a successful save. Pending writes preserve row styling,
focus and hints instead of disabling/repainting the whole page. Failed writes retain prior values;
unreadable/invalid files are preserved and reported, not replaced with defaults.
See [GBA playback](GBA_PLAYBACK.md) and [ADR 0199](adr/0199-curated-core-preferences.md).

## Third-party services

SteamGridDB and Steam Account have separate service pages, credentials and
storage. SteamGridDB offers its API key, artwork preference and cover refresh.
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
