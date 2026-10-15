use std::{cell::RefCell, collections::BTreeSet, error::Error, fmt, rc::Rc};

use thiserror::Error;
use tracing::warn;

use crate::{
    domain::{GameId, LibraryGame, SourceId},
    services::runtime::{
        RuntimeObservationEvent, RuntimeObservationExecutor, RuntimeObservationId,
        RuntimeObservationStartOutcome, RuntimeObservationState, RuntimeObservationTerminalState,
    },
    sources::{SourceCapability, SourceLaunchTarget, SourceRegistry},
};

#[derive(Debug)]
pub struct LaunchExecutionError {
    source: Box<dyn Error + Send + Sync>,
}

impl LaunchExecutionError {
    pub fn new<E>(source: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        Self {
            source: Box::new(source),
        }
    }
}

impl fmt::Display for LaunchExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.source, formatter)
    }
}

impl Error for LaunchExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Platform boundary for executing a normal source-neutral launch target.
///
/// Services decide which registered source owns the launch. Platform adapters
/// decide how an embedded player or desktop session receives the target.
pub trait LaunchExecutor {
    fn execute(&self, target: &SourceLaunchTarget) -> Result<(), LaunchExecutionError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameLaunchMode {
    Embedded,
    External,
    Observed(RuntimeObservationId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameLaunchReceipt {
    source_id: SourceId,
    mode: GameLaunchMode,
}

impl GameLaunchReceipt {
    pub fn new(source_id: SourceId, mode: GameLaunchMode) -> Self {
        Self { source_id, mode }
    }

    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    pub const fn mode(&self) -> GameLaunchMode {
        self.mode
    }
}

#[derive(Debug, Error)]
pub enum GameLaunchError {
    #[error("game {0:?} has no registered source that can launch it")]
    NoLaunchSource(GameId),
    #[error("source {source_id} could not prepare the launch target: {source}")]
    Source {
        source_id: SourceId,
        #[source]
        source: crate::sources::SourceError,
    },
    #[error("source {source_id} launch dispatch failed: {source}")]
    Execution {
        source_id: SourceId,
        #[source]
        source: LaunchExecutionError,
    },
}

/// Selects the best available launch path for a library game.
pub struct GameLaunchService {
    registry: Rc<SourceRegistry>,
    executor: Rc<dyn LaunchExecutor>,
    runtime_observer: Option<Rc<dyn RuntimeObservationExecutor>>,
    active_runtime_observations: RefCell<BTreeSet<RuntimeObservationId>>,
    running_runtime_observations: RefCell<BTreeSet<RuntimeObservationId>>,
}

impl GameLaunchService {
    pub fn new(registry: Rc<SourceRegistry>, executor: Rc<dyn LaunchExecutor>) -> Self {
        Self {
            registry,
            executor,
            runtime_observer: None,
            active_runtime_observations: RefCell::new(BTreeSet::new()),
            running_runtime_observations: RefCell::new(BTreeSet::new()),
        }
    }

    pub fn with_runtime_observer(mut self, observer: Rc<dyn RuntimeObservationExecutor>) -> Self {
        self.runtime_observer = Some(observer);
        self
    }

    pub fn can_launch(&self, game: &LibraryGame) -> bool {
        game.sources().iter().any(|reference| {
            self.registry
                .get(reference.source_id())
                .is_some_and(|source| source.descriptor().supports(SourceCapability::Launch))
        })
    }

    pub fn launch_game(&self, game: &LibraryGame) -> Result<GameLaunchReceipt, GameLaunchError> {
        for source_ref in game.sources() {
            let Some(source) = self.registry.get(source_ref.source_id()) else {
                continue;
            };

            let source_id = source.descriptor().id().clone();
            if !source.descriptor().supports(SourceCapability::Launch) {
                continue;
            }

            let target = source
                .launch_target(source_ref.external_id())
                .map_err(|source| GameLaunchError::Source {
                    source_id: source_id.clone(),
                    source,
                })?;

            let Some(target) = target else {
                continue;
            };

            self.executor
                .execute(&target)
                .map_err(|source| GameLaunchError::Execution {
                    source_id: source_id.clone(),
                    source,
                })?;

            if matches!(target, SourceLaunchTarget::EmbeddedGba { .. }) {
                return Ok(GameLaunchReceipt::new(source_id, GameLaunchMode::Embedded));
            }

            if source
                .descriptor()
                .supports(SourceCapability::RuntimeObservation)
                && let Some(runtime_observer) = &self.runtime_observer
            {
                match runtime_observer.start_observation(&source_id, source_ref.external_id()) {
                    Ok(RuntimeObservationStartOutcome::Started(observation_id)) => {
                        self.active_runtime_observations
                            .borrow_mut()
                            .insert(observation_id);
                        return Ok(GameLaunchReceipt::new(
                            source_id,
                            GameLaunchMode::Observed(observation_id),
                        ));
                    }
                    Ok(RuntimeObservationStartOutcome::Unsupported)
                    | Ok(RuntimeObservationStartOutcome::Unavailable) => {}
                    Err(error) => {
                        warn!(
                            source = %source_id,
                            %error,
                            "source runtime observation could not start; using foreground handoff fallback"
                        );
                    }
                }
            }

            return Ok(GameLaunchReceipt::new(source_id, GameLaunchMode::External));
        }

        Err(GameLaunchError::NoLaunchSource(game.game().id()))
    }

    pub fn poll_runtime_observations(&self) -> Vec<RuntimeObservationEvent> {
        let Some(observer) = &self.runtime_observer else {
            return vec![];
        };

        let observation_ids = self
            .active_runtime_observations
            .borrow()
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let mut events = Vec::new();

        for observation_id in observation_ids {
            match observer.observation_state(observation_id) {
                Ok(RuntimeObservationState::Waiting) => {}
                Ok(RuntimeObservationState::Running { started_at }) => {
                    if self
                        .running_runtime_observations
                        .borrow_mut()
                        .insert(observation_id)
                    {
                        events.push(RuntimeObservationEvent::Started {
                            observation_id,
                            started_at,
                        });
                    }
                }
                Ok(RuntimeObservationState::Exited {
                    started_at,
                    ended_at,
                }) => {
                    if self
                        .running_runtime_observations
                        .borrow_mut()
                        .insert(observation_id)
                    {
                        events.push(RuntimeObservationEvent::Started {
                            observation_id,
                            started_at,
                        });
                    }
                    self.active_runtime_observations
                        .borrow_mut()
                        .remove(&observation_id);
                    self.running_runtime_observations
                        .borrow_mut()
                        .remove(&observation_id);
                    if let Err(error) = observer.forget_observation(observation_id) {
                        warn!(
                            observation_id = observation_id.get(),
                            %error,
                            "host helper could not forget terminal runtime observation"
                        );
                    }
                    events.push(RuntimeObservationEvent::Terminal {
                        observation_id,
                        terminal: RuntimeObservationTerminalState::Exited {
                            started_at,
                            ended_at,
                        },
                    });
                }
                Ok(RuntimeObservationState::Failed { message }) => {
                    self.active_runtime_observations
                        .borrow_mut()
                        .remove(&observation_id);
                    self.running_runtime_observations
                        .borrow_mut()
                        .remove(&observation_id);
                    if let Err(error) = observer.forget_observation(observation_id) {
                        warn!(
                            observation_id = observation_id.get(),
                            %error,
                            "host helper could not forget failed runtime observation"
                        );
                    }
                    events.push(RuntimeObservationEvent::Terminal {
                        observation_id,
                        terminal: RuntimeObservationTerminalState::Failed { message },
                    });
                }
                Ok(RuntimeObservationState::Unknown) => {
                    self.active_runtime_observations
                        .borrow_mut()
                        .remove(&observation_id);
                    self.running_runtime_observations
                        .borrow_mut()
                        .remove(&observation_id);
                    events.push(RuntimeObservationEvent::Terminal {
                        observation_id,
                        terminal: RuntimeObservationTerminalState::Lost,
                    });
                }
                Err(error) => {
                    warn!(
                        observation_id = observation_id.get(),
                        %error,
                        "runtime observation status could not be queried; retaining observation for retry"
                    );
                }
            }
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;
    use crate::{
        domain::{ExternalGameId, Game, GameTitle, SourceGameRef},
        services::runtime::{
            RuntimeObservationError, RuntimeObservationExecutor, RuntimeObservationStartOutcome,
            RuntimeObservationState,
        },
        sources::{
            GameSource, SourceDescriptor, SourceDiscovery, SourceError, SourceLaunchTarget,
            SourceSnapshot,
        },
    };

    struct LaunchableSource {
        descriptor: SourceDescriptor,
    }

    impl LaunchableSource {
        fn new(id: &str, observed: bool) -> Self {
            let mut capabilities = vec![SourceCapability::Launch];
            if observed {
                capabilities.push(SourceCapability::RuntimeObservation);
            }
            Self {
                descriptor: SourceDescriptor::new(
                    SourceId::new(id).expect("source id"),
                    id,
                    capabilities,
                )
                .expect("descriptor"),
            }
        }
    }

    impl GameSource for LaunchableSource {
        fn descriptor(&self) -> &SourceDescriptor {
            &self.descriptor
        }

        fn discover(&self) -> Result<SourceDiscovery, SourceError> {
            Ok(SourceDiscovery::Available(
                SourceSnapshot::new(vec![]).expect("snapshot"),
            ))
        }

        fn launch_target(
            &self,
            external_id: &ExternalGameId,
        ) -> Result<Option<SourceLaunchTarget>, SourceError> {
            Ok(Some(SourceLaunchTarget::Uri(format!(
                "test://{}",
                external_id.as_str()
            ))))
        }
    }

    #[derive(Default)]
    struct RecordingExecutor {
        targets: RefCell<Vec<SourceLaunchTarget>>,
    }

    impl LaunchExecutor for RecordingExecutor {
        fn execute(&self, target: &SourceLaunchTarget) -> Result<(), LaunchExecutionError> {
            self.targets.borrow_mut().push(target.clone());
            Ok(())
        }
    }

    struct FakeRuntimeObserver {
        start: RuntimeObservationStartOutcome,
    }

    impl RuntimeObservationExecutor for FakeRuntimeObserver {
        fn start_observation(
            &self,
            _source_id: &SourceId,
            _external_id: &ExternalGameId,
        ) -> Result<RuntimeObservationStartOutcome, RuntimeObservationError> {
            Ok(self.start.clone())
        }

        fn observation_state(
            &self,
            _observation_id: RuntimeObservationId,
        ) -> Result<RuntimeObservationState, RuntimeObservationError> {
            Ok(RuntimeObservationState::Waiting)
        }

        fn forget_observation(
            &self,
            _observation_id: RuntimeObservationId,
        ) -> Result<(), RuntimeObservationError> {
            Ok(())
        }
    }

    fn game(source: &str) -> LibraryGame {
        LibraryGame::new(
            Game::new(
                GameId::new(1).expect("id"),
                GameTitle::new("Game").expect("title"),
            ),
            vec![SourceGameRef::new(
                SourceId::new(source).expect("source"),
                ExternalGameId::new("42").expect("external id"),
            )],
        )
    }

    #[test]
    fn launch_uses_registered_source_capability_without_source_name_branching() {
        let mut registry = SourceRegistry::new();
        registry
            .register(LaunchableSource::new("provider", false))
            .expect("register");
        let registry = Rc::new(registry);
        let executor = Rc::new(RecordingExecutor::default());
        let service = GameLaunchService::new(registry, executor.clone());

        let receipt = service.launch_game(&game("provider")).expect("launch");
        assert_eq!(receipt.source_id().as_str(), "provider");
        assert_eq!(receipt.mode(), GameLaunchMode::External);
        assert_eq!(
            executor.targets.borrow().as_slice(),
            &[SourceLaunchTarget::Uri("test://42".into())]
        );
    }

    #[test]
    fn gba_dispatch_uses_embedded_content_identity_without_external_handoff() {
        let mut registry = SourceRegistry::new();
        registry
            .register(crate::sources::gba::GbaSource::new().unwrap())
            .unwrap();
        let executor = Rc::new(RecordingExecutor::default());
        let service = GameLaunchService::new(Rc::new(registry), executor.clone());
        let sha256 = "a".repeat(64);
        let game = LibraryGame::new(
            Game::new(GameId::new(1).unwrap(), GameTitle::new("GBA").unwrap()),
            vec![SourceGameRef::new(
                SourceId::new("gba").unwrap(),
                ExternalGameId::new(format!("sha256:{sha256}")).unwrap(),
            )],
        );
        assert_eq!(
            service.launch_game(&game).unwrap().mode(),
            GameLaunchMode::Embedded
        );
        assert_eq!(
            *executor.targets.borrow(),
            vec![SourceLaunchTarget::EmbeddedGba { sha256 }]
        );
    }

    #[test]
    fn runtime_observation_is_used_after_successful_external_dispatch() {
        let mut registry = SourceRegistry::new();
        registry
            .register(LaunchableSource::new("provider", true))
            .expect("register");
        let registry = Rc::new(registry);
        let external = Rc::new(RecordingExecutor::default());
        let observation_id = RuntimeObservationId::new(9).expect("observation id");
        let observer: Rc<dyn RuntimeObservationExecutor> = Rc::new(FakeRuntimeObserver {
            start: RuntimeObservationStartOutcome::Started(observation_id),
        });
        let service =
            GameLaunchService::new(registry, external.clone()).with_runtime_observer(observer);

        let receipt = service.launch_game(&game("provider")).expect("launch");
        assert_eq!(receipt.mode(), GameLaunchMode::Observed(observation_id));
        assert_eq!(external.targets.borrow().len(), 1);
    }
}
