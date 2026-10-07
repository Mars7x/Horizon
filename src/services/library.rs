use std::error::Error;

use crate::domain::{ExternalGameId, GameId, GameTitle, LibraryGame, SourceId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredGame {
    source_id: SourceId,
    external_id: ExternalGameId,
    title: GameTitle,
}

impl DiscoveredGame {
    pub fn new(source_id: SourceId, external_id: ExternalGameId, title: GameTitle) -> Self {
        Self {
            source_id,
            external_id,
            title,
        }
    }

    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    pub fn external_id(&self) -> &ExternalGameId {
        &self.external_id
    }

    pub fn title(&self) -> &GameTitle {
        &self.title
    }
}

/// Boundary implemented by persistence adapters.
///
/// Source adapters and presentation code must not issue SQL directly.
/// `SourceImportService` feeds normalized `DiscoveredGame` values through this
/// interface instead. Batch upserts represent one coherent source snapshot and
/// persistence implementations must commit the batch atomically.
pub trait LibraryRepository {
    type Error: Error + 'static;

    fn upsert_discovered_game(&mut self, game: &DiscoveredGame) -> Result<GameId, Self::Error>;
    fn upsert_discovered_games(
        &mut self,
        games: &[DiscoveredGame],
    ) -> Result<Vec<GameId>, Self::Error>;
    fn list_games(&self) -> Result<Vec<LibraryGame>, Self::Error>;
    fn game_count(&self) -> Result<usize, Self::Error>;
}

pub struct LibraryService<R> {
    repository: R,
}

impl<R> LibraryService<R>
where
    R: LibraryRepository,
{
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    pub fn record_discovered_game(&mut self, game: &DiscoveredGame) -> Result<GameId, R::Error> {
        self.repository.upsert_discovered_game(game)
    }

    pub fn record_discovered_games(
        &mut self,
        games: &[DiscoveredGame],
    ) -> Result<Vec<GameId>, R::Error> {
        self.repository.upsert_discovered_games(games)
    }

    pub fn games(&self) -> Result<Vec<LibraryGame>, R::Error> {
        self.repository.list_games()
    }

    pub fn game_count(&self) -> Result<usize, R::Error> {
        self.repository.game_count()
    }

    pub fn into_repository(self) -> R {
        self.repository
    }
}
