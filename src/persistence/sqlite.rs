use std::{collections::BTreeSet, path::Path, time::Duration};

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::{
    domain::{
        ActivityValidationError, DomainValidationError, ExternalGameId, Game, GameId, GameTitle,
        LibraryGame, PlaySession, PlaySessionId, PlaySessionState, PlaytimeSeconds,
        SessionTrackingMethod, SourceGameRef, SourceId, SourceLifetimePlaytime,
    },
    services::{
        activity::{
            ActivityOverview, ActivityRepository, GameActivitySummary, RecentActivitySession,
        },
        library::{DiscoveredGame, LibraryRepository},
    },
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

    fn activity_value<T>(
        field: &'static str,
        result: Result<T, ActivityValidationError>,
    ) -> Result<T, PersistenceError> {
        result.map_err(|source| PersistenceError::InvalidStoredActivityValue { field, source })
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

    fn synchronize_source_snapshot(
        &mut self,
        source_id: &SourceId,
        discovered_games: &[DiscoveredGame],
        present_external_ids: &[ExternalGameId],
    ) -> Result<Vec<GameId>, Self::Error> {
        let transaction = self.connection.transaction()?;
        let mut game_ids = Vec::with_capacity(discovered_games.len());
        let incoming_ids = present_external_ids
            .iter()
            .map(|external_id| external_id.as_str().to_owned())
            .collect::<BTreeSet<_>>();

        for discovered in discovered_games {
            let raw_game_id = Self::upsert_in_transaction(&transaction, discovered)?;
            game_ids.push(Self::domain_value("games.id", GameId::new(raw_game_id))?);
        }

        let existing_ids = {
            let mut statement = transaction.prepare(
                "SELECT external_id FROM game_sources WHERE source_id = ?1",
            )?;
            let rows = statement.query_map([source_id.as_str()], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        for external_id in existing_ids {
            if incoming_ids.contains(&external_id) {
                continue;
            }
            transaction.execute(
                "DELETE FROM game_sources WHERE source_id = ?1 AND external_id = ?2",
                params![source_id.as_str(), external_id],
            )?;
        }

        // Active source membership and historical identity are separate.
        // Remove a logical game only when authoritative reconciliation removed
        // its final source ref and no Activity/lifetime history still owns it.
        transaction.execute(
            "DELETE FROM games WHERE NOT EXISTS (\
             SELECT 1 FROM game_sources WHERE game_sources.game_id = games.id\
             ) AND NOT EXISTS (\
             SELECT 1 FROM play_sessions WHERE play_sessions.game_id = games.id\
             ) AND NOT EXISTS (\
             SELECT 1 FROM source_lifetime_playtime WHERE source_lifetime_playtime.game_id = games.id\
             )",
            [],
        )?;

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
        let count = self.connection.query_row(
            "SELECT COUNT(DISTINCT games.id) FROM games \
             INNER JOIN game_sources ON game_sources.game_id = games.id",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        usize::try_from(count).map_err(|_| PersistenceError::InvalidCount(count))
    }
}


impl ActivityRepository for SqliteLibraryRepository {
    type Error = PersistenceError;

    fn begin_play_session(
        &mut self,
        game_id: GameId,
        source_id: &SourceId,
        started_at: i64,
        tracking_method: SessionTrackingMethod,
    ) -> Result<PlaySessionId, Self::Error> {
        self.connection.execute(
            "INSERT INTO play_sessions(\
             game_id, source_id, started_at, ended_at, tracking_method, state\
             ) VALUES (?1, ?2, ?3, NULL, ?4, 'open')",
            params![
                game_id.get(),
                source_id.as_str(),
                started_at,
                tracking_method.storage_key(),
            ],
        )?;
        Self::activity_value(
            "play_sessions.id",
            PlaySessionId::new(self.connection.last_insert_rowid()),
        )
    }

    fn complete_play_session(
        &mut self,
        session_id: PlaySessionId,
        ended_at: i64,
    ) -> Result<(), Self::Error> {
        let open_started_at = self
            .connection
            .query_row(
                "SELECT started_at FROM play_sessions WHERE id = ?1 AND state = 'open'",
                [session_id.get()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;

        let Some(started_at) = open_started_at else {
            return Err(PersistenceError::SessionNotOpen(session_id.get()));
        };
        if ended_at < started_at {
            return Err(PersistenceError::SessionEndBeforeStart {
                session_id: session_id.get(),
                started_at,
                ended_at,
            });
        }

        let changed = self.connection.execute(
            "UPDATE play_sessions SET ended_at = ?1, state = 'completed' \
             WHERE id = ?2 AND state = 'open'",
            params![ended_at, session_id.get()],
        )?;
        if changed != 1 {
            return Err(PersistenceError::SessionNotOpen(session_id.get()));
        }
        Ok(())
    }

    fn interrupt_play_session(
        &mut self,
        session_id: PlaySessionId,
    ) -> Result<(), Self::Error> {
        let changed = self.connection.execute(
            "UPDATE play_sessions SET state = 'interrupted' \
             WHERE id = ?1 AND state = 'open'",
            [session_id.get()],
        )?;
        if changed != 1 {
            return Err(PersistenceError::SessionNotOpen(session_id.get()));
        }
        Ok(())
    }

    fn interrupt_open_play_sessions(&mut self) -> Result<usize, Self::Error> {
        self.connection
            .execute("UPDATE play_sessions SET state = 'interrupted' WHERE state = 'open'", [])
            .map_err(Into::into)
    }

    fn activity_overview(
        &self,
        recent_limit: usize,
        top_games_limit: usize,
    ) -> Result<ActivityOverview, Self::Error> {
        let (raw_total, raw_sessions, raw_games) = self.connection.query_row(
            "SELECT \
             COALESCE(SUM(ended_at - started_at), 0), \
             COUNT(*), \
             COUNT(DISTINCT game_id) \
             FROM play_sessions WHERE state = 'completed'",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        let observed_playtime =
            Self::activity_value("play_sessions.duration", PlaytimeSeconds::new(raw_total))?;
        let completed_sessions = usize::try_from(raw_sessions)
            .map_err(|_| PersistenceError::InvalidCount(raw_sessions))?;
        let played_games =
            usize::try_from(raw_games).map_err(|_| PersistenceError::InvalidCount(raw_games))?;

        let recent_limit = i64::try_from(recent_limit)
            .map_err(|_| PersistenceError::InvalidCount(i64::MAX))?;
        let mut recent_statement = self.connection.prepare(
            "SELECT ps.id, ps.game_id, g.title, ps.source_id, ps.started_at, ps.ended_at, \
             ps.tracking_method, ps.state \
             FROM play_sessions AS ps \
             INNER JOIN games AS g ON g.id = ps.game_id \
             WHERE ps.state = 'completed' \
             ORDER BY ps.started_at DESC, ps.id DESC LIMIT ?1",
        )?;
        let recent_rows = recent_statement.query_map([recent_limit], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        })?;
        let mut recent_sessions = Vec::new();
        for row in recent_rows {
            let (
                raw_session_id,
                raw_game_id,
                raw_title,
                raw_source_id,
                started_at,
                ended_at,
                raw_method,
                raw_state,
            ) = row?;
            let session_id =
                Self::activity_value("play_sessions.id", PlaySessionId::new(raw_session_id))?;
            let game_id = Self::domain_value("play_sessions.game_id", GameId::new(raw_game_id))?;
            let title = Self::domain_value("games.title", GameTitle::new(raw_title))?;
            let source_id =
                Self::domain_value("play_sessions.source_id", SourceId::new(raw_source_id))?;
            let tracking_method = Self::activity_value(
                "play_sessions.tracking_method",
                SessionTrackingMethod::from_storage_key(&raw_method),
            )?;
            let state = Self::activity_value(
                "play_sessions.state",
                PlaySessionState::from_storage_key(&raw_state),
            )?;
            let session = Self::activity_value(
                "play_sessions",
                PlaySession::new(
                    session_id,
                    game_id,
                    source_id,
                    started_at,
                    ended_at,
                    tracking_method,
                    state,
                ),
            )?;
            recent_sessions.push(RecentActivitySession::new(session, title));
        }
        drop(recent_statement);

        let top_limit = i64::try_from(top_games_limit)
            .map_err(|_| PersistenceError::InvalidCount(i64::MAX))?;
        let mut top_statement = self.connection.prepare(
            "SELECT ps.game_id, g.title, SUM(ps.ended_at - ps.started_at), COUNT(*), MAX(ps.ended_at) \
             FROM play_sessions AS ps \
             INNER JOIN games AS g ON g.id = ps.game_id \
             WHERE ps.state = 'completed' \
             GROUP BY ps.game_id, g.title \
             ORDER BY SUM(ps.ended_at - ps.started_at) DESC, MAX(ps.ended_at) DESC, g.title ASC \
             LIMIT ?1",
        )?;
        let top_rows = top_statement.query_map([top_limit], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let mut top_games = Vec::new();
        for row in top_rows {
            let (raw_game_id, raw_title, raw_playtime, raw_sessions, last_played_at) = row?;
            let game_id = Self::domain_value("play_sessions.game_id", GameId::new(raw_game_id))?;
            let title = Self::domain_value("games.title", GameTitle::new(raw_title))?;
            let observed_playtime = Self::activity_value(
                "play_sessions.duration",
                PlaytimeSeconds::new(raw_playtime),
            )?;
            let completed_sessions = usize::try_from(raw_sessions)
                .map_err(|_| PersistenceError::InvalidCount(raw_sessions))?;
            top_games.push(GameActivitySummary::new(
                game_id,
                title,
                observed_playtime,
                completed_sessions,
                last_played_at,
            ));
        }

        Ok(ActivityOverview::new(
            observed_playtime,
            completed_sessions,
            played_games,
            recent_sessions,
            top_games,
        ))
    }

    fn upsert_source_lifetime_playtime(
        &mut self,
        report: &SourceLifetimePlaytime,
    ) -> Result<(), Self::Error> {
        self.connection.execute(
            "INSERT INTO source_lifetime_playtime(\
             game_id, source_id, lifetime_seconds, observed_at\
             ) VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(game_id, source_id) DO UPDATE SET \
             lifetime_seconds = excluded.lifetime_seconds, \
             observed_at = excluded.observed_at",
            params![
                report.game_id().get(),
                report.source_id().as_str(),
                report.lifetime().get(),
                report.observed_at(),
            ],
        )?;
        Ok(())
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
    fn authoritative_source_sync_removes_games_missing_from_the_new_snapshot() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        repository
            .upsert_discovered_games(&[
                discovered("steam", "10", "Installed"),
                discovered("steam", "20", "Removed"),
            ])
            .expect("seed");

        repository
            .synchronize_source_snapshot(
                &SourceId::new("steam").expect("source"),
                &[discovered("steam", "10", "Installed")],
                &[ExternalGameId::new("10").expect("external id")],
            )
            .expect("synchronize");

        let games = repository.list_games().expect("games");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].sources()[0].external_id().as_str(), "10");
    }

    #[test]
    fn authoritative_membership_preserves_installed_id_without_fresh_metadata() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        repository
            .upsert_discovered_games(&[
                discovered("steam", "10", "Installed"),
                discovered("steam", "20", "Still Installed"),
                discovered("steam", "30", "Uninstalled"),
            ])
            .expect("seed");

        repository
            .synchronize_source_snapshot(
                &SourceId::new("steam").expect("source"),
                &[discovered("steam", "10", "Installed")],
                &[
                    ExternalGameId::new("10").expect("ten"),
                    ExternalGameId::new("20").expect("twenty"),
                ],
            )
            .expect("synchronize");

        let games = repository.list_games().expect("games");
        let ids = games
            .iter()
            .map(|game| game.sources()[0].external_id().as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["10", "20"]);
    }

    #[test]
    fn authoritative_empty_snapshot_removes_orphaned_source_games() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        repository
            .upsert_discovered_game(&discovered("steam", "10", "Installed"))
            .expect("seed");

        repository
            .synchronize_source_snapshot(
                &SourceId::new("steam").expect("source"),
                &[],
                &[],
            )
            .expect("synchronize");

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

    #[test]
    fn completed_sessions_feed_observed_activity_overview() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        let game_id = repository
            .upsert_discovered_game(&discovered("steam", "10", "Session Game"))
            .expect("game");
        let source_id = SourceId::new("steam").expect("source");

        let first = repository
            .begin_play_session(
                game_id,
                &source_id,
                100,
                SessionTrackingMethod::ForegroundHandoff,
            )
            .expect("first session");
        repository
            .complete_play_session(first, 160)
            .expect("complete first");
        let second = repository
            .begin_play_session(
                game_id,
                &source_id,
                200,
                SessionTrackingMethod::ForegroundHandoff,
            )
            .expect("second session");
        repository
            .complete_play_session(second, 320)
            .expect("complete second");

        let overview = repository.activity_overview(8, 4).expect("overview");
        assert_eq!(overview.observed_playtime().get(), 180);
        assert_eq!(overview.completed_sessions(), 2);
        assert_eq!(overview.played_games(), 1);
        assert_eq!(overview.recent_sessions().len(), 2);
        assert_eq!(overview.top_games().len(), 1);
        assert_eq!(overview.top_games()[0].observed_playtime().get(), 180);
        assert_eq!(overview.top_games()[0].completed_sessions(), 2);
    }

    #[test]
    fn interrupted_sessions_do_not_invent_playtime() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        let game_id = repository
            .upsert_discovered_game(&discovered("steam", "10", "Interrupted"))
            .expect("game");
        repository
            .begin_play_session(
                game_id,
                &SourceId::new("steam").expect("source"),
                100,
                SessionTrackingMethod::ForegroundHandoff,
            )
            .expect("session");

        assert_eq!(repository.interrupt_open_play_sessions().expect("interrupt"), 1);
        let overview = repository.activity_overview(8, 4).expect("overview");
        assert_eq!(overview.observed_playtime().get(), 0);
        assert_eq!(overview.completed_sessions(), 0);
    }

    #[test]
    fn source_lifetime_playtime_stays_separate_from_observed_sessions() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        let game_id = repository
            .upsert_discovered_game(&discovered("steam", "10", "Reported"))
            .expect("game");
        repository
            .upsert_source_lifetime_playtime(&SourceLifetimePlaytime::new(
                game_id,
                SourceId::new("steam").expect("source"),
                PlaytimeSeconds::new(50_000).expect("playtime"),
                123,
            ))
            .expect("report");

        let overview = repository.activity_overview(8, 4).expect("overview");
        assert_eq!(overview.observed_playtime().get(), 0);
        assert_eq!(overview.completed_sessions(), 0);
    }

    #[test]
    fn historical_activity_preserves_game_identity_after_uninstall_without_listing_it() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        let game_id = repository
            .upsert_discovered_game(&discovered("steam", "10", "Historical"))
            .expect("game");
        let source_id = SourceId::new("steam").expect("source");
        let session = repository
            .begin_play_session(
                game_id,
                &source_id,
                100,
                SessionTrackingMethod::ForegroundHandoff,
            )
            .expect("session");
        repository
            .complete_play_session(session, 200)
            .expect("complete");

        repository
            .synchronize_source_snapshot(&source_id, &[], &[])
            .expect("uninstall sync");

        assert!(repository.list_games().expect("library").is_empty());
        assert_eq!(repository.game_count().expect("active count"), 0);
        let overview = repository.activity_overview(8, 4).expect("overview");
        assert_eq!(overview.recent_sessions()[0].title().as_str(), "Historical");
        assert_eq!(overview.observed_playtime().get(), 100);
    }

}
