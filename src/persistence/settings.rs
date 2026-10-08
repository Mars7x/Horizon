//! Atomic, private on-disk settings for user-supplied provider credentials.
//! The API key is not encrypted; Unix permissions limit access to the account.
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use thiserror::Error;
use crate::domain::settings::ThirdPartySettings;

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error)]
pub enum SettingsStoreError {
    #[error("could not read settings file {path}: {source}")]
    Read { path: PathBuf, #[source] source: io::Error },
    #[error("could not parse settings file {path}: {source}")]
    Parse { path: PathBuf, #[source] source: serde_json::Error },
    #[error("could not serialize third-party preferences: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("could not save settings file {path}: {source}")]
    Write { path: PathBuf, #[source] source: io::Error },
}

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self { Self { path } }

    pub fn load(&self) -> Result<ThirdPartySettings, SettingsStoreError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(ThirdPartySettings::default()),
            Err(source) => return Err(SettingsStoreError::Read { path: self.path.clone(), source }),
        };
        serde_json::from_slice(&bytes).map_err(|source| SettingsStoreError::Parse {
            path: self.path.clone(), source,
        })
    }

    pub fn save(&self, settings: &ThirdPartySettings) -> Result<(), SettingsStoreError> {
        let bytes = serde_json::to_vec_pretty(settings)?;
        let parent = self.path.parent().expect("settings path has parent");
        fs::create_dir_all(parent).map_err(|source| SettingsStoreError::Write {
            path: parent.to_path_buf(), source,
        })?;
        // Never leave a newly-created secret file group/world-readable.
        let tmp = self.temporary_path();
        let result = self.write_and_replace(&tmp, &bytes);
        if result.is_err() { let _ = fs::remove_file(&tmp); }
        result
    }

    fn write_and_replace(&self, temporary: &Path, bytes: &[u8]) -> Result<(), SettingsStoreError> {
        let write_error = |source| SettingsStoreError::Write { path: self.path.clone(), source };
        let mut file = OpenOptions::new()
            .write(true).create_new(true).mode(0o600).open(temporary)
            .map_err(write_error)?;
        file.write_all(bytes).map_err(write_error)?;
        file.sync_all().map_err(write_error)?;
        fs::rename(temporary, &self.path).map_err(write_error)?;
        // Linux filesystem rename is atomic for readers. The private mode of
        // the replacement file remains 0600 regardless of older file modes.
        Ok(())
    }

    fn temporary_path(&self) -> PathBuf {
        let n = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let name = self.path.file_name().expect("settings file name").to_string_lossy();
        self.path.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn save_reload_and_replace_is_private() {
        let path = std::env::temp_dir().join(format!("horizon-settings-test-{}-{}", std::process::id(), NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)));
        let store = SettingsStore::new(path.clone());
        let mut settings = store.load().unwrap();
        assert!(!settings.has_steamgriddb_key());
        settings.set_steamgriddb_key("secret-key".into());
        settings.set_prefer_steamgriddb_artwork(true);
        store.save(&settings).unwrap();
        let reloaded = store.load().unwrap();
        assert!(reloaded.has_steamgriddb_key());
        assert!(reloaded.prefer_steamgriddb_artwork());
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        settings.clear_steamgriddb_key();
        store.save(&settings).unwrap();
        assert!(!store.load().unwrap().has_steamgriddb_key());
        let _ = fs::remove_file(path);
    }
}
