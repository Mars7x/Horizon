-- Preserve imported identities, timestamps and game metadata, but remove them
-- from the active library. Account configuration and artwork caches are separate.
CREATE TABLE retired_game_sources (
    id INTEGER PRIMARY KEY,
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE RESTRICT,
    source_id TEXT NOT NULL CHECK (length(trim(source_id)) > 0),
    external_id TEXT NOT NULL CHECK (length(trim(external_id)) > 0),
    first_seen_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    UNIQUE (source_id, external_id)
);
CREATE INDEX idx_retired_game_sources_game_id ON retired_game_sources(game_id);

INSERT INTO retired_game_sources
    (id, game_id, source_id, external_id, first_seen_at, last_seen_at)
SELECT id, game_id, source_id, external_id, first_seen_at, last_seen_at
FROM game_sources WHERE source_id = 'steam';
DELETE FROM game_sources WHERE source_id = 'steam';
