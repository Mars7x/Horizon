//! Controller preferences, kept in `controller.json`.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// What `controller.json` keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ControllerPreferences {
    /// Controllers select with the button that normally goes back, and go
    /// back with the one that normally selects. The keyboard is unchanged.
    pub swap_confirm: bool,
    /// With the swap on, hints show the swapped buttons (the ones to press);
    /// off, they keep the controller's usual buttons.
    pub swap_hints: bool,
}

impl Default for ControllerPreferences {
    fn default() -> Self {
        Self {
            swap_confirm: false,
            swap_hints: true,
        }
    }
}

impl ControllerPreferences {
    /// Whether hints draw select and back swapped.
    pub const fn hints_swapped(self) -> bool {
        self.swap_confirm && self.swap_hints
    }
}

/// The saved preferences; anything unreadable is the default.
pub fn load_preferences(path: &Path) -> ControllerPreferences {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save_preferences(path: &Path, preferences: &ControllerPreferences) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(preferences).map_err(io::Error::other)?;
    let partial = partial_path(path);
    fs::write(&partial, json)?;
    fs::rename(&partial, path)
}

fn partial_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".part");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip_and_default_when_missing() {
        let dir = std::env::temp_dir().join(format!("horizon-controller-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("controller.json");
        assert_eq!(load_preferences(&path), ControllerPreferences::default());

        let swapped = ControllerPreferences {
            swap_confirm: true,
            swap_hints: false,
        };
        save_preferences(&path, &swapped).expect("save");
        assert_eq!(load_preferences(&path), swapped);
        assert!(!swapped.hints_swapped());

        // A file saved before the hints switch keeps hints swapped.
        fs::write(&path, br#"{"swap_confirm": true}"#).expect("write");
        assert!(load_preferences(&path).hints_swapped());

        fs::write(&path, b"not json").expect("write");
        assert_eq!(load_preferences(&path), ControllerPreferences::default());
        fs::write(&path, b"{}").expect("write");
        assert_eq!(load_preferences(&path), ControllerPreferences::default());
        let _ = fs::remove_dir_all(&dir);
    }
}
