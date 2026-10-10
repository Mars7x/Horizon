//! Shared, opt-in Steam Web API identity for Achievements and future providers.
//!
//! The Web API key is *not* a Steam login or OAuth token. Like Horizon's
//! SteamGridDB settings, this is private local storage (0600), NOT encryption.
//! Never return a saved key to Slint or print credentials in logs/errors.
use std::{fs::{self, OpenOptions}, io::{self, Write}, os::unix::fs::{OpenOptionsExt, PermissionsExt}, path::PathBuf, sync::atomic::{AtomicU64, Ordering}, time::Duration};
use serde::{Deserialize, Serialize};

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct SteamAccountCredentials { key: String, steam_id64: String }
impl SteamAccountCredentials {
    pub fn new(key: &str, id: &str) -> Result<Self, &'static str> {
        let key = key.trim();
        let id = id.trim();
        if id.len() != 17 || !id.starts_with("7656119") || id.parse::<u64>().is_err() {
            return Err("Enter your 17-digit SteamID64 (starting with 7656119).");
        }
        if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err("Enter a valid Steam Web API key without spaces (max 128 characters).");
        }
        Ok(Self { key: key.to_owned(), steam_id64: id.to_owned() })
    }
    pub fn from_environment() -> Option<Self> {
        Self::new(&std::env::var("HORIZON_STEAM_WEB_API_KEY").ok()?, &std::env::var("HORIZON_STEAM_ID64").ok()?).ok()
    }
    pub fn steam_id64(&self) -> &str { &self.steam_id64 }
}

#[derive(Serialize, Deserialize)]
struct AccountFile { steam_id64: String, api_key: String }

pub struct SteamAccountService {
    path: PathBuf,
    saved: Option<SteamAccountCredentials>,
    environment: Option<SteamAccountCredentials>,
    pending_id: Option<String>,
}
impl SteamAccountService {
    pub fn load(path: PathBuf) -> Result<Self, &'static str> {
        let saved = match fs::read(&path) {
            Ok(bytes) => {
                let data: AccountFile = serde_json::from_slice(&bytes).map_err(|_| "Steam account settings are invalid.")?;
                Some(SteamAccountCredentials::new(&data.api_key, &data.steam_id64)
                    .map_err(|_| "Saved Steam account settings are invalid.")?)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(_) => return Err("Unable to read Steam account settings."),
        };
        Ok(Self { path, saved, environment: SteamAccountCredentials::from_environment(), pending_id: None })
    }
    pub fn credentials(&self) -> Option<SteamAccountCredentials> {
        self.saved.clone().or_else(|| self.environment.clone())
    }
    pub fn saved(&self) -> bool { self.saved.is_some() }
    pub fn connected(&self) -> bool { self.credentials().is_some() }
    pub fn identity(&self) -> Option<&str> {
        self.pending_id.as_deref().or_else(|| self.saved.as_ref().or(self.environment.as_ref()).map(SteamAccountCredentials::steam_id64))
    }
    /// A SteamID may be staged before the user supplies the key; no incomplete
    /// credential pair is written to disk or shared with consumers.
    pub fn set_steam_id64(&mut self, steam_id64: &str) -> Result<(), &'static str> {
        let id = steam_id64.trim();
        if id.len() != 17 || !id.starts_with("7656119") || id.parse::<u64>().is_err() {
            return Err("Enter your 17-digit SteamID64 (starting with 7656119).");
        }
        if let Some(account) = self.credentials() {
            self.save(&account.key, id)
        } else {
            self.pending_id = Some(id.to_owned());
            Ok(())
        }
    }
    pub fn set_web_api_key(&mut self, key: &str) -> Result<(), &'static str> {
        let id = self.identity().ok_or("Set SteamID64 before adding the Web API key.")?.to_owned();
        self.save(key, &id)
    }
    pub fn save(&mut self, key: &str, steam_id64: &str) -> Result<(), &'static str> {
        let candidate = SteamAccountCredentials::new(key, steam_id64)?;
        let bytes = serde_json::to_vec(&AccountFile {
            steam_id64: candidate.steam_id64.clone(), api_key: candidate.key.clone(),
        }).map_err(|_| "Unable to encode Steam account settings.")?;
        let parent = self.path.parent().ok_or("Invalid Steam account settings path.")?;
        fs::create_dir_all(parent).map_err(|_| "Unable to create Steam account config directory.")?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .map_err(|_| "Unable to secure Steam account config directory.")?;
        let suffix = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        let temp = self.path.with_file_name(format!(".steam-account.{}.{}.tmp", std::process::id(), suffix));
        let result = (|| -> io::Result<()> {
            let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temp, &self.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
            return Err("Unable to save Steam account settings.");
        }
        self.saved = Some(candidate);
        self.pending_id = None;
        Ok(())
    }
    pub fn remove(&mut self) -> Result<(), &'static str> {
        match fs::remove_file(&self.path) {
            Ok(()) => {},
            Err(e) if e.kind() == io::ErrorKind::NotFound => {},
            Err(_) => return Err("Unable to remove saved Steam account settings."),
        }
        self.saved = None;
        self.pending_id = None;
        Ok(())
    }
}

/// Used by current read-only Achievements and later by Friends without sharing
/// provider-specific presentation models or making UI-thread network requests.
pub struct SteamWebApiClient {
    client: reqwest::blocking::Client,
    account: SteamAccountCredentials,
}
impl SteamWebApiClient {
    pub fn new(account: SteamAccountCredentials) -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            account,
        })
    }
    /// Download a public Steam achievement badge with no API-key query string.
    /// This is an allowlisted image transport, not a generic URL fetcher.
    pub fn download_badge(&self, url: &str) -> Option<Vec<u8>> {
        let address = reqwest::Url::parse(url).ok()?;
        let allowed = matches!(address.host_str()?,
            "cdn.cloudflare.steamstatic.com" | "cdn.akamai.steamstatic.com"
            | "steamcdn-a.akamaihd.net" | "community.akamai.steamstatic.com");
        if !allowed || address.scheme() != "https" || address.port().is_some()
            || !address.username().is_empty() || address.password().is_some() { return None; }
        let response = self.client.get(address).send().ok()?.error_for_status().ok()?;
        if response.content_length().is_some_and(|len| len > 256 * 1024) { return None; }
        let bytes = response.bytes().ok()?;
        if bytes.len() > 256 * 1024 { return None; }
        let image = image::load_from_memory(&bytes).ok()?;
        if image.width() > 1024 || image.height() > 1024 { return None; }
        let badge = image.resize_exact(56, 56, image::imageops::FilterType::CatmullRom).to_rgba8();
        Some(badge.into_raw())
    }
    pub fn get(&self, endpoint: &'static str, params: &[(&str, &str)]) -> Result<String, reqwest::Error> {
        // `endpoint` is an application-owned static URL, never user input.
        let mut all = vec![("key", self.account.key.as_str()), ("steamid", self.account.steam_id64.as_str())];
        all.extend_from_slice(params);
        self.client.get(endpoint).query(&all).send()?.error_for_status()?.text()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_and_key_validation() {
        assert!(SteamAccountCredentials::new("key", "76561198000000000").is_ok());
        assert!(SteamAccountCredentials::new("key", "480").is_err());
        assert!(SteamAccountCredentials::new("a b", "76561198000000000").is_err());
    }
    #[test]
    fn identity_is_staged_without_saving_an_incomplete_account() {
        let path = std::env::temp_dir().join(format!("horizon-steam-account-staging-{}-{}.json", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
        let mut service = SteamAccountService::load(path.clone()).unwrap();
        assert!(service.set_steam_id64("76561198000000000").is_ok());
        assert_eq!(service.identity(), Some("76561198000000000"));
        assert!(!service.connected());
        assert!(!path.exists());
        assert!(service.set_web_api_key("bad key").is_err());
        assert!(!path.exists());
        service.set_web_api_key("testkey").unwrap();
        assert!(service.connected());
        service.remove().unwrap();
        assert!(!path.exists());
    }
    #[test]
    fn private_save_reopen_clear() {
        let path = std::env::temp_dir().join(format!("horizon-steam-account-test-{}-{}.json", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
        let mut svc = SteamAccountService::load(path.clone()).unwrap();
        svc.save("privatekey", "76561198000000000").unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(SteamAccountService::load(path.clone()).unwrap().identity(), Some("76561198000000000"));
        assert!(svc.save("has spaces", "76561198000000000").is_err());
        assert!(svc.saved());
        svc.set_steam_id64("76561198000000001").unwrap();
        assert_eq!(svc.identity(), Some("76561198000000001"));
        svc.set_web_api_key("replacementkey").unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        svc.remove().unwrap();
        assert!(!path.exists());
    }
}
