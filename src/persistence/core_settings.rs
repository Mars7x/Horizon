//! Shared playback policy and core preferences are independent atomic files.
use super::settings::{SettingsStore, SettingsStoreError};
use crate::{
    domain::core_settings::{CorePreferences, DsPreferences, MgbaPreferences, ThreeDsPreferences},
    platform::data_paths::CoreSettingsPaths,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PlaybackPreferences {
    freeze_when_unfocused: bool,
    resume_last_state: bool,
}
impl Default for PlaybackPreferences {
    fn default() -> Self {
        Self {
            freeze_when_unfocused: true,
            resume_last_state: true,
        }
    }
}
pub struct CoreSettingsStore {
    paths: CoreSettingsPaths,
}
impl CoreSettingsStore {
    pub fn new(paths: CoreSettingsPaths) -> Self {
        Self { paths }
    }
    pub fn load(&self) -> Result<CorePreferences, SettingsStoreError> {
        let playback: PlaybackPreferences =
            SettingsStore::new(self.paths.playback.clone()).load_value()?;
        let mgba: MgbaPreferences = SettingsStore::new(self.paths.mgba.clone()).load_value()?;
        let ds: DsPreferences = SettingsStore::new(self.paths.ds.clone()).load_value()?;
        let three_ds: ThreeDsPreferences =
            SettingsStore::new(self.paths.three_ds.clone()).load_value()?;
        Ok(CorePreferences {
            freeze_when_unfocused: playback.freeze_when_unfocused,
            resume_last_state: playback.resume_last_state,
            mgba,
            ds,
            three_ds,
        })
    }
    pub fn save(&self, current: CorePreferences, next: CorePreferences) -> Result<(), String> {
        let playback_changed = current.freeze_when_unfocused != next.freeze_when_unfocused
            || current.resume_last_state != next.resume_last_state;
        let mgba_changed = current.mgba != next.mgba;
        let ds_changed = current.ds != next.ds;
        let three_ds_changed = current.three_ds != next.three_ds;
        // Each UI operation has one owner. Never partially commit two files.
        if u8::from(playback_changed)
            + u8::from(mgba_changed)
            + u8::from(ds_changed)
            + u8::from(three_ds_changed)
            > 1
        {
            return Err("Change playback and core preferences separately".into());
        }
        if playback_changed {
            SettingsStore::new(self.paths.playback.clone())
                .save_value(&PlaybackPreferences {
                    freeze_when_unfocused: next.freeze_when_unfocused,
                    resume_last_state: next.resume_last_state,
                })
                .map_err(|error| error.to_string())?;
        } else if mgba_changed {
            SettingsStore::new(self.paths.mgba.clone())
                .save_value(&next.mgba)
                .map_err(|error| error.to_string())?;
        } else if ds_changed {
            SettingsStore::new(self.paths.ds.clone())
                .save_value(&next.ds)
                .map_err(|error| error.to_string())?;
        } else if three_ds_changed {
            SettingsStore::new(self.paths.three_ds.clone())
                .save_value(&next.three_ds)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
    #[cfg(test)]
    pub fn write_fixture(&self, values: CorePreferences) {
        SettingsStore::new(self.paths.playback.clone())
            .save_value(&PlaybackPreferences {
                freeze_when_unfocused: values.freeze_when_unfocused,
                resume_last_state: values.resume_last_state,
            })
            .unwrap();
        SettingsStore::new(self.paths.mgba.clone())
            .save_value(&values.mgba)
            .unwrap();
        SettingsStore::new(self.paths.ds.clone())
            .save_value(&values.ds)
            .unwrap();
        SettingsStore::new(self.paths.three_ds.clone())
            .save_value(&values.three_ds)
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resume_defaults_on_for_existing_playback_files_and_persists_independently() {
        let root =
            std::env::temp_dir().join(format!("horizon-resume-settings-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let paths = CoreSettingsPaths {
            playback: root.join("playback.json"),
            mgba: root.join("mgba.json"),
            ds: root.join("melonds-ds.json"),
            three_ds: root.join("azahar.json"),
        };
        std::fs::write(&paths.playback, b"{\"freeze_when_unfocused\":false}").unwrap();
        let store = CoreSettingsStore::new(paths.clone());
        let old = store.load().unwrap();
        assert!(old.resume_last_state);
        assert!(!old.freeze_when_unfocused);
        let next = CorePreferences {
            resume_last_state: false,
            ..old
        };
        store.save(old, next).unwrap();
        assert_eq!(store.load().unwrap(), next);
        assert!(!paths.mgba.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rejects_cross_owner_commits_without_writing_either_file() {
        let root = std::env::temp_dir().join(format!(
            "horizon-settings-owner-test-{}",
            std::process::id()
        ));
        let paths = CoreSettingsPaths {
            playback: root.join("playback.json"),
            mgba: root.join("cores/mgba.json"),
            ds: root.join("cores/melonds-ds.json"),
            three_ds: root.join("cores/azahar.json"),
        };
        let store = CoreSettingsStore::new(paths.clone());
        let old = CorePreferences::default();
        let mut next = old;
        next.freeze_when_unfocused = false;
        next.mgba.color_correction = true;
        assert!(store.save(old, next).is_err());
        assert!(!paths.playback.exists());
        assert!(!paths.mgba.exists());
    }
}
