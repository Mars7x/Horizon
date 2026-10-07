use std::{error::Error, fmt};

use crate::domain::{ExternalGameId, SourceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ManagedSessionId(u64);

impl ManagedSessionId {
    pub fn new(value: u64) -> Option<Self> {
        (value > 0).then_some(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedStartOutcome {
    Started(ManagedSessionId),
    Unsupported,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedSessionState {
    Running,
    Exited { exit_code: Option<i32> },
    Failed { message: String },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedSessionTerminalState {
    Exited { exit_code: Option<i32> },
    Failed { message: String },
    Lost,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedSessionCompletion {
    session_id: ManagedSessionId,
    terminal: ManagedSessionTerminalState,
}

impl ManagedSessionCompletion {
    pub fn new(session_id: ManagedSessionId, terminal: ManagedSessionTerminalState) -> Self {
        Self {
            session_id,
            terminal,
        }
    }

    pub const fn session_id(&self) -> ManagedSessionId {
        self.session_id
    }

    pub fn terminal(&self) -> &ManagedSessionTerminalState {
        &self.terminal
    }
}

#[derive(Debug)]
pub struct ManagedSessionExecutionError {
    source: Box<dyn Error + Send + Sync>,
}

impl ManagedSessionExecutionError {
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

impl fmt::Display for ManagedSessionExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.source, formatter)
    }
}

impl Error for ManagedSessionExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Platform boundary for starting and observing host-managed game sessions.
///
/// Implementations must not expose arbitrary host command execution to the
/// sandbox. Horizon identifies a registered source/game; the host helper
/// independently resolves the corresponding managed launch recipe.
pub trait ManagedSessionExecutor {
    fn start_session(
        &self,
        source_id: &SourceId,
        external_id: &ExternalGameId,
    ) -> Result<ManagedStartOutcome, ManagedSessionExecutionError>;

    fn session_state(
        &self,
        session_id: ManagedSessionId,
    ) -> Result<ManagedSessionState, ManagedSessionExecutionError>;

    fn stop_session(
        &self,
        session_id: ManagedSessionId,
    ) -> Result<(), ManagedSessionExecutionError>;

    fn forget_session(
        &self,
        session_id: ManagedSessionId,
    ) -> Result<(), ManagedSessionExecutionError>;
}
