# Game options

On a selected game in Home or Library, controller +/Start or keyboard F10/Menu
opens the shared game menu. Right-clicking a cover selects that game and opens
the same commands without launching. The Options hint is clickable. The Library tile,
header utilities, empty grids and other pages do not open game options. Gameplay
keeps its existing Start/Select and pause controls.

The compact floating menu opens beside the selected cover, flips left near the
right edge and stays inside the viewport. The background remains undimmed.
Pointer movement highlights a command; clicking outside dismisses without
activating the underlying page. It uses the shared palette and reduced-motion
timing, compact ListRows and shared Surface styling. The popup contains only action labels, with no title,
helper text or status caption. Properties, failures and uninstall confirmation
use separate detail dialogs. The opening rise is a local offset from the cover
anchor, so the first opening never animates from the screen origin.
Up/Down selects Manage save data, Manage mods, Game properties, Hide/Unhide or Uninstall. A/Enter activates;
B/Escape or + backs out. Closing retains the background page’s selection, filter
and scroll. Global Home closes options and performs its normal navigation reset.
Reduced Motion presents modal changes immediately.

## Visibility and properties

Hide removes a game from Home, normal Library views and Activity cover choices.
It does not change ROMs, artwork, saves or historical totals. Library’s Filter
picker includes Hidden games; opening options there offers Unhide game. Hidden
preferences are keyed by durable GameId and survive restarts and folder rescans.

Properties displays the imported title, system, registered ROM paths, imported
file sizes, SHA-256 identities and title provenance. These are saved import
records, not a claim that the files are currently accessible or unchanged.
Up/Down and the wheel scroll long details. Closing properties or cancelling
uninstall dismisses options entirely, returning to the selected game.

## Manage save data

Manage save data opens a shared dialog with Open save file location, Back up save
data, Delete save data and Back. The first action asks the desktop file manager
to open the identity-bound save folder through XDG OpenDirectory; no host command
or extra sandbox permission is used. Backup copies the battery save and complete
state tree (manual slots, automatic resume and state fallback backups) to a new
timestamped folder under `backups/manual/`, with its identity document. Only a
fully synced snapshot receives its final name; an interrupted copy stays `.part`.
The dialog reports success or failure and never overwrites a previous snapshot.

Delete requires a second confirmation with Cancel initially selected. It deletes
the battery save and state tree, including automatic resume and state fallback
backups. Manual snapshots, previous battery backups, game identity, DS view,
ROMs, captures and history stay. A persistent reset marker prevents a future
launch from silently importing an old save beside the ROM. Links and invalid
paths stop the operation before removal; changes or filesystem failures during
removal are reported without claiming that the reset completed. Save operations
run on a joined worker while gameplay is closed. Back/Cancel closes the entire
options flow rather than bringing back the floating popup. See
[ADR 0230](adr/0230-manual-save-management.md).

## Manage mods

Manage mods lists the mods in the game's `mods/` folder, each On or Off, then
Open mods folder and Back. Select turns a mod on or off; new mods start off.
Choices are saved in the game's `mods.json` and apply the next time the game
starts. Open mods folder creates the folder when missing and opens it through
XDG OpenDirectory. More than five rows scroll with the selection.

- Game Boy, Game Boy Color, GBA and DS mods are `.ips`, `.bps` or `.ups`
  patches. Enabled ones apply in name order to a copy of the cartridge in
  memory after it is read; the game file is never changed. BPS and UPS
  patches check the cartridge's CRC32, so a patch for another version stops the
  launch with its name. The imported identity check still runs on the
  unpatched file.
- 3DS mods are folders in Azahar's LayeredFS layout (`romfs/`, `exefs/`,
  `code.ips`/`code.bps`, `exheader.bin`). At launch the enabled folders merge,
  in name order with later files winning, into Azahar's mod folder for the
  title (`core-data/Azahar/load/mods/<title ID>/`) as hard links, or copies
  across filesystems. Horizon marks the folder it builds and rebuilds only that
  one; a folder placed there by hand is used as it is and never replaced.

Links and other special files in `mods/` are ignored. Patched games share the
game's saves and states. See [ADR 0233](adr/0233-per-game-mods.md).

## Uninstall

Uninstall requires a second confirmation with Cancel selected by default. It
names the game and count of imported ROM files to delete. It retains battery
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
