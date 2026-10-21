//! The one place that decides where Horizon keeps files on disk.
//!
//! Under Flatpak, `XDG_DATA_HOME`, `XDG_CONFIG_HOME` and `XDG_CACHE_HOME` are
//! already private to the app (`~/.var/app/<app-id>/{data,config,cache}`), so
//! Horizon writes directly into them. Outside Flatpak those directories are
//! shared with every other program, so Horizon uses an `<app-id>` subfolder.
//!
//! ```text
//! data/    library/  saves/  album/  profile/  artwork/custom/
//! config/  appearance.json  controller.json  playback.json  cores/  integrations/   (0700 dirs)
//! cache/   artwork/local/  artwork/steamgriddb/  album/  (disposable)
//! ```
//!
//! Every cache folder holds a `.version` file. When a service changes its
//! cache format it bumps its version and the folder is cleared on next start;
//! caches are never migrated. Services receive their folders from `app.rs`
//! and never resolve paths themselves.
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use std::os::unix::fs::PermissionsExt;
use thiserror::Error;

const APP_ID: &str = "io.github.Mars7x.Horizon";
const DATABASE_FILE: &str = "library/library.sqlite3";
const STEAMGRIDDB_FILE: &str = "integrations/steamgriddb.json";
const APPEARANCE_FILE: &str = "appearance.json";
const STEAM_ACCOUNT_FILE: &str = "integrations/steam.json";
const PROFILE_FILE: &str = "profile/profile.json";
const CONTROLLER_FILE: &str = "controller.json";
const CACHE_VERSION_FILE: &str = ".version";
const ALBUM_DIR: &str = "album";

/// Only the packaged core is loaded in Flatpak. Native development can opt in
/// to a built core; this override is never exposed through application settings.
pub fn gba_core_path() -> PathBuf {
    if std::env::var("FLATPAK_ID").is_ok_and(|id| id == "io.github.Mars7x.Horizon") {
        PathBuf::from("/app/lib/horizon/mgba_libretro.so")
    } else {
        std::env::var_os("HORIZON_GBA_CORE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/lib/horizon/mgba_libretro.so"))
    }
}

pub fn ds_core_path() -> PathBuf {
    if std::env::var("FLATPAK_ID").is_ok_and(|id| id == "io.github.Mars7x.Horizon") {
        PathBuf::from("/app/lib/horizon/melondsds_libretro.so")
    } else {
        std::env::var_os("HORIZON_DS_CORE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/lib/horizon/melondsds_libretro.so"))
    }
}

/// melonDS DS recognizes this final component and does not add another subfolder.
pub fn ds_support_directory(game: &Path) -> PathBuf {
    game.join("core-data/melonDS DS")
}

pub fn ds_state_core_directory(game: &Path) -> PathBuf {
    game.join("states/melonds-ds")
}

pub fn gba_save_directory() -> Result<PathBuf, DataPathError> {
    Ok(app_dir(BaseDir::Data)?.join("saves"))
}

/// Shared playback policy and core-specific preferences have separate owners.
#[derive(Debug, Clone)]
pub struct CoreSettingsPaths {
    pub playback: PathBuf,
    pub mgba: PathBuf,
    pub ds: PathBuf,
}

const PROFILE_PICTURE_FILE: &str = "profile/pictures/custom.jxl";
const PROFILE_ANIMATED_FILE: &str = "profile/pictures/custom-animation.jxl";
const STEAM_PICTURE_FILE: &str = "profile/pictures/steam.jxl";
const STEAM_ANIMATED_FILE: &str = "profile/pictures/steam-animation.jxl";

/// Filesystem-safe, readable names with a bounded UTF-8 byte length.
pub fn game_file_stem(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut stem = String::new();
    for c in cleaned.trim_matches([' ', '.']).chars() {
        if stem.len() + c.len_utf8() > 120 {
            break;
        }
        stem.push(c);
    }
    let stem = stem.trim_end_matches([' ', '.']);
    if stem.is_empty() {
        return "Untitled game".into();
    }
    let device = stem.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (device.len() == 4
            && (device.starts_with("COM") || device.starts_with("LPT"))
            && matches!(device.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        game_file_stem(&format!("Game {stem}"))
    } else {
        stem.to_owned()
    }
}

pub fn game_system_directory(root: &Path, system: &str) -> PathBuf {
    root.join(crate::domain::album::encode_path_segment(system))
}
pub fn game_directory(system: &Path, stem: &str, suffix: &str) -> PathBuf {
    system.join(format!("{stem} [{suffix}]"))
}
pub fn game_manifest_path(directory: &Path) -> PathBuf {
    directory.join("game.json")
}
pub fn game_manifest_temporary_path(directory: &Path, nonce: u64) -> PathBuf {
    directory.join(format!(".game.json.{}.{nonce}.part", std::process::id()))
}
pub fn game_save_path(directory: &Path, stem: &str) -> PathBuf {
    directory.join(format!("{stem}.sav"))
}
pub fn save_backup_path(save: &Path) -> std::io::Result<PathBuf> {
    let parent = save
        .parent()
        .ok_or_else(|| std::io::Error::other("save has no folder"))?;
    let stem = save
        .file_stem()
        .ok_or_else(|| std::io::Error::other("save has no name"))?;
    let mut name = stem.to_os_string();
    name.push(".previous.sav");
    Ok(parent.join("backups").join(name))
}
pub fn save_temporary_path(save: &Path) -> PathBuf {
    let mut name = save.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}
pub fn capture_directory(directory: &Path, video: bool) -> PathBuf {
    directory.join(if video { "recordings" } else { "screenshots" })
}
pub fn unassigned_album_directory(root: &Path) -> PathBuf {
    root.join("horizon")
}
pub fn capture_file(directory: &Path, timestamp: &str, extension: &str, copy: u32) -> PathBuf {
    directory.join(if copy == 1 {
        format!("{timestamp}.{extension}")
    } else {
        format!("{timestamp}-{copy}.{extension}")
    })
}

/// Paths for a complete state bundle; selectors never create files.
#[derive(Debug)]
pub struct SaveStatePaths {
    pub state: PathBuf,
    pub metadata: PathBuf,
    pub screenshot: PathBuf,
}

#[derive(Debug, Error)]
pub enum DataPathError {
    #[error("neither the XDG base directory nor HOME provides an application location")]
    MissingDataRoot,
    #[error("{variable} must be an absolute path, got {path}")]
    RelativeEnvironmentPath {
        variable: &'static str,
        path: PathBuf,
    },
    #[error("failed to create or secure Horizon storage directory {path}: {source}")]
    CreateStorageDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// The three XDG base directories Horizon uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BaseDir {
    Data,
    Config,
    Cache,
}

impl BaseDir {
    const fn variable(self) -> &'static str {
        match self {
            Self::Data => "XDG_DATA_HOME",
            Self::Config => "XDG_CONFIG_HOME",
            Self::Cache => "XDG_CACHE_HOME",
        }
    }

    /// The XDG specification's default below `$HOME`.
    const fn home_fallback(self) -> &'static str {
        match self {
            Self::Data => ".local/share",
            Self::Config => ".config",
            Self::Cache => ".cache",
        }
    }
}

pub fn library_database_path() -> Result<PathBuf, DataPathError> {
    data_file_path(DATABASE_FILE)
}

/// SteamGridDB API key and artwork preference.
pub fn steamgriddb_settings_path() -> Result<PathBuf, DataPathError> {
    config_file_path(STEAMGRIDDB_FILE)
}

pub fn steam_account_settings_path() -> Result<PathBuf, DataPathError> {
    config_file_path(STEAM_ACCOUNT_FILE)
}

pub fn appearance_settings_path() -> Result<PathBuf, DataPathError> {
    config_file_path(APPEARANCE_FILE)
}

/// Curated preferences for the bundled core.
pub fn core_settings_paths() -> Result<CoreSettingsPaths, DataPathError> {
    Ok(CoreSettingsPaths {
        playback: config_file_path("playback.json")?,
        mgba: config_file_path("cores/mgba.json")?,
        ds: config_file_path("cores/melonds-ds.json")?,
    })
}

/// Controller preferences (swapping confirm/back and gameplay A/B).
pub fn controller_settings_path() -> Result<PathBuf, DataPathError> {
    config_file_path(CONTROLLER_FILE)
}

/// Which profile picture to show (a chosen picture, Steam's, or a built-in
/// avatar).
pub fn profile_settings_path() -> Result<PathBuf, DataPathError> {
    data_file_path(PROFILE_FILE)
}

/// Normalized cover art built from each source's own files.
pub fn local_artwork_cache_dir(version: &str) -> Option<PathBuf> {
    cache_dir("artwork/local", version)
}

/// Cover art downloaded from SteamGridDB, with its provenance records.
pub fn steamgriddb_cache_dir(version: &str) -> Option<PathBuf> {
    cache_dir("artwork/steamgriddb", version)
}

/// Horizon's own screenshots and clips. Data, not cache: captures cannot be
/// rebuilt. The folder is created when the first capture is written.
pub fn album_dir() -> Result<PathBuf, DataPathError> {
    Ok(app_dir(BaseDir::Data)?.join(ALBUM_DIR))
}

/// The profile picture chosen on the Profile page: Horizon's own square copy, not
/// a link to the original (which may be moved, or live behind the document
/// portal only for a session). Data, not cache, under profile/pictures/.
pub fn profile_picture_path() -> Result<PathBuf, DataPathError> {
    Ok(app_dir(BaseDir::Data)?.join(PROFILE_PICTURE_FILE))
}

/// The chosen profile animation, converted to lossless JPEG XL.
pub fn profile_animated_path() -> Result<PathBuf, DataPathError> {
    Ok(app_dir(BaseDir::Data)?.join(PROFILE_ANIMATED_FILE))
}

/// The last Steam profile picture fetched, kept so it shows offline.
pub fn steam_profile_picture_path() -> Result<PathBuf, DataPathError> {
    Ok(app_dir(BaseDir::Data)?.join(STEAM_PICTURE_FILE))
}

/// The Steam animated avatar last fetched, converted to lossless JPEG XL;
/// absent when none is equipped.
pub fn steam_animated_avatar_path() -> Result<PathBuf, DataPathError> {
    Ok(app_dir(BaseDir::Data)?.join(STEAM_ANIMATED_FILE))
}

/// Read compatibility for retained pre-JXL profile copies. New writes always
/// use the JXL path; this never renames or deletes the older data.
pub fn legacy_profile_image_path(path: &Path) -> Option<PathBuf> {
    let name = match path.file_name()?.to_str()? {
        "custom.jxl" => "custom.png",
        "custom-animation.jxl" => "custom-original",
        "steam.jxl" => "steam.png",
        "steam-animation.jxl" => "steam-original",
        _ => return None,
    };
    Some(path.with_file_name(name))
}

/// Reserved durable custom covers; no folder is made until something is stored.
pub fn custom_artwork_directory() -> Result<PathBuf, DataPathError> {
    Ok(app_dir(BaseDir::Data)?.join("artwork/custom"))
}

/// Album thumbnails and video durations.
pub fn album_cache_dir(version: &str) -> Option<PathBuf> {
    cache_dir("album", version)
}

fn config_file_path(filename: &str) -> Result<PathBuf, DataPathError> {
    private_file_path(BaseDir::Config, filename)
}
fn data_file_path(filename: &str) -> Result<PathBuf, DataPathError> {
    private_file_path(BaseDir::Data, filename)
}
fn private_file_path(base: BaseDir, filename: &str) -> Result<PathBuf, DataPathError> {
    let root = app_dir(base)?;
    let path = root.join(filename);
    // Secure the root and every new nested directory, including credentials.
    let mut directory = root.clone();
    for segment in std::iter::once(None).chain(
        Path::new(filename)
            .parent()
            .into_iter()
            .flat_map(|p| p.components())
            .map(Some),
    ) {
        if let Some(segment) = segment {
            directory.push(segment);
        }
        fs::create_dir_all(&directory)
            .and_then(|()| fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)))
            .map_err(|source| DataPathError::CreateStorageDirectory {
                path: directory.clone(),
                source,
            })?;
    }
    Ok(path)
}

/// A private, versioned cache folder. Caches are best-effort: any failure
/// returns `None` and the caller simply works without a cache.
fn cache_dir(name: &str, version: &str) -> Option<PathBuf> {
    prepare_versioned_dir(app_dir(BaseDir::Cache).ok()?.join(name), version)
}

/// Clears `dir` unless its `.version` file says `version`, then (re)creates it.
fn prepare_versioned_dir(dir: PathBuf, version: &str) -> Option<PathBuf> {
    let marker = dir.join(CACHE_VERSION_FILE);
    if fs::read_to_string(&marker).ok().as_deref() != Some(version) {
        // Missing or older format: start this cache over.
        let _ = fs::remove_dir_all(&dir);
    }
    fs::create_dir_all(&dir).ok()?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).ok()?;
    if !marker.exists() {
        fs::write(&marker, version).ok()?;
    }
    Some(dir)
}

fn app_dir(base: BaseDir) -> Result<PathBuf, DataPathError> {
    let xdg = non_empty_env(base.variable()).map(PathBuf::from);
    let home = non_empty_env("HOME").map(PathBuf::from);
    let root = resolve_root(base, xdg.as_deref(), home.as_deref())?;
    Ok(app_dir_in(&root, in_flatpak()))
}

/// Flatpak's XDG directories already belong to Horizon alone.
fn app_dir_in(root: &Path, flatpak: bool) -> PathBuf {
    if flatpak {
        root.to_owned()
    } else {
        root.join(APP_ID)
    }
}

fn in_flatpak() -> bool {
    non_empty_env("FLATPAK_ID").is_some()
}

fn resolve_root(
    base: BaseDir,
    xdg: Option<&Path>,
    home: Option<&Path>,
) -> Result<PathBuf, DataPathError> {
    if let Some(path) = xdg {
        if !path.is_absolute() {
            return Err(DataPathError::RelativeEnvironmentPath {
                variable: base.variable(),
                path: path.to_owned(),
            });
        }
        return Ok(path.to_owned());
    }
    let home = home.ok_or(DataPathError::MissingDataRoot)?;
    if !home.is_absolute() {
        return Err(DataPathError::RelativeEnvironmentPath {
            variable: "HOME",
            path: home.to_owned(),
        });
    }
    Ok(home.join(base.home_fallback()))
}

fn non_empty_env(name: &str) -> Option<OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_directory_has_priority() {
        let root = resolve_root(
            BaseDir::Data,
            Some(Path::new("/xdg/data")),
            Some(Path::new("/home/user")),
        )
        .expect("data root");
        assert_eq!(root, PathBuf::from("/xdg/data"));
    }

    #[test]
    fn home_falls_back_to_xdg_defaults() {
        let home = Some(Path::new("/home/user"));
        assert_eq!(
            resolve_root(BaseDir::Data, None, home).expect("data"),
            PathBuf::from("/home/user/.local/share")
        );
        assert_eq!(
            resolve_root(BaseDir::Config, None, home).expect("config"),
            PathBuf::from("/home/user/.config")
        );
        assert_eq!(
            resolve_root(BaseDir::Cache, None, home).expect("cache"),
            PathBuf::from("/home/user/.cache")
        );
    }

    #[test]
    fn relative_environment_paths_are_rejected() {
        assert!(matches!(
            resolve_root(
                BaseDir::Cache,
                Some(Path::new("relative")),
                Some(Path::new("/home/user"))
            ),
            Err(DataPathError::RelativeEnvironmentPath {
                variable: "XDG_CACHE_HOME",
                ..
            })
        ));
    }

    #[test]
    fn cache_is_cleared_only_when_its_version_changes() {
        let dir = std::env::temp_dir().join(format!("horizon-cache-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let kept = dir.join("kept.bin");

        let first = prepare_versioned_dir(dir.clone(), "v1").expect("create");
        fs::write(&kept, b"x").expect("write");
        // Same version: contents survive.
        prepare_versioned_dir(dir.clone(), "v1").expect("reuse");
        assert!(kept.exists());
        // New format: the folder starts over with the new marker.
        prepare_versioned_dir(dir.clone(), "v2").expect("reset");
        assert!(!kept.exists());
        assert_eq!(
            fs::read_to_string(first.join(CACHE_VERSION_FILE)).unwrap(),
            "v2"
        );
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn flatpak_uses_its_private_directories_directly() {
        let root = Path::new("/home/user/.var/app/io.github.Mars7x.Horizon/data");
        assert_eq!(app_dir_in(root, true), root);
        assert_eq!(
            app_dir_in(Path::new("/home/user/.local/share"), false),
            PathBuf::from("/home/user/.local/share/io.github.Mars7x.Horizon")
        );
    }
}

#[cfg(test)]
mod game_layout_tests {
    use super::*;
    #[test]
    fn safe_names_keep_unicode_and_cannot_create_path_components() {
        assert_eq!(
            game_file_stem("  Pokemon / Emerald: Version?  "),
            "Pokemon Emerald Version"
        );
        assert_eq!(game_file_stem("..\\..//"), "Untitled game");
        assert_eq!(game_file_stem("CON"), "Game CON");
        let long = game_file_stem(&"遊".repeat(100));
        assert!(long.len() <= 120);
        assert!(long.is_char_boundary(long.len()));
        assert_eq!(game_file_stem(&long), long);
        let reserved = game_file_stem(&format!("CON.{}", "x".repeat(200)));
        assert!(reserved.len() <= 120);
        assert_eq!(game_file_stem(&reserved), reserved);
    }
    #[test]
    fn state_paths_are_core_specific_and_do_not_create_directories() {
        let root = Path::new("/nonexistent/horizon-state-layout");
        let paths = state_bundle_paths(
            &state_slot_directory(&state_core_directory(root), 1),
            "Pokemon - Emerald Version",
            1,
        );
        assert_eq!(
            paths.state,
            root.join("states/mgba/slot-01/Pokemon - Emerald Version.slot-01.state")
        );
        assert_eq!(paths.metadata.extension().unwrap(), "json");
        assert_eq!(paths.screenshot.extension().unwrap(), "jxl");
        assert!(!root.exists());
        let automatic = state_bundle_paths(
            &state_slot_directory(&state_core_directory(root), 0),
            "Game",
            0,
        );
        assert_eq!(
            automatic.state,
            root.join("states/mgba/resume/Game.resume.state")
        );
    }
}

pub fn state_root_directory(game: &Path) -> PathBuf {
    game.join("states")
}
pub fn state_core_directory(game: &Path) -> PathBuf {
    state_root_directory(game).join("mgba")
}
pub fn state_slot_directory(core: &Path, slot: u8) -> PathBuf {
    core.join(if slot == 0 {
        "resume".into()
    } else {
        format!("slot-{slot:02}")
    })
}
pub fn state_bundle_paths(bundle: &Path, stem: &str, slot: u8) -> SaveStatePaths {
    let name = if slot == 0 {
        format!("{stem}.resume")
    } else {
        format!("{stem}.slot-{slot:02}")
    };
    SaveStatePaths {
        state: bundle.join(format!("{name}.state")),
        metadata: bundle.join(format!("{name}.json")),
        screenshot: bundle.join(format!("{name}.jxl")),
    }
}
/// Version-1 state bundles used PNG previews; keep those bundles loadable.
pub fn legacy_state_bundle_paths(bundle: &Path, stem: &str, slot: u8) -> SaveStatePaths {
    let mut paths = state_bundle_paths(bundle, stem, slot);
    paths.screenshot.set_extension("png");
    paths
}
pub fn state_staging_directory(core: &Path, slot: u8, nonce: &str) -> PathBuf {
    core.join(format!(".slot-{slot:02}-{nonce}.part"))
}
pub fn state_backup_root(core: &Path) -> PathBuf {
    core.join("backups")
}
pub fn state_backup_directory(core: &Path, slot: u8, nonce: &str) -> PathBuf {
    state_backup_root(core).join(format!("slot-{slot:02}-{nonce}"))
}
