use zbus::blocking::{Connection, Proxy, connection::Builder};

use crate::{
    domain::{ExternalGameId, SourceId},
    services::{
        runtime::{
            RuntimeObservationError, RuntimeObservationExecutor, RuntimeObservationId,
            RuntimeObservationStartOutcome, RuntimeObservationState,
        },
        session::{
            ManagedSessionExecutionError, ManagedSessionExecutor, ManagedSessionId,
            ManagedSessionState, ManagedStartOutcome,
        },
    },
};

pub const SESSION_HELPER_BUS_NAME: &str = "io.github.Mars7x.Horizon.Session1";
pub const SESSION_HELPER_OBJECT_PATH: &str = "/io/github/Mars7x/Horizon/Session1";
pub const SESSION_HELPER_INTERFACE: &str = "io.github.Mars7x.Horizon.Session1";
pub const SESSION_HELPER_PROTOCOL_VERSION: u32 = 2;

pub(crate) const STARTED: u32 = 0;
pub(crate) const START_UNSUPPORTED: u32 = 1;
pub(crate) const START_UNAVAILABLE: u32 = 2;
pub(crate) const START_FAILED: u32 = 3;

pub(crate) const STATE_RUNNING: u32 = 0;
pub(crate) const STATE_EXITED: u32 = 1;
pub(crate) const STATE_FAILED: u32 = 2;
pub(crate) const STATE_UNKNOWN: u32 = 3;

pub(crate) const OBS_WAITING: u32 = 0;
pub(crate) const OBS_RUNNING: u32 = 1;
pub(crate) const OBS_EXITED: u32 = 2;
pub(crate) const OBS_FAILED: u32 = 3;
pub(crate) const OBS_UNKNOWN: u32 = 4;

fn connection() -> Result<Connection, zbus::Error> {
    Builder::session()?
        .method_timeout(std::time::Duration::from_millis(250))
        .build()
}

fn proxy(connection: &Connection) -> Result<Proxy<'_>, zbus::Error> {
    Proxy::new(
        connection,
        SESSION_HELPER_BUS_NAME,
        SESSION_HELPER_OBJECT_PATH,
        SESSION_HELPER_INTERFACE,
    )
}

/// Narrow D-Bus client for the optional host-side managed-session helper.
///
/// The sandbox sends only source/game identity. It never sends a shell command
/// or arbitrary executable path to the host.
pub struct DbusManagedSessionExecutor {
    connection: Connection,
}

impl DbusManagedSessionExecutor {
    pub fn new() -> Result<Self, ManagedSessionExecutionError> {
        Ok(Self {
            connection: connection().map_err(ManagedSessionExecutionError::new)?,
        })
    }

    fn proxy(&self) -> Result<Proxy<'_>, ManagedSessionExecutionError> {
        proxy(&self.connection).map_err(ManagedSessionExecutionError::new)
    }

    fn helper_info(&self, proxy: &Proxy<'_>) -> Option<(u32, bool, String)> {
        proxy.call("HelperInfo", &()).ok()
    }

    /// Best-effort capability probe used only for diagnostics. Launching still
    /// retries the helper later, so starting Horizon before the helper does not
    /// permanently disable managed sessions for that process.
    pub fn probe(&self) -> bool {
        let Ok(proxy) = self.proxy() else {
            return false;
        };
        matches!(
            self.helper_info(&proxy),
            Some((SESSION_HELPER_PROTOCOL_VERSION, true, _))
        )
    }
}

impl ManagedSessionExecutor for DbusManagedSessionExecutor {
    fn start_session(
        &self,
        source_id: &SourceId,
        external_id: &ExternalGameId,
    ) -> Result<ManagedStartOutcome, ManagedSessionExecutionError> {
        let Ok(proxy) = self.proxy() else {
            return Ok(ManagedStartOutcome::Unavailable);
        };
        let Some((protocol, available, _detail)) = self.helper_info(&proxy) else {
            return Ok(ManagedStartOutcome::Unavailable);
        };
        if protocol != SESSION_HELPER_PROTOCOL_VERSION || !available {
            return Ok(ManagedStartOutcome::Unavailable);
        }

        let result: Result<(u32, u64, String), zbus::Error> =
            proxy.call("StartSession", &(source_id.as_str(), external_id.as_str()));
        let Ok((code, raw_session_id, detail)) = result else {
            return Ok(ManagedStartOutcome::Unavailable);
        };

        match code {
            STARTED => ManagedSessionId::new(raw_session_id)
                .map(ManagedStartOutcome::Started)
                .ok_or_else(|| {
                    ManagedSessionExecutionError::message("host helper returned session id 0")
                }),
            START_UNSUPPORTED => Ok(ManagedStartOutcome::Unsupported),
            START_UNAVAILABLE => Ok(ManagedStartOutcome::Unavailable),
            START_FAILED => Err(ManagedSessionExecutionError::message(detail)),
            other => Err(ManagedSessionExecutionError::message(format!(
                "host helper returned unknown start status {other}: {detail}"
            ))),
        }
    }

    fn session_state(
        &self,
        session_id: ManagedSessionId,
    ) -> Result<ManagedSessionState, ManagedSessionExecutionError> {
        let proxy = self.proxy()?;
        let (code, exit_code, detail): (u32, i32, String) = proxy
            .call("SessionState", &(session_id.get(),))
            .map_err(ManagedSessionExecutionError::new)?;

        match code {
            STATE_RUNNING => Ok(ManagedSessionState::Running),
            STATE_EXITED => Ok(ManagedSessionState::Exited {
                exit_code: (exit_code >= 0).then_some(exit_code),
            }),
            STATE_FAILED => Ok(ManagedSessionState::Failed { message: detail }),
            STATE_UNKNOWN => Ok(ManagedSessionState::Unknown),
            other => Err(ManagedSessionExecutionError::message(format!(
                "host helper returned unknown session status {other}: {detail}"
            ))),
        }
    }

    fn stop_session(
        &self,
        session_id: ManagedSessionId,
    ) -> Result<(), ManagedSessionExecutionError> {
        let proxy = self.proxy()?;
        let (ok, detail): (bool, String) = proxy
            .call("StopSession", &(session_id.get(),))
            .map_err(ManagedSessionExecutionError::new)?;
        if ok {
            Ok(())
        } else {
            Err(ManagedSessionExecutionError::message(detail))
        }
    }

    fn forget_session(
        &self,
        session_id: ManagedSessionId,
    ) -> Result<(), ManagedSessionExecutionError> {
        let proxy = self.proxy()?;
        let (ok, detail): (bool, String) = proxy
            .call("ForgetSession", &(session_id.get(),))
            .map_err(ManagedSessionExecutionError::new)?;
        if ok {
            Ok(())
        } else {
            Err(ManagedSessionExecutionError::message(detail))
        }
    }
}

/// Narrow D-Bus client for source-owned host runtime observation.
///
/// This works while Horizon is a normal Flatpak as well as when managed
/// Gamescope launching is available. The sandbox never supplies process names,
/// PIDs, paths, or arbitrary matching expressions.
pub struct DbusRuntimeObservationExecutor {
    connection: Connection,
}

impl DbusRuntimeObservationExecutor {
    pub fn new() -> Result<Self, RuntimeObservationError> {
        Ok(Self {
            connection: connection().map_err(RuntimeObservationError::new)?,
        })
    }

    fn proxy(&self) -> Result<Proxy<'_>, RuntimeObservationError> {
        proxy(&self.connection).map_err(RuntimeObservationError::new)
    }

    pub fn probe(&self) -> bool {
        let Ok(proxy) = self.proxy() else {
            return false;
        };
        let info: Result<(u32, bool, String), zbus::Error> = proxy.call("RuntimeObserverInfo", &());
        matches!(info, Ok((SESSION_HELPER_PROTOCOL_VERSION, true, _)))
    }
}

impl RuntimeObservationExecutor for DbusRuntimeObservationExecutor {
    fn start_observation(
        &self,
        source_id: &SourceId,
        external_id: &ExternalGameId,
    ) -> Result<RuntimeObservationStartOutcome, RuntimeObservationError> {
        let Ok(proxy) = self.proxy() else {
            return Ok(RuntimeObservationStartOutcome::Unavailable);
        };
        let info: Result<(u32, bool, String), zbus::Error> = proxy.call("RuntimeObserverInfo", &());
        let Ok((protocol, available, _detail)) = info else {
            return Ok(RuntimeObservationStartOutcome::Unavailable);
        };
        if protocol != SESSION_HELPER_PROTOCOL_VERSION || !available {
            return Ok(RuntimeObservationStartOutcome::Unavailable);
        }

        let result: Result<(u32, u64, String), zbus::Error> = proxy.call(
            "StartRuntimeObservation",
            &(source_id.as_str(), external_id.as_str()),
        );
        let Ok((code, raw_id, detail)) = result else {
            return Ok(RuntimeObservationStartOutcome::Unavailable);
        };

        match code {
            STARTED => RuntimeObservationId::new(raw_id)
                .map(RuntimeObservationStartOutcome::Started)
                .ok_or_else(|| {
                    RuntimeObservationError::message("host helper returned observation id 0")
                }),
            START_UNSUPPORTED => Ok(RuntimeObservationStartOutcome::Unsupported),
            START_UNAVAILABLE => Ok(RuntimeObservationStartOutcome::Unavailable),
            START_FAILED => Err(RuntimeObservationError::message(detail)),
            other => Err(RuntimeObservationError::message(format!(
                "host helper returned unknown observation start status {other}: {detail}"
            ))),
        }
    }

    fn observation_state(
        &self,
        observation_id: RuntimeObservationId,
    ) -> Result<RuntimeObservationState, RuntimeObservationError> {
        let proxy = self.proxy()?;
        let (code, started_at, ended_at, detail): (u32, i64, i64, String) = proxy
            .call("RuntimeObservationState", &(observation_id.get(),))
            .map_err(RuntimeObservationError::new)?;

        match code {
            OBS_WAITING => Ok(RuntimeObservationState::Waiting),
            OBS_RUNNING => Ok(RuntimeObservationState::Running { started_at }),
            OBS_EXITED => Ok(RuntimeObservationState::Exited {
                started_at,
                ended_at,
            }),
            OBS_FAILED => Ok(RuntimeObservationState::Failed { message: detail }),
            OBS_UNKNOWN => Ok(RuntimeObservationState::Unknown),
            other => Err(RuntimeObservationError::message(format!(
                "host helper returned unknown runtime observation status {other}: {detail}"
            ))),
        }
    }

    fn forget_observation(
        &self,
        observation_id: RuntimeObservationId,
    ) -> Result<(), RuntimeObservationError> {
        let proxy = self.proxy()?;
        let (ok, detail): (bool, String) = proxy
            .call("ForgetRuntimeObservation", &(observation_id.get(),))
            .map_err(RuntimeObservationError::new)?;
        if ok {
            Ok(())
        } else {
            Err(RuntimeObservationError::message(detail))
        }
    }
}
