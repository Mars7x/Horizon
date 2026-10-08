//! User-owned third-party artwork preferences (no Slint/SQL dependencies).
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThirdPartySettings {
    steamgriddb_api_key: Option<String>,
    prefer_steamgriddb_artwork: bool,
}

impl ThirdPartySettings {
    pub fn has_steamgriddb_key(&self) -> bool {
        self.steamgriddb_api_key.is_some()
    }

    pub fn steamgriddb_key(&self) -> Option<&str> {
        self.steamgriddb_api_key.as_deref()
    }

    pub fn prefer_steamgriddb_artwork(&self) -> bool {
        self.prefer_steamgriddb_artwork
    }

    pub fn set_steamgriddb_key(&mut self, key: String) {
        self.steamgriddb_api_key = Some(key);
    }

    pub fn clear_steamgriddb_key(&mut self) {
        self.steamgriddb_api_key = None;
    }

    pub fn set_prefer_steamgriddb_artwork(&mut self, preferred: bool) {
        self.prefer_steamgriddb_artwork = preferred;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preference_defaults_to_existing_artwork() {
        let p = ThirdPartySettings::default();
        assert!(!p.prefer_steamgriddb_artwork());
        assert!(!p.has_steamgriddb_key());
    }
}
