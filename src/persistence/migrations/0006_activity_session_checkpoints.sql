CREATE TABLE play_sessions_v6 (
    id INTEGER PRIMARY KEY,
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    source_id TEXT NOT NULL CHECK (length(trim(source_id)) > 0),
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    tracking_method TEXT NOT NULL CHECK (
        tracking_method IN ('foreground_handoff', 'managed_session', 'source_runtime')
    ),
    state TEXT NOT NULL CHECK (state IN ('open', 'completed', 'interrupted')),
    checkpoint_at INTEGER NOT NULL,
    CHECK (checkpoint_at >= started_at),
    CHECK (
        (state = 'completed' AND ended_at IS NOT NULL AND ended_at >= started_at AND checkpoint_at = ended_at)
        OR (state = 'open' AND ended_at IS NULL)
        OR (state = 'interrupted' AND (ended_at IS NULL OR (ended_at >= started_at AND checkpoint_at = ended_at)))
    )
);

INSERT INTO play_sessions_v6(
    id, game_id, source_id, started_at, ended_at, tracking_method, state, checkpoint_at
)
SELECT
    id, game_id, source_id, started_at, ended_at, tracking_method, state,
    COALESCE(ended_at, started_at)
FROM play_sessions;

DROP TABLE play_sessions;
ALTER TABLE play_sessions_v6 RENAME TO play_sessions;

CREATE INDEX idx_play_sessions_game_started
    ON play_sessions(game_id, started_at DESC);

CREATE INDEX idx_play_sessions_state
    ON play_sessions(state);
