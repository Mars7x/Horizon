//! Folder import coordination; cartridge formats belong to the source adapter.
pub use crate::sources::gba::recognition::{
    RomCandidate, RomCatalog, RomCatalogs, RomFingerprint, RomImportError, RomScan, RomSystem,
    fingerprint, scan,
};
use std::path::PathBuf;

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
pub fn scan_folder(
    folder: &std::path::Path,
    catalog: Option<RomCatalog>,
) -> Result<RomScan, RomImportError> {
    let mut catalogs = RomCatalogs::bundled()?;
    if let Some(catalog) = catalog {
        catalogs = catalogs.with(catalog);
    }
    let mut result = scan(folder, Some(&catalogs))?;
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
}
