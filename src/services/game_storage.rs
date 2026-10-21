//! Identity-bearing, readable game folders. Call only on storage workers.
use crate::{
    domain::{ExternalGameId, SourceGameRef, SourceId},
    platform::data_paths,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_MANIFEST: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameManifest {
    pub format_version: u32,
    pub system: String,
    pub game_id: String,
    pub title: String,
    pub file_stem: String,
}
impl GameManifest {
    pub fn game(&self) -> io::Result<SourceGameRef> {
        if self.format_version != 1
            || self.file_stem.is_empty()
            || self.file_stem != data_paths::game_file_stem(&self.file_stem)
            || self.system.len() > 120
            || self.game_id.len() > 1024
        {
            return Err(io::Error::other("invalid game folder metadata"));
        }
        Ok(SourceGameRef::new(
            SourceId::new(self.system.clone()).map_err(io::Error::other)?,
            ExternalGameId::new(self.game_id.clone()).map_err(io::Error::other)?,
        ))
    }
}
#[derive(Debug, Clone)]
pub struct GameFolder {
    pub directory: PathBuf,
    pub manifest: GameManifest,
}
impl GameFolder {
    pub fn read(directory: &Path) -> io::Result<Self> {
        if !fs::symlink_metadata(directory)?.file_type().is_dir() {
            return Err(io::Error::other("game folder is not a directory"));
        }
        let path = data_paths::game_manifest_path(directory);
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() || metadata.len() > 16384 {
            return Err(io::Error::other("invalid game folder metadata"));
        }
        let mut bytes = Vec::new();
        File::open(path)?.take(16385).read_to_end(&mut bytes)?;
        if bytes.len() > 16384 {
            return Err(io::Error::other("game metadata is too large"));
        }
        let manifest: GameManifest = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        manifest.game()?;
        Ok(Self {
            directory: directory.to_owned(),
            manifest,
        })
    }
    pub fn prepare(root: &Path, game: &SourceGameRef, title: &str) -> io::Result<Self> {
        if game.source_id().as_str().len() > 40 || game.external_id().as_str().len() > 1024 {
            return Err(io::Error::other("game identity is too long"));
        }
        let system = data_paths::game_system_directory(root, game.source_id().as_str());
        ensure_directory(root, &system)?;
        let mut existing = None;
        for entry in fs::read_dir(&system)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let folder = match Self::read(&entry.path()) {
                Ok(folder) => folder,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if folder.manifest.game()? == *game {
                if existing.is_some() {
                    return Err(io::Error::other("duplicate game folders"));
                }
                existing = Some(folder);
            }
        }
        if let Some(folder) = existing {
            return Ok(folder);
        }
        let stem = data_paths::game_file_stem(title);
        let suffix = identity_suffix(game);
        for length in (8..=64).step_by(4) {
            let directory = data_paths::game_directory(&system, &stem, &suffix[..length]);
            match fs::create_dir(&directory) {
                Ok(()) => {
                    let manifest = GameManifest {
                        format_version: 1,
                        system: game.source_id().as_str().into(),
                        game_id: game.external_id().as_str().into(),
                        title: title.chars().take(512).collect(),
                        file_stem: stem.clone(),
                    };
                    let result = (|| {
                        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
                        publish_manifest(&directory, &manifest)?;
                        File::open(&system)?.sync_all()?;
                        Ok(Self {
                            directory: directory.clone(),
                            manifest,
                        })
                    })();
                    if result.is_err() {
                        // Roll back only our newly created directory, and only
                        // if nothing (including an identity document) is in it.
                        let _ = fs::remove_dir(&directory);
                    }
                    return result;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let folder = Self::read(&directory)?;
                    if folder.manifest.game()? == *game {
                        return Ok(folder);
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other("could not allocate a unique game folder"))
    }
    pub fn save_path(&self) -> PathBuf {
        data_paths::game_save_path(&self.directory, &self.manifest.file_stem)
    }
}
fn identity_suffix(game: &SourceGameRef) -> String {
    if let Some(hash) = game.external_id().as_str().strip_prefix("sha256:")
        && hash.len() == 64
        && hash.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return hash.to_ascii_lowercase();
    }
    let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
    digest.update(game.source_id().as_str().as_bytes());
    digest.update(&[0]);
    digest.update(game.external_id().as_str().as_bytes());
    digest
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn publish_manifest(directory: &Path, manifest: &GameManifest) -> io::Result<()> {
    let temporary = data_paths::game_manifest_temporary_path(
        directory,
        NEXT_MANIFEST.fetch_add(1, Ordering::Relaxed),
    );
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(&serde_json::to_vec_pretty(manifest).map_err(io::Error::other)?)?;
        file.sync_all()?;
        // Publish without replacing any existing identity document.
        fs::hard_link(&temporary, data_paths::game_manifest_path(directory))?;
        File::open(directory)?.sync_all()
    })();
    let cleanup = fs::remove_file(&temporary);
    result.and(cleanup)
}

/// How the player last showed a game: its DS screen layout and whether the
/// screens were swapped. Absent for games never changed from Settings'.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GameView {
    pub ds_layout: Option<crate::domain::core_settings::DsLayout>,
    pub ds_swapped: bool,
}

impl GameFolder {
    /// The remembered view; an unreadable or invalid file counts as none.
    pub fn view(&self) -> GameView {
        fs::read(data_paths::game_view_path(&self.directory))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
    /// Replace the remembered view atomically (written, synced, renamed).
    pub fn remember_view(&self, view: GameView) -> io::Result<()> {
        let temporary = data_paths::game_view_temporary_path(
            &self.directory,
            NEXT_MANIFEST.fetch_add(1, Ordering::Relaxed),
        );
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&serde_json::to_vec_pretty(&view).map_err(io::Error::other)?)?;
            file.sync_all()?;
            fs::rename(&temporary, data_paths::game_view_path(&self.directory))?;
            File::open(&self.directory)?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

/// Reject symlink components so writes cannot escape a Horizon-owned root.
pub fn ensure_directory(root: &Path, path: &Path) -> io::Result<()> {
    let relative = path.strip_prefix(root).map_err(io::Error::other)?;
    fs::create_dir_all(root)?;
    if !fs::symlink_metadata(root)?.is_dir() {
        return Err(io::Error::other("storage root is not a directory"));
    }
    let mut directory = root.to_owned();
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(io::Error::other("invalid storage directory"));
        }
        directory.push(component);
        match fs::create_dir(&directory) {
            Ok(()) => {
                fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
                if let Some(parent) = directory.parent() {
                    File::open(parent)?.sync_all()?;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if !fs::symlink_metadata(&directory)?.is_dir() {
                    return Err(io::Error::other("storage directory is not a directory"));
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "horizon-game-storage-{}-{nonce}",
            std::process::id()
        ))
    }
    fn game(hash: &str) -> SourceGameRef {
        SourceGameRef::new(
            SourceId::new("gba").unwrap(),
            ExternalGameId::new(format!("sha256:{hash}")).unwrap(),
        )
    }
    #[test]
    fn stable_names_full_identity_and_colliding_short_hashes() {
        let root = root();
        let first = game(&"a".repeat(64));
        let other = game(&format!("{}{}", "a".repeat(8), "b".repeat(56)));
        let one = GameFolder::prepare(&root, &first, "Pokemon - Emerald Version").unwrap();
        assert_eq!(
            one.directory,
            root.join("gba/Pokemon - Emerald Version [aaaaaaaa]")
        );
        assert_eq!(
            one.save_path().file_name().unwrap(),
            "Pokemon - Emerald Version.sav"
        );
        fs::write(one.save_path(), b"keep").unwrap();
        let renamed = GameFolder::prepare(&root, &first, "Corrected title").unwrap();
        assert_eq!(renamed.save_path(), one.save_path());
        let two = GameFolder::prepare(&root, &other, "Pokemon - Emerald Version").unwrap();
        assert!(
            two.directory
                .ends_with("Pokemon - Emerald Version [aaaaaaaabbbb]")
        );
        assert_ne!(one.directory, two.directory);
        assert_eq!(
            GameFolder::read(&two.directory)
                .unwrap()
                .manifest
                .game()
                .unwrap(),
            other
        );
        assert_eq!(fs::read(one.save_path()).unwrap(), b"keep");
        assert!(!one.directory.join("states").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn corrupt_or_duplicate_metadata_never_allocates_a_second_save() {
        let root = root();
        let game = game(&"c".repeat(64));
        let folder = GameFolder::prepare(&root, &game, "Game").unwrap();
        let path = data_paths::game_manifest_path(&folder.directory);
        fs::write(&path, b"broken").unwrap();
        assert!(GameFolder::prepare(&root, &game, "Renamed").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken");
        fs::write(&path, serde_json::to_vec(&folder.manifest).unwrap()).unwrap();
        let copy = root.join("gba/Duplicate");
        fs::create_dir(&copy).unwrap();
        fs::copy(&path, data_paths::game_manifest_path(&copy)).unwrap();
        assert!(GameFolder::prepare(&root, &game, "Game").is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn symlink_directories_and_manifest_paths_cannot_escape() {
        let root = root();
        let outside = root.join("outside");
        let saves = root.join("saves");
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir(&saves).unwrap();
        std::os::unix::fs::symlink(&outside, saves.join("gba")).unwrap();
        assert!(GameFolder::prepare(&saves, &game(&"d".repeat(64)), "Game").is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }
}
