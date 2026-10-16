//! Complete per-slot bundles, with crash recovery through retained prior bundles.
//! Every operation runs on the existing GBA worker, never the UI thread.
use crate::{
    platform::{data_paths, gba_core::Frame},
    services::game_storage::{GameFolder, ensure_directory},
};
use image::{ImageDecoder, ImageEncoder};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Cursor, Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
pub const MANUAL_SLOTS: u8 = 4;
static NONCE: AtomicU64 = AtomicU64::new(0);
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub format_version: u32,
    pub slot: u8,
    pub rom_identity: String,
    pub core: String,
    pub core_build: String,
    pub saved_at: i64,
    pub state_size: usize,
    pub state_sha256: String,
    pub preview_sha256: String,
}
#[derive(Clone)]
pub struct Entry {
    pub slot: u8,
    pub metadata: Option<Metadata>,
    pub preview: Option<Frame>,
    pub issue: Option<String>,
}
pub struct Snapshot {
    pub bytes: Vec<u8>,
    pub metadata: Metadata,
    pub preview: Frame,
}
pub struct Store {
    folder: GameFolder,
    core_build: String,
}
impl Store {
    pub fn new(folder: GameFolder, core_build: String) -> Self {
        Self { folder, core_build }
    }
    fn core(&self) -> PathBuf {
        data_paths::state_core_directory(&self.folder.directory)
    }
    fn files(&self, directory: &Path, slot: u8) -> data_paths::SaveStatePaths {
        data_paths::state_bundle_paths(directory, &self.folder.manifest.file_stem, slot)
    }
    fn read_bundle(&self, directory: &Path, slot: u8) -> io::Result<Snapshot> {
        if !fs::symlink_metadata(directory)?.is_dir() {
            return Err(io::Error::other("Saved state is not an owned folder"));
        }
        let files = self.files(directory, slot);
        let metadata: Metadata =
            serde_json::from_slice(&read(&files.metadata, 16384).map_err(incomplete)?)
                .map_err(io::Error::other)?;
        if metadata.format_version != 1
            || metadata.saved_at < 0
            || metadata.slot != slot
            || metadata.core != "mgba"
            || metadata.core_build != self.core_build
            || metadata.rom_identity != self.folder.manifest.game_id
        {
            return Err(io::Error::other(
                "Saved state is incompatible with this game or core build",
            ));
        }
        let bytes = read(&files.state, 16 * 1024 * 1024).map_err(incomplete)?;
        let png = read(&files.screenshot, 512 * 1024).map_err(incomplete)?;
        if bytes.is_empty()
            || bytes.len() != metadata.state_size
            || hash(&bytes) != metadata.state_sha256
            || hash(&png) != metadata.preview_sha256
        {
            return Err(io::Error::other("Saved state is incomplete or damaged"));
        }
        let decoder =
            image::codecs::png::PngDecoder::new(Cursor::new(png)).map_err(io::Error::other)?;
        if decoder.dimensions() != (240, 160) {
            return Err(io::Error::other("Invalid saved state preview"));
        }
        let rgba = image::DynamicImage::from_decoder(decoder)
            .map_err(io::Error::other)?
            .to_rgba8();
        Ok(Snapshot {
            bytes,
            metadata,
            preview: Frame {
                width: 240,
                height: 160,
                rgba: rgba.into_raw(),
                smooth: false,
            },
        })
    }
    fn backups(&self, slot: u8) -> io::Result<Vec<PathBuf>> {
        let root = data_paths::state_backup_root(&self.core());
        match fs::symlink_metadata(&root) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(io::Error::other("Invalid state backup folder")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        }
        let prefix = format!("slot-{slot:02}-");
        let mut paths = Vec::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir()
                && entry.file_name().to_string_lossy().starts_with(&prefix)
            {
                paths.push(entry.path());
            }
        }
        Ok(paths)
    }
    pub fn load(&self, slot: u8) -> io::Result<Option<Snapshot>> {
        if slot > MANUAL_SLOTS {
            return Err(io::Error::other("Invalid state slot"));
        }
        let core = self.core();
        for directory in [
            self.folder.directory.clone(),
            data_paths::state_root_directory(&self.folder.directory),
            core.clone(),
        ] {
            match fs::symlink_metadata(directory) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => return Err(io::Error::other("Invalid state directory")),
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error),
            }
        }
        let current = data_paths::state_slot_directory(&core, slot);
        let mut failure = None;
        match self.read_bundle(&current, slot) {
            Ok(snapshot) => return Ok(Some(snapshot)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => failure = Some(error),
        }
        let mut previous = None;
        for path in self.backups(slot)? {
            match self.read_bundle(&path, slot) {
                Ok(snapshot)
                    if previous.as_ref().is_none_or(|old: &Snapshot| {
                        snapshot.metadata.saved_at > old.metadata.saved_at
                    }) =>
                {
                    previous = Some(snapshot)
                }
                Ok(_) => {}
                Err(error) => failure = Some(error),
            }
        }
        if previous.is_some() {
            return Ok(previous);
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(None),
        }
    }
    pub fn list(&self) -> Vec<Entry> {
        (0..=MANUAL_SLOTS)
            .map(|slot| match self.load(slot) {
                Ok(Some(snapshot)) => Entry {
                    slot,
                    metadata: Some(snapshot.metadata),
                    preview: Some(snapshot.preview),
                    issue: None,
                },
                Ok(None) => Entry {
                    slot,
                    metadata: None,
                    preview: None,
                    issue: None,
                },
                Err(error) => Entry {
                    slot,
                    metadata: None,
                    preview: None,
                    issue: Some(error.to_string()),
                },
            })
            .collect()
    }
    pub fn latest(&self) -> io::Result<Option<Snapshot>> {
        let mut newest = None;
        let mut failure = None;
        for slot in 0..=MANUAL_SLOTS {
            match self.load(slot) {
                Ok(Some(snapshot))
                    if newest.as_ref().is_none_or(|old: &Snapshot| {
                        snapshot.metadata.saved_at > old.metadata.saved_at
                    }) =>
                {
                    newest = Some(snapshot)
                }
                Ok(_) => {}
                Err(error) => failure = Some(error),
            }
        }
        if newest.is_some() {
            return Ok(newest);
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(None),
        }
    }
    pub fn save(&self, slot: u8, bytes: &[u8], frame: &Frame) -> io::Result<()> {
        if slot > MANUAL_SLOTS
            || bytes.is_empty()
            || bytes.len() > 16 * 1024 * 1024
            || frame.width != 240
            || frame.height != 160
            || frame.rgba.len() != 240 * 160 * 4
        {
            return Err(io::Error::other("Invalid saved state data"));
        }
        let core = self.core();
        ensure_directory(&self.folder.directory, &core)?;
        let nonce = format!(
            "{}-{}-{}",
            chrono::Utc::now().timestamp_micros(),
            std::process::id(),
            NONCE.fetch_add(1, Ordering::Relaxed)
        );
        let stage = data_paths::state_staging_directory(&core, slot, &nonce);
        fs::DirBuilder::new().mode(0o700).create(&stage)?;
        let _stage_cleanup = StageCleanup {
            paths: self.files(&stage, slot),
            directory: stage.clone(),
        };
        let files = self.files(&stage, slot);
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&frame.rgba, 240, 160, image::ExtendedColorType::Rgba8)
            .map_err(io::Error::other)?;
        let metadata = Metadata {
            format_version: 1,
            slot,
            rom_identity: self.folder.manifest.game_id.clone(),
            core: "mgba".into(),
            core_build: self.core_build.clone(),
            saved_at: chrono::Utc::now().timestamp_millis(),
            state_size: bytes.len(),
            state_sha256: hash(bytes),
            preview_sha256: hash(&png),
        };
        write(&files.state, bytes)?;
        write(&files.screenshot, &png)?;
        write(
            &files.metadata,
            &serde_json::to_vec_pretty(&metadata).map_err(io::Error::other)?,
        )?;
        File::open(&stage)?.sync_all()?;
        let current = data_paths::state_slot_directory(&core, slot);
        let backup = if current.symlink_metadata().is_ok() {
            if !fs::symlink_metadata(&current)?.is_dir() {
                return Err(io::Error::other("State slot is not an owned directory"));
            }
            let backups = data_paths::state_backup_root(&core);
            ensure_directory(&core, &backups)?;
            let path = data_paths::state_backup_directory(&core, slot, &nonce);
            fs::rename(&current, &path)?;
            File::open(&backups)?.sync_all()?;
            File::open(&core)?.sync_all()?;
            Some(path)
        } else {
            None
        };
        if let Err(error) = fs::rename(&stage, &current) {
            if let Some(backup) = backup {
                let _ = fs::rename(backup, &current);
            }
            return Err(error);
        }
        File::open(&core)?.sync_all()?;
        // Retain the newest complete previous copy. Unknown, incompatible or
        // damaged bundles are preserved rather than being treated as garbage.
        let mut backups: Vec<_> = self
            .backups(slot)
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "State backup cleanup postponed");
                Vec::new()
            })
            .into_iter()
            .filter_map(|path| {
                self.read_bundle(&path, slot)
                    .ok()
                    .map(|s| (s.metadata.saved_at, path))
            })
            .collect();
        backups.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        for (_, path) in backups.into_iter().skip(1) {
            let known = self.files(&path, slot);
            let expected = [known.state, known.metadata, known.screenshot];
            if fs::read_dir(&path).is_ok_and(|entries| {
                entries
                    .into_iter()
                    .all(|entry| entry.is_ok_and(|e| expected.contains(&e.path())))
            }) {
                for file in expected {
                    if let Err(error) = fs::remove_file(file) {
                        tracing::warn!(%error,"Old state backup retained");
                    }
                }
                let _ = fs::remove_dir(path);
            }
        }
        Ok(())
    }
}
fn read(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.len() > limit {
        return Err(io::Error::other("Invalid saved state file"));
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::other("Saved state exceeds its size limit"));
    }
    Ok(bytes)
}
fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}
fn hash(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn incomplete(error: io::Error) -> io::Error {
    if error.kind() == io::ErrorKind::NotFound {
        io::Error::other("Saved state is incomplete")
    } else {
        error
    }
}
struct StageCleanup {
    paths: data_paths::SaveStatePaths,
    directory: PathBuf,
}
impl Drop for StageCleanup {
    fn drop(&mut self) {
        for path in [
            &self.paths.state,
            &self.paths.metadata,
            &self.paths.screenshot,
        ] {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => tracing::warn!(%error, "Incomplete state staging file retained"),
            }
        }
        let _ = fs::remove_dir(&self.directory);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        root: PathBuf,
        store: Store,
        frame: Frame,
    }
    impl Fixture {
        fn new() -> Self {
            use crate::domain::{ExternalGameId, SourceGameRef, SourceId};
            let root = std::env::temp_dir().join(format!(
                "horizon-state-{}-{}",
                std::process::id(),
                NONCE.fetch_add(1, Ordering::Relaxed)
            ));
            let game = SourceGameRef::new(
                SourceId::new("gba").unwrap(),
                ExternalGameId::new(format!("sha256:{}", "a".repeat(64))).unwrap(),
            );
            let folder = GameFolder::prepare(&root, &game, "Test game").unwrap();
            Self {
                root,
                store: Store::new(folder, "mGBA@test:build-one".into()),
                frame: Frame {
                    width: 240,
                    height: 160,
                    rgba: [0, 255, 0, 255].repeat(240 * 160),
                    smooth: false,
                },
            }
        }
        fn save(&self, slot: u8, bytes: &[u8], time: i64) {
            self.store.save(slot, bytes, &self.frame).unwrap();
            let path = self
                .store
                .files(
                    &data_paths::state_slot_directory(&self.store.core(), slot),
                    slot,
                )
                .metadata;
            let mut meta: Metadata = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            meta.saved_at = time;
            fs::write(path, serde_json::to_vec(&meta).unwrap()).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
    #[test]
    fn slots_preview_newest_manual_and_auto_leave_battery_untouched() {
        let f = Fixture::new();
        fs::write(f.store.folder.save_path(), b"battery").unwrap();
        assert!(f.store.latest().unwrap().is_none());
        f.save(1, b"manual", 100);
        f.save(0, b"automatic", 200);
        f.save(4, b"newest manual", 300);
        assert_eq!(f.store.latest().unwrap().unwrap().bytes, b"newest manual");
        let entries = f.store.list();
        assert_eq!(entries.len(), 5);
        assert!(entries[2].metadata.is_none());
        assert_eq!(entries[4].preview.as_ref().unwrap().rgba, f.frame.rgba);
        assert_eq!(fs::read(f.store.folder.save_path()).unwrap(), b"battery");
        assert!(f.store.load(5).is_err());
        assert!(f.store.save(5, b"bad", &f.frame).is_err());
    }
    #[test]
    fn replacement_recovers_complete_previous_bundle_after_damage_or_crash_gap() {
        let f = Fixture::new();
        f.save(1, b"first", 100);
        f.save(1, b"second", 200);
        f.save(1, b"third", 300);
        assert_eq!(f.store.backups(1).unwrap().len(), 1);
        let current = data_paths::state_slot_directory(&f.store.core(), 1);
        fs::write(f.store.files(&current, 1).state, b"damaged").unwrap();
        assert_eq!(f.store.load(1).unwrap().unwrap().bytes, b"second");
        fs::remove_dir_all(&current).unwrap(); // interrupted replacement after moving previous bundle
        assert_eq!(f.store.latest().unwrap().unwrap().bytes, b"second");
        f.save(1, b"recovered", 400);
        assert_eq!(f.store.load(1).unwrap().unwrap().bytes, b"recovered");
    }
    #[test]
    fn damaged_incompatible_and_partial_states_are_visible_and_preserved() {
        let mut f = Fixture::new();
        f.save(2, b"valid", 100);
        f.store.core_build = "different core".into();
        assert!(f.store.latest().is_err());
        assert!(f.store.list()[2].issue.is_some());
        f.store.core_build = "mGBA@test:build-one".into();
        let identity = f.store.folder.manifest.game_id.clone();
        f.store.folder.manifest.game_id = format!("sha256:{}", "b".repeat(64));
        assert!(f.store.latest().is_err());
        f.store.folder.manifest.game_id = identity;
        let current = data_paths::state_slot_directory(&f.store.core(), 2);
        fs::remove_file(f.store.files(&current, 2).metadata).unwrap();
        assert!(f.store.latest().is_err());
        f.save(3, b"compatible", 200);
        assert_eq!(f.store.latest().unwrap().unwrap().bytes, b"compatible");
        assert!(f.store.files(&current, 2).state.exists());
    }
    #[test]
    fn symlink_parents_slots_and_files_cannot_escape_and_failed_save_cleans_staging() {
        let f = Fixture::new();
        let outside = f.root.join("outside");
        fs::create_dir(&outside).unwrap();
        let states = data_paths::state_root_directory(&f.store.folder.directory);
        std::os::unix::fs::symlink(&outside, &states).unwrap();
        assert!(f.store.load(1).is_err());
        assert!(f.store.save(1, b"bad", &f.frame).is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        fs::remove_file(&states).unwrap();
        f.save(1, b"valid", 100);
        let slot = data_paths::state_slot_directory(&f.store.core(), 2);
        std::os::unix::fs::symlink(&outside, &slot).unwrap();
        assert!(f.store.save(2, b"bad", &f.frame).is_err());
        assert!(fs::read_dir(f.store.core()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".part")
        }));
        let state = f
            .store
            .files(&data_paths::state_slot_directory(&f.store.core(), 1), 1)
            .state;
        fs::remove_file(&state).unwrap();
        std::os::unix::fs::symlink(&outside, &state).unwrap();
        assert!(f.store.load(1).is_err());
    }
}
