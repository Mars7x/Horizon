//! Serial settings I/O off the UI thread, using the shared atomic store.
use super::worker::{self, Notify};
use crate::{domain::core_settings::CorePreferences, persistence::settings::SettingsStore};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};

pub struct CoreSettings {
    pub requests: Sender<CorePreferences>,
    pub results: Receiver<Result<CorePreferences, String>>,
}
impl CoreSettings {
    pub fn new(path: PathBuf, notify: Notify) -> Self {
        let (requests, commands) = mpsc::channel();
        let (sender, results) = worker::worker_channel(notify);
        let failed = sender.clone();
        if !worker::spawn("core-settings", move || {
            let store = SettingsStore::new(path);
            let loaded = store.load_value::<CorePreferences>();
            let usable = loaded.is_ok();
            let _ = sender.send(loaded.map_err(|error| error.to_string()));
            // Never overwrite an unreadable settings file with defaults.
            if !usable {
                return;
            }
            while let Ok(next) = commands.recv() {
                let result = store
                    .save_value(&next)
                    .map(|()| next)
                    .map_err(|error| error.to_string());
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
    fn commits_round_trip_and_rejects_invalid_saved_values() {
        let root = std::env::temp_dir().join(format!("horizon-cores-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("cores.json");
        let _ = std::fs::remove_file(&path);
        let worker = CoreSettings::new(path.clone(), worker::no_notify());
        assert_eq!(
            worker.results.recv().unwrap().unwrap(),
            CorePreferences::default()
        );
        let mut next = CorePreferences::default();

        next.mgba.sunlight = 7.try_into().unwrap();
        next.mgba.video_filter = crate::domain::core_settings::VideoFilter::Xbrz;
        next.mgba.color_correction = true;
        next.freeze_when_unfocused = false;
        worker.requests.send(next).unwrap();
        assert_eq!(worker.results.recv().unwrap().unwrap(), next);
        drop(worker);
        assert_eq!(
            SettingsStore::new(path.clone())
                .load_value::<CorePreferences>()
                .unwrap(),
            next
        );
        let invalid = br#"{"mgba":{"sunlight":11}}"#;
        std::fs::write(&path, invalid).unwrap();
        let worker = CoreSettings::new(path.clone(), worker::no_notify());
        assert!(worker.results.recv().unwrap().is_err());
        assert_eq!(std::fs::read(&path).unwrap(), invalid);
        std::fs::remove_dir_all(root).unwrap();
    }
}
