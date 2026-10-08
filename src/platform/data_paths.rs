use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use thiserror::Error;
use std::os::unix::fs::PermissionsExt;

const APP_DATA_DIR: &str = "io.github.Mars7x.Horizon";
const DATABASE_FILE: &str = "library.sqlite3";
const SETTINGS_FILE: &str = "third-party.json";

#[derive(Debug, Error)]
pub enum DataPathError {
    #[error("neither XDG_DATA_HOME nor HOME provides an application data location")]
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
    CreateConfigDirectory { path: PathBuf, #[source] source: std::io::Error },
}

pub fn library_database_path() -> Result<PathBuf, DataPathError> {
    let xdg_data_home = non_empty_env("XDG_DATA_HOME").map(PathBuf::from);
    let home = non_empty_env("HOME").map(PathBuf::from);
    let root = resolve_data_root(xdg_data_home.as_deref(), home.as_deref())?;
    let data_dir = root.join(APP_DATA_DIR);

    fs::create_dir_all(&data_dir).map_err(|source| DataPathError::CreateDataDirectory {
        path: data_dir.clone(),
        source,
    })?;

    Ok(data_dir.join(DATABASE_FILE))
}

/// Settings are kept separate from the library database and within the
/// app-specific XDG configuration directory (Flatpak gets its own XDG_CONFIG_HOME).
pub fn third_party_settings_path() -> Result<PathBuf, DataPathError> {
    let config_home = non_empty_env("XDG_CONFIG_HOME").map(PathBuf::from);
    let home = non_empty_env("HOME").map(PathBuf::from);
    let root = resolve_config_root(config_home.as_deref(), home.as_deref())?;
    let dir = root.join(APP_DATA_DIR);
    fs::create_dir_all(&dir).map_err(|source| DataPathError::CreateConfigDirectory {
        path: dir.clone(), source,
    })?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(|source| {
        DataPathError::CreateConfigDirectory { path: dir.clone(), source }
    })?;
    Ok(dir.join(SETTINGS_FILE))
}

fn resolve_config_root(config_home: Option<&Path>, home: Option<&Path>) -> Result<PathBuf, DataPathError> {
    if let Some(path) = config_home {
        if !path.is_absolute() {
            return Err(DataPathError::RelativeEnvironmentPath {
                variable: "XDG_CONFIG_HOME", path: path.to_owned(),
            });
        }
        return Ok(path.to_owned());
    }
    let home = home.ok_or(DataPathError::MissingDataRoot)?;
    if !home.is_absolute() {
        return Err(DataPathError::RelativeEnvironmentPath {
            variable: "HOME", path: home.to_owned(),
        });
    }
    Ok(home.join(".config"))
}

fn non_empty_env(name: &str) -> Option<OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}

fn resolve_data_root(
    xdg_data_home: Option<&Path>,
    home: Option<&Path>,
) -> Result<PathBuf, DataPathError> {
    if let Some(path) = xdg_data_home {
        if !path.is_absolute() {
            return Err(DataPathError::RelativeEnvironmentPath {
                variable: "XDG_DATA_HOME",
                path: path.to_owned(),
            });
        }
        return Ok(path.to_owned());
    }

    let Some(home) = home else {
        return Err(DataPathError::MissingDataRoot);
    };
    if !home.is_absolute() {
        return Err(DataPathError::RelativeEnvironmentPath {
            variable: "HOME",
            path: home.to_owned(),
        });
    }

    Ok(home.join(".local/share"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_data_home_has_priority() {
        let root = resolve_data_root(Some(Path::new("/xdg/data")), Some(Path::new("/home/user")))
            .expect("data root");
        assert_eq!(root, PathBuf::from("/xdg/data"));
    }

    #[test]
    fn home_falls_back_to_local_share() {
        let root = resolve_data_root(None, Some(Path::new("/home/user"))).expect("data root");
        assert_eq!(root, PathBuf::from("/home/user/.local/share"));
    }

    #[test]
    fn relative_environment_paths_are_rejected() {
        assert!(matches!(
            resolve_data_root(Some(Path::new("relative")), Some(Path::new("/home/user"))),
            Err(DataPathError::RelativeEnvironmentPath {
                variable: "XDG_DATA_HOME",
                ..
            })
        ));
    }
}
