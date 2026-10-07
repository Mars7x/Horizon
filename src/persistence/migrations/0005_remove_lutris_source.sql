-- Retire Lutris as an active Horizon source without erasing historical Activity.
-- Existing play_sessions/source_lifetime_playtime continue to own their logical
-- game rows, but game_sources membership is what makes a game active in Library.
DELETE FROM game_sources WHERE source_id = 'lutris';

DELETE FROM games
WHERE NOT EXISTS (
    SELECT 1 FROM game_sources WHERE game_sources.game_id = games.id
)
AND NOT EXISTS (
    SELECT 1 FROM play_sessions WHERE play_sessions.game_id = games.id
)
AND NOT EXISTS (
    SELECT 1
    FROM source_lifetime_playtime
    WHERE source_lifetime_playtime.game_id = games.id
);
