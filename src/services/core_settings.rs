//! Serial settings I/O off the UI thread, using the shared atomic store.
use super::worker::{self, Notify};
use crate::{
    domain::core_settings::CorePreferences, persistence::core_settings::CoreSettingsStore,
    platform::data_paths::CoreSettingsPaths,
};
use std::sync::mpsc::{self, Receiver, Sender};

pub struct CoreSettings {
    pub requests: Sender<CorePreferences>,
    pub results: Receiver<Result<CorePreferences, String>>,
}
impl CoreSettings {
    pub fn new(paths: CoreSettingsPaths, notify: Notify) -> Self {
        let (requests, commands) = mpsc::channel();
        let (sender, results) = worker::worker_channel(notify);
        let failed = sender.clone();
        if !worker::spawn("core-settings", move || {
            let store = CoreSettingsStore::new(paths);
            let loaded = store.load();
            let mut current = loaded.as_ref().copied().unwrap_or_default();
            let usable = loaded.is_ok();
            let _ = sender.send(loaded.map_err(|error| error.to_string()));
            // Never overwrite an unreadable settings file with defaults.
            if !usable {
                return;
            }
            while let Ok(next) = commands.recv() {
                let result = store.save(current, next).map(|()| {
                    current = next;
                    next
                });
                if sender.send(result).is_err() {
                    break;
                }
            }
        }) {
            let _ = failed.send(Err("Could not start the core settings worker".into()));
        }
        Self { requests, results }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separate_owners_commit_and_failed_writes_preserve_both_files() {
        let root =
            std::env::temp_dir().join(format!("horizon-core-storage-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let paths = CoreSettingsPaths {
            playback: root.join("playback.json"),
            mgba: root.join("cores/mgba.json"),
        };
        let worker = CoreSettings::new(paths.clone(), worker::no_notify());
        let mut next = worker.results.recv().unwrap().unwrap();
        next.mgba.sunlight = 7.try_into().unwrap();
        worker.requests.send(next).unwrap();
        assert_eq!(worker.results.recv().unwrap().unwrap(), next);
        assert!(!paths.playback.exists());
        next.freeze_when_unfocused = false;
        worker.requests.send(next).unwrap();
        assert_eq!(worker.results.recv().unwrap().unwrap(), next);
        let original = std::fs::read(&paths.playback).unwrap();
        std::fs::rename(&paths.mgba, root.join("preserved.json")).unwrap();
        std::fs::create_dir(&paths.mgba).unwrap();
        let mut failed = next;
        failed.mgba.sunlight = 8.try_into().unwrap();
        worker.requests.send(failed).unwrap();
        assert!(worker.results.recv().unwrap().is_err());
        assert_eq!(std::fs::read(&paths.playback).unwrap(), original);
        drop(worker);
        std::fs::remove_dir(&paths.mgba).unwrap();
        std::fs::rename(root.join("preserved.json"), &paths.mgba).unwrap();
        assert_eq!(CoreSettingsStore::new(paths.clone()).load().unwrap(), next);
        std::fs::write(&paths.mgba, br#"{"sunlight":11}"#).unwrap();
        let worker = CoreSettings::new(paths.clone(), worker::no_notify());
        assert!(worker.results.recv().unwrap().is_err());
        assert_eq!(std::fs::read(&paths.mgba).unwrap(), br#"{"sunlight":11}"#);
        std::fs::remove_dir_all(root).unwrap();
    }
}
