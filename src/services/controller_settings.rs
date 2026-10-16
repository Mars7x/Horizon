//! Serial controller preference I/O; failed loads never overwrite user data.
pub use crate::domain::controller::ControllerPreferences;
use crate::{
    persistence::settings::SettingsStore,
    services::worker::{self, Notify},
};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};
pub struct ControllerSettings {
    pub requests: Sender<ControllerPreferences>,
    pub results: Receiver<Result<ControllerPreferences, String>>,
}
impl ControllerSettings {
    pub fn new(path: PathBuf, notify: Notify) -> Self {
        let (requests, commands) = mpsc::channel();
        let (sender, results) = worker::worker_channel(notify);
        let failed = sender.clone();
        if !worker::spawn("controller-settings", move || {
            let store = SettingsStore::new(path);
            let loaded = store
                .load_value::<ControllerPreferences>()
                .map_err(|e| e.to_string());
            let usable = loaded.is_ok();
            let _ = sender.send(loaded);
            if !usable {
                return;
            }
            while let Ok(next) = commands.recv() {
                if sender
                    .send(
                        store
                            .save_value(&next)
                            .map(|()| next)
                            .map_err(|e| e.to_string()),
                    )
                    .is_err()
                {
                    break;
                }
            }
        }) {
            let _ = failed.send(Err("Could not start controller settings".into()));
        }
        Self { requests, results }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_settings_default_bindings_and_invalid_files_are_preserved() {
        let path =
            std::env::temp_dir().join(format!("horizon-mapping-{}.json", std::process::id()));
        std::fs::write(&path, br#"{"swap_confirm":true}"#).unwrap();
        let worker = ControllerSettings::new(path.clone(), worker::no_notify());
        let mut values = worker.results.recv().unwrap().unwrap();
        assert!(values.hints_swapped());
        values.gba_bindings = values.gba_bindings.assign(0, 9, false);
        worker.requests.send(values).unwrap();
        assert_eq!(worker.results.recv().unwrap().unwrap(), values);
        drop(worker);
        std::fs::write(&path, b"broken").unwrap();
        let worker = ControllerSettings::new(path.clone(), worker::no_notify());
        assert!(worker.results.recv().unwrap().is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
        drop(worker);
        std::fs::remove_file(path).unwrap();
    }
}
