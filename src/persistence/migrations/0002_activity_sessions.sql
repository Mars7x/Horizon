CREATE TABLE play_sessions (
    id INTEGER PRIMARY KEY,
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    source_id TEXT NOT NULL CHECK (length(trim(source_id)) > 0),
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    tracking_method TEXT NOT NULL CHECK (tracking_method IN ('foreground_handoff')),
    state TEXT NOT NULL CHECK (state IN ('open', 'completed', 'interrupted')),
    CHECK (
        (state = 'completed' AND ended_at IS NOT NULL AND ended_at >= started_at)
        OR (state IN ('open', 'interrupted') AND ended_at IS NULL)
    )
);

CREATE INDEX idx_play_sessions_game_started
    ON play_sessions(game_id, started_at DESC);

CREATE INDEX idx_play_sessions_state
    ON play_sessions(state);

CREATE TABLE source_lifetime_playtime (
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    source_id TEXT NOT NULL CHECK (length(trim(source_id)) > 0),
    lifetime_seconds INTEGER NOT NULL CHECK (lifetime_seconds >= 0),
    observed_at INTEGER NOT NULL,
    PRIMARY KEY (game_id, source_id)
);
