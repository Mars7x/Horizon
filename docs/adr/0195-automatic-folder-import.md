# ADR 0195 — Choose a folder and import automatically

Status: Accepted. Supersedes the manual catalog/review interaction in
[ADR 0193](0193-reviewed-gba-import.md); its identity and data-retention rules remain.

The user chooses a game folder. Horizon immediately scans and imports readable
supported files, then returns to Library or advances first-run setup. Remove the
catalog chooser, scan button, per-file review and import confirmation. System-neutral
labels allow other formats to join the source boundary later; GBA remains first.

Open the desktop folder portal directly from the existing Add games actions.
Cancel changes nothing. A small noninteractive status replaces the old options
menu while scanning. Failures offer only another folder or Back; unreadable files
are reported briefly, without discarding readable imports or existing games.

Preserve content hashes, atomic writes, duplicate identity, catalog provenance,
folder grants and the append-only shared catalog. Existing catalog configuration
continues automatically. Without an available exact match, import filename
metadata as unverified; do not invent recognition or fetch/redistribute a catalog.
The simplified flow adds no permissions, dependencies, migrations or host commands.
