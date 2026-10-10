//! Read-only Steam achievement import. Never mutates Steam stats or game state.
//!
//! The public Steam Web API requires a user-supplied Web API key and SteamID64.
//! Keep configuration opt-in; do not scrape undocumented client caches or
//! silently interpret inaccessible profiles as having zero achievements.
use super::steam_account::{SteamAccountCredentials, SteamWebApiClient};
use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::mpsc::Sender,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::domain::LibraryGame;

const ENDPOINT: &str = "https://api.steampowered.com/ISteamUserStats/GetPlayerAchievements/v1/";
const SCHEMA_ENDPOINT: &str = "https://api.steampowered.com/ISteamUserStats/GetSchemaForGame/v2/";
// Prefetch only a small number from each game at import. Opening a game
// requests remaining icons on a separate worker and caches them by SteamID.
const BADGE_LIMIT_PER_GAME: usize = 7;
const FRESH_SECS: u64 = 30 * 60;
const MAX_STALE_SECS: u64 = 7 * 24 * 60 * 60;
const BADGE_BYTES: usize = 56 * 56 * 4;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SteamGame {
    pub app_id: u32,
    pub title: String,
}

/// Only provider-owned Steam references are eligible. Heroic IDs must never be
/// interpreted as Steam AppIDs even if they happen to contain digits.
pub fn steam_games(library: &[LibraryGame]) -> Vec<SteamGame> {
    let mut games = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for game in library {
        for source in game.sources() {
            if source.source_id().as_str() != "steam" {
                continue;
            }
            let Ok(app_id) = source.external_id().as_str().parse::<u32>() else {
                continue;
            };
            if app_id == 0 || !seen.insert(app_id) {
                continue;
            }
            games.push(SteamGame {
                app_id,
                title: game.game().title().as_str().to_owned(),
            });
        }
    }
    games.sort_by_key(|a| a.title.to_lowercase());
    games
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SteamAchievement {
    pub api_name: String,
    #[serde(skip)]
    pub badge_rgba: Option<Vec<u8>>,
    pub name: String,
    pub description: String,
    pub unlocked: bool,
    pub unlocked_at: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SteamGameAchievements {
    pub game: SteamGame,
    pub achievements: Vec<SteamAchievement>,
}

pub enum AchievementImportEvent {
    Cached(Vec<SteamGameAchievements>),
    // IDs confirmed accessible during the current refresh. Historical cached
    // records are dropped if Steam now marks their data unavailable/private.
    Refreshed(Vec<u32>),
    Imported(SteamGameAchievements),
    Badge {
        app_id: u32,
        api_name: String,
        rgba: Vec<u8>,
    },
    Unavailable,
    Finished,
}

// An optional, private XDG cache. Never stores the API key or Steam password.
// Account-scoped filenames prevent accidental cross-account result mixing.
#[derive(Serialize, Deserialize)]
struct CatalogCache {
    version: u32,
    steam_id64: String,
    fetched_at: u64,
    requested_appids: Vec<u32>,
    accessible: Vec<SteamGameAchievements>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn cache_directory(steam_id: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let root = std::env::var_os("XDG_CACHE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|path| path.join(".cache")))?;
    if !root.is_absolute() {
        return None;
    }
    let dir = root
        .join("io.github.Mars7x.Horizon")
        .join("achievements")
        .join(format!("steam-{steam_id}"));
    fs::create_dir_all(&dir).ok()?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).ok()?;
    Some(dir)
}

pub fn purge_achievement_cache(steam_id: &str) {
    if steam_id.len() != 17 || !steam_id.bytes().all(|b| b.is_ascii_digit()) {
        return;
    }
    if let Some(dir) = cache_directory(steam_id) {
        let _ = fs::remove_dir_all(dir);
    }
}

fn badge_filename(app_id: u32, api_name: &str, unlocked: bool) -> String {
    // Stable FNV-1a over the provider's API identifier (not over artwork or
    // credentials). Only a hex filename is emitted; no path is user-derived.
    let mut h: u64 = 0xcbf29ce484222325;
    for byte in api_name.as_bytes() {
        h = (h ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    // Steam serves different locked and unlocked artwork. Keying the state
    // keeps a cached locked icon from outliving the unlock itself.
    let state = if unlocked { "u" } else { "l" };
    format!("{app_id}-{h:016x}-{state}.rgba")
}

fn read_badges(dir: &Path, games: &mut [SteamGameAchievements]) {
    for game in games {
        // Load all thumbnails already fetched (prefetch AND detail on-demand).
        for a in &mut game.achievements {
            let path = dir.join(badge_filename(game.game.app_id, &a.api_name, a.unlocked));
            if let Ok(bytes) = fs::read(path)
                && bytes.len() == BADGE_BYTES
            {
                a.badge_rgba = Some(bytes);
            }
        }
    }
}

fn write_private_file(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

fn read_catalog(
    dir: &Path,
    id: &str,
    requested: &[u32],
) -> Option<(Vec<SteamGameAchievements>, bool)> {
    let bytes = fs::read(dir.join("catalog.json")).ok()?;
    if bytes.len() > 12 * 1024 * 1024 {
        return None;
    }
    let record: CatalogCache = serde_json::from_slice(&bytes).ok()?;
    let age = now_secs().checked_sub(record.fetched_at)?;
    if record.version != 1
        || record.steam_id64 != id
        || record.requested_appids.as_slice() != requested
        || age > MAX_STALE_SECS
    {
        return None;
    }
    let mut games = record.accessible;
    read_badges(dir, &mut games);
    Some((games, age <= FRESH_SECS))
}

fn save_catalog(dir: &Path, id: &str, requested: Vec<u32>, accessible: &[SteamGameAchievements]) {
    let record = CatalogCache {
        version: 1,
        steam_id64: id.to_owned(),
        fetched_at: now_secs(),
        requested_appids: requested,
        accessible: accessible.to_vec(),
    };
    if let Ok(bytes) = serde_json::to_vec(&record)
        && bytes.len() <= 12 * 1024 * 1024
    {
        let _ = write_private_file(&dir.join("catalog.json"), &bytes);
    }
}

#[derive(Deserialize)]
struct ApiResponse {
    playerstats: Option<ApiPlayerStats>,
}
#[derive(Deserialize)]
struct ApiPlayerStats {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    achievements: Vec<ApiAchievement>,
}
#[derive(Deserialize)]
struct ApiAchievement {
    apiname: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    achieved: u8,
    #[serde(default)]
    unlocktime: u64,
}

fn parse_response(json: &str, game: SteamGame) -> Option<SteamGameAchievements> {
    let result: ApiResponse = serde_json::from_str(json).ok()?;
    let stats = result.playerstats?;
    if !stats.success || stats.achievements.is_empty() {
        return None;
    }
    let achievements = stats
        .achievements
        .into_iter()
        .map(|entry| SteamAchievement {
            api_name: entry.apiname.clone(),
            badge_rgba: None,
            name: entry
                .name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(entry.apiname),
            description: entry.description.unwrap_or_default(),
            unlocked: entry.achieved == 1,
            unlocked_at: if entry.achieved == 1 && entry.unlocktime > 0 {
                Some(entry.unlocktime)
            } else {
                None
            },
        })
        .collect();
    Some(SteamGameAchievements { game, achievements })
}

// Steam's schema adds genuine per-achievement badge URLs. It is never treated
// as completion data, and a missing schema cannot hide valid unlock results.
fn badge_urls(json: &str) -> HashMap<String, (String, String)> {
    let mut result = HashMap::new();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return result;
    };
    let Some(entries) = value
        .pointer("/game/availableGameStats/achievements")
        .and_then(serde_json::Value::as_array)
    else {
        return result;
    };
    for entry in entries {
        let Some(name) = entry.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let active = entry
            .get("icon")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let locked = entry
            .get("icongray")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        result.insert(name.to_owned(), (active.to_owned(), locked.to_owned()));
    }
    result
}

/// Called only on a background thread. An individual game may be inaccessible,
/// have no achievements, or fail; never convert these into "0 unlocked".
pub fn import_steam_achievements(
    credentials: SteamAccountCredentials,
    games: Vec<SteamGame>,
    sender: Sender<AchievementImportEvent>,
) {
    let steam_id = credentials.steam_id64().to_owned();
    let directory = cache_directory(&steam_id);
    let requested = games.iter().map(|g| g.app_id).collect::<Vec<_>>();
    if let Some((cached, fresh)) = directory
        .as_ref()
        .and_then(|dir| read_catalog(dir, &steam_id, &requested))
    {
        if sender.send(AchievementImportEvent::Cached(cached)).is_err() {
            return;
        }
        if fresh {
            let _ = sender.send(AchievementImportEvent::Finished);
            return; // Reuse all data and badge thumbnails while cache is fresh.
        }
    }

    let client = match SteamWebApiClient::new(credentials) {
        Ok(client) => client,
        Err(_) => {
            let _ = sender.send(AchievementImportEvent::Finished);
            return;
        }
    };
    // Stream completions first. Badge HTTP never blocks another game's status.
    let mut accessible = Vec::new();
    let mut fetched_ids = BTreeSet::new();
    for game in games {
        let app_id = game.app_id.to_string();
        let response = client.get(ENDPOINT, &[("appid", app_id.as_str()), ("l", "english")]);
        let Some(mut data) = response.ok().and_then(|body| parse_response(&body, game)) else {
            if sender.send(AchievementImportEvent::Unavailable).is_err() {
                return;
            }
            continue;
        };
        if let Some(dir) = directory.as_ref() {
            read_badges(dir, std::slice::from_mut(&mut data));
        }
        fetched_ids.insert(data.game.app_id);
        if sender
            .send(AchievementImportEvent::Imported(data.clone()))
            .is_err()
        {
            return;
        }
        accessible.push(data);
    }
    // Never turn an all-failed/offline scan into a fresh 30-minute empty cache.
    if !accessible.is_empty()
        && let Some(dir) = directory.as_ref()
    {
        save_catalog(dir, &steam_id, requested, &accessible);
    }
    if sender
        .send(AchievementImportEvent::Refreshed(
            fetched_ids.into_iter().collect(),
        ))
        .is_err()
    {
        return;
    }
    if sender.send(AchievementImportEvent::Finished).is_err() {
        return;
    }

    // Remaining work is artwork only. Keep it bounded and store thumbnails
    // for the next launch so fresh-cache visits don't issue badge HTTP calls.
    for data in accessible {
        let app_id = data.game.app_id.to_string();
        let mut prioritized = data.achievements.iter().collect::<Vec<_>>();
        prioritized.sort_by(|a, b| {
            b.unlocked
                .cmp(&a.unlocked)
                .then_with(|| b.unlocked_at.cmp(&a.unlocked_at))
        });
        if prioritized
            .iter()
            .take(BADGE_LIMIT_PER_GAME)
            .all(|a| a.badge_rgba.is_some())
        {
            continue;
        }
        let Some(schema) = client
            .get(
                SCHEMA_ENDPOINT,
                &[("appid", app_id.as_str()), ("l", "english")],
            )
            .ok()
        else {
            continue;
        };
        let wanted = prioritized.into_iter().take(BADGE_LIMIT_PER_GAME);
        if !download_badges(
            &client,
            &schema,
            directory.as_deref(),
            data.game.app_id,
            wanted,
            &sender,
        ) {
            return;
        }
    }
}

/// Downloads, caches and streams each wanted badge that is not yet present.
/// Returns false once the receiving controller has gone away.
fn download_badges<'a>(
    client: &SteamWebApiClient,
    schema: &str,
    directory: Option<&Path>,
    app_id: u32,
    wanted: impl Iterator<Item = &'a SteamAchievement>,
    sender: &Sender<AchievementImportEvent>,
) -> bool {
    let urls = badge_urls(schema);
    for achievement in wanted {
        if achievement.badge_rgba.is_some() {
            continue;
        }
        let Some((active, locked)) = urls.get(&achievement.api_name) else {
            continue;
        };
        let url = if achievement.unlocked { active } else { locked };
        let Some(rgba) = client.download_badge(url) else {
            continue;
        };
        if let Some(dir) = directory {
            let _ = write_private_file(
                &dir.join(badge_filename(
                    app_id,
                    &achievement.api_name,
                    achievement.unlocked,
                )),
                &rgba,
            );
        }
        if sender
            .send(AchievementImportEvent::Badge {
                app_id,
                api_name: achievement.api_name.clone(),
                rgba,
            })
            .is_err()
        {
            return false;
        }
    }
    true
}

/// Fills in genuine Steam badge thumbnails for an opened game. Always runs on
/// a background thread and shares the same account-scoped cache and event
/// channel as the normal importer. Invalid/unavailable images stay fallback.
/// Missing artwork never blocks browsing or suppresses achievement records.
pub fn hydrate_game_badges(
    credentials: SteamAccountCredentials,
    mut game: SteamGameAchievements,
    sender: Sender<AchievementImportEvent>,
) {
    let directory = cache_directory(credentials.steam_id64());
    let missing = game
        .achievements
        .iter()
        .filter(|a| a.badge_rgba.is_none())
        .map(|a| a.api_name.clone())
        .collect::<BTreeSet<_>>();
    if let Some(dir) = directory.as_ref() {
        read_badges(dir, std::slice::from_mut(&mut game));
    }
    // Emit thumbnails another worker cached since the controller's snapshot,
    // before any network request. Badges it already holds are not re-sent.
    for a in game
        .achievements
        .iter()
        .filter(|a| missing.contains(&a.api_name))
    {
        if let Some(rgba) = &a.badge_rgba
            && sender
                .send(AchievementImportEvent::Badge {
                    app_id: game.game.app_id,
                    api_name: a.api_name.clone(),
                    rgba: rgba.clone(),
                })
                .is_err()
        {
            return;
        }
    }
    if game.achievements.iter().all(|a| a.badge_rgba.is_some()) {
        return;
    }
    let Ok(client) = SteamWebApiClient::new(credentials) else {
        return;
    };
    let app_id = game.game.app_id.to_string();
    let Ok(schema) = client.get(
        SCHEMA_ENDPOINT,
        &[("appid", app_id.as_str()), ("l", "english")],
    ) else {
        return;
    };
    download_badges(
        &client,
        &schema,
        directory.as_deref(),
        game.game.app_id,
        game.achievements.iter(),
        &sender,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_round_trip_is_account_and_catalog_scoped() {
        let dir = std::env::temp_dir().join(format!(
            "horizon-achievements-test-{}-{}",
            std::process::id(),
            now_secs()
        ));
        fs::create_dir_all(&dir).unwrap();
        let game = SteamGameAchievements {
            game: SteamGame {
                app_id: 480,
                title: "Test Game".into(),
            },
            achievements: vec![SteamAchievement {
                api_name: "WIN".into(),
                badge_rgba: Some(vec![5; BADGE_BYTES]),
                name: "Won".into(),
                description: "A victory".into(),
                unlocked: true,
                unlocked_at: Some(123),
            }],
        };
        save_catalog(&dir, "76561198000000000", vec![480], &[game]);
        let (loaded, fresh) = read_catalog(&dir, "76561198000000000", &[480]).unwrap();
        assert!(fresh);
        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].achievements[0].unlocked);
        assert!(loaded[0].achievements[0].badge_rgba.is_none());
        assert!(read_catalog(&dir, "76561198000000001", &[480]).is_none());
        assert!(read_catalog(&dir, "76561198000000000", &[481]).is_none());
        assert_eq!(
            fs::metadata(dir.join("catalog.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn badge_cache_filename_is_safe_and_stable() {
        let name = badge_filename(480, "some/../achievement", true);
        assert!(name.starts_with("480-"));
        assert!(name.ends_with(".rgba"));
        assert!(!name.contains('/'));
        assert_eq!(name, badge_filename(480, "some/../achievement", true));
        // Unlocking must not reuse the cached locked artwork.
        assert_ne!(name, badge_filename(480, "some/../achievement", false));
    }
    #[test]
    fn only_true_steam_app_ids_are_imported() {
        use crate::domain::{ExternalGameId, Game, GameId, GameTitle, SourceGameRef, SourceId};
        let mk_game = |id, src: &str, external: &str| {
            LibraryGame::new(
                Game::new(
                    GameId::new(id).unwrap(),
                    GameTitle::new(format!("Title {id}")).unwrap(),
                ),
                vec![SourceGameRef::new(
                    SourceId::new(src).unwrap(),
                    ExternalGameId::new(external).unwrap(),
                )],
            )
        };
        let library = vec![
            mk_game(1, "steam", "480"),
            mk_game(2, "heroic", "480"),
            mk_game(3, "steam", "not-numeric"),
            mk_game(4, "steam", "480"),
            mk_game(5, "steam", "0"),
        ];
        let games = steam_games(&library);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].app_id, 480);
    }
    #[test]
    fn credentials_reject_bad_steam_id() {
        assert!(SteamAccountCredentials::new("key", "not-an-id").is_err());
        assert!(SteamAccountCredentials::new("key", "0").is_err());
        assert!(SteamAccountCredentials::new("key", "76561198000000000").is_ok());
    }
    #[test]
    fn parses_unlocks_but_not_private_data_as_zero() {
        let game = SteamGame {
            app_id: 480,
            title: "Test".into(),
        };
        let json = r#"{"playerstats":{"success":true,"achievements":[{"apiname":"WIN","name":"Victory","achieved":1,"unlocktime":123},{"apiname":"SECRET","achieved":0,"unlocktime":0}]}}"#;
        let result = parse_response(json, game.clone()).unwrap();
        assert_eq!(result.achievements.len(), 2);
        assert_eq!(result.achievements[0].unlocked_at, Some(123));
        assert!(!result.achievements[1].unlocked);
        assert_eq!(result.achievements[1].name, "SECRET");
        assert!(parse_response(r#"{"playerstats":{"success":false}}"#, game).is_none());
    }
    #[test]
    fn steam_schema_urls_match_by_api_name_not_display_name() {
        let json = r#"{"game":{"availableGameStats":{"achievements":[{"name":"FIRST_STEP","displayName":"First Step","icon":"https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps/1/foo.jpg","icongray":""}]}}}"#;
        let urls = badge_urls(json);
        assert!(urls.contains_key("FIRST_STEP"));
        assert!(!urls.contains_key("First Step"));
        assert!(badge_urls("{}").is_empty());
    }
}
