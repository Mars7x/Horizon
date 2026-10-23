# ADR 0228: Shared game options and retained game data

Date: 2026-10-23

## Status

Accepted at the user’s request for a clean + menu on a game.

## Decision

Use the same Rust-owned transient menu for selected games in Home and Library.
Offer Play, Game properties, Hide/Unhide and Uninstall. Keep the background route,
focus and scroll. Commands appear as a compact undimmed floating menu beside the
selected game, with edge-aware placement and pointer hover/right-click support.
Properties and confirmation use the shared detail dialogs. All views respect
Reduced Motion.
+ / Start / F10 opens options outside gameplay; existing gameplay input stays intact.

Persist visibility by GameId in migration 0014. Hidden games remain in the shared
append-only catalog and are recoverable in Library’s Hidden games filter. Omit
them from normal browsing/cover choices, while retaining historical measurements.

Properties reads saved import records on a worker. Uninstall is a separately
confirmed ROM-only deletion of the displayed game’s registered copies, with
Cancel initially selected. Validate the complete plan and content before deletion,
reject symlinks/changed files, surface partial failures and retire only records
actually removed. Preserve saves, states, captures, artwork, settings and historical
identity. Reimport restores the original identity. No host command, recursive game
folder deletion, new core API or broader filesystem permission is introduced.

This is an explicit exception to the importer’s previous never-delete-ROM policy;
import itself still only reads files. See [game options](../GAME_OPTIONS.md).
