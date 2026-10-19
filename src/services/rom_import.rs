//! Folder import coordination; cartridge formats belong to the source adapter.
pub use crate::sources::gba::recognition::{
    RomCandidate, RomCatalog, RomCatalogs, RomFingerprint, RomImportError, RomScan, RomSystem,
    fingerprint, scan, scan_skipping,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// A file changed this recently may still be copying; a later refresh takes it.
const SETTLE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default)]
pub struct RomSetup {
    pub completed: bool,
    pub folder: Option<PathBuf>,
    pub catalog_path: Option<PathBuf>,
}

/// Select every readable supported file, retaining exact catalog metadata when
/// available and unverified filename metadata otherwise. Import never asks the
/// user to classify or approve individual files. A chosen catalog replaces
/// the bundled one for its system only.
pub fn scan_folder(folder: &Path, catalog: Option<RomCatalog>) -> Result<RomScan, RomImportError> {
    scan_folder_skipping(folder, catalog, &|_, _| false)
}

/// The chosen folder again: only files not already imported at their
/// current size, and not still being written, are read. Nothing is removed.
pub fn scan_folder_for_new(
    folder: &Path,
    catalog: Option<RomCatalog>,
    imported: &HashSet<(PathBuf, u64)>,
    now: SystemTime,
) -> Result<RomScan, RomImportError> {
    scan_folder_skipping(folder, catalog, &|path, metadata| {
        imported.contains(&(path.to_owned(), metadata.len()))
            || metadata
                .modified()
                .is_ok_and(|modified| now.duration_since(modified).is_ok_and(|age| age < SETTLE))
    })
}

fn scan_folder_skipping(
    folder: &Path,
    catalog: Option<RomCatalog>,
    skip: &dyn Fn(&Path, &std::fs::Metadata) -> bool,
) -> Result<RomScan, RomImportError> {
    let mut catalogs = RomCatalogs::bundled()?;
    if let Some(catalog) = catalog {
        catalogs = catalogs.with(catalog);
    }
    let mut result = scan_skipping(folder, Some(&catalogs), skip)?;
    for candidate in &mut result.candidates {
        candidate.selected = candidate.fingerprint.is_some();
    }
    Ok(result)
}

/// Recheck the selected bytes before a transaction, with no partial writes.
pub fn revalidate(candidates: &[RomCandidate]) -> Result<(), RomImportError> {
    for candidate in candidates.iter().filter(|c| c.selected) {
        if candidate.fingerprint.as_ref() != Some(&fingerprint(&candidate.path)?) {
            return Err(RomImportError::Changed);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{persistence::SqliteLibraryRepository, services::library::LibraryRepository};
    use std::fs;

    #[test]
    fn folder_import_includes_unmatched_files_without_review_and_deduplicates_copies() {
        let dir = std::env::temp_dir().join(format!("horizon-auto-import-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("First.gba"), [0u8; 192]).unwrap();
        fs::write(dir.join("Copy.gba"), [0u8; 192]).unwrap();
        fs::write(dir.join("Second.gba"), [1u8; 192]).unwrap();
        fs::write(dir.join("Broken.gba"), [0u8; 8]).unwrap();
        fs::write(dir.join("notes.txt"), b"not a game").unwrap();
        let scan = scan_folder(&dir, None).unwrap();
        assert_eq!(scan.candidates.iter().filter(|c| c.selected).count(), 3);
        assert_eq!(
            scan.candidates
                .iter()
                .filter(|c| c.fingerprint.is_none())
                .count(),
            1
        );
        assert!(scan.candidates.iter().all(|c| c.catalog.is_none()));
        revalidate(&scan.candidates).unwrap();
        let mut repository = SqliteLibraryRepository::open(dir.join("library.sqlite3")).unwrap();
        let setup = RomSetup {
            completed: true,
            folder: Some(dir.clone()),
            catalog_path: None,
        };
        repository
            .save_rom_import(&setup, &scan.candidates)
            .unwrap();
        assert_eq!(repository.game_count().unwrap(), 2);
        let ids = repository
            .list_games()
            .unwrap()
            .iter()
            .map(|game| game.game().id())
            .collect::<Vec<_>>();
        repository
            .save_rom_import(&setup, &scan.candidates)
            .unwrap();
        assert_eq!(
            repository
                .list_games()
                .unwrap()
                .iter()
                .map(|game| game.game().id())
                .collect::<Vec<_>>(),
            ids
        );
        assert_eq!(repository.rom_setup().unwrap().folder, Some(dir.clone()));
        drop(repository);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn automatic_import_preserves_exact_matches_and_never_guesses_ambiguous_titles() {
        let dir = std::env::temp_dir().join(format!("horizon-auto-catalog-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert!(scan_folder(&dir, None).unwrap().candidates.is_empty());
        let exact = dir.join("Filename.gba");
        let ambiguous = dir.join("Unknown.gba");
        fs::write(&exact, [0u8; 192]).unwrap();
        fs::write(&ambiguous, [1u8; 192]).unwrap();
        let first = fingerprint(&exact).unwrap();
        let second = fingerprint(&ambiguous).unwrap();
        let path = dir.join("catalog.dat");
        fs::write(&path,format!(r#"<datafile><header><name>Nintendo - Game Boy Advance</name></header><game name="Exact Title (Europe)"><rom size="192" sha1="{}"/></game><game name="One (USA)"><rom size="192" sha1="{}"/></game><game name="Two (Japan)"><rom size="192" sha1="{}"/></game></datafile>"#,first.sha1,second.sha1,second.sha1)).unwrap();
        let catalog = RomCatalog::load(&path).unwrap();
        let scan = scan_folder(&dir, Some(catalog)).unwrap();
        assert!(scan.candidates.iter().all(|c| c.selected));
        let exact = scan.candidates.iter().find(|c| c.path == exact).unwrap();
        assert_eq!(exact.title, "Exact Title (Europe)");
        assert!(exact.catalog.is_some());
        let ambiguous = scan
            .candidates
            .iter()
            .find(|c| c.path == ambiguous)
            .unwrap();
        assert_eq!(ambiguous.title, "Unknown");
        assert!(ambiguous.catalog.is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn refresh_adds_new_settled_files_and_retires_deleted_ones() {
        // Process IDs repeat between sandboxed runs: name it by time too, so
        // a folder left by an interrupted run is never reused.
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("horizon-refresh-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("First.gba"), [0u8; 192]).unwrap();
        let mut repository = SqliteLibraryRepository::open(dir.join("library.sqlite3")).unwrap();
        let first = scan_folder(&dir, None).unwrap();
        repository
            .save_rom_import(&RomSetup::default(), &first.candidates)
            .unwrap();
        let settled = SystemTime::now() + SETTLE;
        let imported = repository.imported_rom_files().unwrap();
        assert!(
            scan_folder_for_new(&dir, None, &imported, settled)
                .unwrap()
                .candidates
                .is_empty()
        );
        fs::write(dir.join("Second.gb"), [1u8; 192]).unwrap();
        // Still being copied: left for a later refresh.
        assert!(
            scan_folder_for_new(&dir, None, &imported, SystemTime::now())
                .unwrap()
                .candidates
                .is_empty()
        );
        let settled = SystemTime::now() + SETTLE;
        let added = scan_folder_for_new(&dir, None, &imported, settled).unwrap();
        assert_eq!(added.candidates.len(), 1);
        assert_eq!(added.candidates[0].system, RomSystem::GameBoy);
        let id = repository.list_games().unwrap()[0].game().id();
        repository.add_rom_files(&added.candidates).unwrap();
        let games = repository.list_games().unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].game().id(), id);
        // A refresh never changes the saved setup.
        assert!(!repository.rom_setup().unwrap().completed);
        // Deleted from the folder: the next complete scan retires it.
        let second = games[1].game().id();
        fs::remove_file(dir.join("Second.gb")).unwrap();
        let imported = repository.imported_rom_files().unwrap();
        let scan = scan_folder_for_new(&dir, None, &imported, settled).unwrap();
        assert!(scan.candidates.is_empty());
        let seen = scan.seen.into_iter().collect();
        assert_eq!(repository.retire_missing_rom_files(&dir, &seen).unwrap(), 1);
        assert_eq!(repository.list_games().unwrap().len(), 1);
        // Put back: the same game returns.
        fs::write(dir.join("Second.gb"), [1u8; 192]).unwrap();
        let imported = repository.imported_rom_files().unwrap();
        let settled = SystemTime::now() + SETTLE;
        let scan = scan_folder_for_new(&dir, None, &imported, settled).unwrap();
        repository.add_rom_files(&scan.candidates).unwrap();
        let games = repository.list_games().unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[1].game().id(), second);
        drop(repository);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn large_ds_cartridges_import() {
        // Most DS games are 64-512 MiB, far beyond the GB/GBA 32 MiB limit.
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("horizon-large-ds-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut rom = crate::platform::gba_core::test_ds_rom();
        rom.resize(40 * 1024 * 1024, 0xFF);
        fs::write(dir.join("Large.nds"), &rom).unwrap();
        let scan = scan_folder(&dir, None).unwrap();
        assert_eq!(scan.candidates.len(), 1);
        assert!(scan.candidates[0].selected, "{}", scan.candidates[0].status);
        let mut repository = SqliteLibraryRepository::open(dir.join("library.sqlite3")).unwrap();
        repository
            .save_rom_import(&RomSetup::default(), &scan.candidates)
            .unwrap();
        assert_eq!(repository.list_games().unwrap().len(), 1);
        drop(repository);
        fs::remove_dir_all(dir).unwrap();
    }
}
