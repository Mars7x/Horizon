//! Source adapter contracts.
//!
//! Provider-specific parsing and host integration terminate behind `GameSource`.
//! Services consume only normalized source metadata and discovery snapshots.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use thiserror::Error;

use crate::domain::{ExternalGameId, GameTitle, SourceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceCapability {
    Launch,
    Artwork,
    LifetimePlaytime,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SourceDescriptorError {
    #[error("source display name must not be empty")]
    EmptyDisplayName,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDescriptor {
    id: SourceId,
    display_name: String,
    capabilities: Vec<SourceCapability>,
}

impl SourceDescriptor {
    pub fn new(
        id: SourceId,
        display_name: impl Into<String>,
        mut capabilities: Vec<SourceCapability>,
    ) -> Result<Self, SourceDescriptorError> {
        let display_name = display_name.into();
        let display_name = display_name.trim();
        if display_name.is_empty() {
            return Err(SourceDescriptorError::EmptyDisplayName);
        }

        capabilities.sort_unstable();
        capabilities.dedup();

        Ok(Self {
            id,
            display_name: display_name.to_owned(),
            capabilities,
        })
    }

    pub fn id(&self) -> &SourceId {
        &self.id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn capabilities(&self) -> &[SourceCapability] {
        &self.capabilities
    }

    pub fn supports(&self, capability: SourceCapability) -> bool {
        self.capabilities.binary_search(&capability).is_ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceGame {
    external_id: ExternalGameId,
    title: GameTitle,
}

impl SourceGame {
    pub fn new(external_id: ExternalGameId, title: GameTitle) -> Self {
        Self { external_id, title }
    }

    pub fn external_id(&self) -> &ExternalGameId {
        &self.external_id
    }

    pub fn title(&self) -> &GameTitle {
        &self.title
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SourceSnapshotError {
    #[error("source snapshot contains duplicate external game id {0}")]
    DuplicateExternalGameId(ExternalGameId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSnapshot {
    games: Vec<SourceGame>,
}

impl SourceSnapshot {
    pub fn new(games: Vec<SourceGame>) -> Result<Self, SourceSnapshotError> {
        let mut ids = BTreeSet::new();
        for game in &games {
            if !ids.insert(game.external_id().clone()) {
                return Err(SourceSnapshotError::DuplicateExternalGameId(
                    game.external_id().clone(),
                ));
            }
        }

        Ok(Self { games })
    }

    pub fn games(&self) -> &[SourceGame] {
        &self.games
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceUnavailableReason {
    NotInstalled,
    NotConfigured,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceDiscovery {
    Unavailable(SourceUnavailableReason),
    Available(SourceSnapshot),
}

#[derive(Debug)]
pub struct SourceError {
    source: Box<dyn Error + Send + Sync>,
}

impl SourceError {
    pub fn new<E>(source: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        Self {
            source: Box::new(source),
        }
    }
}

impl fmt::Display for SourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.source, formatter)
    }
}

impl Error for SourceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Provider adapter boundary.
///
/// Implementations own provider-specific paths, file formats, and quirks. They
/// must return normalized values only; SQL, Slint types, and navigation policy
/// do not belong here.
pub trait GameSource: Send + Sync {
    fn descriptor(&self) -> &SourceDescriptor;
    fn discover(&self) -> Result<SourceDiscovery, SourceError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SourceRegistryError {
    #[error("source id {0} is already registered")]
    DuplicateSourceId(SourceId),
}

#[derive(Default)]
pub struct SourceRegistry {
    sources: BTreeMap<SourceId, Box<dyn GameSource>>,
}

impl SourceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<S>(&mut self, source: S) -> Result<(), SourceRegistryError>
    where
        S: GameSource + 'static,
    {
        let source_id = source.descriptor().id().clone();
        if self.sources.contains_key(&source_id) {
            return Err(SourceRegistryError::DuplicateSourceId(source_id));
        }

        self.sources.insert(source_id, Box::new(source));
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn GameSource> {
        self.sources.values().map(Box::as_ref)
    }

    pub fn len(&self) -> usize {
        self.sources.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_id(value: &str) -> SourceId {
        SourceId::new(value).expect("source id")
    }

    fn source_game(external_id: &str, title: &str) -> SourceGame {
        SourceGame::new(
            ExternalGameId::new(external_id).expect("external id"),
            GameTitle::new(title).expect("title"),
        )
    }

    struct EmptySource {
        descriptor: SourceDescriptor,
    }

    impl EmptySource {
        fn new(id: &str) -> Self {
            Self {
                descriptor: SourceDescriptor::new(source_id(id), id, vec![])
                    .expect("descriptor"),
            }
        }
    }

    impl GameSource for EmptySource {
        fn descriptor(&self) -> &SourceDescriptor {
            &self.descriptor
        }

        fn discover(&self) -> Result<SourceDiscovery, SourceError> {
            Ok(SourceDiscovery::Available(
                SourceSnapshot::new(vec![]).expect("empty snapshot"),
            ))
        }
    }

    #[test]
    fn descriptor_normalizes_capabilities_without_source_name_checks() {
        let descriptor = SourceDescriptor::new(
            source_id("example"),
            "  Example Store  ",
            vec![
                SourceCapability::Launch,
                SourceCapability::Artwork,
                SourceCapability::Launch,
            ],
        )
        .expect("descriptor");

        assert_eq!(descriptor.display_name(), "Example Store");
        assert!(descriptor.supports(SourceCapability::Launch));
        assert!(descriptor.supports(SourceCapability::Artwork));
        assert!(!descriptor.supports(SourceCapability::LifetimePlaytime));
        assert_eq!(descriptor.capabilities().len(), 2);
    }

    #[test]
    fn snapshot_rejects_duplicate_external_identity() {
        let error = SourceSnapshot::new(vec![
            source_game("same", "First"),
            source_game("same", "Second"),
        ])
        .expect_err("duplicate external ids must fail");

        assert_eq!(
            error,
            SourceSnapshotError::DuplicateExternalGameId(
                ExternalGameId::new("same").expect("external id")
            )
        );
    }

    #[test]
    fn registry_rejects_duplicate_source_ids_and_iterates_stably() {
        let mut registry = SourceRegistry::new();
        registry.register(EmptySource::new("zeta")).expect("zeta");
        registry.register(EmptySource::new("alpha")).expect("alpha");

        let duplicate = registry
            .register(EmptySource::new("alpha"))
            .expect_err("duplicate source id must fail");
        assert_eq!(
            duplicate,
            SourceRegistryError::DuplicateSourceId(source_id("alpha"))
        );

        let ids = registry
            .iter()
            .map(|source| source.descriptor().id().as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["alpha", "zeta"]);
    }
}
