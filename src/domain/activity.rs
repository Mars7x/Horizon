use thiserror::Error;

use super::{GameId, SourceId};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ActivityValidationError {
    #[error("session id must be positive, got {0}")]
    InvalidSessionId(i64),
    #[error("playtime seconds must not be negative, got {0}")]
    NegativePlaytime(i64),
    #[error("completed session end time {ended_at} precedes start time {started_at}")]
    EndBeforeStart { started_at: i64, ended_at: i64 },
    #[error("completed sessions require an end time")]
    CompletedSessionMissingEnd,
    #[error("open/interrupted sessions must not have an end time")]
    NonCompletedSessionHasEnd,
    #[error("unknown session tracking method: {0}")]
    UnknownTrackingMethod(String),
    #[error("unknown session state: {0}")]
    UnknownSessionState(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlaySessionId(i64);

impl PlaySessionId {
    pub fn new(value: i64) -> Result<Self, ActivityValidationError> {
        if value <= 0 {
            return Err(ActivityValidationError::InvalidSessionId(value));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlaytimeSeconds(i64);

impl PlaytimeSeconds {
    pub fn new(value: i64) -> Result<Self, ActivityValidationError> {
        if value < 0 {
            return Err(ActivityValidationError::NegativePlaytime(value));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

/// Describes how Horizon observed one play session.
///
/// `ForegroundHandoff` is intentionally approximate: Horizon starts timing only
/// after a dispatched launch causes Horizon to lose OS window activation, and
/// stops timing when Horizon becomes active again. It must never be presented
/// as exact child-process lifetime. `ManagedSession` follows the host-managed
/// Gamescope session lifecycle; it is stronger than foreground handoff but also
/// must not be described as exact game-process lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SessionTrackingMethod {
    ForegroundHandoff,
    ManagedSession,
}

impl SessionTrackingMethod {
    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::ForegroundHandoff => "foreground_handoff",
            Self::ManagedSession => "managed_session",
        }
    }

    pub fn from_storage_key(value: &str) -> Result<Self, ActivityValidationError> {
        match value {
            "foreground_handoff" => Ok(Self::ForegroundHandoff),
            "managed_session" => Ok(Self::ManagedSession),
            other => Err(ActivityValidationError::UnknownTrackingMethod(
                other.to_owned(),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlaySessionState {
    Open,
    Completed,
    Interrupted,
}

impl PlaySessionState {
    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Completed => "completed",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn from_storage_key(value: &str) -> Result<Self, ActivityValidationError> {
        match value {
            "open" => Ok(Self::Open),
            "completed" => Ok(Self::Completed),
            "interrupted" => Ok(Self::Interrupted),
            other => Err(ActivityValidationError::UnknownSessionState(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaySession {
    id: PlaySessionId,
    game_id: GameId,
    source_id: SourceId,
    started_at: i64,
    ended_at: Option<i64>,
    tracking_method: SessionTrackingMethod,
    state: PlaySessionState,
}

impl PlaySession {
    pub fn new(
        id: PlaySessionId,
        game_id: GameId,
        source_id: SourceId,
        started_at: i64,
        ended_at: Option<i64>,
        tracking_method: SessionTrackingMethod,
        state: PlaySessionState,
    ) -> Result<Self, ActivityValidationError> {
        match state {
            PlaySessionState::Completed => {
                let ended_at = ended_at.ok_or(ActivityValidationError::CompletedSessionMissingEnd)?;
                if ended_at < started_at {
                    return Err(ActivityValidationError::EndBeforeStart {
                        started_at,
                        ended_at,
                    });
                }
            }
            PlaySessionState::Open | PlaySessionState::Interrupted if ended_at.is_some() => {
                return Err(ActivityValidationError::NonCompletedSessionHasEnd);
            }
            PlaySessionState::Open | PlaySessionState::Interrupted => {}
        }

        Ok(Self {
            id,
            game_id,
            source_id,
            started_at,
            ended_at,
            tracking_method,
            state,
        })
    }

    pub const fn id(&self) -> PlaySessionId {
        self.id
    }

    pub const fn game_id(&self) -> GameId {
        self.game_id
    }

    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    pub const fn started_at(&self) -> i64 {
        self.started_at
    }

    pub const fn ended_at(&self) -> Option<i64> {
        self.ended_at
    }

    pub const fn tracking_method(&self) -> SessionTrackingMethod {
        self.tracking_method
    }

    pub const fn state(&self) -> PlaySessionState {
        self.state
    }

    pub fn duration(&self) -> Option<PlaytimeSeconds> {
        let ended_at = self.ended_at?;
        PlaytimeSeconds::new(ended_at.saturating_sub(self.started_at)).ok()
    }
}

/// A provider-reported lifetime value is deliberately separate from observed
/// Horizon sessions. Callers must not add this value to observed session totals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLifetimePlaytime {
    game_id: GameId,
    source_id: SourceId,
    lifetime: PlaytimeSeconds,
    observed_at: i64,
}

impl SourceLifetimePlaytime {
    pub fn new(
        game_id: GameId,
        source_id: SourceId,
        lifetime: PlaytimeSeconds,
        observed_at: i64,
    ) -> Self {
        Self {
            game_id,
            source_id,
            lifetime,
            observed_at,
        }
    }

    pub const fn game_id(&self) -> GameId {
        self.game_id
    }

    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    pub const fn lifetime(&self) -> PlaytimeSeconds {
        self.lifetime
    }

    pub const fn observed_at(&self) -> i64 {
        self.observed_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game_id() -> GameId {
        GameId::new(1).expect("game id")
    }

    fn source_id() -> SourceId {
        SourceId::new("test").expect("source id")
    }

    #[test]
    fn completed_session_has_non_negative_duration() {
        let session = PlaySession::new(
            PlaySessionId::new(1).expect("session id"),
            game_id(),
            source_id(),
            100,
            Some(160),
            SessionTrackingMethod::ForegroundHandoff,
            PlaySessionState::Completed,
        )
        .expect("session");

        assert_eq!(session.duration().expect("duration").get(), 60);
    }

    #[test]
    fn interrupted_session_keeps_duration_unknown() {
        let session = PlaySession::new(
            PlaySessionId::new(1).expect("session id"),
            game_id(),
            source_id(),
            100,
            None,
            SessionTrackingMethod::ForegroundHandoff,
            PlaySessionState::Interrupted,
        )
        .expect("session");

        assert_eq!(session.duration(), None);
    }

    #[test]
    fn completed_session_rejects_backwards_time() {
        assert!(matches!(
            PlaySession::new(
                PlaySessionId::new(1).expect("session id"),
                game_id(),
                source_id(),
                200,
                Some(100),
                SessionTrackingMethod::ForegroundHandoff,
                PlaySessionState::Completed,
            ),
            Err(ActivityValidationError::EndBeforeStart { .. })
        ));
    }

    #[test]
    fn source_lifetime_playtime_is_a_separate_value() {
        let report = SourceLifetimePlaytime::new(
            game_id(),
            source_id(),
            PlaytimeSeconds::new(3_600).expect("playtime"),
            123,
        );
        assert_eq!(report.lifetime().get(), 3_600);
    }
}
