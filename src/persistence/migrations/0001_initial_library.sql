CREATE TABLE games (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE game_sources (
    id INTEGER PRIMARY KEY,
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    source_id TEXT NOT NULL CHECK (length(trim(source_id)) > 0),
    external_id TEXT NOT NULL CHECK (length(trim(external_id)) > 0),
    first_seen_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    UNIQUE (source_id, external_id)
);

CREATE INDEX idx_game_sources_game_id
    ON game_sources(game_id);
