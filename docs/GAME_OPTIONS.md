# Game options

On a selected game in Home or Library, controller +/Start or keyboard F10/Menu
opens the shared game menu. Right-clicking a cover selects that game and opens
the same commands without launching. The Options hint is clickable. The Library tile,
header utilities, empty grids and other pages do not open game options. Gameplay
keeps its existing Start/Select and pause controls.

Game options are a full-height side panel at the right edge, the mirror of the
in-game quick menu: the same Theme.background, sideways edge shadow, 120 ms
slide in and 180 ms out (immediate with Reduced Motion), header, compact rows,
confirmations, message line and hints. The page behind dims as the game does
behind the quick menu; clicking it, or + / F10 again, closes the panel without
activating anything underneath. The header shows the current page's name in a
small caption (the system's name on the options page) above the game's title.
Pages change inside the panel with the shared forward/back transition while the
header and footer stay put.

The options page lists Manage save data, Manage mods (3DS games only), Game
properties, Hide/Unhide game and, set apart, Uninstall game. Up/Down selects,
A/Enter activates, and B/Escape goes up one page; on the options page it closes
the panel. Closing retains the background page's selection, filter and scroll.
Global Home closes options and performs its normal navigation reset. Results
and failures appear on the message line above the hints, on the page where they
happened; there are no separate error dialogs.

## Visibility and properties

Hide removes a game from Home, normal Library views and Activity cover choices.
It does not change ROMs, artwork, saves or historical totals. Library’s Filter
picker includes Hidden games; opening options there offers Unhide game. Hidden
preferences are keyed by durable GameId and survive restarts and folder rescans.

Game properties lists the imported title, system, registered ROM paths,
imported file sizes, SHA-256 identities and title provenance as captioned facts.
These are saved import records, not a claim that the files are currently
accessible or unchanged. Up/Down and the wheel scroll long details; B returns to
the options page.

## Manage save data

Manage save data opens a page with Open save folder, Back up save data and, set
apart, Delete save data. The first action asks the desktop file manager
to open the identity-bound save folder through XDG OpenDirectory; no host command
or extra sandbox permission is used. Backup copies the battery save and complete
state tree (manual slots, automatic resume and state fallback backups) to a new
timestamped folder under `backups/manual/`, with its identity document. Only a
fully synced snapshot receives its final name; an interrupted copy stays `.part`.
The message line reports success or failure; a backup never overwrites a
previous snapshot.

Delete asks first, on its own page with Cancel focused above Delete save data
(as the quick menu asks before a restart); Cancel returns to Save data. It deletes
the battery save and state tree, including automatic resume and state fallback
backups. Manual snapshots, previous battery backups, game identity, DS view,
ROMs, captures and history stay. A persistent reset marker prevents a future
launch from silently importing an old save beside the ROM. Links and invalid
paths stop the operation before removal; changes or filesystem failures during
removal are reported without claiming that the reset completed. Save operations
run on a joined worker while gameplay is closed. See
[ADR 0230](adr/0230-manual-save-management.md).

## Manage mods

3DS games only, the one system Horizon applies mods to. The page lists the mod
folders in the game's `mods/` folder, each with an on/off switch, then Open mods
folder. Select switches a mod; new mods start off. Choices are saved in the
game's `mods.json` and apply the next time the game starts. Open mods folder
creates the folder when missing and opens it through XDG OpenDirectory. Rows
past the bottom scroll with the selection.

Each mod is a folder in Azahar's LayeredFS layout (`romfs/`, `exefs/`,
`code.ips`/`code.bps`, `exheader.bin`). At launch the enabled folders merge, in
name order with later files winning, into Azahar's mod folder for the title
(`core-data/Azahar/load/mods/<title ID>/`) as hard links, or copies across
filesystems. Horizon marks the folder it builds and rebuilds only that one; a
folder placed there by hand is used as it is and never replaced. Files, links
and other special entries in `mods/` are ignored. See
[ADR 0233](adr/0233-per-game-mods.md).

## Uninstall

Uninstall asks first, on its own page with Cancel focused; Cancel returns to the
options page. It names the count of imported ROM files to delete under the
game's title. It retains battery
saves, save states/backups, screenshots, artwork, settings, identities and history.
All registered copies of the selected content are included; unrelated files and
other games are outside the plan. Missing files can be forgotten.

A worker reloads the confirmed plan and verifies hashes, sizes, file identity
and regular-file paths before deletion. Changed content, changed plans and
symlinks stop removal. Only successfully removed/missing ROM records are retired;
a partial failure remains visible and refreshes the library with actual results.
Reimporting identical content restores its original GameId and retained data.
There is no generic folder deletion, save cleanup or host command execution.

SQL stays in persistence; hashing/filesystem work stays on the worker. A pending
normal window close waits for a durable mutation; signal-driven shutdown joins
the accepted operation before process teardown. Stale property responses cannot
reopen a closed menu or replace another game’s details.
