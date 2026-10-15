//! Review/import coordination; GBA file formats belong to the source adapter.
pub use crate::sources::gba::recognition::{
    GbaCatalog, RomCandidate, RomFingerprint, RomImportError, RomScan, fingerprint, scan,
};
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct RomSetup {
    pub completed: bool,
    pub folder: Option<PathBuf>,
    pub catalog_path: Option<PathBuf>,
}

/// Recheck all reviewed bytes before a transaction, with no partial writes.
pub fn revalidate(candidates: &[RomCandidate]) -> Result<(), RomImportError> {
    for candidate in candidates.iter().filter(|c| c.selected) {
        if candidate.fingerprint.as_ref() != Some(&fingerprint(&candidate.path)?) {
            return Err(RomImportError::Changed);
        }
    }
    Ok(())
}
