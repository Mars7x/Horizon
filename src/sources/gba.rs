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
    fn discover(&self) -> Result<SourceDiscovery, SourceError> {
        // Partial: no scan may erase a previously reviewed import.
        SourceSnapshot::new(vec![])
            .map(SourceDiscovery::Available)
            .map_err(SourceError::new)
    }
}
