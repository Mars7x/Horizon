#!/usr/bin/env python3
"""Exercise retirement SQL without a Rust toolchain or any user database."""

import sqlite3
import unittest
from pathlib import Path

MIGRATIONS = Path(__file__).resolve().parents[1] / 'src/persistence/migrations'


def upgrade(connection, through=10):
    connection.execute(
        'CREATE TABLE IF NOT EXISTS schema_migrations '
        '(version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL)'
    )
    connection.commit()
    current = connection.execute(
        'SELECT COALESCE(MAX(version), 0) FROM schema_migrations'
    ).fetchone()[0]
    for path in sorted(MIGRATIONS.glob('*.sql')):
        version = int(path.name.split('_', 1)[0])
        if current < version <= through:
            # Mirrors Rust's per-migration transaction/ledger boundary.
            try:
                connection.executescript('BEGIN IMMEDIATE;\n' + path.read_text())
                connection.execute(
                    'INSERT INTO schema_migrations VALUES (?, ?, 0)',
                    (version, path.stem.split('_', 1)[1]),
                )
                connection.commit()
            except sqlite3.Error:
                connection.rollback()
                raise


class RetirementMigrations(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        self.db.execute('PRAGMA foreign_keys=ON')
        upgrade(self.db, 8)
        self.db.executescript('''
            INSERT INTO games VALUES
                (1, 'Unplayed Steam', 1, 2), (2, 'Played Steam', 1, 2),
                (3, 'Lifetime Heroic', 1, 2), (4, 'Shared identity', 1, 2),
                (5, 'Unrelated', 1, 2);
            INSERT INTO game_sources VALUES
                (1, 1, 'steam', '480', 3, 4),
                (2, 2, 'steam', '10', 3, 4),
                (3, 3, 'heroic', 'gog:123', 3, 4),
                (4, 4, 'steam', '20', 3, 4),
                (5, 4, 'heroic', 'epic:abc', 3, 4),
                (6, 5, 'example', 'other', 3, 4);
            INSERT INTO play_sessions
                (id, game_id, source_id, started_at, ended_at, tracking_method, state, checkpoint_at)
                VALUES (1, 2, 'steam', 10, 20, 'source_runtime', 'completed', 20),
                       (2, 4, 'heroic', 30, NULL, 'source_runtime', 'open', 35);
            INSERT INTO source_lifetime_playtime VALUES (3, 'heroic', 900, 5);
        ''')

    def tearDown(self):
        self.db.close()

    def rows(self, table):
        return self.db.execute(f'SELECT * FROM {table} ORDER BY 1').fetchall()

    def test_upgrade_preserves_metadata_history_and_exact_identities(self):
        before = {table: self.rows(table) for table in (
            'games', 'game_sources', 'play_sessions', 'source_lifetime_playtime'
        )}
        upgrade(self.db)
        upgrade(self.db)
        for table in ('games', 'play_sessions', 'source_lifetime_playtime'):
            self.assertEqual(self.rows(table), before[table])
        self.assertEqual(self.rows('retired_game_sources'), before['game_sources'][:5])
        self.assertEqual(self.rows('game_sources'), before['game_sources'][5:])
        self.assertEqual(self.db.execute('PRAGMA foreign_key_check').fetchall(), [])
        self.assertEqual(len(self.rows('schema_migrations')), 10)
        # Same lookup used by Album/Achievements: known names/AppIDs survive.
        metadata = self.db.execute('''
            SELECT g.title, gs.source_id, gs.external_id FROM games g
            JOIN retired_game_sources gs ON gs.game_id = g.id
            WHERE gs.source_id = 'steam' ORDER BY gs.id
        ''').fetchall()
        self.assertEqual(metadata[0], ('Unplayed Steam', 'steam', '480'))

    def test_version_nine_to_ten_preserves_existing_archive(self):
        upgrade(self.db, 9)
        steam = self.rows('retired_game_sources')
        upgrade(self.db)
        archived = self.rows('retired_game_sources')
        self.assertEqual([row for row in archived if row[2] == 'steam'], steam)
        self.assertEqual(len(archived), 5)

    def test_archive_protects_games_from_reconciliation(self):
        upgrade(self.db)
        self.db.executescript('''
            DELETE FROM games WHERE NOT EXISTS
                (SELECT 1 FROM game_sources WHERE game_id = games.id)
            AND NOT EXISTS
                (SELECT 1 FROM retired_game_sources WHERE game_id = games.id)
            AND NOT EXISTS
                (SELECT 1 FROM play_sessions WHERE game_id = games.id)
            AND NOT EXISTS
                (SELECT 1 FROM source_lifetime_playtime WHERE game_id = games.id);
        ''')
        self.assertEqual(len(self.rows('games')), 5)
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute('DELETE FROM games WHERE id = 1')
        self.db.rollback()

    def test_failed_archive_rolls_back_without_advancing_ledger(self):
        upgrade(self.db, 9)
        self.db.execute('''
            CREATE TRIGGER reject_heroic BEFORE INSERT ON retired_game_sources
            WHEN NEW.source_id = 'heroic'
            BEGIN SELECT RAISE(ABORT, 'simulated archive failure'); END
        ''')
        self.db.commit()
        before = self.rows('game_sources')
        with self.assertRaises(sqlite3.IntegrityError):
            upgrade(self.db)
        self.assertEqual(self.rows('game_sources'), before)
        self.assertEqual(len(self.rows('schema_migrations')), 9)
        self.db.execute('DROP TRIGGER reject_heroic')
        self.db.commit()
        upgrade(self.db)
        self.assertEqual(len(self.rows('schema_migrations')), 10)

    def test_fresh_database(self):
        with sqlite3.connect(':memory:') as fresh:
            fresh.execute('PRAGMA foreign_keys=ON')
            upgrade(fresh)
            upgrade(fresh)
            self.assertEqual(fresh.execute('SELECT COUNT(*) FROM games').fetchone()[0], 0)
            self.assertEqual(fresh.execute('PRAGMA foreign_key_check').fetchall(), [])


if __name__ == '__main__':
    unittest.main(verbosity=2)
