use std::error::Error;

use thiserror::Error;

use crate::{
    domain::SourceId,
    services::library::{DiscoveredGame, LibraryRepository, LibraryService},
    sources::{
        SourceDiscovery, SourceError, SourceRegistry, SourceUnavailableReason,
    },
};

#[derive(Debug)]
pub struct SourceImportReport {
    source_id: SourceId,
    outcome: SourceImportOutcome,
}

impl SourceImportReport {
    fn new(source_id: SourceId, outcome: SourceImportOutcome) -> Self {
        Self { source_id, outcome }
    }

    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    pub fn outcome(&self) -> &SourceImportOutcome {
        &self.outcome
    }
}

#[derive(Debug)]
pub enum SourceImportOutcome {
    Imported { discovered: usize },
    Unavailable(SourceUnavailableReason),
    Failed(SourceError),
}

#[derive(Debug, Default)]
pub struct SourceImportSummary {
    reports: Vec<SourceImportReport>,
}

impl SourceImportSummary {
    pub fn reports(&self) -> &[SourceImportReport] {
        &self.reports
    }

    pub fn imported_source_count(&self) -> usize {
        self.reports
            .iter()
            .filter(|report| matches!(report.outcome(), SourceImportOutcome::Imported { .. }))
            .count()
    }

    pub fn unavailable_source_count(&self) -> usize {
        self.reports
            .iter()
            .filter(|report| matches!(report.outcome(), SourceImportOutcome::Unavailable(_)))
            .count()
    }

    pub fn failed_source_count(&self) -> usize {
        self.reports
            .iter()
            .filter(|report| matches!(report.outcome(), SourceImportOutcome::Failed(_)))
            .count()
    }
}

#[derive(Debug, Error)]
pub enum SourceImportError<E>
where
    E: Error + 'static,
{
    #[error("failed to persist discovery snapshot from source {source_id}: {source}")]
    Persistence {
        source_id: SourceId,
        #[source]
        source: E,
    },
}

/// Coordinates source discovery without knowing provider-specific formats.
///
/// An unavailable optional source and a failed source are both isolated to a
/// per-source report. Persistence failure stops the pass because continuing
/// after a database write failure could leave caller assumptions invalid.
pub struct SourceImportService;

impl SourceImportService {
    pub fn import_all<R>(
        registry: &SourceRegistry,
        library: &mut LibraryService<R>,
    ) -> Result<SourceImportSummary, SourceImportError<R::Error>>
    where
        R: LibraryRepository,
    {
        let mut summary = SourceImportSummary::default();

        for source in registry.iter() {
            let source_id = source.descriptor().id().clone();
            let outcome = match source.discover() {
                Err(error) => SourceImportOutcome::Failed(error),
                Ok(SourceDiscovery::Unavailable(reason)) => {
                    SourceImportOutcome::Unavailable(reason)
                }
                Ok(SourceDiscovery::Available(snapshot)) => {
                    let discovered_games = snapshot
                        .games()
                        .iter()
                        .map(|game| {
                            DiscoveredGame::new(
                                source_id.clone(),
                                game.external_id().clone(),
                                game.title().clone(),
                            )
                        })
                        .collect::<Vec<_>>();
                    let discovered = discovered_games.len();

                    let persistence_result = if let Some(membership) =
                        snapshot.authoritative_membership()
                    {
                        let present_external_ids = membership.iter().cloned().collect::<Vec<_>>();
                        library.synchronize_source_snapshot(
                            &source_id,
                            &discovered_games,
                            &present_external_ids,
                        )
                    } else {
                        library.record_discovered_games(&discovered_games)
                    };

                    persistence_result.map_err(|source| SourceImportError::Persistence {
                        source_id: source_id.clone(),
                        source,
                    })?;

                    SourceImportOutcome::Imported { discovered }
                }
            };

            summary
                .reports
                .push(SourceImportReport::new(source_id, outcome));
        }

        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{ExternalGameId, GameId, GameTitle, LibraryGame},
        sources::{
            GameSource, SourceDescriptor, SourceGame, SourceSnapshot,
        },
    };

    #[derive(Debug, Error)]
    #[error("fake discovery failure")]
    struct FakeDiscoveryError;

    #[derive(Debug, Error)]
    #[error("fake repository failure")]
    struct FakeRepositoryError;

    enum FakeBehavior {
        Available(Vec<SourceGame>),
        Authoritative(Vec<SourceGame>),
        Unavailable(SourceUnavailableReason),
        Failed,
    }

    struct FakeSource {
        descriptor: SourceDescriptor,
        behavior: FakeBehavior,
    }

    impl FakeSource {
        fn new(id: &str, behavior: FakeBehavior) -> Self {
            Self {
                descriptor: SourceDescriptor::new(
                    SourceId::new(id).expect("source id"),
                    id,
                    vec![],
                )
                .expect("descriptor"),
                behavior,
            }
        }
    }

    impl GameSource for FakeSource {
        fn descriptor(&self) -> &SourceDescriptor {
            &self.descriptor
        }

        fn discover(&self) -> Result<SourceDiscovery, SourceError> {
            match &self.behavior {
                FakeBehavior::Available(games) => Ok(SourceDiscovery::Available(
                    SourceSnapshot::new(games.clone()).expect("fake snapshot"),
                )),
                FakeBehavior::Authoritative(games) => Ok(SourceDiscovery::Available(
                    SourceSnapshot::authoritative(games.clone()).expect("fake snapshot"),
                )),
                FakeBehavior::Unavailable(reason) => Ok(SourceDiscovery::Unavailable(*reason)),
                FakeBehavior::Failed => Err(SourceError::new(FakeDiscoveryError)),
            }
        }
    }

    #[derive(Default)]
    struct RecordingRepository {
        discovered: Vec<DiscoveredGame>,
        synchronized_sources: Vec<SourceId>,
        fail_writes: bool,
    }

    impl LibraryRepository for RecordingRepository {
        type Error = FakeRepositoryError;

        fn upsert_discovered_game(
            &mut self,
            game: &DiscoveredGame,
        ) -> Result<GameId, Self::Error> {
            if self.fail_writes {
                return Err(FakeRepositoryError);
            }
            self.discovered.push(game.clone());
            GameId::new(self.discovered.len() as i64).map_err(|_| FakeRepositoryError)
        }

        fn upsert_discovered_games(
            &mut self,
            games: &[DiscoveredGame],
        ) -> Result<Vec<GameId>, Self::Error> {
            if self.fail_writes {
                return Err(FakeRepositoryError);
            }

            let start = self.discovered.len();
            self.discovered.extend_from_slice(games);
            (0..games.len())
                .map(|offset| {
                    GameId::new((start + offset + 1) as i64)
                        .map_err(|_| FakeRepositoryError)
                })
                .collect()
        }

        fn synchronize_source_snapshot(
            &mut self,
            source_id: &SourceId,
            games: &[DiscoveredGame],
            present_external_ids: &[ExternalGameId],
        ) -> Result<Vec<GameId>, Self::Error> {
            if self.fail_writes {
                return Err(FakeRepositoryError);
            }

            self.synchronized_sources.push(source_id.clone());
            self.discovered.retain(|game| {
                game.source_id() != source_id
                    || present_external_ids.contains(game.external_id())
            });
            let start = self.discovered.len();
            self.discovered.extend_from_slice(games);
            (0..games.len())
                .map(|offset| {
                    GameId::new((start + offset + 1) as i64)
                        .map_err(|_| FakeRepositoryError)
                })
                .collect()
        }

        fn list_games(&self) -> Result<Vec<LibraryGame>, Self::Error> {
            Ok(vec![])
        }

        fn game_count(&self) -> Result<usize, Self::Error> {
            Ok(self.discovered.len())
        }
    }

    fn game(external_id: &str, title: &str) -> SourceGame {
        SourceGame::new(
            ExternalGameId::new(external_id).expect("external id"),
            GameTitle::new(title).expect("title"),
        )
    }

    #[test]
    fn imports_normalized_games_with_the_owning_source_id() {
        let mut registry = SourceRegistry::new();
        registry
            .register(FakeSource::new(
                "alpha",
                FakeBehavior::Available(vec![game("10", "First"), game("20", "Second")]),
            ))
            .expect("register source");

        let repository = RecordingRepository::default();
        let mut library = LibraryService::new(repository);
        let summary = SourceImportService::import_all(&registry, &mut library).expect("import");

        assert_eq!(summary.imported_source_count(), 1);
        assert_eq!(summary.failed_source_count(), 0);
        let repository = library.into_repository();
        assert_eq!(repository.discovered.len(), 2);
        assert!(
            repository
                .discovered
                .iter()
                .all(|game| game.source_id().as_str() == "alpha")
        );
    }

    #[test]
    fn authoritative_snapshots_use_source_synchronization_instead_of_additive_upsert() {
        let mut registry = SourceRegistry::new();
        registry
            .register(FakeSource::new(
                "alpha",
                FakeBehavior::Authoritative(vec![game("10", "Installed")]),
            ))
            .expect("register source");

        let repository = RecordingRepository::default();
        let mut library = LibraryService::new(repository);
        SourceImportService::import_all(&registry, &mut library).expect("import");

        let repository = library.into_repository();
        assert_eq!(repository.synchronized_sources, vec![SourceId::new("alpha").expect("id")]);
        assert_eq!(repository.discovered.len(), 1);
        assert_eq!(repository.discovered[0].external_id().as_str(), "10");
    }

    #[test]
    fn unavailable_and_failed_sources_do_not_block_later_sources() {
        let mut registry = SourceRegistry::new();
        registry
            .register(FakeSource::new("alpha", FakeBehavior::Failed))
            .expect("alpha");
        registry
            .register(FakeSource::new(
                "beta",
                FakeBehavior::Unavailable(SourceUnavailableReason::NotInstalled),
            ))
            .expect("beta");
        registry
            .register(FakeSource::new(
                "gamma",
                FakeBehavior::Available(vec![game("30", "Working")]),
            ))
            .expect("gamma");

        let repository = RecordingRepository::default();
        let mut library = LibraryService::new(repository);
        let summary = SourceImportService::import_all(&registry, &mut library).expect("import");

        assert_eq!(summary.failed_source_count(), 1);
        assert_eq!(summary.unavailable_source_count(), 1);
        assert_eq!(summary.imported_source_count(), 1);
        assert_eq!(library.game_count().expect("game count"), 1);
    }

    #[test]
    fn persistence_failure_stops_the_pass_and_reports_the_source() {
        let mut registry = SourceRegistry::new();
        registry
            .register(FakeSource::new(
                "alpha",
                FakeBehavior::Available(vec![game("10", "First")]),
            ))
            .expect("alpha");

        let repository = RecordingRepository {
            fail_writes: true,
            ..RecordingRepository::default()
        };
        let mut library = LibraryService::new(repository);
        let error = SourceImportService::import_all(&registry, &mut library)
            .expect_err("persistence failure must abort import");

        match error {
            SourceImportError::Persistence { source_id, .. } => {
                assert_eq!(source_id.as_str(), "alpha");
            }
        }
    }
}
