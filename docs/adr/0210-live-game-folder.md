# ADR 0210 — Live game folder

Status: Accepted. The user asked for game detection to be live: adding games
must not require deleting Horizon's data to rerun setup.

The folder chosen during import is checked again at startup and every 10 seconds
while Horizon runs. Polling is used because the folder is usually reached through
the document portal, where host-side changes do not raise file notifications.
Each check walks the folder with the existing scan limits and hashes only
supported files not already recorded at their current size. Files modified in
the last 5 seconds are left for a later check, so partly copied ROMs are not
imported. New games are rehashed before commit, saved without touching the
saved setup, and merged into the shared catalog through the existing reload path.

A check never prunes games, sessions, saves or artwork; missing files stay
unavailable until their folder returns, as before. Failures are logged once
until a check succeeds and never interrupt the user. No dependency, sandbox
permission, migration or new UI is added; choosing a different folder still
uses the existing importer.
