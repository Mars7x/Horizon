use std::{path::Path, time::Duration};

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::{
    domain::{
        DomainValidationError, ExternalGameId, Game, GameId, GameTitle, LibraryGame,
        SourceGameRef, SourceId,
    },
    services::library::{DiscoveredGame, LibraryRepository},
};

use super::{migrations, PersistenceError};

pub struct SqliteLibraryRepository {
    connection: Connection,
}

impl SqliteLibraryRepository {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, PersistenceError> {
        let path = path.as_ref();
        let connection = Connection::open(path).map_err(|source| PersistenceError::OpenDatabase {
            path: path.to_owned(),
            source,
        })?;
        Self::from_connection(connection)
    }

    #[cfg(test)]
    fn open_in_memory() -> Result<Self, PersistenceError> {
        let connection = Connection::open_in_memory()?;
        Self::from_connection(connection)
    }

    fn from_connection(mut connection: Connection) -> Result<Self, PersistenceError> {
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        connection.busy_timeout(Duration::from_secs(5))?;
        migrations::migrate(&mut connection)?;
        Ok(Self { connection })
    }

    pub fn schema_version(&self) -> Result<i64, PersistenceError> {
        migrations::current_schema_version(&self.connection)
    }

    fn domain_value<T>(
        field: &'static str,
        result: Result<T, DomainValidationError>,
    ) -> Result<T, PersistenceError> {
        result.map_err(|source| PersistenceError::InvalidStoredDomainValue { field, source })
    }

    fn upsert_in_transaction(
        transaction: &Transaction<'_>,
        discovered: &DiscoveredGame,
    ) -> Result<i64, PersistenceError> {
        let existing_game_id = transaction
            .query_row(
                "SELECT game_id FROM game_sources WHERE source_id = ?1 AND external_id = ?2",
                params![
                    discovered.source_id().as_str(),
                    discovered.external_id().as_str()
                ],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;

        let game_id = if let Some(game_id) = existing_game_id {
            transaction.execute(
                "UPDATE games SET title = ?1, updated_at = unixepoch() WHERE id = ?2",
                params![discovered.title().as_str(), game_id],
            )?;
            transaction.execute(
                "UPDATE game_sources SET last_seen_at = unixepoch() WHERE source_id = ?1 AND external_id = ?2",
                params![
                    discovered.source_id().as_str(),
                    discovered.external_id().as_str()
                ],
            )?;
            game_id
        } else {
            transaction.execute(
                "INSERT INTO games(title, created_at, updated_at) VALUES (?1, unixepoch(), unixepoch())",
                [discovered.title().as_str()],
            )?;
            let game_id = transaction.last_insert_rowid();
            transaction.execute(
                "INSERT INTO game_sources(game_id, source_id, external_id, first_seen_at, last_seen_at) \
                 VALUES (?1, ?2, ?3, unixepoch(), unixepoch())",
                params![
                    game_id,
                    discovered.source_id().as_str(),
                    discovered.external_id().as_str()
                ],
            )?;
            game_id
        };

        Ok(game_id)
    }
}

impl LibraryRepository for SqliteLibraryRepository {
    type Error = PersistenceError;

    fn upsert_discovered_game(
        &mut self,
        discovered: &DiscoveredGame,
    ) -> Result<GameId, Self::Error> {
        let transaction = self.connection.transaction()?;
        let raw_game_id = Self::upsert_in_transaction(&transaction, discovered)?;
        transaction.commit()?;
        Self::domain_value("games.id", GameId::new(raw_game_id))
    }

    fn upsert_discovered_games(
        &mut self,
        discovered_games: &[DiscoveredGame],
    ) -> Result<Vec<GameId>, Self::Error> {
        let transaction = self.connection.transaction()?;
        let mut game_ids = Vec::with_capacity(discovered_games.len());

        for discovered in discovered_games {
            let raw_game_id = Self::upsert_in_transaction(&transaction, discovered)?;
            game_ids.push(Self::domain_value("games.id", GameId::new(raw_game_id))?);
        }

        transaction.commit()?;
        Ok(game_ids)
    }

    fn list_games(&self) -> Result<Vec<LibraryGame>, Self::Error> {
        let mut statement = self.connection.prepare(
            "SELECT g.id, g.title, gs.source_id, gs.external_id \
             FROM games AS g \
             INNER JOIN game_sources AS gs ON gs.game_id = g.id \
             ORDER BY g.id, gs.source_id, gs.external_id",
        )?;

        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;

        let mut result: Vec<LibraryGame> = Vec::new();
        for row in rows {
            let (raw_id, raw_title, raw_source_id, raw_external_id) = row?;
            let game_id = Self::domain_value("games.id", GameId::new(raw_id))?;
            let title = Self::domain_value("games.title", GameTitle::new(raw_title))?;
            let source_id =
                Self::domain_value("game_sources.source_id", SourceId::new(raw_source_id))?;
            let external_id = Self::domain_value(
                "game_sources.external_id",
                ExternalGameId::new(raw_external_id),
            )?;
            let source = SourceGameRef::new(source_id, external_id);

            if let Some(last) = result.last_mut()
                && last.game().id() == game_id
            {
                last.push_source(source);
                continue;
            }

            result.push(LibraryGame::new(
                Game::new(game_id, title),
                vec![source],
            ));
        }

        Ok(result)
    }

    fn game_count(&self) -> Result<usize, Self::Error> {
        let count = self
            .connection
            .query_row("SELECT COUNT(*) FROM games", [], |row| row.get::<_, i64>(0))?;
        usize::try_from(count).map_err(|_| PersistenceError::InvalidCount(count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discovered(source: &str, external_id: &str, title: &str) -> DiscoveredGame {
        DiscoveredGame::new(
            SourceId::new(source).expect("source id"),
            ExternalGameId::new(external_id).expect("external id"),
            GameTitle::new(title).expect("title"),
        )
    }

    #[test]
    fn opens_with_latest_schema_and_foreign_keys_enabled() {
        let repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        assert_eq!(
            repository.schema_version().expect("schema version"),
            migrations::LATEST_SCHEMA_VERSION
        );

        let foreign_keys = repository
            .connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
            .expect("foreign key status");
        assert_eq!(foreign_keys, 1);
    }

    #[test]
    fn rediscovering_same_source_game_keeps_identity_and_refreshes_title() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        let first = repository
            .upsert_discovered_game(&discovered("steam", "480", "Old Title"))
            .expect("first upsert");
        let second = repository
            .upsert_discovered_game(&discovered("steam", "480", "New Title"))
            .expect("second upsert");

        assert_eq!(first, second);
        let games = repository.list_games().expect("list games");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].game().title().as_str(), "New Title");
        assert_eq!(games[0].sources().len(), 1);
    }

    #[test]
    fn equal_titles_from_different_source_keys_are_not_guessed_to_be_the_same_game() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        repository
            .upsert_discovered_game(&discovered("steam", "10", "Shared Title"))
            .expect("steam upsert");
        repository
            .upsert_discovered_game(&discovered("other", "10", "Shared Title"))
            .expect("other upsert");

        assert_eq!(repository.game_count().expect("count"), 2);
    }

    #[test]
    fn file_database_survives_reopen() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "horizon-library-test-{}-{unique}.sqlite3",
            std::process::id()
        ));

        {
            let mut repository = SqliteLibraryRepository::open(&path).expect("file repository");
            repository
                .upsert_discovered_game(&discovered("test", "persisted", "Persistent Game"))
                .expect("persist game");
        }

        {
            let repository = SqliteLibraryRepository::open(&path).expect("reopen repository");
            assert_eq!(repository.game_count().expect("count"), 1);
            assert_eq!(
                repository.list_games().expect("games")[0]
                    .game()
                    .title()
                    .as_str(),
                "Persistent Game"
            );
        }

        std::fs::remove_file(path).expect("remove test database");
    }

    #[test]
    fn batch_upsert_is_atomic_per_source_snapshot() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        repository
            .connection
            .execute_batch(
                "CREATE TRIGGER reject_test_title \
                 BEFORE INSERT ON games \
                 WHEN NEW.title = 'Reject Me' \
                 BEGIN SELECT RAISE(ABORT, 'test rejection'); END;",
            )
            .expect("test trigger");

        let games = vec![
            discovered("test", "one", "Keep Me"),
            discovered("test", "two", "Reject Me"),
        ];
        repository
            .upsert_discovered_games(&games)
            .expect_err("batch must fail");

        assert_eq!(repository.game_count().expect("count"), 0);
    }

    #[test]
    fn lists_games_in_stable_game_id_order() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        repository
            .upsert_discovered_game(&discovered("test", "2", "zeta"))
            .expect("zeta upsert");
        repository
            .upsert_discovered_game(&discovered("test", "1", "Alpha"))
            .expect("alpha upsert");

        let games = repository.list_games().expect("list games");
        let titles = games
            .iter()
            .map(|entry| entry.game().title().as_str())
            .collect::<Vec<_>>();
        assert_eq!(titles, vec!["zeta", "Alpha"]);
    }
}
