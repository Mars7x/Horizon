//! Local game management. Every operation is called from a worker thread.
use std::path::{Path, PathBuf};

use crate::{
    domain::{GameId, LibraryGame},
    persistence::{PersistenceError, SqliteLibraryRepository},
    services::library::LibraryRepository,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameRom {
    pub path: PathBuf,
    pub sha256: String,
    pub size: u64,
    pub catalog: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GameDetails {
    pub game: LibraryGame,
    pub roms: Vec<GameRom>,
}

#[derive(Debug, thiserror::Error)]
pub enum ManagementError {
    #[error("This game is no longer in the library.")]
    MissingGame,
    #[error("The imported ROM files changed. Reopen Game properties before uninstalling.")]
    Changed,
    #[error("A ROM path contains a link or is not a regular file. File removal stopped.")]
    UnsafePath,
    #[error("The ROM file could not be verified: {0}")]
    Verify(#[from] crate::services::rom_import::RomImportError),
    #[error("The ROM file could not be removed: {0}. Check folder access and try again.")]
    Io(#[from] std::io::Error),
    #[error("The game information could not be saved or read: {0}")]
    Persistence(#[from] PersistenceError),
}

pub fn details(database: &Path, id: GameId) -> Result<GameDetails, ManagementError> {
    let repository = SqliteLibraryRepository::open(database)?;
    let game = repository
        .list_games()?
        .into_iter()
        .find(|game| game.game().id() == id)
        .ok_or(ManagementError::MissingGame)?;
    Ok(GameDetails {
        game,
        roms: repository.game_rom_records(id)?,
    })
}

pub fn set_hidden(
    database: &Path,
    id: GameId,
    hidden: bool,
) -> Result<Vec<LibraryGame>, ManagementError> {
    let mut repository = SqliteLibraryRepository::open(database)?;
    repository.set_game_hidden(id, hidden)?;
    Ok(repository.list_games()?)
}

/// A partial filesystem failure must still publish the records actually removed.
pub struct UninstallOutcome {
    pub games: Vec<LibraryGame>,
    pub error: Option<ManagementError>,
}

pub fn uninstall(
    database: &Path,
    id: GameId,
    expected: &[GameRom],
) -> Result<UninstallOutcome, ManagementError> {
    let mut repository = SqliteLibraryRepository::open(database)?;
    let records = repository.game_rom_records(id)?;
    if records.is_empty() || records != expected {
        return Err(ManagementError::Changed);
    }
    // Validate the entire plan before removing anything. Missing files may be
    // forgotten; changed content and links are always preserved.
    let mut snapshots = Vec::new();
    for rom in &records {
        reject_link_components(&rom.path)?;
        let before = match std::fs::symlink_metadata(&rom.path) {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => return Err(ManagementError::UnsafePath),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                snapshots.push(None);
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let found = crate::services::rom_import::fingerprint(&rom.path)?;
        let after = std::fs::symlink_metadata(&rom.path)?;
        if found.sha256 != rom.sha256 || found.size != rom.size || !same_file(&before, &after) {
            return Err(ManagementError::Changed);
        }
        snapshots.push(Some(after));
    }
    let mut removed = Vec::new();
    let mut error = None;
    for (rom, snapshot) in records.into_iter().zip(snapshots) {
        let result = (|| {
            reject_link_components(&rom.path)?;
            match (std::fs::symlink_metadata(&rom.path), snapshot) {
                (Err(error), _) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                (Ok(current), Some(expected)) if same_file(&current, &expected) => {
                    std::fs::remove_file(&rom.path).map_err(ManagementError::from)
                }
                (Err(error), _) => Err(error.into()),
                _ => Err(ManagementError::Changed),
            }
        })();
        match result {
            Ok(()) => removed.push(rom),
            Err(failure) => {
                error = Some(failure);
                break;
            }
        }
    }
    repository.forget_removed_game_roms(id, &removed)?;
    Ok(UninstallOutcome {
        games: repository.list_games()?,
        error,
    })
}

fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    b.is_file()
        && a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.len() == b.len()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}

fn reject_link_components(path: &Path) -> Result<(), ManagementError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(ManagementError::UnsafePath);
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ManagementError::UnsafePath);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{
        activity::ActivityRepository,
        rom_import::{RomCandidate, RomSetup, RomSystem},
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture {
        root: PathBuf,
        db: PathBuf,
        id: GameId,
        candidates: Vec<RomCandidate>,
    }
    impl Fixture {
        fn new(copies: usize) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "horizon-game-management-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            let db = root.join("library.sqlite3");
            let mut repository = SqliteLibraryRepository::open(&db).unwrap();
            let candidates = (0..copies)
                .map(|index| {
                    let path = root.join(format!("copy-{index}.gba"));
                    std::fs::write(&path, crate::platform::gba_core::test_rom()).unwrap();
                    RomCandidate {
                        fingerprint: Some(crate::services::rom_import::fingerprint(&path).unwrap()),
                        path,
                        system: RomSystem::GameBoyAdvance,
                        title: "Test game".into(),
                        catalog: None,
                        status: String::new(),
                        selected: true,
                    }
                })
                .collect::<Vec<_>>();
            repository
                .save_rom_import(&RomSetup::default(), &candidates)
                .unwrap();
            let id = repository.list_games().unwrap()[0].game().id();
            Self {
                root,
                db,
                id,
                candidates,
            }
        }
        fn plan(&self) -> Vec<GameRom> {
            details(&self.db, self.id).unwrap().roms
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn hidden_preference_survives_restart_rescan_and_unhide_without_changing_identity() {
        let fixture = Fixture::new(1);
        set_hidden(&fixture.db, fixture.id, true).unwrap();
        let mut repository = SqliteLibraryRepository::open(&fixture.db).unwrap();
        repository.add_rom_files(&fixture.candidates).unwrap();
        let games = repository.list_games().unwrap();
        assert_eq!(games[0].game().id(), fixture.id);
        assert!(games[0].hidden());
        assert!(fixture.candidates[0].path.exists());
        assert!(!set_hidden(&fixture.db, fixture.id, false).unwrap()[0].hidden());
    }

    #[test]
    fn hiding_excludes_recent_cover_choices_without_erasing_recorded_activity() {
        let fixture = Fixture::new(1);
        let mut repository = SqliteLibraryRepository::open(&fixture.db).unwrap();
        let session = repository
            .begin_play_session(
                fixture.id,
                &crate::domain::SourceId::new("gba").unwrap(),
                100,
                crate::domain::SessionTrackingMethod::SourceRuntime,
            )
            .unwrap();
        repository.complete_play_session(session, 160).unwrap();
        assert_eq!(repository.recent_game_ids(15).unwrap(), vec![fixture.id]);
        set_hidden(&fixture.db, fixture.id, true).unwrap();
        assert!(repository.recent_game_ids(15).unwrap().is_empty());
        assert_eq!(
            repository
                .game_activity_history(fixture.id)
                .unwrap()
                .sessions
                .len(),
            1
        );
        set_hidden(&fixture.db, fixture.id, false).unwrap();
        assert_eq!(repository.recent_game_ids(15).unwrap(), vec![fixture.id]);
    }

    #[test]
    fn authoritative_reconciliation_preserves_visibility_and_reimport_identity() {
        let fixture = Fixture::new(1);
        set_hidden(&fixture.db, fixture.id, true).unwrap();
        let mut repository = SqliteLibraryRepository::open(&fixture.db).unwrap();
        repository
            .synchronize_source_snapshot(&crate::domain::SourceId::new("gba").unwrap(), &[], &[])
            .unwrap();
        assert!(repository.list_games().unwrap().is_empty());
        repository.add_rom_files(&fixture.candidates).unwrap();
        let games = repository.list_games().unwrap();
        assert_eq!(games[0].game().id(), fixture.id);
        assert!(games[0].hidden());
    }

    #[test]
    fn uninstall_removes_only_confirmed_rom_copies_and_preserves_history_saves_and_identity() {
        let fixture = Fixture::new(2);
        let mut repository = SqliteLibraryRepository::open(&fixture.db).unwrap();
        let session = repository
            .begin_play_session(
                fixture.id,
                &crate::domain::SourceId::new("gba").unwrap(),
                100,
                crate::domain::SessionTrackingMethod::SourceRuntime,
            )
            .unwrap();
        repository.complete_play_session(session, 160).unwrap();
        let retained = ["game.sav", "slot.state", "screenshot.jxl", "unrelated.gba"];
        for file in retained {
            std::fs::write(fixture.root.join(file), b"keep").unwrap();
        }
        let result = uninstall(&fixture.db, fixture.id, &fixture.plan()).unwrap();
        assert!(result.error.is_none());
        assert!(result.games.is_empty());
        for candidate in &fixture.candidates {
            assert!(!candidate.path.exists());
        }
        for file in retained {
            assert_eq!(std::fs::read(fixture.root.join(file)).unwrap(), b"keep");
        }
        assert_eq!(
            repository
                .game_activity_history(fixture.id)
                .unwrap()
                .sessions
                .len(),
            1
        );
        std::fs::write(
            &fixture.candidates[0].path,
            crate::platform::gba_core::test_rom(),
        )
        .unwrap();
        repository.add_rom_files(&fixture.candidates[..1]).unwrap();
        assert_eq!(repository.list_games().unwrap()[0].game().id(), fixture.id);
    }

    #[test]
    fn changed_content_prevents_deleting_any_copy() {
        let fixture = Fixture::new(2);
        let plan = fixture.plan();
        let mut bytes = crate::platform::gba_core::test_rom();
        bytes[200] ^= 1;
        std::fs::write(&fixture.candidates[1].path, &bytes).unwrap();
        assert!(matches!(
            uninstall(&fixture.db, fixture.id, &plan),
            Err(ManagementError::Changed)
        ));
        for candidate in &fixture.candidates {
            assert!(candidate.path.exists());
        }
    }

    #[test]
    fn new_copy_after_confirmation_requires_a_new_plan() {
        let fixture = Fixture::new(1);
        let plan = fixture.plan();
        let mut new = fixture.candidates[0].clone();
        new.path = fixture.root.join("new-copy.gba");
        std::fs::copy(&fixture.candidates[0].path, &new.path).unwrap();
        SqliteLibraryRepository::open(&fixture.db)
            .unwrap()
            .add_rom_files(&[new.clone()])
            .unwrap();
        assert!(matches!(
            uninstall(&fixture.db, fixture.id, &plan),
            Err(ManagementError::Changed)
        ));
        assert!(new.path.exists());
        assert!(fixture.candidates[0].path.exists());
    }

    #[test]
    fn symlink_file_is_preserved_along_with_its_target() {
        let fixture = Fixture::new(1);
        let plan = fixture.plan();
        let target = fixture.root.join("target.gba");
        std::fs::rename(&fixture.candidates[0].path, &target).unwrap();
        std::os::unix::fs::symlink(&target, &fixture.candidates[0].path).unwrap();
        assert!(matches!(
            uninstall(&fixture.db, fixture.id, &plan),
            Err(ManagementError::UnsafePath)
        ));
        assert!(target.exists());
        assert!(
            std::fs::symlink_metadata(&fixture.candidates[0].path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn missing_rom_can_be_uninstalled_without_touching_its_replacement_path() {
        let fixture = Fixture::new(1);
        let plan = fixture.plan();
        std::fs::remove_file(&fixture.candidates[0].path).unwrap();
        let result = uninstall(&fixture.db, fixture.id, &plan).unwrap();
        assert!(result.error.is_none());
        assert!(result.games.is_empty());
    }
}
