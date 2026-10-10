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
];

pub(crate) const LATEST_SCHEMA_VERSION: i64 = 7;

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
        assert_eq!(steam_refs, 1);
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
