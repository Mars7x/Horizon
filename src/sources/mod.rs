//! Source adapter contracts.
//!
//! Provider-specific parsing and host integration terminate behind `GameSource`.
//! Services consume only normalized source metadata and discovery snapshots.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    path::PathBuf,
    time::SystemTime,
};

use thiserror::Error;

use crate::domain::{DomainValidationError, ExternalGameId, GameTitle, PlaytimeSeconds, SourceId};

pub mod heroic;
pub mod steam;
mod support;

/// Source-owned authoritative identifier in an external artwork catalog.
/// This does not couple generic services to source ID strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalArtworkId {
    SteamAppId(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceArtworkLocation {
    File(PathBuf),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceArtworkCandidate {
    location: SourceArtworkLocation,
}

impl SourceArtworkCandidate {
    /// A provider-owned candidate that is expected to be natively 1:1.
    ///
    /// `ArtworkService` still validates the decoded dimensions before the
    /// candidate can reach presentation. A mislabeled non-square file is
    /// rejected rather than cropped, padded, or otherwise converted to 1:1.
    pub fn local_square_icon(path: PathBuf) -> Self {
        Self {
            location: SourceArtworkLocation::File(path),
        }
    }

    /// In-memory equivalent of `local_square_icon`.
    pub fn in_memory_square_icon(bytes: Vec<u8>) -> Self {
        Self {
            location: SourceArtworkLocation::Bytes(bytes),
        }
    }

    pub fn location(&self) -> &SourceArtworkLocation {
        &self.location
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceLaunchTarget {
    Uri(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SourceManagedLaunchTargetError {
    #[error("managed launch program must not be empty")]
    EmptyProgram,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceManagedLaunchTarget {
    program: String,
    args: Vec<String>,
}

impl SourceManagedLaunchTarget {
    pub fn new(
        program: impl Into<String>,
        args: Vec<String>,
    ) -> Result<Self, SourceManagedLaunchTargetError> {
        let program = program.into();
        if program.trim().is_empty() {
            return Err(SourceManagedLaunchTargetError::EmptyProgram);
        }

        Ok(Self { program, args })
    }

    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRuntimeState {
    Running,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceCapability {
    Launch,
    ManagedSession,
    RuntimeObservation,
    Artwork,
    LifetimePlaytime,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SourceDescriptorError {
    #[error("source display name must not be empty")]
    EmptyDisplayName,
}

#[derive(Debug, Error)]
pub enum SourceInitializationError {
    #[error(transparent)]
    Domain(#[from] DomainValidationError),
    #[error(transparent)]
    Descriptor(#[from] SourceDescriptorError),
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
    #[error("authoritative source membership is missing discovered game id {0}")]
    GameOutsideAuthoritativeMembership(ExternalGameId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceSnapshotCompleteness {
    /// Discovery succeeded, but absence from this snapshot is not authoritative.
    Partial,
    /// Discovery includes authoritative membership for the source's installed games.
    Authoritative,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSnapshot {
    games: Vec<SourceGame>,
    authoritative_membership: Option<BTreeSet<ExternalGameId>>,
}

impl SourceSnapshot {
    /// Construct a conservative snapshot. Missing games are not removed from
    /// persistence when a source cannot prove that discovery was complete.
    pub fn new(games: Vec<SourceGame>) -> Result<Self, SourceSnapshotError> {
        Self::validate_unique_games(&games)?;
        Ok(Self {
            games,
            authoritative_membership: None,
        })
    }

    /// Construct a complete snapshot whose authoritative membership is exactly
    /// the games with usable normalized metadata.
    pub fn authoritative(games: Vec<SourceGame>) -> Result<Self, SourceSnapshotError> {
        let membership = games
            .iter()
            .map(|game| game.external_id().clone())
            .collect::<Vec<_>>();
        Self::authoritative_with_membership(games, membership)
    }

    /// Construct a complete snapshot with a broader installed-membership set.
    /// This lets an adapter preserve an installed identity whose presentation
    /// metadata is temporarily unavailable while still pruning truly absent IDs.
    pub fn authoritative_with_membership(
        games: Vec<SourceGame>,
        present_external_ids: impl IntoIterator<Item = ExternalGameId>,
    ) -> Result<Self, SourceSnapshotError> {
        Self::validate_unique_games(&games)?;
        let authoritative_membership = present_external_ids.into_iter().collect::<BTreeSet<_>>();

        for game in &games {
            if !authoritative_membership.contains(game.external_id()) {
                return Err(SourceSnapshotError::GameOutsideAuthoritativeMembership(
                    game.external_id().clone(),
                ));
            }
        }

        Ok(Self {
            games,
            authoritative_membership: Some(authoritative_membership),
        })
    }

    fn validate_unique_games(games: &[SourceGame]) -> Result<(), SourceSnapshotError> {
        let mut ids = BTreeSet::new();
        for game in games {
            if !ids.insert(game.external_id().clone()) {
                return Err(SourceSnapshotError::DuplicateExternalGameId(
                    game.external_id().clone(),
                ));
            }
        }
        Ok(())
    }

    pub fn games(&self) -> &[SourceGame] {
        &self.games
    }

    pub fn completeness(&self) -> SourceSnapshotCompleteness {
        if self.authoritative_membership.is_some() {
            SourceSnapshotCompleteness::Authoritative
        } else {
            SourceSnapshotCompleteness::Partial
        }
    }

    pub fn authoritative_membership(&self) -> Option<&BTreeSet<ExternalGameId>> {
        self.authoritative_membership.as_ref()
    }

    pub fn is_authoritative(&self) -> bool {
        self.authoritative_membership.is_some()
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

    /// Prepare a source-neutral launch target for one source-owned game.
    ///
    /// The default keeps discovery-only adapters simple. Sources advertising
    /// `SourceCapability::Launch` should return a target for valid game IDs.
    fn launch_target(
        &self,
        _external_id: &ExternalGameId,
    ) -> Result<Option<SourceLaunchTarget>, SourceError> {
        Ok(None)
    }

    /// Prepare a direct host-side launch recipe that can be wrapped by a
    /// managed compositor session. The recipe is resolved again by the host
    /// helper; it is never sent across D-Bus as an arbitrary command.
    fn managed_launch_target(
        &self,
        _external_id: &ExternalGameId,
    ) -> Result<Option<SourceManagedLaunchTarget>, SourceError> {
        Ok(None)
    }

    /// Observe whether a source-owned game is currently running.
    ///
    /// Sources advertising `RuntimeObservation` must derive this from reliable
    /// provider-owned state behind the adapter boundary. Implementations used
    /// by the normal Flatpak must not depend on visibility into host `/proc`.
    fn runtime_state(
        &self,
        _external_id: &ExternalGameId,
    ) -> Result<Option<SourceRuntimeState>, SourceError> {
        Ok(None)
    }

    /// Observe a launch associated with one newly armed Horizon observation.
    ///
    /// Sources with non-authoritative, persisted runtime traces should ignore
    /// evidence predating this observation. The default retains existing
    /// semantics for sources with authoritative current-process state.
    fn runtime_state_for_observation(
        &self,
        external_id: &ExternalGameId,
        _armed_at: SystemTime,
    ) -> Result<Option<SourceRuntimeState>, SourceError> {
        self.runtime_state(external_id)
    }

    /// Read a provider-reported cumulative playtime snapshot. This is NOT a
    /// live session signal and MUST NOT be combined with Horizon-observed time.
    /// Adapters advertising LifetimePlaytime must normalize values to seconds.
    fn lifetime_playtime_snapshot(
        &self,
    ) -> Result<Vec<(ExternalGameId, PlaytimeSeconds)>, SourceError> {
        Ok(Vec::new())
    }

    /// An authoritative cross-catalog identifier, if this adapter knows one.
    /// A None result permits conservative title search instead.
    fn external_artwork_id(&self, _external_id: &ExternalGameId) -> Option<ExternalArtworkId> {
        None
    }

    /// Return provider-owned artwork candidates for one source-owned game.
    ///
    /// Sources only expose candidates and provenance. Selection, decoding, and
    /// 1:1 normalization are owned by the generic artwork service.
    fn artwork_candidates(
        &self,
        _external_id: &ExternalGameId,
    ) -> Result<Vec<SourceArtworkCandidate>, SourceError> {
        Ok(vec![])
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SourceRegistryError {
    #[error("source id {0} is already registered")]
    DuplicateSourceId(SourceId),
}

#[derive(Debug, Error)]
pub enum SourceRegistryBuildError {
    #[error(transparent)]
    Initialization(#[from] SourceInitializationError),
    #[error(transparent)]
    Registry(#[from] SourceRegistryError),
}

/// Build the production source registry used by every Horizon process.
///
/// Keeping this composition in the source layer prevents the application and
/// host helper from drifting to different provider sets/capabilities.
pub fn production_source_registry() -> Result<SourceRegistry, SourceRegistryBuildError> {
    let mut registry = SourceRegistry::new();
    registry.register(steam::SteamSource::new()?)?;
    registry.register(heroic::HeroicSource::new()?)?;
    Ok(registry)
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

    pub fn get(&self, source_id: &SourceId) -> Option<&dyn GameSource> {
        self.sources.get(source_id).map(Box::as_ref)
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
                descriptor: SourceDescriptor::new(source_id(id), id, vec![]).expect("descriptor"),
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
    fn production_registry_excludes_retired_lutris_source() {
        let registry = production_source_registry().expect("production registry");

        assert_eq!(registry.len(), 2);
        assert!(registry.get(&source_id("steam")).is_some());
        assert!(registry.get(&source_id("bottles")).is_none());
        assert!(registry.get(&source_id("heroic")).is_some());
        assert!(registry.get(&source_id("lutris")).is_none());
    }

    #[test]
    fn managed_launch_target_rejects_an_empty_program() {
        assert_eq!(
            SourceManagedLaunchTarget::new("   ", vec![]).expect_err("empty program must fail"),
            SourceManagedLaunchTargetError::EmptyProgram
        );
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
        assert!(!descriptor.supports(SourceCapability::ManagedSession));
        assert!(!descriptor.supports(SourceCapability::RuntimeObservation));
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
    fn snapshots_are_partial_by_default_and_must_opt_into_authoritative_membership() {
        let partial = SourceSnapshot::new(vec![source_game("1", "One")]).expect("partial");
        let authoritative =
            SourceSnapshot::authoritative(vec![source_game("1", "One")]).expect("authoritative");

        assert_eq!(partial.completeness(), SourceSnapshotCompleteness::Partial);
        assert!(!partial.is_authoritative());
        assert_eq!(
            authoritative.completeness(),
            SourceSnapshotCompleteness::Authoritative
        );
        assert!(authoritative.is_authoritative());

        let broader = SourceSnapshot::authoritative_with_membership(
            vec![source_game("1", "One")],
            vec![
                ExternalGameId::new("1").expect("one"),
                ExternalGameId::new("2").expect("two"),
            ],
        )
        .expect("broader membership");
        assert_eq!(
            broader
                .authoritative_membership()
                .expect("membership")
                .len(),
            2
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
        assert_eq!(
            registry
                .get(&source_id("alpha"))
                .expect("registered source")
                .descriptor()
                .id()
                .as_str(),
            "alpha"
        );
    }
}
