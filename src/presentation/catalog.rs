//! The library as Home, Library and Activity show it: one shared list whose
//! indices never change while Horizon runs.
//!
//! Every page keys its cards, artwork and Playing state by catalog index, so
//! a library reload must not move a game. New games are appended; a game
//! that's gone keeps its slot, marked absent, and every page skips it.

use std::{cell::RefCell, ops::Index, rc::Rc};

use crate::domain::{GameId, LibraryGame};

/// The one catalog the pages share.
pub type SharedCatalog = Rc<RefCell<Catalog>>;

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    games: Vec<LibraryGame>,
    present: Vec<bool>,
}

/// What a reload changed, by catalog index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogChanges {
    /// New slots, in order, at the end.
    pub added: Vec<usize>,
    /// Still (or again) present, with a new title or sources.
    pub updated: Vec<usize>,
    pub removed: Vec<usize>,
}

impl CatalogChanges {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.updated.is_empty() && self.removed.is_empty()
    }
}

impl Catalog {
    pub fn new(games: Vec<LibraryGame>) -> Self {
        let present = vec![true; games.len()];
        Self { games, present }
    }

    pub fn shared(games: Vec<LibraryGame>) -> SharedCatalog {
        Rc::new(RefCell::new(Self::new(games)))
    }

    /// The games in the library now.
    pub fn len(&self) -> usize {
        self.present.iter().filter(|present| **present).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A game in the library now.
    pub fn get(&self, index: usize) -> Option<&LibraryGame> {
        self.games
            .get(index)
            .filter(|_| self.present.get(index).copied().unwrap_or(false))
    }

    /// The games in the library now, with their indices.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &LibraryGame)> {
        self.games
            .iter()
            .enumerate()
            .filter(|(index, _)| self.present[*index])
    }

    /// Where a game in the library now is.
    pub fn position(&self, game_id: GameId) -> Option<usize> {
        self.iter()
            .find(|(_, game)| game.game().id() == game_id)
            .map(|(index, _)| index)
    }

    /// Take in a fresh read of the library, keeping every index.
    pub fn merge(&mut self, fresh: Vec<LibraryGame>) -> CatalogChanges {
        let mut changes = CatalogChanges::default();
        let mut seen = vec![false; self.games.len()];
        for game in fresh {
            let slot = self
                .games
                .iter()
                .position(|known| known.game().id() == game.game().id());
            match slot {
                Some(index) => {
                    seen[index] = true;
                    if !self.present[index] || self.games[index] != game {
                        self.games[index] = game;
                        self.present[index] = true;
                        changes.updated.push(index);
                    }
                }
                None => {
                    changes.added.push(self.games.len());
                    self.games.push(game);
                    self.present.push(true);
                }
            }
        }
        for (index, seen) in seen.into_iter().enumerate() {
            if !seen && self.present[index] {
                self.present[index] = false;
                changes.removed.push(index);
            }
        }
        changes
    }
}

/// Any slot's game, present or not. Pages index only slots they got from
/// `iter`/`position` or a change list.
impl Index<usize> for Catalog {
    type Output = LibraryGame;

    fn index(&self, index: usize) -> &LibraryGame {
        &self.games[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ExternalGameId, Game, GameTitle, SourceGameRef, SourceId};

    fn game(id: i64, title: &str) -> LibraryGame {
        LibraryGame::new(
            Game::new(GameId::new(id).unwrap(), GameTitle::new(title).unwrap()),
            vec![SourceGameRef::new(
                SourceId::new("flatpak").unwrap(),
                ExternalGameId::new(format!("org.example.G{id}")).unwrap(),
            )],
        )
    }

    #[test]
    fn a_reload_keeps_every_index() {
        let mut catalog = Catalog::new(vec![game(1, "One"), game(2, "Two"), game(3, "Three")]);
        let changes = catalog.merge(vec![game(3, "Three!"), game(4, "Four"), game(1, "One")]);
        assert_eq!(
            changes,
            CatalogChanges {
                added: vec![3],
                updated: vec![2],
                removed: vec![1],
            }
        );
        assert_eq!(catalog[2].game().title().as_str(), "Three!");
        assert_eq!(catalog.get(3).map(|g| g.game().id().get()), Some(4));
        assert!(catalog.get(1).is_none());
        assert_eq!(catalog.len(), 3);
        assert_eq!(catalog.position(GameId::new(2).unwrap()), None);
        assert_eq!(
            catalog.iter().map(|(index, _)| index).collect::<Vec<_>>(),
            vec![0, 2, 3]
        );
    }

    #[test]
    fn a_game_that_returns_gets_its_slot_back() {
        let mut catalog = Catalog::new(vec![game(1, "One"), game(2, "Two")]);
        catalog.merge(vec![game(1, "One")]);
        let changes = catalog.merge(vec![game(1, "One"), game(2, "Two")]);
        assert_eq!(changes.updated, vec![1]);
        assert_eq!(catalog.position(GameId::new(2).unwrap()), Some(1));
    }

    #[test]
    fn the_same_library_changes_nothing() {
        let mut catalog = Catalog::new(vec![game(1, "One")]);
        assert!(catalog.merge(vec![game(1, "One")]).is_empty());
    }
}
