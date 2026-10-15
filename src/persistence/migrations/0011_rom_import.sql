-- Content identity is independent of title, filename and future achievements.
CREATE TABLE rom_files (
    path BLOB NOT NULL,
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    sha1 TEXT NOT NULL CHECK(length(sha1) = 40),
    sha256 TEXT NOT NULL CHECK(length(sha256) = 64),
    size INTEGER NOT NULL CHECK(size BETWEEN 192 AND 33554432),
    catalog_name TEXT,
    imported_at INTEGER NOT NULL,
    PRIMARY KEY(path, sha256)
);
CREATE INDEX idx_rom_files_game_id ON rom_files(game_id);
CREATE TABLE rom_setup (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    completed INTEGER NOT NULL CHECK(completed IN (0,1)),
    folder BLOB,
    catalog_path BLOB
);
INSERT INTO rom_setup(id, completed) VALUES (1, 0);
