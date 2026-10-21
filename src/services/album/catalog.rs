//! Finding captures: Horizon's own folder plus every capture source.
//!
//! Game captures live in album/<system>/<readable game folder>/screenshots, alongside game.json. Unassigned captures use album/horizon/.
//! UTC stamps use YYYY-MM-DD_HH-MM-SS-mmm; .part files are unpublished.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use chrono::{DateTime, NaiveDateTime, Utc};

use crate::{
    domain::{
        SourceGameRef,
        album::{Capture, CaptureOrigin, MediaKind, newest_first},
    },
    sources::CaptureSource,
};

const HORIZON_FOLDER: &str = "horizon";
const STAMP_FORMAT: &str = "%Y-%m-%d_%H-%M-%S";

/// Horizon's own capture folder.
#[derive(Debug, Clone)]
pub struct HorizonAlbum {
    root: PathBuf,
}

impl HorizonAlbum {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Where a new screenshot goes, with its folder created. The writer uses
    /// `<path>.part`, then atomically publishes it as `path`. Never returns an existing file.
    pub fn new_capture_path(
        &self,
        game: Option<&SourceGameRef>,
        title: &str,
        extension: &str,
        now: DateTime<Utc>,
    ) -> std::io::Result<PathBuf> {
        use crate::{
            platform::data_paths,
            services::game_storage::{GameFolder, ensure_directory},
        };
        MediaKind::from_extension(extension)
            .ok_or_else(|| std::io::Error::other("unsupported capture format"))?;
        let directory = match game {
            Some(game) => GameFolder::prepare(&self.root, game, title)?.directory,
            None => data_paths::unassigned_album_directory(&self.root),
        };
        let folder = data_paths::capture_directory(&directory);
        ensure_directory(&self.root, &folder)?;
        let stamp = format!(
            "{}-{:03}",
            now.format(STAMP_FORMAT),
            now.timestamp_subsec_millis()
        );
        let mut copy = 1;
        let mut candidate = data_paths::capture_file(&folder, &stamp, extension, copy);
        while candidate.symlink_metadata().is_ok()
            || part_path(&candidate).symlink_metadata().is_ok()
        {
            copy = copy
                .checked_add(1)
                .ok_or_else(|| std::io::Error::other("too many captures at this timestamp"))?;
            candidate = data_paths::capture_file(&folder, &stamp, extension, copy);
        }
        Ok(candidate)
    }

    /// Permanently delete one of Horizon's own captures, then any game
    /// folder it leaves empty. Refuses anything outside this album or that
    /// is not a capture, so a source's files can never be deleted here.
    pub fn delete(&self, path: &Path) -> std::io::Result<()> {
        if !fs::symlink_metadata(path)?.is_file()
            || !self.scan().iter().any(|capture| capture.path() == path)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "not one of Horizon's captures",
            ));
        }
        let root = fs::canonicalize(&self.root)?;
        let file = fs::canonicalize(path)?;
        if !file.starts_with(&root) || !file.is_file() || MediaKind::from_path(&file).is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "not one of Horizon's captures",
            ));
        }
        fs::remove_file(&file)?;
        let mut folder = file.parent();
        while let Some(current) = folder
            && current != root
            && fs::remove_dir(current).is_ok()
        {
            folder = current.parent();
        }
        Ok(())
    }

    pub fn scan(&self) -> Vec<Capture> {
        let mut captures = Vec::new();
        if !fs::symlink_metadata(&self.root).is_ok_and(|m| m.is_dir()) {
            return captures;
        }
        let Ok(entries) = fs::read_dir(&self.root) else {
            return captures;
        };
        for entry in entries.filter_map(Result::ok) {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            if entry.file_name() == HORIZON_FOLDER {
                collect_capture_types(&entry.path(), None, &mut captures);
                continue;
            }
            let Ok(games) = fs::read_dir(entry.path()) else {
                continue;
            };
            for game in games.filter_map(Result::ok) {
                if !game.file_type().is_ok_and(|kind| kind.is_dir()) {
                    continue;
                }
                match crate::services::game_storage::GameFolder::read(&game.path()) {
                    Ok(folder) => {
                        let Ok(link) = folder.manifest.game() else {
                            continue;
                        };
                        if crate::platform::data_paths::game_system_directory(
                            &self.root,
                            link.source_id().as_str(),
                        ) != entry.path()
                        {
                            continue;
                        }
                        collect_capture_types(&folder.directory, Some(&link), &mut captures);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        tracing::warn!(%error, "Album game metadata unavailable; files preserved")
                    }
                }
            }
        }
        captures
    }
}

/// The temporary name a writer uses until the capture is complete.
pub fn part_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

fn collect_capture_types(folder: &Path, game: Option<&SourceGameRef>, out: &mut Vec<Capture>) {
    let path = crate::platform::data_paths::capture_directory(folder);
    if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_dir()) {
        collect_files(&path, game, out);
    }
}

fn collect_files(folder: &Path, game: Option<&SourceGameRef>, out: &mut Vec<Capture>) {
    let Ok(files) = fs::read_dir(folder) else {
        return;
    };
    for file in files.filter_map(Result::ok) {
        let path = file.path();
        let Some(kind) = MediaKind::from_path(&path) else {
            continue;
        };
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let captured_at = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(parse_stamp)
            .or_else(|| metadata.modified().ok().and_then(unix_millis))
            .unwrap_or(0);
        out.push(Capture::new(
            path,
            kind,
            CaptureOrigin::Horizon,
            game.cloned(),
            captured_at,
        ));
    }
}

/// `YYYY-MM-DD_HH-MM-SS-mmm`, optionally followed by `-<copy>`.
fn parse_stamp(stem: &str) -> Option<i64> {
    let seconds = NaiveDateTime::parse_from_str(stem.get(..19)?, STAMP_FORMAT).ok()?;
    let millis: i64 = stem.get(20..23)?.parse().ok()?;
    (stem.as_bytes().get(19) == Some(&b'-')).then(|| seconds.and_utc().timestamp_millis() + millis)
}

fn unix_millis(time: SystemTime) -> Option<i64> {
    let millis = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_millis();
    i64::try_from(millis).ok()
}

/// Every capture Horizon can show, newest first.
pub fn scan_all(
    horizon: Option<&HorizonAlbum>,
    sources: &[Arc<dyn CaptureSource>],
) -> Vec<Capture> {
    let mut captures = horizon.map(HorizonAlbum::scan).unwrap_or_default();
    for source in sources {
        let origin = CaptureOrigin::Source(source.source_id().clone());
        captures.extend(source.captures().into_iter().map(|capture| {
            let game = capture
                .external_id
                .map(|id| SourceGameRef::new(source.source_id().clone(), id));
            Capture::new(
                capture.path,
                capture.kind,
                origin.clone(),
                game,
                capture.captured_at,
            )
            .with_preview(capture.preview)
            .with_deletable(capture.deletable)
        }));
    }
    captures.sort_by(newest_first);
    captures
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ExternalGameId, SourceId};
    use chrono::TimeZone;

    fn temp_dir(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("horizon-album-{name}-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp directory");
        path
    }

    fn game(source: &str, id: &str) -> SourceGameRef {
        SourceGameRef::new(
            SourceId::new(source).expect("source"),
            ExternalGameId::new(id).expect("id"),
        )
    }

    #[test]
    fn production_album_keeps_own_captures_without_reading_steam_screenshots() {
        let root = temp_dir("horizon-only");
        let album = HorizonAlbum::new(root.join("album"));
        let now = Utc.with_ymd_and_hms(2026, 10, 15, 12, 0, 0).unwrap();
        let own = album
            .new_capture_path(None, "Horizon", "jxl", now)
            .expect("capture path");
        fs::write(&own, b"own capture").expect("capture");
        let steam = root.join("Steam/userdata/123/760/remote/480/screenshots");
        fs::create_dir_all(&steam).expect("Steam screenshots");
        let external = steam.join("20261015120000_1.jpg");
        fs::write(&external, b"Steam screenshot").expect("external capture");

        let sources = crate::sources::production_capture_sources().expect("capture registry");
        assert!(sources.is_empty());
        let captures = scan_all(Some(&album), &sources);
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].path(), own.as_path());
        assert!(captures[0].origin().is_horizon());
        assert_eq!(
            fs::read(&external).expect("untouched screenshot"),
            b"Steam screenshot"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn new_captures_are_filed_by_game_and_found_again_by_a_scan() {
        let root = temp_dir("round-trip");
        let album = HorizonAlbum::new(root.clone());
        let now = Utc.with_ymd_and_hms(2026, 10, 10, 14, 30, 12).unwrap()
            + chrono::Duration::milliseconds(345);
        let heroic = game("heroic", "Some/Game");

        let first = album
            .new_capture_path(Some(&heroic), "Some Game", "jxl", now)
            .unwrap();
        assert_eq!(first.file_name().unwrap(), "2026-10-10_14-30-12-345.jxl");
        assert!(first.parent().unwrap().ends_with("screenshots"));
        assert!(
            first
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("Some Game [")
        );
        fs::write(&first, b"jxl").unwrap();
        // The same millisecond never overwrites.
        let second = album
            .new_capture_path(Some(&heroic), "Some Game", "jxl", now)
            .unwrap();
        assert_eq!(second.file_name().unwrap(), "2026-10-10_14-30-12-345-2.jxl");
        fs::write(&second, b"jxl").unwrap();
        // A capture still being written is invisible.
        let pending = album.new_capture_path(None, "Horizon", "jxl", now).unwrap();
        fs::write(part_path(&pending), b"incomplete image").unwrap();
        assert!(album.new_capture_path(None, "Horizon", "mp4", now).is_err());
        let recordings = root.join("horizon/recordings");
        fs::create_dir_all(&recordings).unwrap();
        let recording = recordings.join("2026-10-10_14-30-12-345.mp4");
        fs::write(&recording, b"preserved recording").unwrap();
        // The retired folder is never scanned, even for a supported extension.
        let retired_image = recordings.join("unlisted.jxl");
        fs::write(&retired_image, b"preserved image in retired folder").unwrap();
        let misplaced_clip = first.parent().unwrap().join("unlisted.mp4");
        fs::write(&misplaced_clip, b"preserved unsupported format").unwrap();
        // Retired image formats remain untouched but are no longer listed.
        let legacy = first.parent().unwrap().join("legacy.png");
        fs::write(&legacy, b"preserved legacy picture").unwrap();
        assert!(album.new_capture_path(None, "Horizon", "png", now).is_err());
        let mut captures = album.scan();
        captures.sort_by(newest_first);
        assert_eq!(captures.len(), 2);
        assert_eq!(fs::read(&legacy).unwrap(), b"preserved legacy picture");
        assert!(captures.iter().all(|c| c.game() == Some(&heroic)));
        assert!(
            captures
                .iter()
                .all(|c| c.captured_at() == now.timestamp_millis())
        );
        assert!(captures.iter().all(|c| c.origin().is_horizon()));

        assert_eq!(album.scan().len(), 2);
        assert_eq!(fs::read(&recording).unwrap(), b"preserved recording");
        assert!(album.delete(&recording).is_err());
        assert!(album.delete(&retired_image).is_err());
        assert!(album.delete(&misplaced_clip).is_err());
        assert!(retired_image.is_file());
        assert!(misplaced_clip.is_file());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn delete_removes_only_horizon_captures_and_tidies_empty_folders() {
        let root = temp_dir("delete");
        let album = HorizonAlbum::new(root.join("album"));
        let now = Utc.with_ymd_and_hms(2026, 10, 10, 14, 30, 12).unwrap();
        let shot = album
            .new_capture_path(Some(&game("steam", "10")), "Test game", "jxl", now)
            .unwrap();
        fs::write(&shot, b"jxl").unwrap();
        let outside = root.join("steam-screenshot.jxl");
        fs::write(&outside, b"jxl").unwrap();
        let not_media = root.join("album/horizon/notes.txt");
        fs::create_dir_all(not_media.parent().unwrap()).unwrap();
        fs::write(&not_media, b"text").unwrap();

        assert!(album.delete(&outside).is_err(), "outside the album");
        assert!(outside.exists());
        assert!(album.delete(&not_media).is_err(), "not a capture");
        assert!(
            album
                .delete(&root.join("album/../steam-screenshot.jxl"))
                .is_err()
        );
        album.delete(&shot).unwrap();
        assert!(!shot.exists());
        assert!(
            !shot.parent().unwrap().exists(),
            "empty capture folders removed; game metadata retained"
        );
        assert!(root.join("album").exists(), "the album itself stays");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_missing_album_folder_is_simply_empty() {
        let album = HorizonAlbum::new(PathBuf::from("/nonexistent/horizon-album"));
        assert!(album.scan().is_empty());
    }

    #[test]
    fn metadata_and_media_symlinks_cannot_retarget_captures() {
        let root = temp_dir("metadata");
        let album = HorizonAlbum::new(root.join("album"));
        let reference = game("gba", &format!("sha256:{}", "a".repeat(64)));
        let now = Utc.with_ymd_and_hms(2026, 10, 10, 14, 30, 12).unwrap();
        let shot = album
            .new_capture_path(Some(&reference), "Readable title", "jxl", now)
            .unwrap();
        fs::write(&shot, b"keep capture").unwrap();
        let outside = root.join("outside.jxl");
        fs::write(&outside, b"outside").unwrap();
        let link = shot.parent().unwrap().join("linked.jxl");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert_eq!(album.scan().len(), 1);
        assert!(album.delete(&link).is_err());
        let manifest = crate::platform::data_paths::game_manifest_path(
            shot.parent().unwrap().parent().unwrap(),
        );
        fs::write(&manifest, b"invalid metadata").unwrap();
        assert!(album.scan().is_empty());
        assert!(album.delete(&shot).is_err());
        assert_eq!(fs::read(&shot).unwrap(), b"keep capture");
        assert_eq!(fs::read(&outside).unwrap(), b"outside");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stamps_parse_with_or_without_a_copy_suffix() {
        let at = Utc
            .with_ymd_and_hms(2026, 1, 2, 3, 4, 5)
            .unwrap()
            .timestamp_millis()
            + 6;
        assert_eq!(parse_stamp("2026-01-02_03-04-05-006"), Some(at));
        assert_eq!(parse_stamp("2026-01-02_03-04-05-006-3"), Some(at));
        assert_eq!(parse_stamp("holiday"), None);
    }
}
