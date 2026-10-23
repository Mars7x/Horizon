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
timing; properties and uninstall confirmation retain separate detail dialogs.
Up/Down selects Play, Game properties, Hide/Unhide or Uninstall. A/Enter activates;
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
Up/Down and the wheel scroll long details; Back returns to the options row.

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
