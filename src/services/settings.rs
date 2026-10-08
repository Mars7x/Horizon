//! Third-party settings use case. Validation and transaction semantics live here.
use crate::{domain::settings::ThirdPartySettings, persistence::settings::{SettingsStore, SettingsStoreError}};

/// Copy only into the short-lived background worker. Never expose keys to Slint.
#[derive(Clone)]
pub struct ArtworkPreferences {
    pub api_key: Option<String>,
    pub prefer_steamgriddb: bool,
}

pub struct SettingsService {
    store: SettingsStore,
    values: ThirdPartySettings,
}

impl SettingsService {
    pub fn load(store: SettingsStore) -> Result<Self, SettingsStoreError> {
        let values = store.load()?;
        Ok(Self { store, values })
    }

    /// Build an on-demand client without sending the saved secret to Slint.
    /// Call its blocking methods only on a background worker.
    pub fn steamgriddb_client(
        &self,
    ) -> Result<Option<super::steamgriddb::SteamGridDbClient>, super::steamgriddb::SteamGridDbError> {
        self.values.steamgriddb_key()
            .map(super::steamgriddb::SteamGridDbClient::new)
            .transpose()
    }

    pub fn artwork_preferences(&self) -> ArtworkPreferences {
        ArtworkPreferences {
            api_key: self.values.steamgriddb_key().map(str::to_owned),
            prefer_steamgriddb: self.values.prefer_steamgriddb_artwork(),
        }
    }

    pub fn has_steamgriddb_key(&self) -> bool { self.values.has_steamgriddb_key() }
    pub fn prefer_steamgriddb_artwork(&self) -> bool { self.values.prefer_steamgriddb_artwork() }

    pub fn set_steamgriddb_key(&mut self, key: &str) -> Result<(), String> {
        let key = key.trim();
        if key.is_empty() || key.len() > 512 || key.chars().any(char::is_whitespace) {
            return Err("Enter a nonempty API key without spaces (maximum 512 characters).".into());
        }
        let mut next = self.values.clone();
        next.set_steamgriddb_key(key.to_string());
        self.commit(next).map_err(|_| "Could not save API key. Check Horizon's config directory.".into())
    }

    pub fn remove_steamgriddb_key(&mut self) -> Result<(), SettingsStoreError> {
        let mut next = self.values.clone();
        next.clear_steamgriddb_key();
        self.commit(next)
    }

    pub fn set_prefer_steamgriddb_artwork(&mut self, preferred: bool) -> Result<(), SettingsStoreError> {
        let mut next = self.values.clone();
        next.set_prefer_steamgriddb_artwork(preferred);
        self.commit(next)
    }

    fn commit(&mut self, next: ThirdPartySettings) -> Result<(), SettingsStoreError> {
        self.store.save(&next)?;
        self.values = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static TEMP_TEST_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn invalid_key_is_never_saved() {
        let n = TEMP_TEST_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("horizon-key-validation-{}-{n}", std::process::id()));
        let mut service = SettingsService::load(SettingsStore::new(path.clone())).unwrap();
        assert!(service.set_steamgriddb_key("   ").is_err());
        assert!(service.set_steamgriddb_key("has spaces").is_err());
        assert!(!service.has_steamgriddb_key());
        assert!(!path.exists());
    }
}
