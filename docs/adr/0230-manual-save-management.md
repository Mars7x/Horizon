# ADR 0230 — Manual save management

Status: Accepted. The user requested save-folder access, backup and deletion
from the game context menu, replacing Play.

Use the shared identity-bearing save folder and worker boundary. Open location
uses the XDG OpenDirectory portal, without host execution or sandbox expansion.
Backup copies the battery save and complete state tree to a unique, timestamped
`backups/manual/` snapshot with the game identity. Sync files and directories
before publishing the completed folder; preserve incomplete copies as `.part`.

Delete is a separate confirmation, defaulting to Cancel. It resets the battery
save, manual states, automatic resume and state fallback backups together, so
resume cannot restore the progress the user explicitly deleted. Keep manual
snapshots, previous battery backups, identity, DS view, ROMs, Album and history.
Persist an explicit reset marker first to prevent automatic beside-ROM save
import from undoing the reset on next launch. No automatic cleanup or save reset
runs during uninstall, folder scans or menu opening.

Reject links and non-regular save files, validate the complete plan before
removal, report partial failure and join accepted operations during shutdown.
Game management is unavailable while gameplay owns the save files. Save reset
has no database migration or change to historical measurements.
