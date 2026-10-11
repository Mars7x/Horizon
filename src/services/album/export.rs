//! Saving captures for the user: a copy in their Pictures folder
//! (`platform::data_paths::pictures_export_dir`), under a name a person can
//! read: `<Game> 2026-10-11 14.30.12.png`, or `Horizon …` for captures of
//! Horizon itself. An existing file is never replaced (` (2)`, ` (3)`… are
//! added), and the copy is written to `.part` first, so a half-copied file
//! never appears under its real name.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use chrono::{Local, TimeZone};

use crate::domain::album::Capture;

/// Copies `capture` into `folder` (created when missing) and returns the
/// copy's path. `title` names its game.
pub fn export(capture: &Capture, title: Option<&str>, folder: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let extension = capture
        .path()
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let stem = export_stem(title, capture.captured_at());
    let target = (1..1000)
        .map(|n| {
            let name = if n == 1 {
                format!("{stem}.{extension}")
            } else {
                format!("{stem} ({n}).{extension}")
            };
            folder.join(name)
        })
        .find(|path| !path.exists())
        .ok_or_else(|| io::Error::new(io::ErrorKind::AlreadyExists, "no free name"))?;
    let partial = super::part_path(&target);
    let copied = fs::copy(capture.path(), &partial).and_then(|_| {
        // Not over a file that appeared meanwhile.
        if target.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "taken meanwhile",
            ));
        }
        fs::rename(&partial, &target)
    });
    if let Err(error) = copied {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    Ok(target)
}

/// `<Game> 2026-10-11 14.30.12` in local time, with the title made safe to
/// use as a file name.
fn export_stem(title: Option<&str>, captured_at: i64) -> String {
    let title: String = title
        .unwrap_or("Horizon")
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let title: String = title.trim_matches('.').chars().take(80).collect();
    let title = if title.is_empty() {
        "Horizon".to_owned()
    } else {
        title
    };
    let when = Local
        .timestamp_millis_opt(captured_at)
        .single()
        .map(|time| time.format("%Y-%m-%d %H.%M.%S").to_string())
        .unwrap_or_else(|| "capture".to_owned());
    format!("{title} {when}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::album::{CaptureOrigin, MediaKind};

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("horizon-export-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn exports_a_readable_copy_and_never_replaces_one() {
        let dir = temp_dir("copy");
        let source = dir.join("20261011-183012-000.png");
        fs::write(&source, b"pixels").unwrap();
        let at = Local
            .with_ymd_and_hms(2026, 10, 11, 14, 30, 12)
            .unwrap()
            .timestamp_millis();
        let capture = Capture::new(source, MediaKind::Photo, CaptureOrigin::Horizon, None, at);
        let pictures = dir.join("Pictures/Horizon");

        let first = export(&capture, Some("Hollow Knight: Silksong"), &pictures).unwrap();
        assert_eq!(
            first.file_name().unwrap(),
            "Hollow Knight: Silksong 2026-10-11 14.30.12.png"
        );
        assert_eq!(fs::read(&first).unwrap(), b"pixels");
        let second = export(&capture, Some("Hollow Knight: Silksong"), &pictures).unwrap();
        assert_eq!(
            second.file_name().unwrap(),
            "Hollow Knight: Silksong 2026-10-11 14.30.12 (2).png"
        );
        assert_eq!(
            fs::read_dir(&pictures).unwrap().count(),
            2,
            "no .part left behind"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn names_are_safe_file_names() {
        let at = Local
            .with_ymd_and_hms(2026, 1, 2, 3, 4, 5)
            .unwrap()
            .timestamp_millis();
        assert_eq!(export_stem(None, at), "Horizon 2026-01-02 03.04.05");
        assert_eq!(
            export_stem(Some("A/B\\C\n D"), at),
            "A B C D 2026-01-02 03.04.05"
        );
        assert_eq!(export_stem(Some(".."), at), "Horizon 2026-01-02 03.04.05");
    }

    #[test]
    fn a_missing_capture_fails_and_leaves_nothing() {
        let dir = temp_dir("missing");
        let capture = Capture::new(
            dir.join("gone.png"),
            MediaKind::Photo,
            CaptureOrigin::Horizon,
            None,
            0,
        );
        let pictures = dir.join("out");
        assert!(export(&capture, None, &pictures).is_err());
        assert_eq!(fs::read_dir(&pictures).unwrap().count(), 0);
        fs::remove_dir_all(dir).unwrap();
    }
}
