//! What Horizon still reads from a Steam installation: the active account's
//! screenshots for the Album (`SteamCaptures`) and Steam Input's list of the
//! controllers behind its virtual pads (`virtual_gamepad_info_file`). Steam is
//! no longer a game source (2026-10-12).

use std::{
    collections::BTreeSet,
    env, fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

use steam_vdf_parser::{Obj, parse_text};

use crate::domain::{ExternalGameId, SourceId, album::MediaKind};

use super::{CaptureSource, SourceCapture, SourceInitializationError};

const STEAM_SOURCE_ID: &str = "steam";

const STEAM_ID64_ACCOUNT_BASE: u64 = 76_561_197_960_265_728;

/// `userdata/<account>/` of the account in use: the one Steam marks
/// `MostRecent` in `loginusers.vdf`, otherwise a lone account. Ambiguity
/// reports nothing rather than guessing.
fn active_account_dir(root: &Path) -> Option<PathBuf> {
    let userdata = root.join("userdata");
    let accounts = fs::read_dir(&userdata)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            // Account 0 is Steam's anonymous placeholder folder.
            (name.parse::<u64>().ok()? > 0).then_some(name)
        })
        .collect::<BTreeSet<_>>();

    let recent = fs::read_to_string(root.join("config/loginusers.vdf"))
        .ok()
        .and_then(|content| most_recent_account_id(&content))
        .filter(|account| accounts.contains(account));

    let account = match recent {
        Some(account) => account,
        None if accounts.len() == 1 => accounts.into_iter().next()?,
        None => return None,
    };
    Some(userdata.join(account))
}

fn most_recent_account_id(content: &str) -> Option<String> {
    let vdf = parse_text(content).ok()?;
    let root = vdf.as_obj()?;
    let users = vdf_key_ci(root, "users")
        .and_then(|key| root.get(key.as_str()))
        .and_then(|value| value.as_obj())
        .unwrap_or(root);
    let mut most_recent = None;
    for (steam_id, value) in users.iter() {
        let Some(entry) = value.as_obj() else {
            continue;
        };
        let Some(key) = vdf_key_ci(entry, "MostRecent") else {
            continue;
        };
        if entry.get(key.as_str()).and_then(|value| value.as_str()) != Some("1") {
            continue;
        }
        let account = steam_id
            .parse::<u64>()
            .ok()
            .and_then(|id| id.checked_sub(STEAM_ID64_ACCOUNT_BASE))
            .filter(|account| *account != 0);
        let Some(account) = account else { continue };
        // Multiple MostRecent=1 entries are ambiguous. Do not let the VDF's
        // enumeration order pick an arbitrary account.
        if most_recent.replace(account.to_string()).is_some() {
            return None;
        }
    }
    most_recent
}

/// Case-insensitive key lookup; Steam has changed key casing between clients.
fn vdf_key_ci(object: &Obj<'_>, key: &str) -> Option<String> {
    object
        .keys()
        .find(|candidate| candidate.eq_ignore_ascii_case(key))
        .map(|candidate| candidate.to_string())
}

/// Steam's list of the real controllers behind Steam Input's virtual pads
/// (`config/virtualgamepadinfo.txt`), the newest in any Steam root. Steam
/// names it in `SteamVirtualGamepadInfo` for what it launches; SDL reads it.
pub fn virtual_gamepad_info_file() -> Option<PathBuf> {
    default_steam_roots()
        .into_iter()
        .map(|root| root.join("config/virtualgamepadinfo.txt"))
        .filter_map(|file| {
            let modified = fs::metadata(&file).and_then(|meta| meta.modified()).ok()?;
            Some((modified, file))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, file)| file)
}

fn default_steam_roots() -> Vec<PathBuf> {
    let Some(home) = env::var_os("HOME").map(PathBuf::from) else {
        return vec![];
    };

    let mut candidates = Vec::new();
    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        let data_home = PathBuf::from(data_home);
        candidates.push(data_home.join("Steam"));
        candidates.push(data_home.join("steam"));
    }
    // Flatpak exposes the host's XDG data directory separately from the
    // sandbox's own XDG_DATA_HOME. Honor it when available so custom host XDG
    // layouts can be discovered without granting broad filesystem access.
    if let Some(data_home) = env::var_os("HOST_XDG_DATA_HOME") {
        let data_home = PathBuf::from(data_home);
        candidates.push(data_home.join("Steam"));
        candidates.push(data_home.join("steam"));
    }

    candidates.extend([
        home.join(".local/share/Steam"),
        home.join(".local/share/steam"),
        home.join(".steam/steam"),
        home.join(".steam/debian-installation"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/steam"),
        home.join(".var/app/com.valvesoftware.Steam/data/Steam"),
        home.join(".var/app/com.valvesoftware.Steam/data/steam"),
    ]);

    let mut seen = BTreeSet::new();
    candidates
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

/// Steam's own screenshots, read in place: `userdata/<account>/760/remote/
/// <appid>/screenshots/`. Only the active account's (see `active_account_dir`),
/// so another person's screenshots on a shared PC never appear.
pub struct SteamCaptures {
    source_id: SourceId,
    roots: Vec<PathBuf>,
}

impl SteamCaptures {
    pub fn new() -> Result<Self, SourceInitializationError> {
        Ok(Self {
            source_id: SourceId::new(STEAM_SOURCE_ID)?,
            roots: default_steam_roots(),
        })
    }

    #[cfg(test)]
    fn with_roots(roots: Vec<PathBuf>) -> Self {
        Self {
            source_id: SourceId::new(STEAM_SOURCE_ID).expect("steam source id"),
            roots,
        }
    }
}

impl CaptureSource for SteamCaptures {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    fn captures(&self) -> Vec<SourceCapture> {
        let mut seen_accounts = BTreeSet::new();
        let mut captures = Vec::new();
        for root in &self.roots {
            let Some(account) = active_account_dir(root) else {
                continue;
            };
            // Several candidate roots are usually links to one installation.
            let identity = fs::canonicalize(&account).unwrap_or_else(|_| account.clone());
            if !seen_accounts.insert(identity) {
                continue;
            }
            let Ok(games) = fs::read_dir(account.join("760/remote")) else {
                continue;
            };
            for game in games.filter_map(Result::ok) {
                let Some(app_id) = game
                    .file_name()
                    .to_str()
                    .filter(|name| name.parse::<u32>().is_ok_and(|id| id > 0))
                    .map(str::to_owned)
                else {
                    continue;
                };
                let external_id = ExternalGameId::new(app_id).ok();
                let folder = game.path().join("screenshots");
                let Ok(files) = fs::read_dir(&folder) else {
                    continue;
                };
                let deletable = super::support::writable(&folder);
                for file in files.filter_map(Result::ok) {
                    let path = file.path();
                    let Some(kind) = MediaKind::from_path(&path) else {
                        continue;
                    };
                    let Ok(metadata) = file.metadata() else {
                        continue;
                    };
                    if !metadata.is_file() {
                        continue;
                    }
                    let name = file.file_name();
                    let captured_at = name
                        .to_str()
                        .and_then(steam_screenshot_time)
                        .or_else(|| metadata.modified().ok().and_then(unix_millis))
                        .unwrap_or(0);
                    let preview = folder.join("thumbnails").join(&name);
                    captures.push(SourceCapture {
                        path,
                        kind,
                        external_id: external_id.clone(),
                        captured_at,
                        preview: preview.is_file().then_some(preview),
                        deletable,
                    });
                }
            }
        }
        captures
    }

    /// Deletes a screenshot and Steam's thumbnail of it. Only a media file
    /// directly in `userdata/<active account>/760/remote/<appid>/screenshots/`
    /// of a known Steam root qualifies; links are resolved first, so nothing
    /// outside those folders can be reached. Steam's own `screenshots.vdf`
    /// index is left to Steam.
    fn delete(&self, path: &Path) -> io::Result<()> {
        let refuse = || io::Error::new(io::ErrorKind::PermissionDenied, "not a Steam screenshot");
        let file = fs::canonicalize(path)?;
        if !fs::symlink_metadata(&file)?.is_file() || MediaKind::from_path(&file).is_none() {
            return Err(refuse());
        }
        let folder = file.parent().ok_or_else(refuse)?;
        let game = folder.parent().ok_or_else(refuse)?;
        let remote = game.parent().ok_or_else(refuse)?;
        let screenshots = remote.parent().ok_or_else(refuse)?;
        let account = screenshots.parent().ok_or_else(refuse)?;
        let named = |dir: &Path, name: &str| dir.file_name().is_some_and(|n| n == name);
        let is_game = game
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.parse::<u32>().is_ok_and(|id| id > 0));
        if !named(folder, "screenshots")
            || !is_game
            || !named(remote, "remote")
            || !named(screenshots, "760")
        {
            return Err(refuse());
        }
        let known = self
            .roots
            .iter()
            .filter_map(|root| active_account_dir(root))
            .filter_map(|dir| fs::canonicalize(dir).ok())
            .any(|dir| dir == account);
        if !known {
            return Err(refuse());
        }
        fs::remove_file(&file)?;
        if let Some(name) = file.file_name() {
            // Steam's small copy; absent is fine.
            let _ = fs::remove_file(folder.join("thumbnails").join(name));
        }
        Ok(())
    }
}

/// Steam names screenshots `YYYYMMDDHHMMSS_N.jpg` in local time.
fn steam_screenshot_time(name: &str) -> Option<i64> {
    use chrono::{Local, NaiveDateTime, TimeZone};
    let stamp = name.get(..14)?;
    let naive = NaiveDateTime::parse_from_str(stamp, "%Y%m%d%H%M%S").ok()?;
    Some(
        Local
            .from_local_datetime(&naive)
            .earliest()?
            .timestamp_millis(),
    )
}

fn unix_millis(time: SystemTime) -> Option<i64> {
    let millis = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_millis();
    i64::try_from(millis).ok()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            env::temp_dir().join(format!("horizon-steam-{name}-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).expect("temp directory");
        path
    }

    #[test]
    fn active_account_prefers_the_most_recent_login() {
        let root = temp_dir("account-recent");
        for account in ["1000", "2000"] {
            fs::create_dir_all(root.join("userdata").join(account)).expect("account");
        }
        fs::create_dir_all(root.join("config")).expect("config");
        let id_1000 = STEAM_ID64_ACCOUNT_BASE + 1000;
        let id_2000 = STEAM_ID64_ACCOUNT_BASE + 2000;
        fs::write(
            root.join("config/loginusers.vdf"),
            format!(
                "\"users\" {{ \"{id_1000}\" {{ \"MostRecent\" \"0\" }} \"{id_2000}\" {{ \"MostRecent\" \"1\" }} }}"
            ),
        )
        .expect("loginusers");
        assert_eq!(active_account_dir(&root), Some(root.join("userdata/2000")));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn multiple_most_recent_markers_are_ambiguous() {
        let id_1000 = STEAM_ID64_ACCOUNT_BASE + 1000;
        let id_2000 = STEAM_ID64_ACCOUNT_BASE + 2000;
        let loginusers = format!(
            "\"users\" {{ \"{id_1000}\" {{ \"MostRecent\" \"1\" }} \"{id_2000}\" {{ \"MostRecent\" \"1\" }} }}"
        );
        assert_eq!(most_recent_account_id(&loginusers), None);
    }

    #[test]
    fn captures_list_the_single_accounts_screenshots_with_their_thumbnails() {
        let root = temp_dir("captures");
        let shots = root.join("userdata/123/760/remote/1245620/screenshots");
        fs::create_dir_all(shots.join("thumbnails")).expect("screenshots");
        fs::write(shots.join("20261010143012_1.jpg"), b"jpg").expect("shot");
        fs::write(shots.join("thumbnails/20261010143012_1.jpg"), b"jpg").expect("thumb");
        fs::write(shots.join("other.png"), b"png").expect("undated shot");
        fs::write(shots.join("screenshots.vdf"), b"vdf").expect("not media");
        // Non-numeric folders are not Steam games.
        fs::create_dir_all(root.join("userdata/123/760/remote/junk/screenshots")).expect("junk");
        // The same installation reached through two roots is listed once.
        let captures = SteamCaptures::with_roots(vec![root.clone(), root.clone()]).captures();

        assert_eq!(captures.len(), 2);
        let dated = captures
            .iter()
            .find(|c| c.path.ends_with("20261010143012_1.jpg"))
            .expect("dated capture");
        assert_eq!(dated.kind, MediaKind::Photo);
        assert_eq!(
            dated.external_id.as_ref().map(ExternalGameId::as_str),
            Some("1245620")
        );
        assert_eq!(
            dated.captured_at,
            steam_screenshot_time("20261010143012_1.jpg").expect("time")
        );
        assert!(
            dated
                .preview
                .as_ref()
                .is_some_and(|p| p.ends_with("thumbnails/20261010143012_1.jpg"))
        );
        let undated = captures
            .iter()
            .find(|c| c.path.ends_with("other.png"))
            .expect("undated");
        assert!(
            undated.captured_at > 0,
            "falls back to the file's modified time"
        );
        assert!(undated.preview.is_none());
        assert!(
            dated.deletable,
            "a writable folder's screenshots can be deleted"
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn deleting_removes_a_screenshot_and_its_thumbnail_and_nothing_else() {
        let root = temp_dir("delete");
        let shots = root.join("userdata/123/760/remote/1245620/screenshots");
        fs::create_dir_all(shots.join("thumbnails")).expect("screenshots");
        fs::write(shots.join("20261010143012_1.jpg"), b"jpg").expect("shot");
        fs::write(shots.join("thumbnails/20261010143012_1.jpg"), b"jpg").expect("thumb");
        fs::write(shots.join("screenshots.vdf"), b"vdf").expect("index");
        fs::write(root.join("userdata/123/localconfig.jpg"), b"x").expect("elsewhere");
        let steam = SteamCaptures::with_roots(vec![root.clone()]);

        steam
            .delete(&shots.join("20261010143012_1.jpg"))
            .expect("deleted");
        assert!(!shots.join("20261010143012_1.jpg").exists());
        assert!(!shots.join("thumbnails/20261010143012_1.jpg").exists());
        assert!(
            shots.join("screenshots.vdf").exists(),
            "Steam's index is Steam's"
        );

        // Anything else is refused: not media, not in a screenshots folder,
        // or reached through `..`.
        assert!(steam.delete(&shots.join("screenshots.vdf")).is_err());
        assert!(
            steam
                .delete(&root.join("userdata/123/localconfig.jpg"))
                .is_err()
        );
        assert!(
            steam
                .delete(&shots.join("../../../../localconfig.jpg"))
                .is_err()
        );
        assert!(root.join("userdata/123/localconfig.jpg").exists());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn captures_ignore_ambiguous_accounts() {
        let root = temp_dir("captures-ambiguous");
        for account in ["111", "222"] {
            let shots = root.join(format!("userdata/{account}/760/remote/10/screenshots"));
            fs::create_dir_all(&shots).expect("screenshots");
            fs::write(shots.join("20261010143012_1.jpg"), b"jpg").expect("shot");
        }
        assert!(
            SteamCaptures::with_roots(vec![root.clone()])
                .captures()
                .is_empty()
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
