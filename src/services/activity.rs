use std::{cell::RefCell, collections::BTreeMap, error::Error, rc::Rc};

use chrono::Utc;
use tracing::warn;

use crate::{
    domain::{
        GameId, GameTitle, PlaySession, PlaySessionId, PlaytimeSeconds, SessionTrackingMethod,
        SourceId, SourceLifetimePlaytime,
    },
    services::{
        runtime::{
            RuntimeObservationEvent, RuntimeObservationId, RuntimeObservationTerminalState,
        },
        session::{ManagedSessionId, ManagedSessionTerminalState},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentActivitySession {
    session: PlaySession,
    title: GameTitle,
}

impl RecentActivitySession {
    pub fn new(session: PlaySession, title: GameTitle) -> Self {
        Self { session, title }
    }

    pub fn session(&self) -> &PlaySession {
        &self.session
    }

    pub fn title(&self) -> &GameTitle {
        &self.title
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameActivitySummary {
    game_id: GameId,
    title: GameTitle,
    observed_playtime: PlaytimeSeconds,
    completed_sessions: usize,
    last_played_at: i64,
}

impl GameActivitySummary {
    pub fn new(
        game_id: GameId,
        title: GameTitle,
        observed_playtime: PlaytimeSeconds,
        completed_sessions: usize,
        last_played_at: i64,
    ) -> Self {
        Self {
            game_id,
            title,
            observed_playtime,
            completed_sessions,
            last_played_at,
        }
    }

    pub const fn game_id(&self) -> GameId {
        self.game_id
    }

    pub fn title(&self) -> &GameTitle {
        &self.title
    }

    pub const fn observed_playtime(&self) -> PlaytimeSeconds {
        self.observed_playtime
    }

    pub const fn completed_sessions(&self) -> usize {
        self.completed_sessions
    }

    pub const fn last_played_at(&self) -> i64 {
        self.last_played_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityOverview {
    observed_playtime: PlaytimeSeconds,
    completed_sessions: usize,
    played_games: usize,
    recent_sessions: Vec<RecentActivitySession>,
    top_games: Vec<GameActivitySummary>,
}

impl ActivityOverview {
    pub fn new(
        observed_playtime: PlaytimeSeconds,
        completed_sessions: usize,
        played_games: usize,
        recent_sessions: Vec<RecentActivitySession>,
        top_games: Vec<GameActivitySummary>,
    ) -> Self {
        Self {
            observed_playtime,
            completed_sessions,
            played_games,
            recent_sessions,
            top_games,
        }
    }

    pub const fn observed_playtime(&self) -> PlaytimeSeconds {
        self.observed_playtime
    }

    pub const fn completed_sessions(&self) -> usize {
        self.completed_sessions
    }

    pub const fn played_games(&self) -> usize {
        self.played_games
    }

    pub fn recent_sessions(&self) -> &[RecentActivitySession] {
        &self.recent_sessions
    }

    pub fn top_games(&self) -> &[GameActivitySummary] {
        &self.top_games
    }
}

pub trait ActivityRepository {
    type Error: Error + 'static;

    fn begin_play_session(
        &mut self,
        game_id: GameId,
        source_id: &SourceId,
        started_at: i64,
        tracking_method: SessionTrackingMethod,
    ) -> Result<PlaySessionId, Self::Error>;

    fn complete_play_session(
        &mut self,
        session_id: PlaySessionId,
        ended_at: i64,
    ) -> Result<(), Self::Error>;

    fn interrupt_play_session(
        &mut self,
        session_id: PlaySessionId,
    ) -> Result<(), Self::Error>;

    fn interrupt_open_play_sessions(&mut self) -> Result<usize, Self::Error>;

    fn activity_overview(
        &self,
        recent_limit: usize,
        top_games_limit: usize,
    ) -> Result<ActivityOverview, Self::Error>;

    fn upsert_source_lifetime_playtime(
        &mut self,
        report: &SourceLifetimePlaytime,
    ) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingLaunch {
    game_id: GameId,
    source_id: SourceId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeActivity {
    game_id: GameId,
    source_id: SourceId,
    play_session_id: Option<PlaySessionId>,
}

const FOREGROUND_LAUNCH_STABILIZATION_MS: i64 = 30_000;
const FOREGROUND_STARTUP_RETURN_GRACE_MS: i64 = 10_000;
const FOREGROUND_RETURN_GRACE_MS: i64 = 1_500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ForegroundSession {
    id: PlaySessionId,
    started_at_millis: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ForegroundReturnCandidate {
    session_id: PlaySessionId,
    returned_at_millis: i64,
    confirm_after_millis: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivitySessionTransition {
    None,
    Started(PlaySessionId),
    Completed(PlaySessionId),
    Interrupted(PlaySessionId),
}

impl ActivitySessionTransition {
    pub const fn changed(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Small source-neutral hook used by Home after a launch path is selected.
///
/// Home does not know how activity persistence works. The sink records which
/// observation model owns the launch: foreground handoff, source runtime, or a
/// managed session. Provider-specific lifecycle details never enter Home.
pub trait LaunchActivitySink {
    fn launch_dispatched(&self, game_id: GameId, source_id: SourceId);
    fn runtime_observation_armed(
        &self,
        observation_id: RuntimeObservationId,
        game_id: GameId,
        source_id: SourceId,
    );
    fn managed_session_started(
        &self,
        managed_session_id: ManagedSessionId,
        game_id: GameId,
        source_id: SourceId,
    );
    fn cancel_pending_launch(&self);
}

/// Coordinates Horizon-observed play sessions.
///
/// A URI/portal launch does not provide exact process lifetime. Horizon therefore
/// records the explicitly labeled `ForegroundHandoff` method: timing begins when
/// a pending launch causes the Horizon window to become inactive. A later
/// activation is treated as a candidate return rather than an immediate end so
/// launcher handoff focus bounces cannot truncate the session. If Horizon loses
/// activation again before the candidate is confirmed, the same session remains
/// open. When a source provides a host runtime observer, that stronger signal
/// owns start/end timing instead and window focus is ignored for that launch.
/// Source-reported lifetime values are persisted through a separate method and
/// are never added to these observed totals.
pub struct ActivityService<R> {
    repository: Rc<RefCell<R>>,
    pending_launch: RefCell<Option<PendingLaunch>>,
    active_session: RefCell<Option<ForegroundSession>>,
    foreground_return: RefCell<Option<ForegroundReturnCandidate>>,
    runtime_observations: RefCell<BTreeMap<RuntimeObservationId, RuntimeActivity>>,
    managed_sessions: RefCell<BTreeMap<ManagedSessionId, PlaySessionId>>,
}

impl<R> ActivityService<R>
where
    R: ActivityRepository,
{
    pub fn new(repository: Rc<RefCell<R>>) -> Self {
        Self {
            repository,
            pending_launch: RefCell::new(None),
            active_session: RefCell::new(None),
            foreground_return: RefCell::new(None),
            runtime_observations: RefCell::new(BTreeMap::new()),
            managed_sessions: RefCell::new(BTreeMap::new()),
        }
    }

    pub fn recover_interrupted_sessions(&self) -> Result<usize, R::Error> {
        self.pending_launch.borrow_mut().take();
        self.active_session.borrow_mut().take();
        self.foreground_return.borrow_mut().take();
        self.runtime_observations.borrow_mut().clear();
        self.managed_sessions.borrow_mut().clear();
        self.repository.borrow_mut().interrupt_open_play_sessions()
    }

    pub fn handle_application_active_changed(
        &self,
        active: bool,
    ) -> Result<ActivitySessionTransition, R::Error> {
        self.handle_application_active_changed_at_millis(active, Utc::now().timestamp_millis())
    }

    fn handle_application_active_changed_at_millis(
        &self,
        active: bool,
        now_millis: i64,
    ) -> Result<ActivitySessionTransition, R::Error> {
        if active {
            let Some(session) = *self.active_session.borrow() else {
                return Ok(ActivitySessionTransition::None);
            };

            // Do not end a foreground-handoff session on the first activation.
            // URI launchers can briefly return focus to Horizon between their
            // own handoff and the real game window. Arm a candidate return and
            // let the periodic confirmation path decide whether focus stayed.
            if self.foreground_return.borrow().is_none() {
                let startup_window_end = session
                    .started_at_millis
                    .saturating_add(FOREGROUND_LAUNCH_STABILIZATION_MS);
                let elapsed = now_millis.saturating_sub(session.started_at_millis);
                let grace = if elapsed < FOREGROUND_LAUNCH_STABILIZATION_MS {
                    FOREGROUND_STARTUP_RETURN_GRACE_MS
                } else {
                    FOREGROUND_RETURN_GRACE_MS
                };
                let confirm_after_millis = now_millis
                    .saturating_add(grace)
                    .max(startup_window_end);

                self.foreground_return
                    .borrow_mut()
                    .replace(ForegroundReturnCandidate {
                        session_id: session.id,
                        returned_at_millis: now_millis,
                        confirm_after_millis,
                    });
            }

            return Ok(ActivitySessionTransition::None);
        }

        // A renewed deactivation before the return candidate matures means the
        // game/launcher took the foreground again. Keep the existing session
        // open instead of recording a short false session.
        self.foreground_return.borrow_mut().take();
        if self.active_session.borrow().is_some() {
            return Ok(ActivitySessionTransition::None);
        }

        let Some(pending) = self.pending_launch.borrow_mut().take() else {
            return Ok(ActivitySessionTransition::None);
        };

        let started_at = now_millis.div_euclid(1_000);
        let session_id = self.repository.borrow_mut().begin_play_session(
            pending.game_id,
            &pending.source_id,
            started_at,
            SessionTrackingMethod::ForegroundHandoff,
        )?;
        self.active_session.borrow_mut().replace(ForegroundSession {
            id: session_id,
            started_at_millis: now_millis,
        });
        Ok(ActivitySessionTransition::Started(session_id))
    }

    /// Confirm a candidate return after it has remained stable long enough to
    /// distinguish a real return to Horizon from launcher startup focus bounce.
    /// The persisted end timestamp is the original return instant, not the end
    /// of the grace period, so the debounce does not inflate observed playtime.
    pub fn poll_foreground_return(&self) -> Result<ActivitySessionTransition, R::Error> {
        self.poll_foreground_return_at_millis(Utc::now().timestamp_millis())
    }

    fn poll_foreground_return_at_millis(
        &self,
        now_millis: i64,
    ) -> Result<ActivitySessionTransition, R::Error> {
        let Some(candidate) = *self.foreground_return.borrow() else {
            return Ok(ActivitySessionTransition::None);
        };
        if now_millis < candidate.confirm_after_millis {
            return Ok(ActivitySessionTransition::None);
        }

        let Some(session) = *self.active_session.borrow() else {
            self.foreground_return.borrow_mut().take();
            return Ok(ActivitySessionTransition::None);
        };
        if session.id != candidate.session_id {
            self.foreground_return.borrow_mut().take();
            return Ok(ActivitySessionTransition::None);
        }

        self.foreground_return.borrow_mut().take();
        self.active_session.borrow_mut().take();
        let ended_at = candidate.returned_at_millis.div_euclid(1_000);

        if let Err(error) = self
            .repository
            .borrow_mut()
            .complete_play_session(session.id, ended_at)
        {
            self.active_session.borrow_mut().replace(session);
            self.foreground_return.borrow_mut().replace(candidate);
            return Err(error);
        }

        Ok(ActivitySessionTransition::Completed(session.id))
    }

    pub fn handle_runtime_observation_event(
        &self,
        event: &RuntimeObservationEvent,
    ) -> Result<ActivitySessionTransition, R::Error> {
        match event {
            RuntimeObservationEvent::Started {
                observation_id,
                started_at,
            } => self.begin_runtime_session(*observation_id, *started_at),
            RuntimeObservationEvent::Terminal {
                observation_id,
                terminal,
            } => self.finish_runtime_session(*observation_id, terminal),
        }
    }

    fn begin_runtime_session(
        &self,
        observation_id: RuntimeObservationId,
        started_at: i64,
    ) -> Result<ActivitySessionTransition, R::Error> {
        let Some(activity) = self
            .runtime_observations
            .borrow()
            .get(&observation_id)
            .cloned()
        else {
            return Ok(ActivitySessionTransition::None);
        };
        if activity.play_session_id.is_some() {
            return Ok(ActivitySessionTransition::None);
        }

        let play_session_id = self.repository.borrow_mut().begin_play_session(
            activity.game_id,
            &activity.source_id,
            started_at,
            SessionTrackingMethod::SourceRuntime,
        )?;
        if let Some(activity) = self
            .runtime_observations
            .borrow_mut()
            .get_mut(&observation_id)
        {
            activity.play_session_id = Some(play_session_id);
        }
        Ok(ActivitySessionTransition::Started(play_session_id))
    }

    fn finish_runtime_session(
        &self,
        observation_id: RuntimeObservationId,
        terminal: &RuntimeObservationTerminalState,
    ) -> Result<ActivitySessionTransition, R::Error> {
        let Some(mut activity) = self
            .runtime_observations
            .borrow_mut()
            .remove(&observation_id)
        else {
            return Ok(ActivitySessionTransition::None);
        };

        if activity.play_session_id.is_none()
            && let RuntimeObservationTerminalState::Exited { started_at, .. } = terminal
        {
            match self.repository.borrow_mut().begin_play_session(
                activity.game_id,
                &activity.source_id,
                *started_at,
                SessionTrackingMethod::SourceRuntime,
            ) {
                Ok(play_session_id) => activity.play_session_id = Some(play_session_id),
                Err(error) => {
                    self.runtime_observations
                        .borrow_mut()
                        .insert(observation_id, activity);
                    return Err(error);
                }
            }
        }

        let Some(play_session_id) = activity.play_session_id else {
            return Ok(ActivitySessionTransition::None);
        };

        let result = match terminal {
            RuntimeObservationTerminalState::Exited { ended_at, .. } => self
                .repository
                .borrow_mut()
                .complete_play_session(play_session_id, *ended_at)
                .map(|()| ActivitySessionTransition::Completed(play_session_id)),
            RuntimeObservationTerminalState::Failed { .. }
            | RuntimeObservationTerminalState::Lost => self
                .repository
                .borrow_mut()
                .interrupt_play_session(play_session_id)
                .map(|()| ActivitySessionTransition::Interrupted(play_session_id)),
        };

        if result.is_err() {
            self.runtime_observations
                .borrow_mut()
                .insert(observation_id, activity);
        }
        result
    }

    pub fn handle_managed_session_terminal(
        &self,
        managed_session_id: ManagedSessionId,
        terminal: &ManagedSessionTerminalState,
    ) -> Result<ActivitySessionTransition, R::Error> {
        let Some(play_session_id) = self
            .managed_sessions
            .borrow_mut()
            .remove(&managed_session_id)
        else {
            return Ok(ActivitySessionTransition::None);
        };

        let result = match terminal {
            ManagedSessionTerminalState::Exited { .. } => self
                .repository
                .borrow_mut()
                .complete_play_session(play_session_id, Utc::now().timestamp())
                .map(|()| ActivitySessionTransition::Completed(play_session_id)),
            ManagedSessionTerminalState::Failed { .. } | ManagedSessionTerminalState::Lost => self
                .repository
                .borrow_mut()
                .interrupt_play_session(play_session_id)
                .map(|()| ActivitySessionTransition::Interrupted(play_session_id)),
        };

        if result.is_err() {
            self.managed_sessions
                .borrow_mut()
                .insert(managed_session_id, play_session_id);
        }
        result
    }

    fn begin_managed_session_at(
        &self,
        managed_session_id: ManagedSessionId,
        game_id: GameId,
        source_id: &SourceId,
        now: i64,
    ) -> Result<PlaySessionId, R::Error> {
        if let Some(existing) = self.managed_sessions.borrow().get(&managed_session_id).copied() {
            return Ok(existing);
        }

        let play_session_id = self.repository.borrow_mut().begin_play_session(
            game_id,
            source_id,
            now,
            SessionTrackingMethod::ManagedSession,
        )?;
        self.managed_sessions
            .borrow_mut()
            .insert(managed_session_id, play_session_id);
        Ok(play_session_id)
    }

    pub fn overview(
        &self,
        recent_limit: usize,
        top_games_limit: usize,
    ) -> Result<ActivityOverview, R::Error> {
        self.repository
            .borrow()
            .activity_overview(recent_limit, top_games_limit)
    }

    pub fn record_source_lifetime_playtime(
        &self,
        report: &SourceLifetimePlaytime,
    ) -> Result<(), R::Error> {
        self.repository
            .borrow_mut()
            .upsert_source_lifetime_playtime(report)
    }
}

impl<R> LaunchActivitySink for ActivityService<R>
where
    R: ActivityRepository,
{
    fn launch_dispatched(&self, game_id: GameId, source_id: SourceId) {
        self.pending_launch
            .borrow_mut()
            .replace(PendingLaunch { game_id, source_id });
    }

    fn runtime_observation_armed(
        &self,
        observation_id: RuntimeObservationId,
        game_id: GameId,
        source_id: SourceId,
    ) {
        self.pending_launch.borrow_mut().take();
        self.foreground_return.borrow_mut().take();
        self.runtime_observations.borrow_mut().insert(
            observation_id,
            RuntimeActivity {
                game_id,
                source_id,
                play_session_id: None,
            },
        );
    }

    fn managed_session_started(
        &self,
        managed_session_id: ManagedSessionId,
        game_id: GameId,
        source_id: SourceId,
    ) {
        self.pending_launch.borrow_mut().take();
        if let Err(error) = self.begin_managed_session_at(
            managed_session_id,
            game_id,
            &source_id,
            Utc::now().timestamp(),
        ) {
            warn!(
                session_id = managed_session_id.get(),
                game_id = game_id.get(),
                source = %source_id,
                %error,
                "managed activity session could not be persisted"
            );
        }
    }

    fn cancel_pending_launch(&self) {
        self.pending_launch.borrow_mut().take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ActivityValidationError, PlaySessionState};

    #[derive(Default)]
    struct FakeRepository {
        next_id: i64,
        sessions: Vec<PlaySession>,
        reports: Vec<SourceLifetimePlaytime>,
    }

    impl ActivityRepository for FakeRepository {
        type Error = std::io::Error;

        fn begin_play_session(
            &mut self,
            game_id: GameId,
            source_id: &SourceId,
            started_at: i64,
            tracking_method: SessionTrackingMethod,
        ) -> Result<PlaySessionId, Self::Error> {
            self.next_id += 1;
            let id = PlaySessionId::new(self.next_id).expect("session id");
            self.sessions.push(
                PlaySession::new(
                    id,
                    game_id,
                    source_id.clone(),
                    started_at,
                    None,
                    tracking_method,
                    PlaySessionState::Open,
                )
                .expect("session"),
            );
            Ok(id)
        }

        fn complete_play_session(
            &mut self,
            session_id: PlaySessionId,
            ended_at: i64,
        ) -> Result<(), Self::Error> {
            let index = self
                .sessions
                .iter()
                .position(|session| session.id() == session_id)
                .expect("open session");
            let session = self.sessions[index].clone();
            self.sessions[index] = PlaySession::new(
                session.id(),
                session.game_id(),
                session.source_id().clone(),
                session.started_at(),
                Some(ended_at),
                session.tracking_method(),
                PlaySessionState::Completed,
            )
            .expect("completed session");
            Ok(())
        }

        fn interrupt_play_session(
            &mut self,
            session_id: PlaySessionId,
        ) -> Result<(), Self::Error> {
            let index = self
                .sessions
                .iter()
                .position(|session| session.id() == session_id)
                .expect("open session");
            let session = self.sessions[index].clone();
            self.sessions[index] = PlaySession::new(
                session.id(),
                session.game_id(),
                session.source_id().clone(),
                session.started_at(),
                None,
                session.tracking_method(),
                PlaySessionState::Interrupted,
            )
            .expect("interrupted session");
            Ok(())
        }

        fn interrupt_open_play_sessions(&mut self) -> Result<usize, Self::Error> {
            let mut interrupted = 0;
            for session in &mut self.sessions {
                if session.state() != PlaySessionState::Open {
                    continue;
                }
                interrupted += 1;
                *session = PlaySession::new(
                    session.id(),
                    session.game_id(),
                    session.source_id().clone(),
                    session.started_at(),
                    None,
                    session.tracking_method(),
                    PlaySessionState::Interrupted,
                )
                .expect("interrupted session");
            }
            Ok(interrupted)
        }

        fn activity_overview(
            &self,
            _recent_limit: usize,
            _top_games_limit: usize,
        ) -> Result<ActivityOverview, Self::Error> {
            let total = self
                .sessions
                .iter()
                .filter_map(PlaySession::duration)
                .map(PlaytimeSeconds::get)
                .sum();
            Ok(ActivityOverview::new(
                PlaytimeSeconds::new(total).expect("total"),
                self.sessions
                    .iter()
                    .filter(|session| session.state() == PlaySessionState::Completed)
                    .count(),
                0,
                vec![],
                vec![],
            ))
        }

        fn upsert_source_lifetime_playtime(
            &mut self,
            report: &SourceLifetimePlaytime,
        ) -> Result<(), Self::Error> {
            self.reports.push(report.clone());
            Ok(())
        }
    }

    fn game_id() -> GameId {
        GameId::new(1).expect("game id")
    }

    fn source_id() -> SourceId {
        SourceId::new("steam").expect("source id")
    }

    #[test]
    fn pending_launch_starts_only_after_horizon_loses_activation() {
        let repository = Rc::new(RefCell::new(FakeRepository::default()));
        let service = ActivityService::new(Rc::clone(&repository));
        service.launch_dispatched(game_id(), source_id());

        assert_eq!(
            service
                .handle_application_active_changed_at_millis(true, 100_000)
                .expect("active"),
            ActivitySessionTransition::None
        );
        assert!(repository.borrow().sessions.is_empty());

        assert!(matches!(
            service
                .handle_application_active_changed_at_millis(false, 110_000)
                .expect("inactive"),
            ActivitySessionTransition::Started(_)
        ));
        assert_eq!(repository.borrow().sessions.len(), 1);
    }

    #[test]
    fn mature_return_completes_after_short_grace_without_inflating_duration() {
        let repository = Rc::new(RefCell::new(FakeRepository::default()));
        let service = ActivityService::new(Rc::clone(&repository));
        service.launch_dispatched(game_id(), source_id());
        service
            .handle_application_active_changed_at_millis(false, 100_000)
            .expect("start");

        assert_eq!(
            service
                .handle_application_active_changed_at_millis(true, 220_000)
                .expect("candidate return"),
            ActivitySessionTransition::None
        );
        assert_eq!(
            service
                .poll_foreground_return_at_millis(221_499)
                .expect("still in grace"),
            ActivitySessionTransition::None
        );
        assert!(matches!(
            service
                .poll_foreground_return_at_millis(221_500)
                .expect("confirmed return"),
            ActivitySessionTransition::Completed(_)
        ));

        let repository = repository.borrow();
        assert_eq!(repository.sessions[0].state(), PlaySessionState::Completed);
        assert_eq!(repository.sessions[0].duration().expect("duration").get(), 120);
    }

    #[test]
    fn startup_focus_bounce_keeps_the_same_foreground_session_open() {
        let repository = Rc::new(RefCell::new(FakeRepository::default()));
        let service = ActivityService::new(Rc::clone(&repository));
        service.launch_dispatched(game_id(), source_id());
        service
            .handle_application_active_changed_at_millis(false, 100_000)
            .expect("start");

        service
            .handle_application_active_changed_at_millis(true, 101_000)
            .expect("launcher bounced focus back");
        assert_eq!(
            service
                .poll_foreground_return_at_millis(120_000)
                .expect("startup candidate remains pending"),
            ActivitySessionTransition::None
        );

        service
            .handle_application_active_changed_at_millis(false, 125_000)
            .expect("game took focus");
        assert_eq!(repository.borrow().sessions.len(), 1);
        assert_eq!(repository.borrow().sessions[0].state(), PlaySessionState::Open);

        service
            .handle_application_active_changed_at_millis(true, 280_000)
            .expect("real return");
        assert!(matches!(
            service
                .poll_foreground_return_at_millis(281_500)
                .expect("confirmed real return"),
            ActivitySessionTransition::Completed(_)
        ));

        let repository = repository.borrow();
        assert_eq!(repository.sessions[0].state(), PlaySessionState::Completed);
        assert_eq!(repository.sessions[0].duration().expect("duration").get(), 180);
    }

    #[test]
    fn startup_candidate_uses_original_return_time_after_stabilization() {
        let repository = Rc::new(RefCell::new(FakeRepository::default()));
        let service = ActivityService::new(Rc::clone(&repository));
        service.launch_dispatched(game_id(), source_id());
        service
            .handle_application_active_changed_at_millis(false, 100_000)
            .expect("start");
        service
            .handle_application_active_changed_at_millis(true, 101_000)
            .expect("early return");

        assert_eq!(
            service
                .poll_foreground_return_at_millis(129_999)
                .expect("still stabilizing"),
            ActivitySessionTransition::None
        );
        assert!(matches!(
            service
                .poll_foreground_return_at_millis(130_000)
                .expect("stabilized return"),
            ActivitySessionTransition::Completed(_)
        ));

        let repository = repository.borrow();
        assert_eq!(repository.sessions[0].state(), PlaySessionState::Completed);
        assert_eq!(repository.sessions[0].duration().expect("duration").get(), 1);
    }

    #[test]
    fn unrelated_window_activation_does_not_fabricate_a_session() {
        let repository = Rc::new(RefCell::new(FakeRepository::default()));
        let service = ActivityService::new(Rc::clone(&repository));
        service
            .handle_application_active_changed_at_millis(false, 100_000)
            .expect("inactive");
        service
            .handle_application_active_changed_at_millis(true, 110_000)
            .expect("active");
        assert!(repository.borrow().sessions.is_empty());
    }

    #[test]
    fn cancelling_pending_launch_prevents_later_false_handoff() {
        let repository = Rc::new(RefCell::new(FakeRepository::default()));
        let service = ActivityService::new(Rc::clone(&repository));
        service.launch_dispatched(game_id(), source_id());
        service.cancel_pending_launch();
        service
            .handle_application_active_changed_at_millis(false, 100_000)
            .expect("inactive");
        assert!(repository.borrow().sessions.is_empty());
    }

    #[test]
    fn recovery_marks_open_sessions_interrupted_without_inventing_duration() {
        let repository = Rc::new(RefCell::new(FakeRepository::default()));
        let service = ActivityService::new(Rc::clone(&repository));
        service.launch_dispatched(game_id(), source_id());
        service
            .handle_application_active_changed_at_millis(false, 100_000)
            .expect("start");

        assert_eq!(service.recover_interrupted_sessions().expect("recover"), 1);
        let repository = repository.borrow();
        assert_eq!(repository.sessions[0].state(), PlaySessionState::Interrupted);
        assert_eq!(repository.sessions[0].duration(), None);
    }

    #[test]
    fn managed_session_uses_managed_tracking_and_ignores_window_focus_changes() {
        let repository = Rc::new(RefCell::new(FakeRepository::default()));
        let service = ActivityService::new(Rc::clone(&repository));
        let managed_id = ManagedSessionId::new(7).expect("managed session id");
        let bottles = SourceId::new("bottles").expect("source id");

        service.managed_session_started(managed_id, game_id(), bottles);
        assert_eq!(repository.borrow().sessions.len(), 1);
        assert_eq!(
            repository.borrow().sessions[0].tracking_method(),
            SessionTrackingMethod::ManagedSession
        );

        assert_eq!(
            service
                .handle_application_active_changed_at_millis(false, 100_000)
                .expect("inactive"),
            ActivitySessionTransition::None
        );
        assert_eq!(
            service
                .handle_application_active_changed_at_millis(true, 200_000)
                .expect("active"),
            ActivitySessionTransition::None
        );
        assert_eq!(
            repository.borrow().sessions[0].state(),
            PlaySessionState::Open
        );

        assert!(matches!(
            service
                .handle_managed_session_terminal(
                    managed_id,
                    &ManagedSessionTerminalState::Exited { exit_code: Some(0) },
                )
                .expect("managed completion"),
            ActivitySessionTransition::Completed(_)
        ));
        assert_eq!(
            repository.borrow().sessions[0].state(),
            PlaySessionState::Completed
        );
    }

    #[test]
    fn lost_managed_session_is_interrupted_without_inventing_duration() {
        let repository = Rc::new(RefCell::new(FakeRepository::default()));
        let service = ActivityService::new(Rc::clone(&repository));
        let managed_id = ManagedSessionId::new(8).expect("managed session id");

        service.managed_session_started(managed_id, game_id(), source_id());
        assert!(matches!(
            service
                .handle_managed_session_terminal(
                    managed_id,
                    &ManagedSessionTerminalState::Lost,
                )
                .expect("managed interruption"),
            ActivitySessionTransition::Interrupted(_)
        ));

        let repository = repository.borrow();
        assert_eq!(repository.sessions[0].state(), PlaySessionState::Interrupted);
        assert_eq!(repository.sessions[0].duration(), None);
    }

    #[test]
    fn source_runtime_observation_ignores_focus_and_uses_host_timestamps() {
        let repository = Rc::new(RefCell::new(FakeRepository::default()));
        let service = ActivityService::new(Rc::clone(&repository));
        let observation_id = RuntimeObservationId::new(11).expect("observation id");
        service.runtime_observation_armed(observation_id, game_id(), source_id());

        assert!(matches!(
            service
                .handle_runtime_observation_event(&RuntimeObservationEvent::Started {
                    observation_id,
                    started_at: 100,
                })
                .expect("runtime start"),
            ActivitySessionTransition::Started(_)
        ));
        assert_eq!(
            repository.borrow().sessions[0].tracking_method(),
            SessionTrackingMethod::SourceRuntime
        );

        service
            .handle_application_active_changed_at_millis(true, 150_000)
            .expect("focus in");
        service
            .handle_application_active_changed_at_millis(false, 160_000)
            .expect("focus out");
        assert_eq!(repository.borrow().sessions[0].state(), PlaySessionState::Open);

        assert!(matches!(
            service
                .handle_runtime_observation_event(&RuntimeObservationEvent::Terminal {
                    observation_id,
                    terminal: RuntimeObservationTerminalState::Exited {
                        started_at: 100,
                        ended_at: 280,
                    },
                })
                .expect("runtime end"),
            ActivitySessionTransition::Completed(_)
        ));
        assert_eq!(
            repository.borrow().sessions[0].duration().expect("duration").get(),
            180
        );
    }

    #[test]
    fn lost_runtime_observation_interrupts_started_session() {
        let repository = Rc::new(RefCell::new(FakeRepository::default()));
        let service = ActivityService::new(Rc::clone(&repository));
        let observation_id = RuntimeObservationId::new(12).expect("observation id");
        service.runtime_observation_armed(observation_id, game_id(), source_id());
        service
            .handle_runtime_observation_event(&RuntimeObservationEvent::Started {
                observation_id,
                started_at: 100,
            })
            .expect("runtime start");

        assert!(matches!(
            service
                .handle_runtime_observation_event(&RuntimeObservationEvent::Terminal {
                    observation_id,
                    terminal: RuntimeObservationTerminalState::Lost,
                })
                .expect("runtime lost"),
            ActivitySessionTransition::Interrupted(_)
        ));
        assert_eq!(repository.borrow().sessions[0].duration(), None);
    }

    #[test]
    fn source_reported_lifetime_is_not_added_to_observed_totals() {
        let repository = Rc::new(RefCell::new(FakeRepository::default()));
        let service = ActivityService::new(Rc::clone(&repository));
        let report = SourceLifetimePlaytime::new(
            game_id(),
            source_id(),
            PlaytimeSeconds::new(9_999).expect("playtime"),
            500,
        );
        service
            .record_source_lifetime_playtime(&report)
            .expect("report");

        assert_eq!(
            service.overview(5, 5).expect("overview").observed_playtime().get(),
            0
        );
    }

    #[test]
    fn activity_validation_error_type_remains_domain_owned() {
        assert!(matches!(
            PlaytimeSeconds::new(-1),
            Err(ActivityValidationError::NegativePlaytime(-1))
        ));
    }
}
