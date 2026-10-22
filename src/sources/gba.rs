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
            .map(|hash| SourceLaunchTarget::EmbeddedCartridge {
                system: self.system,
                sha256: hash.to_owned(),
            }))
    }
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }
    fn artwork_search_title(&self, title: &str) -> Option<String> {
        crate::domain::rom_titles::artwork_title(title)
    }

    fn discover(&self) -> Result<SourceDiscovery, SourceError> {
        // Partial: no scan may erase a previously reviewed import.
        SourceSnapshot::new(vec![])
            .map(SourceDiscovery::Available)
            .map_err(SourceError::new)
    }
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
            (
                "Pokemon - Gold Version (USA, Europe) (SGB Enhanced) (GB Compatible)",
                "Pokemon - Gold Version",
            ),
            (
                "Pokemon - Black Version 2 (USA, Europe) (NDSi Enhanced)",
                "Pokemon - Black Version 2",
            ),
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
