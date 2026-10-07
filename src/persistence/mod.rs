mod migrations;
mod sqlite;

use std::path::PathBuf;

use thiserror::Error;

use crate::domain::{ActivityValidationError, DomainValidationError};

pub use sqlite::SqliteLibraryRepository;

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("failed to open SQLite database {path}: {source}")]
    OpenDatabase {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database schema version {found} is newer than supported version {supported}")]
    UnsupportedSchemaVersion { found: i64, supported: i64 },
    #[error("database migration history expected version {expected} but found {found}")]
    MigrationHistoryGap { expected: i64, found: i64 },
    #[error("database migration index exceeded the supported integer range")]
    MigrationIndexOverflow,
    #[error("invalid persisted domain value in {field}: {source}")]
    InvalidStoredDomainValue {
        field: &'static str,
        #[source]
        source: DomainValidationError,
    },
    #[error("invalid persisted activity value in {field}: {source}")]
    InvalidStoredActivityValue {
        field: &'static str,
        #[source]
        source: ActivityValidationError,
    },
    #[error("database returned an invalid library/activity count: {0}")]
    InvalidCount(i64),
    #[error("play session {0} is not open")]
    SessionNotOpen(i64),
    #[error(
        "play session {session_id} cannot end at {ended_at} before its start time {started_at}"
    )]
    SessionEndBeforeStart {
        session_id: i64,
        started_at: i64,
        ended_at: i64,
    },
}
