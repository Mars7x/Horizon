//! Shared, opt-in Steam Web API identity for Achievements and future providers.
//!
//! The Web API key is *not* a Steam login or OAuth token. Like Horizon's
//! SteamGridDB settings, this is private local storage (0600), NOT encryption.
//! Never return a saved key to Slint or print credentials in logs/errors.
use crate::services::friends;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

const PLAYER_SUMMARIES: &str = "https://api.steampowered.com/ISteamUser/GetPlayerSummaries/v2/";
const FRIEND_LIST: &str = "https://api.steampowered.com/ISteamUser/GetFriendList/v1/";
const PROFILE_ITEMS: &str =
    "https://api.steampowered.com/IPlayerService/GetProfileItemsEquipped/v1/";
/// Where Steam's community item images (avatar frames) live; the API gives
/// only the path after it.
const ITEM_IMAGES: &str = "https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/";
/// Where Steam serves profile avatars.
const AVATAR_HOSTS: &[&str] = &[
    "avatars.steamstatic.com",
    "avatars.akamai.steamstatic.com",
    "avatars.cloudflare.steamstatic.com",
    "steamcdn-a.akamaihd.net",
];
/// Where Steam serves achievement badges.
const BADGE_HOSTS: &[&str] = &[
    "cdn.cloudflare.steamstatic.com",
    "cdn.akamai.steamstatic.com",
    "steamcdn-a.akamaihd.net",
    "community.akamai.steamstatic.com",
];

/// An equipped Steam profile item (the animated avatar), as its image file.
#[derive(Debug, PartialEq)]
pub enum ProfileItem {
    NotEquipped,
    /// The image file's bytes.
    Downloaded(Vec<u8>),
    /// Equipped, but it couldn't be downloaded (so keep any earlier copy).
    Failed,
}

/// Why the Steam profile picture couldn't be fetched. Never carries a
/// request URL: the Web API one holds the key.
#[derive(Debug, thiserror::Error)]
pub enum SteamAvatarError {
    #[error("Steam didn't answer: {0}")]
    Network(String),
    #[error("Steam's answer had no profile picture")]
    Missing,
    #[error("the profile picture couldn't be downloaded")]
    Download,
}

#[derive(Clone)]
pub struct SteamAccountCredentials {
    key: String,
    steam_id64: String,
}
impl SteamAccountCredentials {
    pub fn new(key: &str, id: &str) -> Result<Self, &'static str> {
        let key = key.trim();
        let id = id.trim();
        if id.len() != 17 || !id.starts_with("7656119") || id.parse::<u64>().is_err() {
            return Err("Enter the 17-digit Steam ID that starts with 7656119");
        }
        if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err("Enter a valid key, with no spaces");
        }
        Ok(Self {
            key: key.to_owned(),
            steam_id64: id.to_owned(),
        })
    }
    pub fn from_environment() -> Option<Self> {
        Self::new(
            &std::env::var("HORIZON_STEAM_WEB_API_KEY").ok()?,
            &std::env::var("HORIZON_STEAM_ID64").ok()?,
        )
        .ok()
    }
    pub fn steam_id64(&self) -> &str {
        &self.steam_id64
    }
}

#[derive(Serialize, Deserialize)]
struct AccountFile {
    steam_id64: String,
    api_key: String,
}

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
                let data: AccountFile = serde_json::from_slice(&bytes)
                    .map_err(|_| "Steam account settings are invalid.")?;
                Some(
                    SteamAccountCredentials::new(&data.api_key, &data.steam_id64)
                        .map_err(|_| "Saved Steam account settings are invalid.")?,
                )
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(_) => return Err("Couldn't read your Steam account"),
        };
        Ok(Self {
            path,
            saved,
            environment: SteamAccountCredentials::from_environment(),
            pending_id: None,
        })
    }
    pub fn credentials(&self) -> Option<SteamAccountCredentials> {
        self.saved.clone().or_else(|| self.environment.clone())
    }
    pub fn saved(&self) -> bool {
        self.saved.is_some()
    }
    pub fn connected(&self) -> bool {
        self.credentials().is_some()
    }
    pub fn identity(&self) -> Option<&str> {
        self.pending_id.as_deref().or_else(|| {
            self.saved
                .as_ref()
                .or(self.environment.as_ref())
                .map(SteamAccountCredentials::steam_id64)
        })
    }
    /// A SteamID may be staged before the user supplies the key; no incomplete
    /// credential pair is written to disk or shared with consumers.
    pub fn set_steam_id64(&mut self, steam_id64: &str) -> Result<(), &'static str> {
        let id = steam_id64.trim();
        if id.len() != 17 || !id.starts_with("7656119") || id.parse::<u64>().is_err() {
            return Err("Enter the 17-digit Steam ID that starts with 7656119");
        }
        if let Some(account) = self.credentials() {
            self.save(&account.key, id)
        } else {
            self.pending_id = Some(id.to_owned());
            Ok(())
        }
    }
    pub fn set_web_api_key(&mut self, key: &str) -> Result<(), &'static str> {
        let id = self.identity().ok_or("Add your Steam ID first")?.to_owned();
        self.save(key, &id)
    }
    pub fn save(&mut self, key: &str, steam_id64: &str) -> Result<(), &'static str> {
        let candidate = SteamAccountCredentials::new(key, steam_id64)?;
        let bytes = serde_json::to_vec(&AccountFile {
            steam_id64: candidate.steam_id64.clone(),
            api_key: candidate.key.clone(),
        })
        .map_err(|_| "Unable to encode Steam account settings.")?;
        let parent = self
            .path
            .parent()
            .ok_or("Invalid Steam account settings path.")?;
        fs::create_dir_all(parent)
            .map_err(|_| "Unable to create Steam account config directory.")?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .map_err(|_| "Unable to secure Steam account config directory.")?;
        let suffix = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        let temp = self.path.with_file_name(format!(
            ".steam-account.{}.{}.tmp",
            std::process::id(),
            suffix
        ));
        let result = (|| -> io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temp, &self.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
            return Err("Couldn't save");
        }
        self.saved = Some(candidate);
        self.pending_id = None;
        Ok(())
    }
    pub fn remove(&mut self) -> Result<(), &'static str> {
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err("Couldn't remove"),
        }
        self.saved = None;
        self.pending_id = None;
        Ok(())
    }
}

/// Used by read-only Achievements, the profile and its friends without sharing
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
        let bytes = self.download_public_image(url, BADGE_HOSTS, 256 * 1024)?;
        let image = crate::services::image::decode(&bytes, None).ok()?;
        let badge = image
            .resize_exact(56, 56, image::imageops::FilterType::CatmullRom)
            .to_rgba8();
        Some(badge.into_raw())
    }
    /// The account's profile picture (Steam's largest, 184 px): the image
    /// file's bytes, checked to be a picture of sane size but not decoded.
    pub fn profile_avatar(&self) -> Result<Vec<u8>, SteamAvatarError> {
        let steam_id = self.account.steam_id64.clone();
        let body = self
            .get(PLAYER_SUMMARIES, &[("steamids", steam_id.as_str())])
            .map_err(|error| SteamAvatarError::Network(error.without_url().to_string()))?;
        let url = avatar_url(&body).ok_or(SteamAvatarError::Missing)?;
        self.download_public_image(&url, AVATAR_HOSTS, 1024 * 1024)
            .ok_or(SteamAvatarError::Download)
    }

    /// The account's equipped animated avatar (a GIF), as the image file.
    pub fn animated_avatar(&self) -> Result<ProfileItem, SteamAvatarError> {
        let body = self
            .get(PROFILE_ITEMS, &[])
            .map_err(|error| SteamAvatarError::Network(error.without_url().to_string()))?;
        let Some(path) = animated_avatar_path(&body) else {
            return Ok(ProfileItem::NotEquipped);
        };
        Ok(
            match self.download_public_image(
                &format!("{ITEM_IMAGES}{path}"),
                BADGE_HOSTS,
                8 * 1024 * 1024,
            ) {
                Some(bytes) => ProfileItem::Downloaded(bytes),
                None => {
                    tracing::warn!("the Steam animated avatar could not be downloaded");
                    ProfileItem::Failed
                }
            },
        )
    }

    /// The account's Steam friends with what each is doing, unsorted. A
    /// private friend list is [`FriendsError::Private`]. Avatars are not
    /// downloaded here (see [`Self::friend_avatar`]).
    pub fn friends(&self) -> Result<Vec<friends::Friend>, friends::FriendsError> {
        let network = |error: reqwest::Error| {
            if error.status() == Some(reqwest::StatusCode::UNAUTHORIZED) {
                friends::FriendsError::Private
            } else {
                friends::FriendsError::Network(error.without_url().to_string())
            }
        };
        let body = self
            .get(FRIEND_LIST, &[("relationship", "friend")])
            .map_err(network)?;
        let ids = friends::friend_ids(&body)?;
        let mut all = Vec::with_capacity(ids.len());
        for batch in ids.chunks(friends::SUMMARIES_PER_REQUEST) {
            let joined = batch.join(",");
            let body = self
                .get(PLAYER_SUMMARIES, &[("steamids", joined.as_str())])
                .map_err(network)?;
            all.extend(friends::summaries(&body)?);
        }
        Ok(all)
    }

    /// A friend's avatar from Steam's avatar hosts: the image file's bytes,
    /// checked to be a picture of sane size but not decoded.
    pub fn friend_avatar(&self, url: &str) -> Option<Vec<u8>> {
        self.download_public_image(url, AVATAR_HOSTS, 256 * 1024)
    }

    /// An image from one of Steam's public image hosts, over HTTPS, with no
    /// API key: at most `max_bytes`, and no larger than 1024 px a side by its
    /// header. An allowlisted image transport, not a generic URL fetcher.
    fn download_public_image(&self, url: &str, hosts: &[&str], max_bytes: u64) -> Option<Vec<u8>> {
        let address = reqwest::Url::parse(url).ok()?;
        if !hosts.contains(&address.host_str()?)
            || address.scheme() != "https"
            || address.port().is_some()
            || !address.username().is_empty()
            || address.password().is_some()
        {
            return None;
        }
        let response = self
            .client
            .get(address)
            .send()
            .ok()?
            .error_for_status()
            .ok()?;
        if response.content_length().is_some_and(|len| len > max_bytes) {
            return None;
        }
        let bytes = response.bytes().ok()?;
        if bytes.len() as u64 > max_bytes {
            return None;
        }
        // Check the size from the header before decoding anything.
        let (width, height) = crate::services::image::dimensions(&bytes, None).ok()?;
        if width > 1024 || height > 1024 {
            return None;
        }
        Some(bytes.to_vec())
    }

    pub fn get(
        &self,
        endpoint: &'static str,
        params: &[(&str, &str)],
    ) -> Result<String, reqwest::Error> {
        // `endpoint` is an application-owned static URL, never user input.
        let mut all = vec![
            ("key", self.account.key.as_str()),
            ("steamid", self.account.steam_id64.as_str()),
        ];
        all.extend_from_slice(params);
        self.client
            .get(endpoint)
            .query(&all)
            .send()?
            .error_for_status()?
            .text()
    }
}

/// `avatarfull` (else `avatarmedium`) of the first player in a
/// GetPlayerSummaries answer.
fn avatar_url(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Answer {
        response: Players,
    }
    #[derive(Deserialize)]
    struct Players {
        players: Vec<Player>,
    }
    #[derive(Deserialize)]
    struct Player {
        avatarfull: Option<String>,
        avatarmedium: Option<String>,
    }
    let player = serde_json::from_str::<Answer>(body)
        .ok()?
        .response
        .players
        .into_iter()
        .next()?;
    player
        .avatarfull
        .or(player.avatarmedium)
        .filter(|url| !url.is_empty())
}

/// The equipped animated avatar's image path (`items/<appid>/<file>`) in a
/// GetProfileItemsEquipped answer. It prefers `image_small`: for this item it
/// is the animated file (a GIF), while `image_large` is a still preview
/// (often a JPEG). Only a plain relative path is accepted, since it is joined
/// onto Steam's address.
fn animated_avatar_path(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Answer {
        response: Items,
    }
    #[derive(Deserialize)]
    struct Items {
        animated_avatar: Option<Item>,
    }
    #[derive(Deserialize)]
    struct Item {
        image_large: Option<String>,
        image_small: Option<String>,
    }
    let item = serde_json::from_str::<Answer>(body)
        .ok()?
        .response
        .animated_avatar?;
    [item.image_small, item.image_large]
        .into_iter()
        .flatten()
        .find(|path| {
            path.starts_with("items/")
                && !path.contains("..")
                && path
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_animated_avatar_path_is_a_plain_item_path_or_nothing() {
        let body = r#"{"response":{"avatar_frame":{"communityitemid":"1",
            "image_small":"items/2861690/small.png","image_large":"items/2861690/large.png"},
            "animated_avatar":{"image_small":"items/2873080/a.gif","image_large":"items/2873080/b.jpg"}}}"#;
        // The small image is the animated one; the large is a still preview.
        assert_eq!(
            animated_avatar_path(body).as_deref(),
            Some("items/2873080/a.gif")
        );
        let large_only = r#"{"response":{"animated_avatar":{"image_large":"items/1/l.gif"}}}"#;
        assert_eq!(
            animated_avatar_path(large_only).as_deref(),
            Some("items/1/l.gif")
        );
        let frame_only = r#"{"response":{"avatar_frame":{"image_small":"items/1/s.png"}}}"#;
        assert_eq!(animated_avatar_path(frame_only), None);
        assert_eq!(animated_avatar_path(r#"{"response":{}}"#), None);
        assert_eq!(animated_avatar_path("not json"), None);
        let bad_small = r#"{"response":{"animated_avatar":{"image_small":"../x.gif","image_large":"items/1/l.gif"}}}"#;
        assert_eq!(
            animated_avatar_path(bad_small).as_deref(),
            Some("items/1/l.gif")
        );
        for bad in [
            "../x.png",
            "items/../../x",
            "https://evil/x.png",
            "items/1/x.png?k=v",
            "items/a b.png",
        ] {
            let body = format!(r#"{{"response":{{"animated_avatar":{{"image_large":"{bad}"}}}}}}"#);
            assert_eq!(animated_avatar_path(&body), None, "{bad}");
        }
    }

    #[test]
    fn the_avatar_comes_from_the_first_player() {
        let body = r#"{"response":{"players":[{"steamid":"76561198000000000",
            "avatarmedium":"https://avatars.steamstatic.com/abc_medium.jpg",
            "avatarfull":"https://avatars.steamstatic.com/abc_full.jpg"}]}}"#;
        assert_eq!(
            avatar_url(body).as_deref(),
            Some("https://avatars.steamstatic.com/abc_full.jpg")
        );
        let medium_only = r#"{"response":{"players":[{"avatarmedium":"https://avatars.steamstatic.com/m.jpg"}]}}"#;
        assert_eq!(
            avatar_url(medium_only).as_deref(),
            Some("https://avatars.steamstatic.com/m.jpg")
        );
        assert_eq!(avatar_url(r#"{"response":{"players":[]}}"#), None);
        assert_eq!(avatar_url("not json"), None);
    }

    #[test]
    fn public_images_come_only_from_steam_hosts_over_https() {
        let client = SteamWebApiClient::new(
            SteamAccountCredentials::new("key", "76561198000000000").unwrap(),
        )
        .unwrap();
        // Rejected before any request is made.
        for url in [
            "http://avatars.steamstatic.com/a.jpg",
            "https://example.com/a.jpg",
            "https://avatars.steamstatic.com:8443/a.jpg",
            "https://user@avatars.steamstatic.com/a.jpg",
            "not a url",
        ] {
            assert!(
                client
                    .download_public_image(url, AVATAR_HOSTS, 1024)
                    .is_none(),
                "{url}"
            );
        }
        assert!(
            client
                .download_public_image(
                    "https://cdn.akamai.steamstatic.com/b.jpg",
                    AVATAR_HOSTS,
                    1024
                )
                .is_none()
        );
    }
    #[test]
    fn identity_and_key_validation() {
        assert!(SteamAccountCredentials::new("key", "76561198000000000").is_ok());
        assert!(SteamAccountCredentials::new("key", "480").is_err());
        assert!(SteamAccountCredentials::new("a b", "76561198000000000").is_err());
    }
    #[test]
    fn identity_is_staged_without_saving_an_incomplete_account() {
        let path = std::env::temp_dir().join(format!(
            "horizon-steam-account-staging-{}-{}.json",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
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
        let path = std::env::temp_dir().join(format!(
            "horizon-steam-account-test-{}-{}.json",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut svc = SteamAccountService::load(path.clone()).unwrap();
        svc.save("privatekey", "76561198000000000").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            SteamAccountService::load(path.clone()).unwrap().identity(),
            Some("76561198000000000")
        );
        assert!(svc.save("has spaces", "76561198000000000").is_err());
        assert!(svc.saved());
        svc.set_steam_id64("76561198000000001").unwrap();
        assert_eq!(svc.identity(), Some("76561198000000001"));
        svc.set_web_api_key("replacementkey").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        svc.remove().unwrap();
        assert!(!path.exists());
    }
}
