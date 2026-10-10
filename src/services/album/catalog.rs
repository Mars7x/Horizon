//! Finding captures: Horizon's own folder plus every capture source.
//!
//! Horizon's folder layout (see docs/ALBUM.md):
//!
//! ```text
//! album/<source>/<external id>/<UTC stamp>.<ext>   a game's captures
//! album/horizon/<UTC stamp>.<ext>                   Horizon's own screens
//! ```
//!
//! Folder names are `encode_path_segment` of the ids; the stamp is
//! `YYYYMMDD-HHMMSS-mmm` in UTC. A writer saves to `<name>.part` and renames
//! when complete, so a scan never lists a half-written file (`.part` is not a
//! media extension).
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use chrono::{DateTime, NaiveDateTime, Utc};

use crate::{
    domain::{
        ExternalGameId, SourceGameRef, SourceId,
        album::{
            Capture, CaptureOrigin, MediaKind, decode_path_segment, encode_path_segment,
            newest_first,
        },
    },
    sources::CaptureSource,
};

const HORIZON_FOLDER: &str = "horizon";
const STAMP_FORMAT: &str = "%Y%m%d-%H%M%S";

/// Horizon's own capture folder.
#[derive(Debug, Clone)]
pub struct HorizonAlbum {
    root: PathBuf,
}

impl HorizonAlbum {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Where a new capture goes, with its folder created. The screenshot and
    /// recording features call this, write `<path>.part`, then rename it to
    /// `path`. Never returns an existing file.
    pub fn new_capture_path(
        &self,
        game: Option<&SourceGameRef>,
        extension: &str,
        now: DateTime<Utc>,
    ) -> std::io::Result<PathBuf> {
        let folder = match game {
            Some(game) => self
                .root
                .join(encode_path_segment(game.source_id().as_str()))
                .join(encode_path_segment(game.external_id().as_str())),
            None => self.root.join(HORIZON_FOLDER),
        };
        fs::create_dir_all(&folder)?;
        let stamp = format!(
            "{}-{:03}",
            now.format(STAMP_FORMAT),
            now.timestamp_subsec_millis()
        );
        let mut candidate = folder.join(format!("{stamp}.{extension}"));
        let mut copy = 2;
        while candidate.exists() || part_path(&candidate).exists() {
            candidate = folder.join(format!("{stamp}-{copy}.{extension}"));
            copy += 1;
        }
        Ok(candidate)
    }

    /// Permanently delete one of Horizon's own captures, then any game
    /// folder it leaves empty. Refuses anything outside this album or that
    /// is not a capture, so a source's files can never be deleted here.
    pub fn delete(&self, path: &Path) -> std::io::Result<()> {
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
        let Ok(entries) = fs::read_dir(&self.root) else {
            return captures;
        };
        for entry in entries.filter_map(Result::ok) {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if name == HORIZON_FOLDER {
                collect_files(&path, None, &mut captures);
                continue;
            }
            let Some(source_id) = decode_path_segment(&name).and_then(|n| SourceId::new(n).ok())
            else {
                continue;
            };
            let Ok(games) = fs::read_dir(&path) else {
                continue;
            };
            for game in games.filter_map(Result::ok) {
                let Some(external_id) = game
                    .file_name()
                    .to_str()
                    .and_then(decode_path_segment)
                    .and_then(|id| ExternalGameId::new(id).ok())
                else {
                    continue;
                };
                let link = SourceGameRef::new(source_id.clone(), external_id);
                collect_files(&game.path(), Some(&link), &mut captures);
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

fn collect_files(folder: &Path, game: Option<&SourceGameRef>, out: &mut Vec<Capture>) {
    let Ok(files) = fs::read_dir(folder) else {
        return;
    };
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

/// `YYYYMMDD-HHMMSS-mmm`, optionally followed by `-<copy>`.
fn parse_stamp(stem: &str) -> Option<i64> {
    let seconds = NaiveDateTime::parse_from_str(stem.get(..15)?, STAMP_FORMAT).ok()?;
    let millis: i64 = stem.get(16..19)?.parse().ok()?;
    (stem.as_bytes().get(15) == Some(&b'-')).then(|| seconds.and_utc().timestamp_millis() + millis)
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
        }));
    }
    captures.sort_by(newest_first);
    captures
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn new_captures_are_filed_by_game_and_found_again_by_a_scan() {
        let root = temp_dir("round-trip");
        let album = HorizonAlbum::new(root.clone());
        let now = Utc.with_ymd_and_hms(2026, 10, 10, 14, 30, 12).unwrap()
            + chrono::Duration::milliseconds(345);
        let heroic = game("heroic", "Some/Game");

        let first = album.new_capture_path(Some(&heroic), "png", now).unwrap();
        assert_eq!(
            first,
            root.join("heroic/Some%2FGame/20261010-143012-345.png")
        );
        fs::write(&first, b"png").unwrap();
        // The same millisecond never overwrites.
        let second = album.new_capture_path(Some(&heroic), "png", now).unwrap();
        assert_eq!(second.file_name().unwrap(), "20261010-143012-345-2.png");
        fs::write(&second, b"png").unwrap();
        // A capture still being written is invisible.
        let writing = album.new_capture_path(None, "mp4", now).unwrap();
        assert_eq!(writing, root.join("horizon/20261010-143012-345.mp4"));
        fs::write(part_path(&writing), b"mp4").unwrap();

        let mut captures = album.scan();
        captures.sort_by(newest_first);
        assert_eq!(captures.len(), 2);
        assert!(captures.iter().all(|c| c.game() == Some(&heroic)));
        assert!(
            captures
                .iter()
                .all(|c| c.captured_at() == now.timestamp_millis())
        );
        assert!(captures.iter().all(|c| c.origin().is_horizon()));

        fs::rename(part_path(&writing), &writing).unwrap();
        let finished = album.scan();
        let video = finished
            .iter()
            .find(|c| c.kind() == MediaKind::Video)
            .expect("finished video");
        assert_eq!(video.game(), None);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn delete_removes_only_horizon_captures_and_tidies_empty_folders() {
        let root = temp_dir("delete");
        let album = HorizonAlbum::new(root.join("album"));
        let now = Utc.with_ymd_and_hms(2026, 10, 10, 14, 30, 12).unwrap();
        let shot = album
            .new_capture_path(Some(&game("steam", "10")), "png", now)
            .unwrap();
        fs::write(&shot, b"png").unwrap();
        let outside = root.join("steam-screenshot.png");
        fs::write(&outside, b"png").unwrap();
        let not_media = root.join("album/horizon/notes.txt");
        fs::create_dir_all(not_media.parent().unwrap()).unwrap();
        fs::write(&not_media, b"text").unwrap();

        assert!(album.delete(&outside).is_err(), "outside the album");
        assert!(outside.exists());
        assert!(album.delete(&not_media).is_err(), "not a capture");
        assert!(
            album
                .delete(&root.join("album/../steam-screenshot.png"))
                .is_err()
        );
        album.delete(&shot).unwrap();
        assert!(!shot.exists());
        assert!(
            !root.join("album/steam").exists(),
            "empty game folders removed"
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
    fn stamps_parse_with_or_without_a_copy_suffix() {
        let at = Utc
            .with_ymd_and_hms(2026, 1, 2, 3, 4, 5)
            .unwrap()
            .timestamp_millis()
            + 6;
        assert_eq!(parse_stamp("20260102-030405-006"), Some(at));
        assert_eq!(parse_stamp("20260102-030405-006-3"), Some(at));
        assert_eq!(parse_stamp("holiday"), None);
    }
}
