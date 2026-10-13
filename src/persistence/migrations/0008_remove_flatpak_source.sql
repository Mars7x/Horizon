-- Retire Flatpak games from the active library without erasing Activity
-- history (Horizon moved to embedded emulator cores). Preserve source IDs in
-- historical play sessions and lifetime counters.
DELETE FROM game_sources WHERE source_id = 'flatpak';

DELETE FROM games
WHERE NOT EXISTS (
    SELECT 1 FROM game_sources WHERE game_sources.game_id = games.id
)
AND NOT EXISTS (
    SELECT 1 FROM play_sessions WHERE play_sessions.game_id = games.id
)
AND NOT EXISTS (
    SELECT 1 FROM source_lifetime_playtime
    WHERE source_lifetime_playtime.game_id = games.id
);
