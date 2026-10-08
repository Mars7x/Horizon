//! Durable, atomic appearance preferences. No reliance on the UI event loop.
use std::{fs::{self, OpenOptions}, io::{self, Write}, path::PathBuf,
    os::unix::fs::OpenOptionsExt, sync::atomic::{AtomicU64, Ordering}};
use thiserror::Error;
use super::AppearancePreferences;

static WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error)]
pub enum AppearanceStoreError {
    #[error("could not read appearance preferences: {0}")]
    Read(#[source] io::Error),
    #[error("could not parse appearance preferences: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("could not write appearance preferences: {0}")]
    Write(#[source] io::Error),
}

#[derive(Debug, Clone)]
pub struct AppearanceStore { path: PathBuf }
impl AppearanceStore {
    pub fn new(path: PathBuf) -> Self { Self { path } }
    pub fn load(&self) -> Result<AppearancePreferences, AppearanceStoreError> {
        match fs::read(&self.path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(AppearancePreferences::default()),
            Err(e) => Err(AppearanceStoreError::Read(e)),
        }
    }
    pub fn save(&self, value: &AppearancePreferences) -> Result<(), AppearanceStoreError> {
        let bytes = serde_json::to_vec_pretty(value)?;
        let dir = self.path.parent().expect("appearance path has parent");
        fs::create_dir_all(dir).map_err(AppearanceStoreError::Write)?;
        let file_name = self.path.file_name().expect("appearance path has name").to_string_lossy();
        let tmp = self.path.with_file_name(format!(".{file_name}.{}.{}.tmp",
            std::process::id(), WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        let result = (|| {
            let mut file = OpenOptions::new().create_new(true).write(true).mode(0o600)
                .open(&tmp).map_err(AppearanceStoreError::Write)?;
            file.write_all(&bytes).map_err(AppearanceStoreError::Write)?;
            file.sync_all().map_err(AppearanceStoreError::Write)?;
            fs::rename(&tmp, &self.path).map_err(AppearanceStoreError::Write)
        })();
        if result.is_err() { let _ = fs::remove_file(&tmp); }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::{AccentPreference, Rgb, ThemePreference};
    #[test]
    fn older_settings_default_ui_sounds_to_on() {
        let previous = r#"{"theme":"dark","accent":"system"}"#;
        let settings: AppearancePreferences = serde_json::from_str(previous).unwrap();
        assert!(settings.ui_sounds_enabled);
    }

    #[test]
    fn saves_and_loads_independent_theme_and_accent_preferences() {
        let path = std::env::temp_dir().join(format!("horizon-appearance-{}-{}.json",
            std::process::id(), WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        let store = AppearanceStore::new(path.clone());
        assert_eq!(store.load().unwrap(), AppearancePreferences::default());
        let prefs = AppearancePreferences { theme: ThemePreference::Dark,
            accent: AccentPreference::Custom(Rgb::new(242, 201, 76)),
            ui_sounds_enabled: false };
        store.save(&prefs).unwrap();
        assert_eq!(store.load().unwrap(), prefs);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let _ = fs::remove_file(path);
    }
}
