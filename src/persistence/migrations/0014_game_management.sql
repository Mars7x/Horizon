CREATE TABLE game_preferences (
    game_id INTEGER PRIMARY KEY REFERENCES games(id) ON DELETE RESTRICT,
    hidden INTEGER NOT NULL DEFAULT 0 CHECK (hidden IN (0, 1))
);
