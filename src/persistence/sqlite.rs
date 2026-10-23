use std::{collections::BTreeSet, path::Path, time::Duration};

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{
    domain::{
        ActivityValidationError, DomainValidationError, ExternalGameId, Game, GameId, GameTitle,
        LibraryGame, PlaySession, PlaySessionId, PlaySessionState, PlaytimeSeconds,
        SessionTrackingMethod, SourceGameRef, SourceId, SourceLifetimePlaytime,
    },
    services::{
        activity::{
            ActivityOverview, ActivityRepository, GameActivityHistory, GameActivitySummary,
            LibrarySortMetrics, RecentActivitySession, ReportedGameSummary,
        },
        library::{DiscoveredGame, LibraryRepository},
    },
};

use super::{PersistenceError, migrations};

/// Games created this soon after the library's first one came with the
/// first import (a startup scan takes seconds), not a later install.
const FIRST_IMPORT_WINDOW_SECONDS: i64 = 60;

pub struct SqliteLibraryRepository {
    connection: Connection,
}

impl SqliteLibraryRepository {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, PersistenceError> {
        let path = path.as_ref();
        let connection =
            Connection::open(path).map_err(|source| PersistenceError::OpenDatabase {
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
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL;")?;
        connection.busy_timeout(Duration::from_secs(5))?;
        migrations::migrate(&mut connection)?;
        Ok(Self { connection })
    }

    pub fn schema_version(&self) -> Result<i64, PersistenceError> {
        migrations::current_schema_version(&self.connection)
    }

    /// Retire only the exact ROM records the worker removed, preserving identities/history.
    pub fn forget_removed_game_roms(
        &mut self,
        game: GameId,
        removed: &[crate::services::game_management::GameRom],
    ) -> Result<(), PersistenceError> {
        use std::os::unix::ffi::OsStrExt;
        let transaction = self.connection.transaction()?;
        for rom in removed {
            transaction.execute(
                "DELETE FROM rom_files WHERE game_id = ?1 AND path = ?2 AND sha256 = ?3",
                params![game.get(), rom.path.as_os_str().as_bytes(), rom.sha256],
            )?;
        }
        let remaining: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM rom_files WHERE game_id = ?1",
            [game.get()],
            |row| row.get(0),
        )?;
        if remaining == 0 {
            transaction.execute("INSERT OR REPLACE INTO retired_game_sources (game_id, source_id, external_id, first_seen_at, last_seen_at) SELECT game_id, source_id, external_id, first_seen_at, unixepoch() FROM game_sources WHERE game_id = ?1", [game.get()])?;
            transaction.execute("DELETE FROM game_sources WHERE game_id = ?1", [game.get()])?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn set_game_hidden(&mut self, game: GameId, hidden: bool) -> Result<(), PersistenceError> {
        self.connection.execute(
            "INSERT INTO game_preferences(game_id, hidden) VALUES (?1, ?2) ON CONFLICT(game_id) DO UPDATE SET hidden = excluded.hidden",
            params![game.get(), hidden],
        )?;
        Ok(())
    }

    pub fn game_rom_records(
        &self,
        game: GameId,
    ) -> Result<Vec<crate::services::game_management::GameRom>, PersistenceError> {
        use std::os::unix::ffi::OsStringExt;
        let mut statement = self.connection.prepare("SELECT path, sha256, size, catalog_name FROM rom_files WHERE game_id = ?1 ORDER BY path")?;
        let records = statement
            .query_map([game.get()], |row| {
                Ok(crate::services::game_management::GameRom {
                    path: std::path::PathBuf::from(std::ffi::OsString::from_vec(
                        row.get::<_, Vec<u8>>(0)?,
                    )),
                    sha256: row.get(1)?,
                    size: row.get(2)?,
                    catalog: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    /// Retain every location for a content identity; launch revalidates bytes.
    pub fn rom_locations(
        &self,
        system: crate::services::rom_import::RomSystem,
        sha256: &str,
    ) -> Result<Vec<crate::services::gba_player::RomLocation>, PersistenceError> {
        use std::os::unix::ffi::OsStringExt;
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT g.id, g.title, r.path FROM games g \
             JOIN game_sources s ON s.game_id = g.id AND s.source_id = ?2 \
             JOIN rom_files r ON r.game_id = g.id \
             WHERE r.sha256 = ?1 AND s.external_id = 'sha256:' || r.sha256 ORDER BY r.path",
        )?;
        let rows = statement.query_map(params![sha256, system.source_id()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (id, title, path) = row?;
            Ok(crate::services::gba_player::RomLocation {
                game_id: Self::domain_value("ROM game ID", GameId::new(id))?,
                title,
                path: std::ffi::OsString::from_vec(path).into(),
            })
        })
        .collect()
    }

    /// Retired metadata remains available for Horizon-owned capture titles.
    pub fn retired_games(&self) -> Result<Vec<LibraryGame>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT g.id, g.title, gs.source_id, gs.external_id \
             FROM games AS g \
             INNER JOIN retired_game_sources AS gs ON gs.game_id = g.id \
             ORDER BY g.id, gs.source_id, gs.external_id",
        )?;
        Self::read_games(&mut statement)
    }

    pub fn onboarding_progress(&self) -> Result<(i32, bool), PersistenceError> {
        self.connection
            .query_row(
                "SELECT step, completed FROM onboarding WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(Into::into)
    }

    pub fn save_onboarding_progress(
        &self,
        step: i32,
        completed: bool,
    ) -> Result<(), PersistenceError> {
        self.connection.execute(
            "UPDATE onboarding SET step = ?1, completed = ?2 WHERE id = 1",
            params![step, completed],
        )?;
        Ok(())
    }

    pub fn rom_setup(&self) -> Result<crate::services::rom_import::RomSetup, PersistenceError> {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt, path::PathBuf};
        self.connection
            .query_row(
                "SELECT completed, folder, catalog_path FROM rom_setup WHERE id = 1",
                [],
                |row| {
                    let path = |bytes: Option<Vec<u8>>| {
                        bytes.map(|bytes| PathBuf::from(OsString::from_vec(bytes)))
                    };
                    Ok(crate::services::rom_import::RomSetup {
                        completed: row.get(0)?,
                        folder: path(row.get(1)?),
                        catalog_path: path(row.get(2)?),
                    })
                },
            )
            .map_err(Into::into)
    }

    /// Recognize earlier filename imports from their saved full-file hashes.
    /// A conflicting/unknown hash or existing catalog provenance is untouched.
    pub fn recognize_saved_rom_titles(
        &mut self,
        catalogs: &crate::services::rom_import::RomCatalogs,
    ) -> Result<usize, PersistenceError> {
        use crate::services::rom_import::RomSystem;
        let transaction = self.connection.transaction()?;
        let mut titles = std::collections::BTreeMap::<i64, Option<(String, String)>>::new();
        {
            let mut statement = transaction.prepare(
                "SELECT DISTINCT r.game_id, gs.source_id, r.sha1, r.size FROM rom_files r \
                 INNER JOIN game_sources gs ON gs.game_id = r.game_id \
                 AND gs.external_id = 'sha256:' || r.sha256 \
                 WHERE NOT EXISTS (SELECT 1 FROM rom_files verified \
                 WHERE verified.game_id = r.game_id AND verified.catalog_name IS NOT NULL)",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u64>(3)?,
                ))
            })?;
            for row in rows {
                let (id, source, sha1, size) = row?;
                let Some(catalog) = RomSystem::from_source_id(&source).map(|s| catalogs.get(s))
                else {
                    continue;
                };
                let title = catalog
                    .title_for_hash(&sha1, size)
                    .map(|title| (title.to_owned(), catalog.name.clone()));
                titles
                    .entry(id)
                    .and_modify(|known| {
                        if *known != title {
                            *known = None;
                        }
                    })
                    .or_insert(title);
            }
        }
        let mut recognized = 0;
        for (id, title) in titles {
            let Some((title, catalog)) = title else {
                continue;
            };
            transaction.execute(
                "UPDATE games SET title = ?1, updated_at = unixepoch() WHERE id = ?2",
                params![title, id],
            )?;
            transaction.execute(
                "UPDATE rom_files SET catalog_name = ?1 WHERE game_id = ?2",
                params![catalog, id],
            )?;
            recognized += 1;
        }
        transaction.commit()?;
        Ok(recognized)
    }

    /// Imported ROM paths with their sizes, so a refresh can skip them.
    pub fn imported_rom_files(
        &self,
    ) -> Result<std::collections::HashSet<(std::path::PathBuf, u64)>, PersistenceError> {
        use std::os::unix::ffi::OsStringExt;
        let mut statement = self
            .connection
            .prepare("SELECT path, size FROM rom_files")?;
        let rows = statement.query_map([], |row| {
            Ok((
                std::ffi::OsString::from_vec(row.get::<_, Vec<u8>>(0)?).into(),
                row.get::<_, u64>(1)?,
            ))
        })?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    /// A complete scan of `folder` saw only `seen`: games whose every file in
    /// the folder is gone leave the library, keeping their GameId, history,
    /// saves and title, and return as they were if their file comes back.
    /// Files outside the folder are untouched. A scan that finds none of the
    /// folder's imported files looks like a missing drive, so it changes
    /// nothing. Returns the games retired.
    pub fn retire_missing_rom_files(
        &mut self,
        folder: &std::path::Path,
        seen: &std::collections::HashSet<std::path::PathBuf>,
    ) -> Result<usize, PersistenceError> {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let transaction = self.connection.transaction()?;
        let files = {
            let mut statement = transaction.prepare("SELECT path, game_id FROM rom_files")?;
            statement
                .query_map([], |row| {
                    Ok((
                        std::path::PathBuf::from(std::ffi::OsString::from_vec(
                            row.get::<_, Vec<u8>>(0)?,
                        )),
                        row.get::<_, i64>(1)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let in_folder: Vec<_> = files
            .into_iter()
            .filter(|(path, _)| path.starts_with(folder))
            .collect();
        if !in_folder.iter().any(|(path, _)| seen.contains(path)) {
            return Ok(0);
        }
        let mut affected = std::collections::BTreeSet::new();
        for (path, game_id) in in_folder.iter().filter(|(path, _)| !seen.contains(path)) {
            transaction.execute(
                "DELETE FROM rom_files WHERE path = ?1",
                [path.as_os_str().as_bytes()],
            )?;
            affected.insert(*game_id);
        }
        let mut retired = 0;
        for game_id in affected {
            let remaining: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM rom_files WHERE game_id = ?1",
                [game_id],
                |row| row.get(0),
            )?;
            if remaining > 0 {
                continue;
            }
            transaction.execute(
                "INSERT OR REPLACE INTO retired_game_sources \
                 (game_id, source_id, external_id, first_seen_at, last_seen_at) \
                 SELECT game_id, source_id, external_id, first_seen_at, unixepoch() \
                 FROM game_sources WHERE game_id = ?1",
                [game_id],
            )?;
            retired +=
                transaction.execute("DELETE FROM game_sources WHERE game_id = ?1", [game_id])?;
        }
        transaction.commit()?;
        Ok(retired)
    }

    /// Both registration and setup completion commit together. A repeat import
    /// reuses content identity, never title; unavailable folders never prune games.
    pub fn save_rom_import(
        &mut self,
        setup: &crate::services::rom_import::RomSetup,
        candidates: &[crate::services::rom_import::RomCandidate],
    ) -> Result<(), PersistenceError> {
        self.import_roms(Some(setup), candidates)
    }

    /// New files from the chosen folder; the saved setup is left as it is.
    pub fn add_rom_files(
        &mut self,
        candidates: &[crate::services::rom_import::RomCandidate],
    ) -> Result<(), PersistenceError> {
        self.import_roms(None, candidates)
    }

    fn import_roms(
        &mut self,
        setup: Option<&crate::services::rom_import::RomSetup>,
        candidates: &[crate::services::rom_import::RomCandidate],
    ) -> Result<(), PersistenceError> {
        use std::os::unix::ffi::OsStrExt;
        let transaction = self.connection.transaction()?;
        for candidate in candidates.iter().filter(|candidate| candidate.selected) {
            let Some(hash) = &candidate.fingerprint else {
                return Err(PersistenceError::InvalidRomImport(
                    "selected file has no fingerprint",
                ));
            };
            let source_id = candidate.system.source_id();
            let discovered = DiscoveredGame::new(
                SourceId::new(source_id).map_err(|source| {
                    PersistenceError::InvalidStoredDomainValue {
                        field: "ROM source",
                        source,
                    }
                })?,
                ExternalGameId::new(format!("sha256:{}", hash.sha256)).map_err(|source| {
                    PersistenceError::InvalidStoredDomainValue {
                        field: "ROM identity",
                        source,
                    }
                })?,
                GameTitle::new(candidate.title.clone()).map_err(|source| {
                    PersistenceError::InvalidStoredDomainValue {
                        field: "ROM title",
                        source,
                    }
                })?,
            );
            // A game retired because its file went missing returns as itself.
            transaction.execute(
                "INSERT OR IGNORE INTO game_sources \
                 (game_id, source_id, external_id, first_seen_at, last_seen_at) \
                 SELECT game_id, source_id, external_id, first_seen_at, unixepoch() \
                 FROM retired_game_sources WHERE source_id = ?1 AND external_id = ?2",
                params![source_id, discovered.external_id().as_str()],
            )?;
            transaction.execute(
                "DELETE FROM retired_game_sources WHERE source_id = ?1 AND external_id = ?2",
                params![source_id, discovered.external_id().as_str()],
            )?;
            // An unverified filename must never overwrite an earlier catalog title.
            let existing: Option<i64> = transaction
                .query_row(
                    "SELECT game_id FROM game_sources WHERE source_id = ?1 AND external_id = ?2",
                    params![source_id, discovered.external_id().as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            let game_id = match existing {
                Some(id) if candidate.catalog.is_none() => id,
                _ => Self::upsert_in_transaction(&transaction, &discovered)?,
            };
            transaction.execute("INSERT INTO rom_files(path, game_id, sha1, sha256, size, catalog_name, imported_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, unixepoch()) ON CONFLICT(path, sha256) DO UPDATE SET game_id=excluded.game_id, sha1=excluded.sha1, sha256=excluded.sha256, size=excluded.size, catalog_name=excluded.catalog_name, imported_at=excluded.imported_at",
                params![candidate.path.as_os_str().as_bytes(),game_id,hash.sha1,hash.sha256,hash.size,candidate.catalog])?;
        }
        if let Some(setup) = setup {
            transaction.execute(
                "UPDATE rom_setup SET completed = ?1, folder = ?2, catalog_path = ?3 WHERE id = 1",
                params![
                    setup.completed,
                    setup.folder.as_ref().map(|p| p.as_os_str().as_bytes()),
                    setup
                        .catalog_path
                        .as_ref()
                        .map(|p| p.as_os_str().as_bytes())
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn read_games(
        statement: &mut rusqlite::Statement<'_>,
    ) -> Result<Vec<LibraryGame>, PersistenceError> {
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

            result.push(LibraryGame::new(Game::new(game_id, title), vec![source]));
        }

        Ok(result)
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
            let mut statement =
                transaction.prepare("SELECT external_id FROM game_sources WHERE source_id = ?1")?;
            let rows = statement.query_map([source_id.as_str()], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        for external_id in existing_ids {
            if incoming_ids.contains(&external_id) {
                continue;
            }
            transaction.execute(
                "INSERT OR REPLACE INTO retired_game_sources (game_id, source_id, external_id, first_seen_at, last_seen_at) SELECT game_id, source_id, external_id, first_seen_at, unixepoch() FROM game_sources WHERE source_id = ?1 AND external_id = ?2 AND EXISTS (SELECT 1 FROM game_preferences WHERE game_preferences.game_id = game_sources.game_id)",
                params![source_id.as_str(), external_id],
            )?;
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
             SELECT 1 FROM retired_game_sources WHERE retired_game_sources.game_id = games.id\
             ) AND NOT EXISTS (\
             SELECT 1 FROM play_sessions WHERE play_sessions.game_id = games.id\
             ) AND NOT EXISTS (\
             SELECT 1 FROM source_lifetime_playtime WHERE source_lifetime_playtime.game_id = games.id\
             ) AND NOT EXISTS (\
             SELECT 1 FROM game_preferences WHERE game_preferences.game_id = games.id\
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

        let games = Self::read_games(&mut statement)?;
        let mut hidden = self
            .connection
            .prepare("SELECT game_id FROM game_preferences WHERE hidden = 1")?;
        let hidden = hidden
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<Result<BTreeSet<_>, _>>()?;
        Ok(games
            .into_iter()
            .map(|game| {
                let is_hidden = hidden.contains(&game.game().id().get());
                game.with_hidden(is_hidden)
            })
            .collect())
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
    fn recent_game_ids(&self, limit: usize) -> Result<Vec<GameId>, Self::Error> {
        let limit = i64::try_from(limit).map_err(|_| PersistenceError::InvalidCount(i64::MAX))?;
        // Installed/source-backed games, latest first by their latest real
        // session start (never a source lifetime-total import timestamp) or,
        // like a console's home row, when they joined the library. The first
        // import fills the library rather than adding to it: games that
        // arrived within a minute of the earliest one count only once played.
        let mut statement = self.connection.prepare(
            "WITH first_import AS (SELECT MIN(created_at) AS at FROM games), \
             recency AS ( \
               SELECT g.id AS game_id, MAX( \
                 COALESCE((SELECT MAX(ps.started_at) FROM play_sessions AS ps \
                           WHERE ps.game_id = g.id), -1), \
                 CASE WHEN g.created_at > (SELECT at FROM first_import) + ?2 \
                      THEN g.created_at ELSE -1 END) AS at \
               FROM games AS g \
               WHERE EXISTS (SELECT 1 FROM game_sources AS gs WHERE gs.game_id = g.id) \
                 AND NOT EXISTS (SELECT 1 FROM game_preferences AS p WHERE p.game_id = g.id AND p.hidden = 1)) \
             SELECT game_id FROM recency WHERE at >= 0 \
             ORDER BY at DESC, game_id ASC LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit, FIRST_IMPORT_WINDOW_SECONDS], |row| {
            row.get::<_, i64>(0)
        })?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(Self::domain_value("games.id", GameId::new(row?))?);
        }
        Ok(ids)
    }

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
             game_id, source_id, started_at, ended_at, tracking_method, state, checkpoint_at\
             ) VALUES (?1, ?2, ?3, NULL, ?4, 'open', ?3)",
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
            "UPDATE play_sessions \
             SET ended_at = ?1, checkpoint_at = ?1, state = 'completed' \
             WHERE id = ?2 AND state = 'open'",
            params![ended_at, session_id.get()],
        )?;
        if changed != 1 {
            return Err(PersistenceError::SessionNotOpen(session_id.get()));
        }
        Ok(())
    }

    // Interrupted sessions keep only time confirmed by a checkpoint. With no
    // checkpoint after the start, the duration is unknown (NULL), not zero.
    fn interrupt_play_session(&mut self, session_id: PlaySessionId) -> Result<(), Self::Error> {
        let changed = self.connection.execute(
            "UPDATE play_sessions \
             SET ended_at = NULLIF(checkpoint_at, started_at), state = 'interrupted' \
             WHERE id = ?1 AND state = 'open'",
            [session_id.get()],
        )?;
        if changed != 1 {
            return Err(PersistenceError::SessionNotOpen(session_id.get()));
        }
        Ok(())
    }

    fn checkpoint_open_play_sessions(&mut self, observed_at: i64) -> Result<usize, Self::Error> {
        self.connection
            .execute(
                "UPDATE play_sessions \
                 SET checkpoint_at = ?1 \
                 WHERE state = 'open' AND started_at <= ?1 AND checkpoint_at < ?1",
                [observed_at],
            )
            .map_err(Into::into)
    }

    fn interrupt_open_play_sessions(&mut self) -> Result<usize, Self::Error> {
        self.connection
            .execute(
                "UPDATE play_sessions \
                 SET ended_at = NULLIF(checkpoint_at, started_at), state = 'interrupted' \
                 WHERE state = 'open'",
                [],
            )
            .map_err(Into::into)
    }

    fn game_activity_history(&self, game_id: GameId) -> Result<GameActivityHistory, Self::Error> {
        let mut stmt = self.connection.prepare(
            "SELECT id, source_id, started_at, ended_at, tracking_method, state \
             FROM play_sessions WHERE game_id = ?1 \
             ORDER BY started_at DESC, id DESC",
        )?;
        let result = stmt.query_map([game_id.get()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        let mut sessions = Vec::new();
        for row in result {
            let (id, source, start, end, method, state) = row?;
            sessions.push(Self::activity_value(
                "play_sessions",
                PlaySession::new(
                    Self::activity_value("play_sessions.id", PlaySessionId::new(id))?,
                    game_id,
                    Self::domain_value("play_sessions.source_id", SourceId::new(source))?,
                    start,
                    end,
                    Self::activity_value(
                        "play_sessions.tracking_method",
                        SessionTrackingMethod::from_storage_key(&method),
                    )?,
                    Self::activity_value(
                        "play_sessions.state",
                        PlaySessionState::from_storage_key(&state),
                    )?,
                ),
            )?);
        }

        let mut stmt = self.connection.prepare(
            "SELECT source_id, lifetime_seconds FROM source_lifetime_playtime \
             WHERE game_id = ?1 ORDER BY lifetime_seconds DESC, source_id ASC",
        )?;
        let result = stmt.query_map([game_id.get()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut reported = Vec::new();
        for row in result {
            let (source, seconds) = row?;
            reported.push((
                Self::domain_value("source_lifetime_playtime.source_id", SourceId::new(source))?,
                Self::activity_value(
                    "source_lifetime_playtime.lifetime_seconds",
                    PlaytimeSeconds::new(seconds),
                )?,
            ));
        }
        Ok(GameActivityHistory { sessions, reported })
    }

    fn library_sort_metrics(&self) -> Result<Vec<LibrarySortMetrics>, Self::Error> {
        // Read-only query; no new tables, migrations or provider-specific joins.
        // A game's multiple source totals MUST NOT be summed. Prefer the
        // greatest reported lifetime when there are multiple source references.
        let mut statement = self.connection.prepare(
            "SELECT g.id, g.created_at, \
             (SELECT MAX(ps.started_at) FROM play_sessions AS ps \
                WHERE ps.game_id = g.id AND (ps.state = 'completed' \
                   OR (ps.state = 'interrupted' AND ps.ended_at IS NOT NULL))), \
             (SELECT COALESCE(SUM(ps.ended_at - ps.started_at), 0) \
                FROM play_sessions AS ps WHERE ps.game_id = g.id \
                AND (ps.state = 'completed' \
                   OR (ps.state = 'interrupted' AND ps.ended_at IS NOT NULL))), \
             (SELECT MAX(sl.lifetime_seconds) FROM source_lifetime_playtime AS sl \
                WHERE sl.game_id = g.id) \
             FROM games AS g WHERE EXISTS \
                (SELECT 1 FROM game_sources AS gs WHERE gs.game_id = g.id)",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        })?;
        let mut metrics = Vec::new();
        for row in rows {
            let (raw_game_id, added_at, last_played_at, observed_seconds, reported_seconds) = row?;
            let game_id = Self::domain_value("games.id", GameId::new(raw_game_id))?;
            metrics.push(LibrarySortMetrics {
                game_id,
                added_at,
                last_played_at,
                observed_seconds,
                reported_seconds,
            });
        }
        Ok(metrics)
    }

    fn observed_seconds_in_ranges(&self, ranges: &[(i64, i64)]) -> Result<Vec<i64>, Self::Error> {
        let mut totals = Vec::with_capacity(ranges.len());
        // SQLite's scalar min/max split sessions exactly at interval boundaries.
        // This avoids assigning an entire midnight-crossing session to one day.
        let mut statement = self.connection.prepare(
            "SELECT COALESCE(SUM(MAX(0, MIN(ended_at, ?2) - MAX(started_at, ?1))), 0) \
             FROM play_sessions \
             WHERE state IN ('completed', 'interrupted') \
                AND ended_at IS NOT NULL AND started_at < ?2 AND ended_at > ?1",
        )?;
        for &(start, end) in ranges {
            if end <= start {
                totals.push(0);
                continue;
            }
            let value: i64 = statement.query_row(params![start, end], |row| row.get(0))?;
            totals.push(value.max(0));
        }
        Ok(totals)
    }

    fn activity_overview(
        &self,
        recent_limit: usize,
        top_games_limit: usize,
    ) -> Result<ActivityOverview, Self::Error> {
        let (raw_total, raw_sessions, raw_games) = self.connection.query_row(
            "SELECT \
             COALESCE(SUM(CASE \
                 WHEN state IN ('completed', 'interrupted') AND ended_at IS NOT NULL \
                 THEN ended_at - started_at ELSE 0 END), 0), \
             COALESCE(SUM(CASE WHEN state = 'completed' THEN 1 ELSE 0 END), 0), \
             COUNT(DISTINCT CASE \
                 WHEN state IN ('completed', 'interrupted') AND ended_at IS NOT NULL \
                 THEN game_id END) \
             FROM play_sessions",
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

        let recent_limit =
            i64::try_from(recent_limit).map_err(|_| PersistenceError::InvalidCount(i64::MAX))?;
        let mut recent_statement = self.connection.prepare(
            "SELECT ps.id, ps.game_id, g.title, ps.source_id, ps.started_at, ps.ended_at, \
             ps.tracking_method, ps.state \
             FROM play_sessions AS ps \
             INNER JOIN games AS g ON g.id = ps.game_id \
             WHERE ps.state = 'completed' \
                OR (ps.state = 'interrupted' AND ps.ended_at IS NOT NULL) \
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

        // Open sessions are returned separately from durable completed/recovered
        // history so presentation can show live elapsed time without treating
        // an unfinished session as completed or recovered playtime.
        let mut active_statement = self.connection.prepare(
            "SELECT ps.id, ps.game_id, g.title, ps.source_id, ps.started_at, ps.ended_at, \
             ps.tracking_method, ps.state \
             FROM play_sessions AS ps \
             INNER JOIN games AS g ON g.id = ps.game_id \
             WHERE ps.state = 'open' \
             ORDER BY ps.started_at DESC, ps.id DESC LIMIT ?1",
        )?;
        let active_rows = active_statement.query_map([recent_limit], |row| {
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
        let mut active_sessions = Vec::new();
        for row in active_rows {
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
            active_sessions.push(RecentActivitySession::new(session, title));
        }
        drop(active_statement);

        let top_limit =
            i64::try_from(top_games_limit).map_err(|_| PersistenceError::InvalidCount(i64::MAX))?;
        let mut top_statement = self.connection.prepare(
            "SELECT ps.game_id, g.title, SUM(ps.ended_at - ps.started_at), \
             SUM(CASE WHEN ps.state = 'completed' THEN 1 ELSE 0 END), MAX(ps.ended_at) \
             FROM play_sessions AS ps \
             INNER JOIN games AS g ON g.id = ps.game_id \
             WHERE (ps.state = 'completed' \
                OR (ps.state = 'interrupted' AND ps.ended_at IS NOT NULL)) \
               AND EXISTS (SELECT 1 FROM game_sources AS gs WHERE gs.game_id = ps.game_id) \
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
            let observed_playtime =
                Self::activity_value("play_sessions.duration", PlaytimeSeconds::new(raw_playtime))?;
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

        // Provider totals are independent snapshots, never included in the
        // observed session aggregate above. Currently Heroic is the only
        // provider of lifetime playtime; each (game, provider) is one row.
        let raw_reported_total: i64 = self.connection.query_row(
            "SELECT COALESCE(SUM(lifetime_seconds), 0) FROM source_lifetime_playtime",
            [],
            |row| row.get(0),
        )?;
        let reported_total = Self::activity_value(
            "source_lifetime_playtime.lifetime_seconds",
            PlaytimeSeconds::new(raw_reported_total),
        )?;
        let mut reported_statement = self.connection.prepare(
            "SELECT g.title, sl.source_id, sl.lifetime_seconds \
             FROM source_lifetime_playtime AS sl \
             JOIN games AS g ON g.id = sl.game_id \
             ORDER BY sl.lifetime_seconds DESC, g.title ASC LIMIT ?1",
        )?;
        let reported_rows = reported_statement.query_map([top_limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut reported_games = Vec::new();
        for row in reported_rows {
            let (title, source_id, lifetime) = row?;
            reported_games.push(ReportedGameSummary::new(
                Self::domain_value("games.title", GameTitle::new(title))?,
                Self::domain_value(
                    "source_lifetime_playtime.source_id",
                    SourceId::new(source_id),
                )?,
                Self::activity_value(
                    "source_lifetime_playtime.lifetime_seconds",
                    PlaytimeSeconds::new(lifetime),
                )?,
            ));
        }
        Ok(ActivityOverview::new(
            observed_playtime,
            completed_sessions,
            played_games,
            recent_sessions,
            top_games,
        )
        .with_active_sessions(active_sessions)
        .with_source_reported(reported_total, reported_games))
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

    fn seed_game(
        repository: &mut SqliteLibraryRepository,
        title: &str,
        source: &str,
        external_id: &str,
    ) -> GameId {
        repository
            .upsert_discovered_game(&discovered(source, external_id, title))
            .expect("seed game")
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
    fn retired_metadata_stays_out_of_library_and_survives_reconciliation() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        let id = seed_game(&mut repository, "Remembered Game", "steam", "480");
        repository
            .connection
            .execute_batch(
                "INSERT INTO retired_game_sources SELECT * FROM game_sources;
                 DELETE FROM game_sources;",
            )
            .expect("archive identity");
        repository
            .synchronize_source_snapshot(&SourceId::new("example").expect("source"), &[], &[])
            .expect("reconcile another source");
        assert!(repository.list_games().expect("active games").is_empty());
        assert_eq!(repository.game_count().expect("active count"), 0);
        let retained = repository.retired_games().expect("retained metadata");
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].game().id(), id);
        assert_eq!(retained[0].game().title().as_str(), "Remembered Game");
        assert_eq!(retained[0].sources()[0].external_id().as_str(), "480");
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
            .synchronize_source_snapshot(&SourceId::new("steam").expect("source"), &[], &[])
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
    fn home_recents_put_games_added_after_the_first_import_with_the_played() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        let played = seed_game(&mut repository, "Played", "steam", "101");
        let unplayed = seed_game(&mut repository, "Unplayed", "steam", "102");
        let installed = seed_game(&mut repository, "Installed", "flatpak", "org.example.Game");
        let source = SourceId::new("steam").expect("source");
        repository
            .connection
            .execute("UPDATE games SET created_at = 1000", [])
            .expect("first import");
        // Installed later, after the game played at 2000.
        repository
            .connection
            .execute(
                "UPDATE games SET created_at = 3000 WHERE id = ?1",
                [installed.get()],
            )
            .expect("later install");
        let session = repository
            .begin_play_session(played, &source, 2000, SessionTrackingMethod::SourceRuntime)
            .expect("begin");
        repository
            .complete_play_session(session, 2010)
            .expect("complete");

        assert_eq!(
            repository.recent_game_ids(15).expect("recents"),
            vec![installed, played]
        );
        // Played after the install: the played game leads again.
        let session = repository
            .begin_play_session(played, &source, 4000, SessionTrackingMethod::SourceRuntime)
            .expect("begin");
        repository
            .complete_play_session(session, 4010)
            .expect("complete");
        let recents = repository.recent_game_ids(15).expect("recents");
        assert_eq!(recents, vec![played, installed]);
        assert!(!recents.contains(&unplayed));
    }

    #[test]
    fn home_recents_are_distinct_and_ordered_by_latest_play_not_total_time() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        let first = seed_game(&mut repository, "First", "steam", "101");
        let second = seed_game(&mut repository, "Second", "steam", "102");
        let third = seed_game(&mut repository, "Unplayed", "steam", "103");
        let source = SourceId::new("steam").expect("source");
        for (id, time) in [(first, 100), (second, 200), (first, 300)] {
            let session = repository
                .begin_play_session(id, &source, time, SessionTrackingMethod::SourceRuntime)
                .expect("begin");
            repository
                .complete_play_session(session, time + 10)
                .expect("complete");
        }
        assert_eq!(
            repository.recent_game_ids(15).expect("recents"),
            vec![first, second]
        );
        assert_eq!(repository.recent_game_ids(1).expect("limit"), vec![first]);
        assert!(
            !repository
                .recent_game_ids(15)
                .expect("recents")
                .contains(&third)
        );
        repository
            .synchronize_source_snapshot(&source, &[], &[])
            .expect("uninstall");
        assert!(
            repository
                .recent_game_ids(15)
                .expect("installed only")
                .is_empty()
        );
    }

    #[test]
    fn open_sessions_surface_as_live_without_entering_completed_totals() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        let game_id = seed_game(&mut repository, "Live Game", "steam", "10");
        repository
            .begin_play_session(
                game_id,
                &SourceId::new("steam").expect("source"),
                100,
                SessionTrackingMethod::SourceRuntime,
            )
            .expect("session");

        let overview = repository.activity_overview(8, 4).expect("overview");
        assert_eq!(overview.observed_playtime().get(), 0);
        assert_eq!(overview.completed_sessions(), 0);
        assert_eq!(overview.active_sessions().len(), 1);
        assert_eq!(overview.active_sessions()[0].title().as_str(), "Live Game");
        assert_eq!(
            overview.active_sessions()[0].session().tracking_method(),
            SessionTrackingMethod::SourceRuntime
        );
    }

    #[test]
    fn observed_time_ranges_clip_sessions_at_day_boundaries() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        let game_id = seed_game(&mut repository, "Crosses midnight", "steam", "88");
        let source_id = SourceId::new("steam").expect("source");
        let session = repository
            .begin_play_session(
                game_id,
                &source_id,
                100,
                SessionTrackingMethod::SourceRuntime,
            )
            .expect("start");
        repository
            .complete_play_session(session, 200)
            .expect("complete");
        let unfinished = repository
            .begin_play_session(
                game_id,
                &source_id,
                210,
                SessionTrackingMethod::SourceRuntime,
            )
            .expect("start open");
        let totals = repository
            .observed_seconds_in_ranges(&[(0, 130), (130, 160), (160, 250), (250, 300), (140, 140)])
            .expect("daily observations");
        assert_eq!(totals, vec![30, 30, 40, 0, 0]);
        repository
            .interrupt_play_session(unfinished)
            .expect("interrupt open");
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
    fn interrupted_sessions_preserve_only_confirmed_checkpoint_time() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        let game_id = repository
            .upsert_discovered_game(&discovered("steam", "10", "Interrupted"))
            .expect("game");
        repository
            .begin_play_session(
                game_id,
                &SourceId::new("steam").expect("source"),
                100,
                SessionTrackingMethod::SourceRuntime,
            )
            .expect("session");
        assert_eq!(
            repository
                .checkpoint_open_play_sessions(145)
                .expect("checkpoint"),
            1
        );

        assert_eq!(
            repository
                .interrupt_open_play_sessions()
                .expect("interrupt"),
            1
        );
        let overview = repository.activity_overview(8, 4).expect("overview");
        assert_eq!(overview.observed_playtime().get(), 45);
        assert_eq!(overview.completed_sessions(), 0);
        assert_eq!(overview.played_games(), 1);
        assert_eq!(overview.recent_sessions().len(), 1);
        assert_eq!(
            overview.recent_sessions()[0].session().state(),
            PlaySessionState::Interrupted
        );
        assert_eq!(
            overview.recent_sessions()[0]
                .session()
                .duration()
                .expect("recovered duration")
                .get(),
            45
        );
    }

    #[test]
    fn interrupted_session_without_checkpoint_has_unknown_duration() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        let game_id = repository
            .upsert_discovered_game(&discovered("steam", "10", "Lost"))
            .expect("game");
        let session = repository
            .begin_play_session(
                game_id,
                &SourceId::new("steam").expect("source"),
                100,
                SessionTrackingMethod::SourceRuntime,
            )
            .expect("session");
        repository
            .interrupt_play_session(session)
            .expect("interrupt");
        // Shown in game history as unknown; never counted as played time.
        let history = repository.game_activity_history(game_id).expect("history");
        assert_eq!(history.sessions.len(), 1);
        assert_eq!(history.sessions[0].ended_at(), None);
        assert_eq!(history.sessions[0].duration(), None);
        let overview = repository.activity_overview(8, 4).expect("overview");
        assert_eq!(overview.observed_playtime().get(), 0);
        assert_eq!(overview.played_games(), 0);
        assert!(overview.recent_sessions().is_empty());
    }

    #[test]
    fn per_game_history_is_complete_ordered_and_source_totals_remain_separate() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        let game = repository
            .upsert_discovered_game(&discovered("steam", "10", "History game"))
            .expect("game");
        let other = repository
            .upsert_discovered_game(&discovered("steam", "20", "Other game"))
            .expect("other");
        let steam = SourceId::new("steam").expect("source");
        for (game_id, start, end) in [
            (game, 100, 160),
            (game, 200, 380),
            (other, 250, 255),
            (game, 400, 445),
        ] {
            let session = repository
                .begin_play_session(game_id, &steam, start, SessionTrackingMethod::SourceRuntime)
                .expect("begin");
            repository
                .complete_play_session(session, end)
                .expect("complete");
        }
        repository
            .upsert_source_lifetime_playtime(&SourceLifetimePlaytime::new(
                game,
                steam.clone(),
                PlaytimeSeconds::new(30_000).unwrap(),
                500,
            ))
            .expect("report");
        let history = repository.game_activity_history(game).expect("history");
        assert_eq!(history.sessions.len(), 3);
        assert_eq!(
            history
                .sessions
                .iter()
                .map(|row| row.started_at())
                .collect::<Vec<_>>(),
            [400, 200, 100]
        );
        assert_eq!(
            history
                .sessions
                .iter()
                .map(|row| row.duration().unwrap().get())
                .sum::<i64>(),
            285
        );
        assert_eq!(history.reported.len(), 1);
        assert_eq!(history.reported[0].0, steam);
        assert_eq!(history.reported[0].1.get(), 30_000);
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

        let source_id = SourceId::new("steam").expect("source");
        let session = repository
            .begin_play_session(
                game_id,
                &source_id,
                100,
                SessionTrackingMethod::SourceRuntime,
            )
            .expect("observed session");
        repository
            .complete_play_session(session, 160)
            .expect("completed");
        let overview = repository.activity_overview(8, 4).expect("overview");
        assert_eq!(overview.observed_playtime().get(), 60);
        assert_eq!(overview.completed_sessions(), 1);
        // 50,000 provider seconds are not added to the 60 observed seconds.
        assert_eq!(overview.reported_playtime().get(), 50_000);
        assert_eq!(overview.reported_games().len(), 1);
        assert_eq!(overview.reported_games()[0].title().as_str(), "Reported");
        assert_eq!(overview.reported_games()[0].source_id().as_str(), "steam");
    }

    #[test]
    fn library_sort_metrics_keep_reported_and_observed_separate() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        let game_id = repository
            .upsert_discovered_game(&discovered("steam", "10", "Test game"))
            .expect("game");
        // One imported game can legitimately have multiple provider entries.
        repository.connection.execute(
            "INSERT INTO game_sources(game_id, source_id, external_id, first_seen_at, last_seen_at) \
             VALUES (?1, 'heroic', 'test-game', 0, 0)",
            [game_id.get()],
        ).expect("additional source");
        for (source, seconds) in [("steam", 7200), ("heroic", 1800)] {
            repository
                .upsert_source_lifetime_playtime(&SourceLifetimePlaytime::new(
                    game_id,
                    SourceId::new(source).unwrap(),
                    PlaytimeSeconds::new(seconds).unwrap(),
                    123,
                ))
                .expect("provider report");
        }
        let first = repository
            .begin_play_session(
                game_id,
                &SourceId::new("steam").unwrap(),
                100,
                SessionTrackingMethod::SourceRuntime,
            )
            .expect("session");
        repository
            .complete_play_session(first, 160)
            .expect("complete session");
        let rows = repository.library_sort_metrics().expect("sort metrics");
        assert_eq!(
            rows.len(),
            1,
            "multiple providers must not duplicate a game"
        );
        let item = rows[0];
        assert_eq!(item.game_id, game_id);
        assert_eq!(item.last_played_at, Some(100));
        assert_eq!(item.observed_seconds, 60);
        assert_eq!(item.reported_seconds, Some(7200));
        assert_eq!(
            item.time_played_seconds(),
            7200,
            "never sum source and observed time"
        );

        let no_report_id = repository
            .upsert_discovered_game(&discovered("heroic", "20", "Without provider report"))
            .expect("other game");
        let fallback = repository
            .begin_play_session(
                no_report_id,
                &SourceId::new("heroic").unwrap(),
                200,
                SessionTrackingMethod::SourceRuntime,
            )
            .expect("fallback session");
        repository
            .complete_play_session(fallback, 260)
            .expect("complete fallback");
        let rows = repository.library_sort_metrics().expect("sort metrics");
        let fallback = rows
            .iter()
            .find(|item| item.game_id == no_report_id)
            .unwrap();
        assert_eq!(fallback.reported_seconds, None);
        assert_eq!(fallback.time_played_seconds(), 60);
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
        assert!(
            overview.top_games().is_empty(),
            "uninstalled games are not Activity cover choices"
        );
    }

    #[test]
    fn activity_top_games_skip_uninstalled_history_before_applying_limit() {
        let mut repository = SqliteLibraryRepository::open_in_memory().expect("repository");
        let retired = seed_game(&mut repository, "Retired", "steam", "100");
        let installed = seed_game(&mut repository, "Installed", "heroic", "200");
        for (game_id, source, start, end) in [
            (retired, "steam", 100, 1_000),
            (installed, "heroic", 1_100, 1_110),
        ] {
            let source = SourceId::new(source).expect("source");
            let session = repository
                .begin_play_session(
                    game_id,
                    &source,
                    start,
                    SessionTrackingMethod::SourceRuntime,
                )
                .expect("begin session");
            repository
                .complete_play_session(session, end)
                .expect("end session");
        }
        repository
            .synchronize_source_snapshot(&SourceId::new("steam").expect("source"), &[], &[])
            .expect("uninstall retired game");

        let overview = repository.activity_overview(6, 1).expect("overview");
        assert_eq!(overview.top_games().len(), 1);
        assert_eq!(overview.top_games()[0].game_id(), installed);
        assert_eq!(
            overview.observed_playtime().get(),
            910,
            "the global observed total retains the retired game's history"
        );
    }
}

#[cfg(test)]
mod rom_import_tests {
    use super::*;
    use crate::services::rom_import::{RomCandidate, RomFingerprint, RomSetup, RomSystem};

    fn candidate(path: &str, title: &str, hash: &str, verified: bool) -> RomCandidate {
        RomCandidate {
            path: path.into(),
            system: RomSystem::GameBoyAdvance,
            title: title.into(),
            selected: true,
            fingerprint: Some(RomFingerprint {
                sha1: "a".repeat(40),
                sha256: hash.repeat(64),
                size: 192,
            }),
            catalog: verified.then(|| "GBA catalog".into()),
            status: String::new(),
        }
    }

    #[test]
    fn bundled_hash_titles_upgrade_old_filenames_but_preserve_verified_and_unknown_games() {
        let mut repository = SqliteLibraryRepository::open_in_memory().unwrap();
        let catalogs = crate::services::rom_import::RomCatalogs::bundled().unwrap();
        let catalog = catalogs.get(RomSystem::GameBoyAdvance);
        let mut recognized = candidate("/roms/completely-wrong.gba", "Wrong filename", "1", false);
        recognized.fingerprint.as_mut().unwrap().sha1 =
            "f3ae088181bf583e55daf962a92bb46f4f1d07b7".into();
        recognized.fingerprint.as_mut().unwrap().size = 16_777_216;
        let mut verified = recognized.clone();
        verified.path = "/roms/custom.gba".into();
        verified.title = "Previously verified title".into();
        verified.catalog = Some("Custom XML DAT".into());
        verified.fingerprint.as_mut().unwrap().sha256 = "2".repeat(64);
        let unknown = candidate("/roms/homebrew.gba", "Homebrew", "3", false);
        // Each system is checked against its own catalog only.
        let mut red = candidate("/roms/red.gb", "red", "5", false);
        red.system = RomSystem::GameBoy;
        red.fingerprint.as_mut().unwrap().sha1 = "ea9bcae617fdf159b045185467ae58b2e4a48b9a".into();
        red.fingerprint.as_mut().unwrap().size = 1_048_576;
        let mut misfiled = red.clone();
        misfiled.path = "/roms/red-as-color.gbc".into();
        misfiled.title = "red as color".into();
        misfiled.system = RomSystem::GameBoyColor;
        misfiled.fingerprint.as_mut().unwrap().sha256 = "6".repeat(64);
        let mut wrong_size = recognized.clone();
        wrong_size.path = "/roms/wrong-size.gba".into();
        wrong_size.title = "Size mismatch".into();
        wrong_size.fingerprint.as_mut().unwrap().sha256 = "4".repeat(64);
        wrong_size.fingerprint.as_mut().unwrap().size = 192;
        repository
            .save_rom_import(
                &RomSetup::default(),
                &[recognized, verified, unknown, wrong_size, red, misfiled],
            )
            .unwrap();
        let before = repository.list_games().unwrap();
        assert_eq!(repository.recognize_saved_rom_titles(&catalogs).unwrap(), 2);
        let after = repository.list_games().unwrap();
        assert_eq!(
            before.iter().map(|g| g.game().id()).collect::<Vec<_>>(),
            after.iter().map(|g| g.game().id()).collect::<Vec<_>>()
        );
        let titles = after
            .iter()
            .map(|g| g.game().title().as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            titles,
            vec![
                "Pokemon - Emerald Version (USA, Europe)",
                "Previously verified title",
                "Homebrew",
                "Size mismatch",
                "Pokemon - Red Version (USA, Europe) (SGB Enhanced)",
                "red as color"
            ]
        );
        assert_eq!(repository.recognize_saved_rom_titles(&catalogs).unwrap(), 0);
        let provenance: String = repository
            .connection
            .query_row(
                "SELECT catalog_name FROM rom_files WHERE path = ?1",
                [b"/roms/completely-wrong.gba".as_slice()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(provenance, catalog.name);
    }

    #[test]
    fn content_identity_survives_duplicates_and_filename_fallbacks() {
        let mut repository = SqliteLibraryRepository::open_in_memory().unwrap();
        let setup = RomSetup {
            completed: true,
            folder: Some("/roms".into()),
            catalog_path: None,
        };
        repository
            .save_rom_import(
                &setup,
                &[
                    candidate("/roms/first.gba", "Catalog (USA)", "1", true),
                    candidate("/roms/copy.gba", "Wrong filename", "1", false),
                    candidate("/roms/other.gba", "Catalog (USA)", "2", true),
                ],
            )
            .unwrap();
        let games = repository.list_games().unwrap();
        assert_eq!(games.len(), 2);
        assert!(
            games
                .iter()
                .all(|g| g.game().title().as_str() == "Catalog (USA)")
        );
        let id = games[0].game().id();
        repository
            .save_rom_import(
                &setup,
                &[candidate("/roms/copy.gba", "Wrong filename", "1", false)],
            )
            .unwrap();
        assert_eq!(repository.list_games().unwrap()[0].game().id(), id);
        assert!(repository.rom_setup().unwrap().completed);
    }

    #[test]
    fn deleted_files_retire_their_games_and_returning_files_restore_them() {
        let mut repository = SqliteLibraryRepository::open_in_memory().unwrap();
        let setup = RomSetup::default();
        repository
            .save_rom_import(
                &setup,
                &[
                    candidate("/roms/kept.gba", "Kept", "1", false),
                    candidate("/roms/gone.gba", "Gone (USA)", "2", true),
                    candidate("/roms/copy-a.gba", "Copied", "3", false),
                    candidate("/roms/copy-b.gba", "Copied", "3", false),
                    candidate("/elsewhere/outside.gba", "Outside", "4", false),
                ],
            )
            .unwrap();
        let ids = |repository: &SqliteLibraryRepository| {
            repository
                .list_games()
                .unwrap()
                .iter()
                .map(|g| (g.game().title().as_str().to_owned(), g.game().id()))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        let before = ids(&repository);
        let seen = |paths: &[&str]| {
            paths
                .iter()
                .map(std::path::PathBuf::from)
                .collect::<std::collections::HashSet<_>>()
        };
        let folder = std::path::Path::new("/roms");
        // Nothing of the folder found: a missing drive, not deleted games.
        assert_eq!(
            repository
                .retire_missing_rom_files(folder, &seen(&[]))
                .unwrap(),
            0
        );
        assert_eq!(ids(&repository), before);
        // One copy left keeps its game; the folder outside is not checked.
        assert_eq!(
            repository
                .retire_missing_rom_files(folder, &seen(&["/roms/kept.gba", "/roms/copy-b.gba"]))
                .unwrap(),
            1
        );
        let after = ids(&repository);
        assert!(!after.contains_key("Gone (USA)"));
        assert_eq!(after.len(), 3);
        assert_eq!(
            repository.retired_games().unwrap()[0].game().id(),
            before["Gone (USA)"]
        );
        // The file comes back, under any name: same game, same catalog title.
        repository
            .add_rom_files(&[candidate("/roms/renamed.gba", "renamed", "2", false)])
            .unwrap();
        assert_eq!(
            ids(&repository),
            before
                .iter()
                .map(|(title, id)| (title.clone(), *id))
                .collect()
        );
        assert!(repository.retired_games().unwrap().is_empty());
    }

    #[test]
    fn onboarding_progress_resumes_without_affecting_imports() {
        let repository = SqliteLibraryRepository::open_in_memory().unwrap();
        assert_eq!(repository.onboarding_progress().unwrap(), (0, false));
        repository.save_onboarding_progress(2, false).unwrap();
        assert_eq!(repository.onboarding_progress().unwrap(), (2, false));
        assert!(repository.save_onboarding_progress(2, true).is_err());
        assert_eq!(repository.onboarding_progress().unwrap(), (2, false));
        repository.save_onboarding_progress(3, true).unwrap();
        assert_eq!(repository.onboarding_progress().unwrap(), (3, true));
        assert!(repository.save_onboarding_progress(4, false).is_err());
        assert_eq!(repository.game_count().unwrap(), 0);
    }

    #[test]
    fn invalid_review_rolls_back_games_files_and_setup() {
        let mut repository = SqliteLibraryRepository::open_in_memory().unwrap();
        let setup = RomSetup {
            completed: true,
            ..Default::default()
        };
        let mut invalid = candidate("/bad.gba", "Bad", "2", false);
        invalid.fingerprint = None;
        assert!(
            repository
                .save_rom_import(&setup, &[candidate("/one.gba", "One", "1", true), invalid])
                .is_err()
        );
        assert_eq!(repository.game_count().unwrap(), 0);
        assert!(!repository.rom_setup().unwrap().completed);
        let count: i64 = repository
            .connection
            .query_row("SELECT COUNT(*) FROM rom_files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
