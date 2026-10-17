//! Imported Game Boy, Game Boy Color and GBA content is played by Horizon's
//! bundled mGBA core. Each system is its own source.

pub mod recognition;
use super::{
    GameSource, SourceDescriptor, SourceDiscovery, SourceError, SourceInitializationError,
    SourceSnapshot,
};
use super::{SourceCapability, SourceLaunchTarget};
use crate::domain::{ExternalGameId, SourceId};
use recognition::RomSystem;

pub struct CartridgeSource {
    system: RomSystem,
    descriptor: SourceDescriptor,
}
impl CartridgeSource {
    pub fn new(system: RomSystem) -> Result<Self, SourceInitializationError> {
        Ok(Self {
            system,
            descriptor: SourceDescriptor::new(
                SourceId::new(system.source_id())?,
                system.name(),
                vec![SourceCapability::Launch],
            )?,
        })
    }
}
impl GameSource for CartridgeSource {
    fn launch_target(
        &self,
        id: &ExternalGameId,
    ) -> Result<Option<SourceLaunchTarget>, SourceError> {
        Ok(id
            .as_str()
            .strip_prefix("sha256:")
            .filter(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .map(|hash| SourceLaunchTarget::EmbeddedMgba {
                system: self.system,
                sha256: hash.to_owned(),
            }))
    }
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
        let source = CartridgeSource::new(RomSystem::GameBoyAdvance).unwrap();
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
