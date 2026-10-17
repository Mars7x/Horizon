# ADR 0215 — Deleted games leave the library

Status: Accepted. Amends ADR 0210, which never removed games. The user deleted a
game's file and expected it to disappear.

After a complete check of the chosen folder, a game none of whose files under that
folder remain is retired: its missing `rom_files` rows are removed and its source
reference moves to `retired_game_sources`, as the Steam retirement did. Nothing
else is deleted; history, saves, states, artwork and Album pictures stay, and the
same content imported again (any filename) moves back with its GameId.

Partial or unavailable scans still prune nothing: a scan error stops the check, and
a scan that finds none of the folder's imported files is treated as a missing
drive and changes nothing. Files outside the chosen folder are not checked. No
migration, dependency or UI is added.
