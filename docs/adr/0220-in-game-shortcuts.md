# ADR 0220 — In-game shortcuts

Status: Accepted. The user asked for hold-to-fast-forward, quick save, quick
load and screenshot shortcuts.

Shortcuts are shared by every system and set on their own Controller page.
Each is one button or a pair pressed together. A single shortcut button is
withheld from the game, so one press never does two things; a pair's buttons
reach the game alone and are withheld only while the whole pair is down
(pressing one first can briefly reach the game). Both triggers together
fast-forward by default, which keeps either trigger alone for games (the DS
microphone is the left trigger); the others start unset so nothing fires by
accident. A button serves one shortcut. Stick clicks never open the quick menu
in any core; Guide and Escape do.
Keyboard shortcuts are fixed (Tab, F5, F8, F12).

Quick save and load use manual slot 1 rather than a new slot, keeping the
state layout; quick save overwrites it without confirmation, and the previous
bundle remains in the existing state backups.

Everything the player writes while a game can be running (state bundles,
screenshots, the automatic resume point) goes through one ordered background
writer; the game thread only takes snapshots and waits for the writer before
reading or writing states itself.
