use std::{error::Error, fmt};

use crate::domain::{ExternalGameId, SourceId};

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

/// Narrow platform boundary for observing a source-owned game's host lifecycle.
///
/// The sandbox sends only source/game identity. Provider-specific host process
/// knowledge remains inside the host helper and source adapter.
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
