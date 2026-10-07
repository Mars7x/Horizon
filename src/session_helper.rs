use std::{
    collections::BTreeMap,
    env,
    error::Error,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, atomic::{AtomicU64, Ordering}},
    thread,
    time::Duration,
};

use tracing::{info, warn};
use zbus::{blocking::Connection, interface};

use crate::{
    domain::{ExternalGameId, SourceId},
    platform::session_helper::{
        SESSION_HELPER_BUS_NAME, SESSION_HELPER_INTERFACE, SESSION_HELPER_OBJECT_PATH,
        SESSION_HELPER_PROTOCOL_VERSION, STARTED, START_FAILED, START_UNAVAILABLE,
        START_UNSUPPORTED, STATE_EXITED, STATE_FAILED, STATE_RUNNING, STATE_UNKNOWN,
    },
    sources::{SourceCapability, SourceRegistry, production_source_registry},
};

struct ManagedChild {
    child: Option<Child>,
    exit_code: Option<i32>,
    failed: Option<String>,
}

impl ManagedChild {
    fn running(child: Child) -> Self {
        Self {
            child: Some(child),
            exit_code: None,
            failed: None,
        }
    }

    fn refresh(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                self.exit_code = status.code();
                self.child = None;
            }
            Ok(None) => {}
            Err(error) => {
                self.failed = Some(format!("failed to observe Gamescope child: {error}"));
                self.child = None;
            }
        }
    }
}

struct SessionBroker {
    registry: SourceRegistry,
    gamescope: Option<PathBuf>,
    next_session_id: AtomicU64,
    sessions: Mutex<BTreeMap<u64, ManagedChild>>,
}

impl SessionBroker {
    fn new(registry: SourceRegistry) -> Self {
        Self {
            registry,
            gamescope: find_program("gamescope"),
            next_session_id: AtomicU64::new(1),
            sessions: Mutex::new(BTreeMap::new()),
        }
    }
}

#[interface(name = "io.github.Mars7x.Horizon.Session1")]
impl SessionBroker {
    fn helper_info(&self) -> (u32, bool, String) {
        let available = self.gamescope.is_some();
        let detail = self
            .gamescope
            .as_deref()
            .map(|path| format!("Gamescope available at {}", path.display()))
            .unwrap_or_else(|| "Gamescope was not found in the host PATH".to_owned());
        (SESSION_HELPER_PROTOCOL_VERSION, available, detail)
    }

    fn start_session(&self, source_id: String, external_id: String) -> (u32, u64, String) {
        let Some(gamescope) = self.gamescope.as_deref() else {
            return (
                START_UNAVAILABLE,
                0,
                "Gamescope is not installed or not visible to the host helper".to_owned(),
            );
        };

        let source_id = match SourceId::new(source_id) {
            Ok(value) => value,
            Err(error) => return (START_FAILED, 0, error.to_string()),
        };
        let external_id = match ExternalGameId::new(external_id) {
            Ok(value) => value,
            Err(error) => return (START_FAILED, 0, error.to_string()),
        };
        let Some(source) = self.registry.get(&source_id) else {
            return (
                START_UNSUPPORTED,
                0,
                format!("source {source_id} is not registered in the host helper"),
            );
        };
        if !source.descriptor().supports(SourceCapability::ManagedSession) {
            return (
                START_UNSUPPORTED,
                0,
                format!("source {source_id} does not support managed sessions"),
            );
        }

        let target = match source.managed_launch_target(&external_id) {
            Ok(Some(target)) => target,
            Ok(None) => {
                return (
                    START_UNSUPPORTED,
                    0,
                    format!("source {source_id} has no managed launch target for this game"),
                );
            }
            Err(error) => return (START_FAILED, 0, error.to_string()),
        };

        let Some(target_program) = find_program(target.program()) else {
            return (
                START_UNAVAILABLE,
                0,
                format!(
                    "managed launch program {} is not installed or not visible to the host helper",
                    target.program()
                ),
            );
        };

        let child = match Command::new(gamescope)
            .arg("-f")
            .arg("--")
            .arg(target_program)
            .args(target.args())
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                return (
                    START_FAILED,
                    0,
                    format!("could not start Gamescope: {error}"),
                );
            }
        };

        let session_id = self.next_session_id.fetch_add(1, Ordering::Relaxed);
        self.sessions
            .lock()
            .expect("session helper mutex poisoned")
            .insert(session_id, ManagedChild::running(child));

        info!(
            session_id,
            source = %source_id,
            external_id = %external_id,
            "managed Gamescope session started"
        );
        (STARTED, session_id, String::new())
    }

    fn session_state(&self, session_id: u64) -> (u32, i32, String) {
        let mut sessions = self.sessions.lock().expect("session helper mutex poisoned");
        let Some(session) = sessions.get_mut(&session_id) else {
            return (STATE_UNKNOWN, -1, "managed session is unknown".to_owned());
        };
        session.refresh();

        if let Some(message) = &session.failed {
            return (STATE_FAILED, -1, message.clone());
        }
        if session.child.is_some() {
            return (STATE_RUNNING, -1, String::new());
        }

        (
            STATE_EXITED,
            session.exit_code.unwrap_or(-1),
            String::new(),
        )
    }

    fn stop_session(&self, session_id: u64) -> (bool, String) {
        let mut sessions = self.sessions.lock().expect("session helper mutex poisoned");
        let Some(session) = sessions.get_mut(&session_id) else {
            return (false, "managed session is unknown".to_owned());
        };
        session.refresh();
        let Some(child) = session.child.as_mut() else {
            return (true, String::new());
        };

        match child.kill() {
            Ok(()) => (true, String::new()),
            Err(error) => (false, format!("could not stop Gamescope session: {error}")),
        }
    }

    fn forget_session(&self, session_id: u64) -> (bool, String) {
        let mut sessions = self.sessions.lock().expect("session helper mutex poisoned");
        let Some(session) = sessions.get_mut(&session_id) else {
            return (true, String::new());
        };
        session.refresh();
        if session.child.is_some() {
            return (false, "cannot forget a running managed session".to_owned());
        }
        sessions.remove(&session_id);
        (true, String::new())
    }
}

pub fn run() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("horizon=info")),
        )
        .init();

    let broker = SessionBroker::new(production_source_registry()?);
    if broker.gamescope.is_none() {
        warn!(
            "Gamescope is not currently available; helper will report managed sessions unavailable"
        );
    }

    let connection = Connection::session()?;
    connection.object_server().at(SESSION_HELPER_OBJECT_PATH, broker)?;
    connection.request_name(SESSION_HELPER_BUS_NAME)?;
    info!(
        bus_name = SESSION_HELPER_BUS_NAME,
        interface = SESSION_HELPER_INTERFACE,
        protocol = SESSION_HELPER_PROTOCOL_VERSION,
        "Horizon managed-session helper ready"
    );

    loop {
        thread::park_timeout(Duration::from_secs(60));
        if connection.is_closed() {
            return Err("session D-Bus connection closed".into());
        }
    }
}

fn find_program(program: &str) -> Option<PathBuf> {
    let candidate = Path::new(program);
    if candidate.components().count() > 1 {
        return is_executable(candidate).then(|| candidate.to_owned());
    }

    env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|directory| directory.join(program))
            .find(|candidate| is_executable(candidate))
    })
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_lookup_does_not_invent_missing_programs() {
        assert!(find_program("horizon-definitely-not-a-real-command-95").is_none());
    }
}
