use std::{error::Error, fmt, rc::Rc};

use thiserror::Error;

use crate::{
    domain::{GameId, LibraryGame, SourceId},
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

/// Platform boundary for executing a source-neutral launch target.
///
/// Services decide which registered source owns the launch. Platform adapters
/// decide how the target is handed to the desktop/session.
pub trait LaunchExecutor {
    fn execute(&self, target: &SourceLaunchTarget) -> Result<(), LaunchExecutionError>;
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

pub struct GameLaunchService {
    registry: Rc<SourceRegistry>,
    executor: Rc<dyn LaunchExecutor>,
}

impl GameLaunchService {
    pub fn new(registry: Rc<SourceRegistry>, executor: Rc<dyn LaunchExecutor>) -> Self {
        Self { registry, executor }
    }

    pub fn launch_game(&self, game: &LibraryGame) -> Result<SourceId, GameLaunchError> {
        for source_ref in game.sources() {
            let Some(source) = self.registry.get(source_ref.source_id()) else {
                continue;
            };
            if !source.descriptor().supports(SourceCapability::Launch) {
                continue;
            }

            let source_id = source.descriptor().id().clone();
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
            return Ok(source_id);
        }

        Err(GameLaunchError::NoLaunchSource(game.game().id()))
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;
    use crate::{
        domain::{ExternalGameId, Game, GameTitle, SourceGameRef},
        sources::{
            GameSource, SourceDescriptor, SourceDiscovery, SourceError, SourceLaunchTarget,
            SourceSnapshot,
        },
    };

    struct LaunchableSource {
        descriptor: SourceDescriptor,
    }

    impl LaunchableSource {
        fn new(id: &str) -> Self {
            Self {
                descriptor: SourceDescriptor::new(
                    SourceId::new(id).expect("source id"),
                    id,
                    vec![SourceCapability::Launch],
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

    #[test]
    fn launch_uses_registered_source_capability_without_source_name_branching() {
        let mut registry = SourceRegistry::new();
        registry
            .register(LaunchableSource::new("provider"))
            .expect("register");
        let registry = Rc::new(registry);
        let executor = Rc::new(RecordingExecutor::default());
        let service = GameLaunchService::new(registry, executor.clone());

        let game = LibraryGame::new(
            Game::new(
                GameId::new(1).expect("id"),
                GameTitle::new("Game").expect("title"),
            ),
            vec![SourceGameRef::new(
                SourceId::new("provider").expect("source"),
                ExternalGameId::new("42").expect("external id"),
            )],
        );

        let source_id = service.launch_game(&game).expect("launch");
        assert_eq!(source_id.as_str(), "provider");
        assert_eq!(
            executor.targets.borrow().as_slice(),
            &[SourceLaunchTarget::Uri("test://42".into())]
        );
    }
}
