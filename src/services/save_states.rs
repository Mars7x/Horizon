//! Complete per-slot bundles, with crash recovery through retained prior bundles.
//! Every operation runs on the existing GBA worker, never the UI thread.
use crate::{
    platform::{data_paths, gba_core::Frame},
    services::game_storage::{GameFolder, ensure_directory},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
pub const MANUAL_SLOTS: u8 = 4;
/// Native frame sizes: GBA, Game Boy and Game Boy Color, and a Super Game
/// Boy game with its border.
const PREVIEW_SIZES: [(u32, u32); 3] = [(240, 160), (160, 144), (256, 224)];

/// States follow the mGBA release (`mGBA@<version>`), not the exact binary:
/// rebuilding the same release, such as adding its Game Boy core, keeps
/// them. mGBA itself rejects a state it cannot read, and the session rolls back.
fn same_release(saved: &str, current: &str) -> bool {
    fn release(build: &str) -> &str {
        build.rsplit_once(':').map_or(build, |(release, _)| release)
    }
    release(saved) == release(current)
}
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
    #[cfg(test)]
    preview_decodes: std::cell::Cell<usize>,
}
impl Store {
    pub fn new(folder: GameFolder, core_build: String) -> Self {
        Self {
            folder,
            core_build,
            #[cfg(test)]
            preview_decodes: std::cell::Cell::new(0),
        }
    }
    fn core(&self) -> PathBuf {
        data_paths::state_core_directory(&self.folder.directory)
    }
    fn files(&self, directory: &Path, slot: u8) -> data_paths::SaveStatePaths {
        data_paths::state_bundle_paths(directory, &self.folder.manifest.file_stem, slot)
    }
    fn read_metadata(&self, directory: &Path, slot: u8) -> io::Result<Metadata> {
        if !fs::symlink_metadata(directory)?.is_dir() {
            return Err(io::Error::other("Saved state is not an owned folder"));
        }
        let files = self.files(directory, slot);
        let metadata: Metadata =
            serde_json::from_slice(&read(&files.metadata, 16384).map_err(incomplete)?)
                .map_err(io::Error::other)?;
        if !matches!(metadata.format_version, 1 | 2)
            || metadata.saved_at < 0
            || metadata.slot != slot
            || metadata.core != "mgba"
            || !same_release(&metadata.core_build, &self.core_build)
            || metadata.rom_identity != self.folder.manifest.game_id
        {
            return Err(io::Error::other(
                "Saved state is incompatible with this game or core build",
            ));
        }
        Ok(metadata)
    }
    fn read_bundle(&self, directory: &Path, slot: u8) -> io::Result<Snapshot> {
        let metadata = self.read_metadata(directory, slot)?;
        let files = self.files(directory, slot);
        let bytes = read(&files.state, 16 * 1024 * 1024).map_err(incomplete)?;
        let files = if metadata.format_version == 1 {
            data_paths::legacy_state_bundle_paths(directory, &self.folder.manifest.file_stem, slot)
        } else {
            files
        };
        let preview = read(&files.screenshot, 512 * 1024).map_err(incomplete)?;
        if bytes.is_empty()
            || bytes.len() != metadata.state_size
            || hash(&bytes) != metadata.state_sha256
            || hash(&preview) != metadata.preview_sha256
        {
            return Err(io::Error::other("Saved state is incomplete or damaged"));
        }
        let (width, height) =
            crate::services::image::dimensions(&preview, None).map_err(io::Error::other)?;
        if !PREVIEW_SIZES.contains(&(width, height))
            || (metadata.format_version == 2 && !crate::services::image::is_jxl(&preview))
        {
            return Err(io::Error::other("Invalid saved state preview"));
        }
        #[cfg(test)]
        self.preview_decodes.set(self.preview_decodes.get() + 1);
        let rgba = crate::services::image::decode(&preview, None)
            .map_err(io::Error::other)?
            .into_rgba8();
        Ok(Snapshot {
            bytes,
            metadata,
            preview: Frame {
                width,
                height,
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
    /// Check every parent before probing metadata, including for an empty store.
    fn available_core(&self) -> io::Result<Option<PathBuf>> {
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
        Ok(Some(core))
    }
    pub fn load(&self, slot: u8) -> io::Result<Option<Snapshot>> {
        if slot > MANUAL_SLOTS {
            return Err(io::Error::other("Invalid state slot"));
        }
        let Some(core) = self.available_core()? else {
            return Ok(None);
        };
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
    /// A timestamp ceiling, not a validity decision. Current bundles still
    /// precede backups within a slot; a backup can win only after current data
    /// fails full validation. Unreadable metadata forces a normal load attempt.
    fn newest_possible_time(&self, core: &Path, slot: u8) -> io::Result<i64> {
        let mut newest = -1;
        for directory in
            std::iter::once(data_paths::state_slot_directory(core, slot)).chain(self.backups(slot)?)
        {
            match self.read_metadata(&directory, slot) {
                Ok(metadata) => newest = newest.max(metadata.saved_at),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(newest)
    }
    pub fn latest(&self) -> io::Result<Option<Snapshot>> {
        let Some(core) = self.available_core()? else {
            return Ok(None);
        };
        let mut candidates: Vec<_> = (0..=MANUAL_SLOTS)
            .map(|slot| {
                let ceiling = self.newest_possible_time(&core, slot).unwrap_or(i64::MAX);
                (ceiling, std::cmp::Reverse(slot))
            })
            .collect();
        candidates.sort_by_key(|key| std::cmp::Reverse(*key));
        let mut newest: Option<Snapshot> = None;
        let mut failure = None;
        for (ceiling, std::cmp::Reverse(slot)) in candidates {
            if ceiling < 0 {
                continue;
            }
            if newest.as_ref().is_some_and(|snapshot| {
                (
                    snapshot.metadata.saved_at,
                    std::cmp::Reverse(snapshot.metadata.slot),
                ) >= (ceiling, std::cmp::Reverse(slot))
            }) {
                break;
            }
            match self.load(slot) {
                Ok(Some(snapshot))
                    if newest.as_ref().is_none_or(|old| {
                        (snapshot.metadata.saved_at, std::cmp::Reverse(slot))
                            > (old.metadata.saved_at, std::cmp::Reverse(old.metadata.slot))
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
            || !PREVIEW_SIZES.contains(&(frame.width, frame.height))
            || frame.rgba.len() != (frame.width * frame.height * 4) as usize
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
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&files.screenshot)?;
        crate::platform::jpeg_xl::write_rgba(&file, &frame.rgba, frame.width, frame.height)?;
        file.sync_all()?;
        let preview = read(&files.screenshot, 512 * 1024)?;
        let metadata = Metadata {
            format_version: 2,
            slot,
            rom_identity: self.folder.manifest.game_id.clone(),
            core: "mgba".into(),
            core_build: self.core_build.clone(),
            saved_at: chrono::Utc::now().timestamp_millis(),
            state_size: bytes.len(),
            state_sha256: hash(bytes),
            preview_sha256: hash(&preview),
        };
        write(&files.state, bytes)?;
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
    fn resume_decodes_only_the_winning_slot_and_keeps_tie_order() {
        let f = Fixture::new();
        for slot in 0..=MANUAL_SLOTS {
            f.save(slot, &[slot + 1], 100 + i64::from(slot));
        }
        f.store.preview_decodes.set(0);
        assert_eq!(f.store.latest().unwrap().unwrap().metadata.slot, 4);
        assert_eq!(f.store.preview_decodes.get(), 1);
        f.save(0, b"automatic tie", 104);
        assert_eq!(f.store.latest().unwrap().unwrap().metadata.slot, 0);
    }
    #[test]
    fn metadata_priority_preserves_current_precedence_and_damaged_state_recovery() {
        let f = Fixture::new();
        f.save(1, b"backup with newer clock", 400);
        f.save(1, b"valid current with older clock", 100);
        f.save(2, b"other slot", 200);
        assert_eq!(f.store.latest().unwrap().unwrap().bytes, b"other slot");
        let files = f
            .store
            .files(&data_paths::state_slot_directory(&f.store.core(), 1), 1);
        fs::write(&files.state, b"broken").unwrap();
        assert_eq!(
            f.store.latest().unwrap().unwrap().bytes,
            b"backup with newer clock"
        );
        // Bad metadata is unknown, so the slot still attempts backup recovery.
        fs::write(files.metadata, b"broken metadata").unwrap();
        assert_eq!(
            f.store.latest().unwrap().unwrap().bytes,
            b"backup with newer clock"
        );
    }
    #[test]
    #[ignore = "opt-in resume I/O and preview decode benchmark"]
    fn resume_selection_benchmark() {
        let mut f = Fixture::new();
        f.frame.rgba = (0..240 * 160)
            .flat_map(|i| {
                [
                    ((i % 240) / 8 * 7) as u8,
                    ((i / 240) / 8 * 11) as u8,
                    (i % 97) as u8,
                    255,
                ]
            })
            .collect();
        let state = vec![7; 512 * 1024];
        for slot in 0..=MANUAL_SLOTS {
            f.save(slot, &state, 100 + i64::from(slot));
            f.save(slot, &state, 200 + i64::from(slot));
        }
        let mut baseline = Vec::new();
        let mut ordered = Vec::new();
        for _ in 0..20 {
            f.store.preview_decodes.set(0);
            let start = std::time::Instant::now();
            // Previous production selection: fully load every slot.
            let mut newest: Option<Snapshot> = None;
            for slot in 0..=MANUAL_SLOTS {
                if let Some(snapshot) = f.store.load(slot).unwrap()
                    && newest
                        .as_ref()
                        .is_none_or(|old| snapshot.metadata.saved_at > old.metadata.saved_at)
                {
                    newest = Some(snapshot);
                }
            }
            baseline.push(start.elapsed());
            assert_eq!(f.store.preview_decodes.get(), 5);
            f.store.preview_decodes.set(0);
            let start = std::time::Instant::now();
            let selected = f.store.latest().unwrap().unwrap();
            ordered.push(start.elapsed());
            assert_eq!(selected.bytes, newest.unwrap().bytes);
            assert_eq!(selected.metadata.slot, 4);
            assert_eq!(f.store.preview_decodes.get(), 1);
        }
        baseline.sort();
        ordered.sort();
        println!(
            "5 slots + 5 backups, 512 KiB states, 20 paired warm-cache samples: all-slot median {:.3} ms, metadata-ordered median {:.3} ms; preview decodes 5 -> 1",
            baseline[10].as_secs_f64() * 1000.0,
            ordered[10].as_secs_f64() * 1000.0
        );
    }
    #[test]
    fn legacy_png_bundles_still_load_and_new_previews_are_jxl() {
        let f = Fixture::new();
        f.save(1, b"retained", 100);
        let directory = data_paths::state_slot_directory(&f.store.core(), 1);
        let files = f.store.files(&directory, 1);
        assert_eq!(files.screenshot.extension().unwrap(), "jxl");
        let mut metadata: Metadata =
            serde_json::from_slice(&fs::read(&files.metadata).unwrap()).unwrap();
        assert_eq!(metadata.format_version, 2);
        let legacy = data_paths::legacy_state_bundle_paths(
            &directory,
            &f.store.folder.manifest.file_stem,
            1,
        );
        let image = image::RgbaImage::from_raw(240, 160, f.frame.rgba.clone()).unwrap();
        image.save(&legacy.screenshot).unwrap();
        metadata.format_version = 1;
        metadata.preview_sha256 = hash(&fs::read(&legacy.screenshot).unwrap());
        fs::write(&files.metadata, serde_json::to_vec(&metadata).unwrap()).unwrap();
        fs::remove_file(&files.screenshot).unwrap();
        assert_eq!(f.store.load(1).unwrap().unwrap().preview.rgba, f.frame.rgba);
        f.save(1, b"new", 200);
        assert_eq!(f.store.load(1).unwrap().unwrap().metadata.format_version, 2);
        assert!(f.store.backups(1).unwrap().iter().any(|dir| {
            data_paths::legacy_state_bundle_paths(dir, &f.store.folder.manifest.file_stem, 1)
                .screenshot
                .exists()
        }));
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
        // A rebuild of the same release keeps its states.
        f.store.core_build = "mGBA@test:build-two".into();
        assert_eq!(f.store.latest().unwrap().unwrap().bytes, b"valid");
        f.store.core_build = "mGBA@other:build-one".into();
        assert!(f.store.latest().is_err());
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
        assert!(f.store.latest().is_err());
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
    #[test]
    fn game_boy_states_keep_their_own_preview_size() {
        let f = Fixture::new();
        let frame = Frame {
            width: 160,
            height: 144,
            rgba: [0, 0, 255, 255].repeat(160 * 144),
            smooth: false,
        };
        f.store.save(1, b"game boy", &frame).unwrap();
        let snapshot = f.store.load(1).unwrap().unwrap();
        assert_eq!(
            (snapshot.preview.width, snapshot.preview.height),
            (160, 144)
        );
        assert_eq!(snapshot.preview.rgba, frame.rgba);
        let bordered = Frame {
            width: 256,
            height: 224,
            rgba: vec![0; 256 * 224 * 4],
            smooth: false,
        };
        f.store.save(2, b"bordered", &bordered).unwrap();
        let wrong = Frame {
            width: 320,
            height: 240,
            rgba: vec![0; 320 * 240 * 4],
            smooth: false,
        };
        assert!(f.store.save(3, b"other", &wrong).is_err());
    }
}
