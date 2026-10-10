//! Album captures: screenshots and video clips, whoever made them.
//!
//! A capture is a file on disk. The file is the record: there is no database
//! row to keep in step, so a capture deleted outside Horizon simply stops
//! appearing. Everything else (thumbnails, durations) is rebuildable cache.
use std::{cmp::Ordering, path::PathBuf};

use super::{SourceGameRef, SourceId};

/// What a capture contains. Decided by file extension; content is never
/// sniffed, so a scan only lists directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaKind {
    Photo,
    Video,
}

impl MediaKind {
    /// Formats Horizon can decode: photos through `image`, videos through the
    /// platform's video backend.
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "png" | "jpg" | "jpeg" => Some(Self::Photo),
            "mp4" | "m4v" | "mkv" | "webm" | "mov" => Some(Self::Video),
            _ => None,
        }
    }

    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        path.extension()
            .and_then(|extension| extension.to_str())
            .and_then(Self::from_extension)
    }
}

/// Who made a capture, and therefore who owns the file. Horizon only ever
/// reads files a source made; only its own captures may later be edited or
/// deleted from the Album.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CaptureOrigin {
    Horizon,
    Source(SourceId),
}

impl CaptureOrigin {
    pub fn is_horizon(&self) -> bool {
        matches!(self, Self::Horizon)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    path: PathBuf,
    kind: MediaKind,
    origin: CaptureOrigin,
    /// The game on screen, as its source names it. `None` for captures of
    /// Horizon itself or files whose game is unknown.
    game: Option<SourceGameRef>,
    /// Unix milliseconds.
    captured_at: i64,
    /// A smaller copy the owner already keeps (Steam's `thumbnails/`).
    preview: Option<PathBuf>,
}

impl Capture {
    pub fn new(
        path: PathBuf,
        kind: MediaKind,
        origin: CaptureOrigin,
        game: Option<SourceGameRef>,
        captured_at: i64,
    ) -> Self {
        Self {
            path,
            kind,
            origin,
            game,
            captured_at,
            preview: None,
        }
    }

    pub fn with_preview(mut self, preview: Option<PathBuf>) -> Self {
        self.preview = preview;
        self
    }

    /// The path is the capture's identity.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn kind(&self) -> MediaKind {
        self.kind
    }

    pub fn origin(&self) -> &CaptureOrigin {
        &self.origin
    }

    pub fn game(&self) -> Option<&SourceGameRef> {
        self.game.as_ref()
    }

    pub fn captured_at(&self) -> i64 {
        self.captured_at
    }

    /// Only Horizon's own captures can be deleted from the Album; a
    /// source's files belong to that source.
    pub fn deletable(&self) -> bool {
        self.origin.is_horizon()
    }

    pub fn preview(&self) -> Option<&std::path::Path> {
        self.preview.as_deref()
    }
}

/// Album order: newest first; the path breaks ties so order is stable.
pub fn newest_first(a: &Capture, b: &Capture) -> Ordering {
    b.captured_at
        .cmp(&a.captured_at)
        .then_with(|| a.path.cmp(&b.path))
}

/// Make an id safe as one directory name. Unreserved characters stay
/// readable; everything else becomes `%XX`, so decoding is exact.
pub fn encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    // "." and ".." are not names.
    if encoded.chars().all(|c| c == '.') {
        encoded = encoded.replace('.', "%2E");
    }
    encoded
}

pub fn decode_path_segment(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = segment.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_kind_follows_the_extension_case_insensitively() {
        assert_eq!(MediaKind::from_extension("PNG"), Some(MediaKind::Photo));
        assert_eq!(MediaKind::from_extension("jpeg"), Some(MediaKind::Photo));
        assert_eq!(MediaKind::from_extension("webm"), Some(MediaKind::Video));
        assert_eq!(MediaKind::from_extension("part"), None);
        assert_eq!(MediaKind::from_extension("vdf"), None);
    }

    #[test]
    fn path_segments_round_trip_and_never_escape_their_folder() {
        for id in ["1245620", "Fortnite", "a/b", "..", "空 game", "%41"] {
            let encoded = encode_path_segment(id);
            assert!(!encoded.contains('/'));
            assert_ne!(encoded, "..");
            assert_ne!(encoded, ".");
            assert_eq!(decode_path_segment(&encoded).as_deref(), Some(id));
        }
        assert_eq!(encode_path_segment("1245620"), "1245620");
        assert_eq!(decode_path_segment("%zz"), None);
    }

    #[test]
    fn newest_captures_come_first_with_a_stable_tie_break() {
        let capture = |path: &str, at| {
            Capture::new(
                PathBuf::from(path),
                MediaKind::Photo,
                CaptureOrigin::Horizon,
                None,
                at,
            )
        };
        let mut captures = [capture("b", 1), capture("a", 1), capture("c", 5)];
        captures.sort_by(newest_first);
        let paths: Vec<_> = captures.iter().map(|c| c.path().to_owned()).collect();
        assert_eq!(paths, ["c", "a", "b"].map(PathBuf::from));
    }
}
