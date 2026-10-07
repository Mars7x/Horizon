use zbus::blocking::{Connection, Proxy, connection::Builder};

use crate::{
    domain::{ExternalGameId, SourceId},
    services::session::{
        ManagedSessionExecutionError, ManagedSessionExecutor, ManagedSessionId, ManagedSessionState,
        ManagedStartOutcome,
    },
};

pub const SESSION_HELPER_BUS_NAME: &str = "io.github.Mars7x.Horizon.Session1";
pub const SESSION_HELPER_OBJECT_PATH: &str = "/io/github/Mars7x/Horizon/Session1";
pub const SESSION_HELPER_INTERFACE: &str = "io.github.Mars7x.Horizon.Session1";
pub const SESSION_HELPER_PROTOCOL_VERSION: u32 = 1;

pub(crate) const STARTED: u32 = 0;
pub(crate) const START_UNSUPPORTED: u32 = 1;
pub(crate) const START_UNAVAILABLE: u32 = 2;
pub(crate) const START_FAILED: u32 = 3;

pub(crate) const STATE_RUNNING: u32 = 0;
pub(crate) const STATE_EXITED: u32 = 1;
pub(crate) const STATE_FAILED: u32 = 2;
pub(crate) const STATE_UNKNOWN: u32 = 3;

/// Narrow D-Bus client for the optional host-side managed-session helper.
///
/// The sandbox sends only source/game identity. It never sends a shell command
/// or arbitrary executable path to the host.
pub struct DbusManagedSessionExecutor {
    connection: Connection,
}

impl DbusManagedSessionExecutor {
    pub fn new() -> Result<Self, ManagedSessionExecutionError> {
        let connection = Builder::session()
            .map_err(ManagedSessionExecutionError::new)?
            .method_timeout(std::time::Duration::from_millis(250))
            .build()
            .map_err(ManagedSessionExecutionError::new)?;
        Ok(Self { connection })
    }

    fn proxy(&self) -> Result<Proxy<'_>, ManagedSessionExecutionError> {
        Proxy::new(
            &self.connection,
            SESSION_HELPER_BUS_NAME,
            SESSION_HELPER_OBJECT_PATH,
            SESSION_HELPER_INTERFACE,
        )
        .map_err(ManagedSessionExecutionError::new)
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

        let result: Result<(u32, u64, String), zbus::Error> = proxy.call(
            "StartSession",
            &(source_id.as_str(), external_id.as_str()),
        );
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
