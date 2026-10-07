use rusqlite::{params, Connection, TransactionBehavior};

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
];

pub(crate) const LATEST_SCHEMA_VERSION: i64 = 3;

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

    for migration in MIGRATIONS.iter().filter(|migration| migration.version > current) {
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
                    id, game_id, source_id, started_at, ended_at, tracking_method, state
                ) VALUES (1, 1, 'bottles', 10, 20, 'managed_session', 'completed')
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
