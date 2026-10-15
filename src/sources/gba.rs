//! Reviewed GBA imports are persisted explicitly; startup never auto-imports.

pub mod recognition;
use super::{
    GameSource, SourceDescriptor, SourceDiscovery, SourceError, SourceInitializationError,
    SourceSnapshot,
};
use crate::domain::SourceId;

pub struct GbaSource {
    descriptor: SourceDescriptor,
}
impl GbaSource {
    pub fn new() -> Result<Self, SourceInitializationError> {
        Ok(Self {
            descriptor: SourceDescriptor::new(SourceId::new("gba")?, "Game Boy Advance", vec![])?,
        })
    }
}
impl GameSource for GbaSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }
    fn artwork_search_title(&self, title: &str) -> Option<String> {
        let mut search = title.trim();
        while let Some(without_close) = search.strip_suffix(')') {
            let Some((base, tag)) = without_close.rsplit_once(" (") else {
                break;
            };
            if base.trim().is_empty() || !is_release_metadata(tag) {
                break;
            }
            search = base.trim_end();
        }
        (search != title.trim()).then(|| search.to_owned())
    }
    fn discover(&self) -> Result<SourceDiscovery, SourceError> {
        // Partial: no scan may erase a previously reviewed import.
        SourceSnapshot::new(vec![])
            .map(SourceDiscovery::Available)
            .map_err(SourceError::new)
    }
}

fn is_release_metadata(tag: &str) -> bool {
    let tag = tag.to_ascii_lowercase();
    if let Some(revision) = tag.strip_prefix("rev ") {
        return !revision.is_empty() && revision.chars().all(|c| c.is_ascii_digit());
    }
    tag.split(',').all(|part| {
        matches!(
            part.trim(),
            "usa"
                | "europe"
                | "japan"
                | "world"
                | "australia"
                | "canada"
                | "france"
                | "germany"
                | "italy"
                | "spain"
                | "netherlands"
                | "sweden"
                | "korea"
                | "china"
                | "brazil"
                | "en"
                | "fr"
                | "de"
                | "es"
                | "it"
                | "ja"
                | "nl"
                | "pt"
                | "sv"
                | "da"
                | "fi"
                | "no"
                | "ko"
                | "zh"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artwork_title_removes_only_known_trailing_release_metadata() {
        let source = GbaSource::new().unwrap();
        for (title, expected) in [
            (
                "Pokemon - LeafGreen Version (USA)",
                "Pokemon - LeafGreen Version",
            ),
            (
                "Pokemon - Emerald Version (USA, Europe)",
                "Pokemon - Emerald Version",
            ),
            ("Game (Europe) (En,Fr,De) (Rev 1)", "Game"),
            ("Game (Special Edition) (USA)", "Game (Special Edition)"),
        ] {
            assert_eq!(
                source.artwork_search_title(title).as_deref(),
                Some(expected)
            );
        }
        for title in [
            "Game",
            "Game (Beta)",
            "Game (Hack)",
            "Game (Unknown)",
            "Game (USA) Deluxe",
            " (USA)",
        ] {
            assert_eq!(source.artwork_search_title(title), None);
        }
    }
}
