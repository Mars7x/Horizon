-- Retire Heroic without deleting metadata, source identities or user history.
INSERT INTO retired_game_sources
    (id, game_id, source_id, external_id, first_seen_at, last_seen_at)
SELECT id, game_id, source_id, external_id, first_seen_at, last_seen_at
FROM game_sources WHERE source_id = 'heroic';
DELETE FROM game_sources WHERE source_id = 'heroic';
