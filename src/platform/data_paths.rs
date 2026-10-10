//! The one place that decides where Horizon keeps files on disk.
//!
//! Under Flatpak, `XDG_DATA_HOME`, `XDG_CONFIG_HOME` and `XDG_CACHE_HOME` are
//! already private to the app (`~/.var/app/<app-id>/{data,config,cache}`), so
//! Horizon writes directly into them. Outside Flatpak those directories are
//! shared with every other program, so Horizon uses an `<app-id>` subfolder.
//!
//! ```text
//! data/    library.sqlite3
//! config/  appearance.json  steamgriddb.json  steam-account.json   (0700 dir)
//! cache/   artwork/local/  artwork/steamgriddb/  achievements/     (disposable)
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
const DATABASE_FILE: &str = "library.sqlite3";
const STEAMGRIDDB_FILE: &str = "steamgriddb.json";
const APPEARANCE_FILE: &str = "appearance.json";
const STEAM_ACCOUNT_FILE: &str = "steam-account.json";
const CACHE_VERSION_FILE: &str = ".version";

#[derive(Debug, Error)]
pub enum DataPathError {
    #[error("neither the XDG base directory nor HOME provides an application location")]
    MissingDataRoot,
    #[error("{variable} must be an absolute path, got {path}")]
    RelativeEnvironmentPath {
        variable: &'static str,
        path: PathBuf,
    },
    #[error("failed to create Horizon data directory {path}: {source}")]
    CreateDataDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to create or secure Horizon config directory {path}: {source}")]
    CreateConfigDirectory {
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
    let data_dir = app_dir(BaseDir::Data)?;
    fs::create_dir_all(&data_dir).map_err(|source| DataPathError::CreateDataDirectory {
        path: data_dir.clone(),
        source,
    })?;
    Ok(data_dir.join(DATABASE_FILE))
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

/// Normalized cover art built from each source's own files.
pub fn local_artwork_cache_dir(version: &str) -> Option<PathBuf> {
    cache_dir("artwork/local", version)
}

/// Cover art downloaded from SteamGridDB, with its provenance records.
pub fn steamgriddb_cache_dir(version: &str) -> Option<PathBuf> {
    cache_dir("artwork/steamgriddb", version)
}

/// Per-account achievement snapshots and badge thumbnails.
pub fn achievements_cache_dir(version: &str) -> Option<PathBuf> {
    cache_dir("achievements", version)
}

fn config_file_path(filename: &str) -> Result<PathBuf, DataPathError> {
    let dir = app_dir(BaseDir::Config)?;
    fs::create_dir_all(&dir).map_err(|source| DataPathError::CreateConfigDirectory {
        path: dir.clone(),
        source,
    })?;
    // Holds API keys: owner-only, even though this is not encryption.
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(|source| {
        DataPathError::CreateConfigDirectory {
            path: dir.clone(),
            source,
        }
    })?;
    Ok(dir.join(filename))
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
