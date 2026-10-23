//! Manual save management. Call only on a storage worker, while gameplay is closed.
use super::game_storage::{GameFolder, ensure_directory};
use crate::{domain::GameId, platform::data_paths};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn folder(database: &Path, root: &Path, id: GameId) -> Result<GameFolder, String> {
    let details = super::game_management::details(database, id).map_err(|e| e.to_string())?;
    let mut sources = details
        .game
        .sources()
        .iter()
        .filter(|source| matches!(source.source_id().as_str(), "gb" | "gbc" | "gba" | "nds"));
    let source = sources
        .next()
        .ok_or("This game has no supported save folder.")?;
    if sources.next().is_some() {
        return Err("This game has more than one save identity. No files were changed.".into());
    }
    safe_path(root).map_err(|e| e.to_string())?;
    GameFolder::prepare(root, source, details.game.game().title().as_str())
        .map_err(|e| format!("Could not access this game's save folder: {e}"))
}

struct Entry {
    path: PathBuf,
    metadata: fs::Metadata,
}
fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.len() == b.len()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}
fn safe_path(path: &Path) -> io::Result<()> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(io::Error::other("Invalid save path"));
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(io::Error::other("Save paths cannot contain links"));
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => break,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
fn collect(path: &Path, entries: &mut Vec<Entry>, depth: usize) -> io::Result<()> {
    safe_path(path)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if depth > 16 || entries.len() >= 10000 {
        return Err(io::Error::other("Save data contains too many nested files"));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            collect(&entry?.path(), entries, depth + 1)?;
        }
    } else if !metadata.is_file() {
        return Err(io::Error::other("Save data contains a non-regular file"));
    }
    entries.push(Entry {
        path: path.to_owned(),
        metadata,
    });
    Ok(())
}
fn plan(folder: &GameFolder) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    match fs::symlink_metadata(folder.save_path()) {
        Ok(metadata) if !metadata.is_file() => {
            return Err(io::Error::other("Battery save is not a regular file"));
        }
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    collect(&folder.save_path(), &mut entries, 0)?;
    collect(
        &data_paths::state_root_directory(&folder.directory),
        &mut entries,
        0,
    )?;
    Ok(entries)
}
fn verify(entry: &Entry) -> io::Result<()> {
    safe_path(&entry.path)?;
    if !same_file(&entry.metadata, &fs::symlink_metadata(&entry.path)?) {
        return Err(io::Error::other(
            "Save data changed. Close this dialog and try again",
        ));
    }
    Ok(())
}

/// Copy the battery save and complete state tree to a new immutable snapshot.
/// Only a fully synced snapshot receives its final name; failures stay `.part`.
pub fn backup(folder: &GameFolder) -> io::Result<PathBuf> {
    let entries = plan(folder)?;
    if !entries.iter().any(|e| e.metadata.is_file()) {
        return Err(io::Error::other("There is no save data to back up yet"));
    }
    let root = data_paths::manual_save_backup_root(&folder.directory);
    safe_path(&root)?;
    ensure_directory(&folder.directory, &root)?;
    let name = format!(
        "{}-{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let stage = data_paths::manual_save_backup_path(&folder.directory, &format!("{name}.part"));
    let destination = data_paths::manual_save_backup_path(&folder.directory, &name);
    fs::DirBuilder::new().mode(0o700).create(&stage)?;
    ensure_directory(&folder.directory, &stage)?;
    for entry in &entries {
        verify(entry)?;
        let relative = entry
            .path
            .strip_prefix(&folder.directory)
            .map_err(io::Error::other)?;
        let target = stage.join(relative);
        if entry.metadata.is_dir() {
            ensure_directory(&stage, &target)?;
            continue;
        }
        let parent = target
            .parent()
            .ok_or_else(|| io::Error::other("Invalid backup path"))?;
        ensure_directory(&stage, parent)?;
        let mut input = File::open(&entry.path)?;
        if !same_file(&entry.metadata, &input.metadata()?) {
            return Err(io::Error::other("Save data changed before backup"));
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&target)?;
        io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        if !same_file(&entry.metadata, &input.metadata()?) {
            return Err(io::Error::other("Save data changed during backup"));
        }
    }
    // Carry the complete identity, independent of any later title change.
    let mut manifest = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(data_paths::game_manifest_path(&stage))?;
    manifest.write_all(&serde_json::to_vec_pretty(&folder.manifest).map_err(io::Error::other)?)?;
    manifest.sync_all()?;
    for entry in &entries {
        verify(entry)?;
        if entry.metadata.is_dir() {
            let relative = entry
                .path
                .strip_prefix(&folder.directory)
                .map_err(io::Error::other)?;
            File::open(stage.join(relative))?.sync_all()?;
        }
    }
    File::open(&stage)?.sync_all()?;
    fs::rename(&stage, &destination)?;
    File::open(&root)?.sync_all()?;
    Ok(destination)
}

/// Explicitly reset progress, keeping identity, view, manual/previous battery
/// backups, ROMs and Album. State fallback backups are part of the reset so
/// automatic resume cannot resurrect deleted progress.
pub fn delete(folder: &GameFolder) -> io::Result<()> {
    let entries = plan(folder)?;
    for entry in &entries {
        verify(entry)?;
    }
    // Persist first so next launch cannot silently reimport the external save.
    let marker = data_paths::save_reset_marker(&folder.directory);
    safe_path(&marker)?;
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&marker)
    {
        Ok(mut file) => {
            file.write_all(b"Save data explicitly reset by the user.\n")?;
            file.sync_all()?;
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if !fs::symlink_metadata(&marker)?.is_file() {
                return Err(io::Error::other("Invalid save reset marker"));
            }
        }
        Err(error) => return Err(error),
    }
    File::open(&folder.directory)?.sync_all()?;
    for entry in entries {
        // Removing children changes directory timestamps, so only files use
        // the full snapshot comparison during the removal pass.
        safe_path(&entry.path)?;
        if entry.metadata.is_dir() {
            fs::remove_dir(&entry.path)?;
        } else {
            verify(&entry)?;
            fs::remove_file(&entry.path)?;
        }
    }
    File::open(&folder.directory)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ExternalGameId, SourceGameRef, SourceId};
    struct Fixture {
        root: PathBuf,
        folder: GameFolder,
    }
    impl Fixture {
        fn new(system: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "horizon-save-data-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let game = SourceGameRef::new(
                SourceId::new(system).unwrap(),
                ExternalGameId::new(
                    "sha256:1111111111111111111111111111111111111111111111111111111111111111",
                )
                .unwrap(),
            );
            let folder = GameFolder::prepare(&root, &game, "Test game").unwrap();
            fs::write(folder.save_path(), b"battery").unwrap();
            let core = if system == "nds" {
                data_paths::ds_state_core_directory(&folder.directory)
            } else {
                data_paths::state_core_directory(&folder.directory)
            };
            for slot in [0, 1] {
                let bundle = data_paths::state_slot_directory(&core, slot);
                ensure_directory(&folder.directory, &bundle).unwrap();
                fs::write(
                    data_paths::state_bundle_paths(&bundle, &folder.manifest.file_stem, slot).state,
                    b"state",
                )
                .unwrap();
            }
            Self { root, folder }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn backups_are_complete_unique_and_survive_progress_reset_for_both_cores() {
        for system in ["gba", "nds"] {
            let fixture = Fixture::new(system);
            let folder = &fixture.folder;
            let a = backup(folder).unwrap();
            let b = backup(folder).unwrap();
            assert_ne!(a, b);
            let name = folder.save_path().file_name().unwrap().to_owned();
            assert_eq!(fs::read(a.join(&name)).unwrap(), b"battery");
            assert_eq!(
                GameFolder::read(&a).unwrap().manifest.game_id,
                folder.manifest.game_id
            );
            assert!(data_paths::state_root_directory(&a).exists());
            fs::write(data_paths::game_view_path(&folder.directory), b"view").unwrap();
            delete(folder).unwrap();
            assert!(!folder.save_path().exists());
            assert!(!data_paths::state_root_directory(&folder.directory).exists());
            assert!(data_paths::save_reset_marker(&folder.directory).exists());
            assert_eq!(fs::read(a.join(&name)).unwrap(), b"battery");
            assert_eq!(
                fs::read(data_paths::game_view_path(&folder.directory)).unwrap(),
                b"view"
            );
            assert!(GameFolder::read(&folder.directory).is_ok());
            assert!(backup(folder).is_err());
        }
    }
    #[test]
    fn links_and_invalid_battery_paths_stop_backup_and_deletion_before_mutation() {
        let fixture = Fixture::new("gba");
        let outside = fixture.root.join("outside");
        fs::write(&outside, b"unrelated").unwrap();
        let link = data_paths::state_root_directory(&fixture.folder.directory).join("unexpected");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(backup(&fixture.folder).is_err());
        assert!(delete(&fixture.folder).is_err());
        assert!(!data_paths::save_reset_marker(&fixture.folder.directory).exists());
        assert_eq!(fs::read(fixture.folder.save_path()).unwrap(), b"battery");
        assert_eq!(fs::read(&outside).unwrap(), b"unrelated");
        fs::remove_file(link).unwrap();
        fs::remove_file(fixture.folder.save_path()).unwrap();
        fs::create_dir(fixture.folder.save_path()).unwrap();
        assert!(delete(&fixture.folder).is_err());
    }
}
