use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    error::Error,
    fmt,
    rc::Rc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    domain::{ExternalGameId, SourceId},
    sources::{SourceCapability, SourceRegistry, SourceRuntimeState},
};

const RUNTIME_START_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeObservationId(u64);

impl RuntimeObservationId {
    pub fn new(value: u64) -> Option<Self> {
        (value > 0).then_some(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeObservationStartOutcome {
    Started(RuntimeObservationId),
    Unsupported,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeObservationState {
    Waiting,
    Running { started_at: i64 },
    Exited { started_at: i64, ended_at: i64 },
    Failed { message: String },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeObservationTerminalState {
    Exited { started_at: i64, ended_at: i64 },
    Failed { message: String },
    Lost,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeObservationEvent {
    Started {
        observation_id: RuntimeObservationId,
        started_at: i64,
    },
    Terminal {
        observation_id: RuntimeObservationId,
        terminal: RuntimeObservationTerminalState,
    },
}

#[derive(Debug)]
pub struct RuntimeObservationError {
    source: Box<dyn Error + Send + Sync>,
}

impl RuntimeObservationError {
    pub fn new<E>(source: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        Self {
            source: Box::new(source),
        }
    }

    pub fn message(message: impl Into<String>) -> Self {
        Self::new(std::io::Error::other(message.into()))
    }
}

impl fmt::Display for RuntimeObservationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.source, formatter)
    }
}

impl Error for RuntimeObservationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Narrow execution boundary for observing a source-owned game's lifecycle.
///
/// An implementation may use provider-owned state readable in the application
/// sandbox, or an optional host integration when a provider truly requires it.
/// Generic launch/activity code only ever supplies source/game identity.
pub trait RuntimeObservationExecutor {
    fn start_observation(
        &self,
        source_id: &SourceId,
        external_id: &ExternalGameId,
    ) -> Result<RuntimeObservationStartOutcome, RuntimeObservationError>;

    fn observation_state(
        &self,
        observation_id: RuntimeObservationId,
    ) -> Result<RuntimeObservationState, RuntimeObservationError>;

    fn forget_observation(
        &self,
        observation_id: RuntimeObservationId,
    ) -> Result<(), RuntimeObservationError>;
}

#[derive(Debug)]
struct SourceRuntimeObservation {
    source_id: SourceId,
    external_id: ExternalGameId,
    created_at: Instant,
    started_at: Option<i64>,
    ended_at: Option<i64>,
    failed: Option<String>,
}

impl SourceRuntimeObservation {
    fn new(source_id: SourceId, external_id: ExternalGameId) -> Self {
        Self {
            source_id,
            external_id,
            created_at: Instant::now(),
            started_at: None,
            ended_at: None,
            failed: None,
        }
    }

    fn terminal(&self) -> bool {
        self.ended_at.is_some() || self.failed.is_some()
    }

    fn refresh(&mut self, registry: &SourceRegistry) {
        if self.terminal() {
            return;
        }

        let Some(source) = registry.get(&self.source_id) else {
            self.failed = Some(format!("source {} is no longer registered", self.source_id));
            return;
        };
        if !source
            .descriptor()
            .supports(SourceCapability::RuntimeObservation)
        {
            self.failed = Some(format!(
                "source {} does not support runtime observation",
                self.source_id
            ));
            return;
        }

        match source.runtime_state(&self.external_id) {
            Ok(Some(SourceRuntimeState::Running)) => {
                if self.started_at.is_none() {
                    self.started_at = Some(unix_timestamp());
                }
            }
            Ok(Some(SourceRuntimeState::Stopped)) => {
                if self.started_at.is_some() {
                    self.ended_at = Some(unix_timestamp());
                } else if self.created_at.elapsed() >= RUNTIME_START_TIMEOUT {
                    self.failed = Some(format!(
                        "source {} game {} did not enter a running state within {} seconds",
                        self.source_id,
                        self.external_id,
                        RUNTIME_START_TIMEOUT.as_secs()
                    ));
                }
            }
            Ok(None) => {
                self.failed = Some(format!(
                    "source {} did not provide runtime state for game {}",
                    self.source_id, self.external_id
                ));
            }
            Err(error) => {
                self.failed = Some(format!(
                    "source {} runtime observation failed: {error}",
                    self.source_id
                ));
            }
        }
    }
}

/// In-process source-runtime observer used by the ordinary Horizon Flatpak.
///
/// This deliberately does not inspect host `/proc`. Providers may advertise
/// RuntimeObservation only when they can expose trustworthy provider-owned
/// state through resources Horizon is already permitted to read. Steam, for
/// example, reads `logs/gameprocess_log.txt` from its own data directory.
pub struct SourceRuntimeObservationExecutor {
    registry: Rc<SourceRegistry>,
    next_observation_id: Cell<u64>,
    observations: RefCell<BTreeMap<RuntimeObservationId, SourceRuntimeObservation>>,
}

impl SourceRuntimeObservationExecutor {
    pub fn new(registry: Rc<SourceRegistry>) -> Self {
        Self {
            registry,
            next_observation_id: Cell::new(1),
            observations: RefCell::new(BTreeMap::new()),
        }
    }

    fn next_id(&self) -> RuntimeObservationId {
        let raw = self.next_observation_id.get().max(1);
        self.next_observation_id.set(raw.saturating_add(1));
        RuntimeObservationId::new(raw).expect("runtime observation ids start at one")
    }
}

impl RuntimeObservationExecutor for SourceRuntimeObservationExecutor {
    fn start_observation(
        &self,
        source_id: &SourceId,
        external_id: &ExternalGameId,
    ) -> Result<RuntimeObservationStartOutcome, RuntimeObservationError> {
        let Some(source) = self.registry.get(source_id) else {
            return Ok(RuntimeObservationStartOutcome::Unsupported);
        };
        if !source
            .descriptor()
            .supports(SourceCapability::RuntimeObservation)
        {
            return Ok(RuntimeObservationStartOutcome::Unsupported);
        }

        match source.runtime_state(external_id) {
            Ok(Some(_)) => {}
            Ok(None) => return Ok(RuntimeObservationStartOutcome::Unavailable),
            Err(error) => return Err(RuntimeObservationError::message(error.to_string())),
        }

        let observation_id = self.next_id();
        self.observations.borrow_mut().insert(
            observation_id,
            SourceRuntimeObservation::new(source_id.clone(), external_id.clone()),
        );
        Ok(RuntimeObservationStartOutcome::Started(observation_id))
    }

    fn observation_state(
        &self,
        observation_id: RuntimeObservationId,
    ) -> Result<RuntimeObservationState, RuntimeObservationError> {
        let mut observations = self.observations.borrow_mut();
        let Some(observation) = observations.get_mut(&observation_id) else {
            return Ok(RuntimeObservationState::Unknown);
        };
        observation.refresh(&self.registry);

        if let Some(message) = &observation.failed {
            return Ok(RuntimeObservationState::Failed {
                message: message.clone(),
            });
        }
        if let Some(ended_at) = observation.ended_at {
            return Ok(RuntimeObservationState::Exited {
                started_at: observation.started_at.unwrap_or(ended_at),
                ended_at,
            });
        }
        if let Some(started_at) = observation.started_at {
            return Ok(RuntimeObservationState::Running { started_at });
        }
        Ok(RuntimeObservationState::Waiting)
    }

    fn forget_observation(
        &self,
        observation_id: RuntimeObservationId,
    ) -> Result<(), RuntimeObservationError> {
        let mut observations = self.observations.borrow_mut();
        let Some(observation) = observations.get_mut(&observation_id) else {
            return Ok(());
        };
        observation.refresh(&self.registry);
        if !observation.terminal() {
            return Err(RuntimeObservationError::message(
                "cannot forget a runtime observation before it is terminal",
            ));
        }
        observations.remove(&observation_id);
        Ok(())
    }
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}
