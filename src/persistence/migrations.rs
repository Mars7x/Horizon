use rusqlite::{Connection, TransactionBehavior, params};

use super::PersistenceError;

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_library",
        sql: include_str!("migrations/0001_initial_library.sql"),
    },
    Migration {
        version: 2,
        name: "activity_sessions",
        sql: include_str!("migrations/0002_activity_sessions.sql"),
    },
    Migration {
        version: 3,
        name: "managed_session_tracking",
        sql: include_str!("migrations/0003_managed_session_tracking.sql"),
    },
    Migration {
        version: 4,
        name: "source_runtime_tracking",
        sql: include_str!("migrations/0004_source_runtime_tracking.sql"),
    },
    Migration {
        version: 5,
        name: "remove_lutris_source",
        sql: include_str!("migrations/0005_remove_lutris_source.sql"),
    },
    Migration {
        version: 6,
        name: "activity_session_checkpoints",
        sql: include_str!("migrations/0006_activity_session_checkpoints.sql"),
    },
    Migration {
        version: 7,
        name: "remove_bottles_source",
        sql: include_str!("migrations/0007_remove_bottles_source.sql"),
    },
    Migration {
        version: 8,
        name: "remove_flatpak_source",
        sql: include_str!("migrations/0008_remove_flatpak_source.sql"),
    },
    Migration {
        version: 9,
        name: "retire_steam_games",
        sql: include_str!("migrations/0009_retire_steam_games.sql"),
    },
    Migration {
        version: 10,
        name: "retire_heroic_games",
        sql: include_str!("migrations/0010_retire_heroic_games.sql"),
    },
    Migration {
        version: 11,
        name: "rom_import",
        sql: include_str!("migrations/0011_rom_import.sql"),
    },
    Migration {
        version: 12,
        name: "onboarding",
        sql: include_str!("migrations/0012_onboarding.sql"),
    },
    Migration {
        version: 13,
        name: "large_rom_files",
        sql: include_str!("migrations/0013_large_rom_files.sql"),
    },
];

pub(crate) const LATEST_SCHEMA_VERSION: i64 = 13;

const MIGRATION_LEDGER_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at INTEGER NOT NULL
);
"#;

pub(crate) fn migrate(connection: &mut Connection) -> Result<(), PersistenceError> {
    connection.execute_batch(MIGRATION_LEDGER_SQL)?;

    let applied = applied_versions(connection)?;
    let current = applied.last().copied().unwrap_or(0);
    if current > LATEST_SCHEMA_VERSION {
        return Err(PersistenceError::UnsupportedSchemaVersion {
            found: current,
            supported: LATEST_SCHEMA_VERSION,
        });
    }
    validate_history(&applied)?;

    for migration in MIGRATIONS
        .iter()
        .filter(|migration| migration.version > current)
    {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(migration.sql)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, unixepoch())",
            params![migration.version, migration.name],
        )?;
        transaction.commit()?;
    }

    Ok(())
}

pub(crate) fn current_schema_version(connection: &Connection) -> Result<i64, PersistenceError> {
    let version = connection.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;
    Ok(version)
}

fn applied_versions(connection: &Connection) -> Result<Vec<i64>, PersistenceError> {
    let mut statement =
        connection.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
    let versions = statement
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(versions)
}

fn validate_history(applied: &[i64]) -> Result<(), PersistenceError> {
    for (index, version) in applied.iter().copied().enumerate() {
        let expected =
            i64::try_from(index + 1).map_err(|_| PersistenceError::MigrationIndexOverflow)?;
        if version != expected {
            return Err(PersistenceError::MigrationHistoryGap {
                expected,
                found: version,
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_idempotent() {
        let mut connection = Connection::open_in_memory().expect("in-memory database");
        migrate(&mut connection).expect("first migration run");
        migrate(&mut connection).expect("second migration run");

        assert_eq!(
            current_schema_version(&connection).expect("schema version"),
            LATEST_SCHEMA_VERSION
        );
    }

    #[test]
    fn retiring_flatpak_keeps_played_games_and_other_sources() {
        let mut connection = Connection::open_in_memory().expect("in-memory database");
        migrate(&mut connection).expect("migrate");
        connection
            .execute_batch(
                r#"
                INSERT INTO games(id, title, created_at, updated_at) VALUES
                    (1, 'Unplayed Flatpak', 0, 0),
                    (2, 'Played Flatpak', 0, 0),
                    (3, 'Steam Game', 0, 0);
                INSERT INTO game_sources(game_id, source_id, external_id, first_seen_at, last_seen_at) VALUES
                    (1, 'flatpak', 'org.example.One', 0, 0),
                    (2, 'flatpak', 'org.example.Two', 0, 0),
                    (3, 'steam', '70', 0, 0);
                INSERT INTO play_sessions(
                    id, game_id, source_id, started_at, ended_at, tracking_method, state, checkpoint_at
                ) VALUES (1, 2, 'flatpak', 10, 20, 'source_runtime', 'completed', 20);
                "#,
            )
            .expect("seed");

        connection
            .execute_batch(include_str!("migrations/0008_remove_flatpak_source.sql"))
            .expect("retire flatpak");

        let count = |sql: &str| {
            connection
                .query_row(sql, [], |row| row.get::<_, i64>(0))
                .expect("count")
        };
        assert_eq!(count("SELECT COUNT(*) FROM games WHERE id = 1"), 0);
        assert_eq!(count("SELECT COUNT(*) FROM games WHERE id = 2"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM games WHERE id = 3"), 1);
        assert_eq!(
            count("SELECT COUNT(*) FROM game_sources WHERE source_id = 'flatpak'"),
            0
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM game_sources WHERE source_id = 'steam'"),
            1
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM play_sessions WHERE game_id = 2"),
            1
        );
    }

    #[test]
    fn retiring_launchers_preserves_all_metadata_and_history() {
        let mut connection = Connection::open_in_memory().expect("database");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("foreign keys");
        connection
            .execute_batch(MIGRATION_LEDGER_SQL)
            .expect("ledger");
        for migration in MIGRATIONS.iter().filter(|migration| migration.version <= 8) {
            connection
                .execute_batch(migration.sql)
                .expect("old migration");
            connection
                .execute(
                    "INSERT INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, 0)",
                    params![migration.version, migration.name],
                )
                .expect("ledger entry");
        }
        connection
            .execute_batch(
                "INSERT INTO games(id, title, created_at, updated_at) VALUES
                    (1, 'Unplayed', 10, 20), (2, 'Played', 10, 20),
                    (3, 'Lifetime', 10, 20), (4, 'Other', 10, 20),
                    (5, 'Unplayed Heroic', 10, 20);
                 INSERT INTO game_sources(game_id, source_id, external_id, first_seen_at, last_seen_at) VALUES
                    (1, 'steam', '10', 11, 22), (2, 'steam', '20', 11, 22),
                    (3, 'heroic', 'gog:30', 11, 22), (4, 'example', '40', 11, 22),
                    (5, 'heroic', 'epic:50', 11, 22);
                 INSERT INTO play_sessions(id, game_id, source_id, started_at, ended_at, tracking_method, state, checkpoint_at)
                    VALUES (1, 2, 'steam', 100, 200, 'source_runtime', 'completed', 200);
                 INSERT INTO source_lifetime_playtime(game_id, source_id, lifetime_seconds, observed_at)
                    VALUES (3, 'heroic', 900, 100);",
            )
            .expect("seed version 8");
        migrate(&mut connection).expect("upgrade");
        migrate(&mut connection).expect("reopen");
        let count = |sql: &str| {
            connection
                .query_row(sql, [], |row| row.get::<_, i64>(0))
                .expect("query")
        };
        assert_eq!(count("SELECT COUNT(*) FROM games"), 5);
        assert_eq!(
            count(
                "SELECT COUNT(*) FROM retired_game_sources WHERE first_seen_at = 11 AND last_seen_at = 22"
            ),
            4
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM game_sources WHERE source_id IN ('steam', 'heroic')"),
            0
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM game_sources WHERE source_id = 'example'"),
            1
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM play_sessions WHERE ended_at - started_at = 100"),
            1
        );
        assert_eq!(
            count("SELECT lifetime_seconds FROM source_lifetime_playtime"),
            900
        );
    }

    #[test]
    fn latest_schema_accepts_managed_session_activity_rows() {
        let mut connection = Connection::open_in_memory().expect("in-memory database");
        migrate(&mut connection).expect("migrate");

        connection
            .execute(
                "INSERT INTO games(id, title, created_at, updated_at) VALUES (1, 'Game', 0, 0)",
                [],
            )
            .expect("game");
        connection
            .execute(
                r#"
                INSERT INTO play_sessions(
                    id, game_id, source_id, started_at, ended_at, tracking_method, state, checkpoint_at
                ) VALUES (1, 1, 'bottles', 10, 20, 'managed_session', 'completed', 20)
                "#,
                [],
            )
            .expect("managed session row");

        let method = connection
            .query_row(
                "SELECT tracking_method FROM play_sessions WHERE id = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("tracking method");
        assert_eq!(method, "managed_session");
    }

    #[test]
    fn latest_schema_accepts_source_runtime_activity_rows() {
        let mut connection = Connection::open_in_memory().expect("in-memory database");
        migrate(&mut connection).expect("migrate");

        connection
            .execute(
                "INSERT INTO games(id, title, created_at, updated_at) VALUES (1, 'Game', 0, 0)",
                [],
            )
            .expect("game");
        connection
            .execute(
                r#"
                INSERT INTO play_sessions(
                    id, game_id, source_id, started_at, ended_at, tracking_method, state, checkpoint_at
                ) VALUES (1, 1, 'steam', 10, 20, 'source_runtime', 'completed', 20)
                "#,
                [],
            )
            .expect("source runtime row");

        let method = connection
            .query_row(
                "SELECT tracking_method FROM play_sessions WHERE id = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("tracking method");
        assert_eq!(method, "source_runtime");
    }

    #[test]
    fn lutris_removal_migration_drops_active_refs_but_preserves_activity_history() {
        let mut connection = Connection::open_in_memory().expect("in-memory database");
        connection
            .execute_batch(MIGRATION_LEDGER_SQL)
            .expect("create migration ledger");

        for migration in MIGRATIONS.iter().filter(|migration| migration.version <= 4) {
            connection
                .execute_batch(migration.sql)
                .expect("apply old migration");
            connection
                .execute(
                    "INSERT INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, 0)",
                    params![migration.version, migration.name],
                )
                .expect("record old migration");
        }

        connection
            .execute_batch(
                r#"
                INSERT INTO games(id, title, created_at, updated_at) VALUES
                    (1, 'Lutris Only', 0, 0),
                    (2, 'Lutris History', 0, 0),
                    (3, 'Steam Game', 0, 0);

                INSERT INTO game_sources(
                    id, game_id, source_id, external_id, first_seen_at, last_seen_at
                ) VALUES
                    (1, 1, 'lutris', 'native:1', 0, 0),
                    (2, 2, 'lutris', 'native:2', 0, 0),
                    (3, 3, 'steam', '10', 0, 0);

                INSERT INTO play_sessions(
                    id, game_id, source_id, started_at, ended_at, tracking_method, state
                ) VALUES
                    (1, 2, 'lutris', 10, 70, 'foreground_handoff', 'completed');
                "#,
            )
            .expect("seed version-4 data");

        migrate(&mut connection).expect("apply Lutris retirement migration");

        let lutris_refs = connection
            .query_row(
                "SELECT COUNT(*) FROM game_sources WHERE source_id = 'lutris'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("count Lutris refs");
        assert_eq!(lutris_refs, 0);

        let lutris_only_games = connection
            .query_row("SELECT COUNT(*) FROM games WHERE id = 1", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count source-only game");
        assert_eq!(lutris_only_games, 0);

        let historical_games = connection
            .query_row("SELECT COUNT(*) FROM games WHERE id = 2", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count historical game");
        assert_eq!(historical_games, 1);

        let historical_sessions = connection
            .query_row(
                "SELECT COUNT(*) FROM play_sessions WHERE game_id = 2",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("count historical session");
        assert_eq!(historical_sessions, 1);

        let steam_refs = connection
            .query_row(
                "SELECT COUNT(*) FROM game_sources WHERE source_id = 'steam'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("count Steam refs");
        assert_eq!(steam_refs, 0);
        let archived = connection
            .query_row(
                "SELECT COUNT(*) FROM retired_game_sources WHERE source_id = 'steam'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("archived Steam identity");
        assert_eq!(archived, 1);
    }

    #[test]
    fn large_rom_files_migration_keeps_rows_and_admits_ds_sizes() {
        let mut connection = Connection::open_in_memory().expect("in-memory database");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("foreign keys");
        connection
            .execute_batch(MIGRATION_LEDGER_SQL)
            .expect("create migration ledger");
        for migration in MIGRATIONS
            .iter()
            .filter(|migration| migration.version <= 12)
        {
            connection
                .execute_batch(migration.sql)
                .expect("apply old migration");
            connection
                .execute(
                    "INSERT INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, 0)",
                    params![migration.version, migration.name],
                )
                .expect("record old migration");
        }
        let (sha1, sha256) = ("a".repeat(40), "b".repeat(64));
        connection
            .execute_batch(&format!(
                "INSERT INTO games(id, title, created_at, updated_at) VALUES (1, 'Game', 0, 0);
                 INSERT INTO rom_files(path, game_id, sha1, sha256, size, catalog_name, imported_at)
                 VALUES (x'2f612e676261', 1, '{sha1}', '{sha256}', 4096, 'Catalog', 7);"
            ))
            .expect("seed version-12 rom file");
        let oversized = format!(
            "INSERT INTO rom_files(path, game_id, sha1, sha256, size, catalog_name, imported_at)
             VALUES (x'2f622e6e6473', 1, '{sha1}', '{}', 419430400, NULL, 8)",
            "c".repeat(64)
        );
        assert!(
            connection.execute_batch(&oversized).is_err(),
            "32 MiB limit before"
        );
        migrate(&mut connection).expect("apply large rom files migration");
        let kept: (Vec<u8>, i64, String, i64, Option<String>, i64) = connection
            .query_row(
                "SELECT path, game_id, sha256, size, catalog_name, imported_at FROM rom_files",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .expect("row kept");
        assert_eq!(
            kept,
            (
                b"/a.gba".to_vec(),
                1,
                sha256,
                4096,
                Some("Catalog".into()),
                7
            )
        );
        connection
            .execute_batch(&oversized)
            .expect("400 MiB DS file fits");
        let index: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_rom_files_game_id'",
                [],
                |row| row.get(0),
            )
            .expect("index");
        assert_eq!(index, 1);
        // Deleting the game still removes its files.
        connection
            .execute_batch("DELETE FROM games WHERE id = 1")
            .expect("delete game");
        let left: i64 = connection
            .query_row("SELECT COUNT(*) FROM rom_files", [], |row| row.get(0))
            .expect("count");
        assert_eq!(left, 0);
    }

    #[test]
    fn checkpoint_migration_preserves_existing_sessions_and_initializes_checkpoint() {
        let mut connection = Connection::open_in_memory().expect("in-memory database");
        connection
            .execute_batch(MIGRATION_LEDGER_SQL)
            .expect("create migration ledger");

        for migration in MIGRATIONS.iter().filter(|migration| migration.version <= 5) {
            connection
                .execute_batch(migration.sql)
                .expect("apply old migration");
            connection
                .execute(
                    "INSERT INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, 0)",
                    params![migration.version, migration.name],
                )
                .expect("record old migration");
        }

        connection
            .execute_batch(
                r#"
                INSERT INTO games(id, title, created_at, updated_at)
                VALUES (1, 'Game', 0, 0);

                INSERT INTO play_sessions(
                    id, game_id, source_id, started_at, ended_at, tracking_method, state
                ) VALUES
                    (1, 1, 'steam', 10, NULL, 'source_runtime', 'open'),
                    (2, 1, 'steam', 20, 80, 'source_runtime', 'completed'),
                    (3, 1, 'steam', 90, NULL, 'source_runtime', 'interrupted');
                "#,
            )
            .expect("seed version-5 activity");

        migrate(&mut connection).expect("apply checkpoint migration");

        let mut statement = connection
            .prepare(
                "SELECT id, started_at, ended_at, checkpoint_at, state FROM play_sessions ORDER BY id",
            )
            .expect("prepare");
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("rows");

        assert_eq!(rows[0], (1, 10, None, 10, "open".to_owned()));
        assert_eq!(rows[1], (2, 20, Some(80), 80, "completed".to_owned()));
        assert_eq!(rows[2], (3, 90, None, 90, "interrupted".to_owned()));
    }

    #[test]
    fn rejects_a_future_database_schema() {
        let mut connection = Connection::open_in_memory().expect("in-memory database");
        connection
            .execute_batch(MIGRATION_LEDGER_SQL)
            .expect("create migration ledger");
        connection
            .execute(
                "INSERT INTO schema_migrations(version, name, applied_at) VALUES (?1, 'future', 0)",
                [LATEST_SCHEMA_VERSION + 1],
            )
            .expect("insert future version");

        assert!(matches!(
            migrate(&mut connection),
            Err(PersistenceError::UnsupportedSchemaVersion { .. })
        ));
    }
}
