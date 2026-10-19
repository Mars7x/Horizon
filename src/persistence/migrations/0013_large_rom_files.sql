-- Nintendo DS cartridges reach 512 MiB; the GB/GBA-era 32 MiB limit rejected
-- them. Rebuild the table with the wider limit, keeping every row unchanged.
CREATE TABLE rom_files_large (
    path BLOB NOT NULL,
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    sha1 TEXT NOT NULL CHECK(length(sha1) = 40),
    sha256 TEXT NOT NULL CHECK(length(sha256) = 64),
    size INTEGER NOT NULL CHECK(size BETWEEN 192 AND 536870912),
    catalog_name TEXT,
    imported_at INTEGER NOT NULL,
    PRIMARY KEY(path, sha256)
);
INSERT INTO rom_files_large(path, game_id, sha1, sha256, size, catalog_name, imported_at)
    SELECT path, game_id, sha1, sha256, size, catalog_name, imported_at FROM rom_files;
DROP TABLE rom_files;
ALTER TABLE rom_files_large RENAME TO rom_files;
CREATE INDEX idx_rom_files_game_id ON rom_files(game_id);
