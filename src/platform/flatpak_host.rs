//! The host's Flatpak, for the Shop: what's installed, what can be updated,
//! and installing, updating and uninstalling apps; and for the Flatpak game
//! source: running a game and listing what's running (ADR 0183).
//!
//! This is Horizon's one host-execution path (ADR 0181). Inside the sandbox
//! it runs the host's `flatpak` through `flatpak-spawn --host`
//! (`--talk-name=org.freedesktop.Flatpak`); outside one, `flatpak` directly.
//! Only the fixed subcommands below are ever run, with an app ID checked
//! first (`services::flathub::valid_app_id`) and passed as one argument, never
//! through a shell. Nothing here takes a program, an argument list or a path
//! from anywhere else. Blocking: call it off the UI thread.

use std::{
    collections::{BTreeSet, HashSet},
    io::Read,
    path::Path,
    process::{Command, Stdio},
};

use thiserror::Error;

use crate::{services::flathub::valid_app_id, sources::flatpak::FlatpakHost};

/// The remote apps are installed from.
const REMOTE: &str = "flathub";
const REMOTE_URL: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";

#[derive(Debug, Error)]
pub enum HostFlatpakError {
    #[error("not a Flathub app ID")]
    InvalidAppId,
    #[error("Horizon can't remove itself")]
    OwnApp,
    #[error("could not run flatpak on the host: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("flatpak {command} failed: {detail}")]
    Failed {
        command: &'static str,
        detail: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installation {
    System,
    User,
}

impl Installation {
    fn flag(self) -> &'static str {
        match self {
            Self::System => "--system",
            Self::User => "--user",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    pub app_id: String,
    pub name: String,
    pub installation: Installation,
    /// The remote it came from ("flathub" for Flathub's).
    pub origin: String,
}

/// `flatpak …` on the host.
fn flatpak(args: &[&str]) -> Command {
    let mut command = if Path::new("/.flatpak-info").exists() {
        let mut command = Command::new("flatpak-spawn");
        command.args(["--host", "flatpak"]);
        command
    } else {
        Command::new("flatpak")
    };
    command.args(args).stdin(Stdio::null());
    command
}

/// Run to the end; its output, or what it said went wrong.
fn output(command: &'static str, args: &[&str]) -> Result<String, HostFlatpakError> {
    let result = flatpak(args).output()?;
    if result.status.success() {
        Ok(String::from_utf8_lossy(&result.stdout).into_owned())
    } else {
        Err(HostFlatpakError::Failed {
            command,
            detail: last_line(&String::from_utf8_lossy(&result.stderr)),
        })
    }
}

/// Run to the end, reporting each percentage it prints.
fn with_progress(
    command: &'static str,
    args: &[&str],
    progress: &mut dyn FnMut(u8),
) -> Result<(), HostFlatpakError> {
    let mut child = flatpak(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(stderr) = stderr.as_mut() {
            let _ = stderr.read_to_string(&mut text);
        }
        text
    });
    if let Some(mut stdout) = child.stdout.take() {
        let mut buffer = [0_u8; 4096];
        let mut line = Vec::new();
        while let Ok(read) = stdout.read(&mut buffer) {
            if read == 0 {
                break;
            }
            for &byte in &buffer[..read] {
                if byte == b'\n' || byte == b'\r' {
                    if let Some(percent) = percent(&String::from_utf8_lossy(&line)) {
                        progress(percent);
                    }
                    line.clear();
                } else {
                    line.push(byte);
                }
            }
        }
    }
    let status = child.wait()?;
    let errors = errors.join().unwrap_or_default();
    if status.success() {
        Ok(())
    } else {
        Err(HostFlatpakError::Failed {
            command,
            detail: last_line(&errors),
        })
    }
}

fn last_line(text: &str) -> String {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no message")
        .to_owned()
}

/// The last "NN%" in a line of flatpak's progress output.
fn percent(line: &str) -> Option<u8> {
    line.rmatch_indices('%').find_map(|(at, _)| {
        let digits: String = line[..at]
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        digits.parse::<u8>().ok().filter(|value| *value <= 100)
    })
}

fn checked(app_id: &str) -> Result<&str, HostFlatpakError> {
    valid_app_id(app_id).map_err(|_| HostFlatpakError::InvalidAppId)?;
    Ok(app_id)
}

/// The installed apps, both installations.
pub fn installed() -> Result<Vec<InstalledApp>, HostFlatpakError> {
    let text = output(
        "list",
        &[
            "list",
            "--app",
            "--columns=application,name,installation,origin",
        ],
    )?;
    Ok(parse_installed(&text))
}

fn parse_installed(text: &str) -> Vec<InstalledApp> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let app_id = fields.next()?.trim();
            let name = fields.next()?.trim();
            let installation = match fields.next()?.trim() {
                "user" => Installation::User,
                _ => Installation::System,
            };
            let origin = fields.next().unwrap_or_default().trim();
            valid_app_id(app_id).ok()?;
            Some(InstalledApp {
                app_id: app_id.to_owned(),
                name: if name.is_empty() { app_id } else { name }.to_owned(),
                installation,
                origin: origin.to_owned(),
            })
        })
        .collect()
}

/// The installed apps with an update waiting (asks the remotes).
pub fn updates() -> Result<HashSet<String>, HostFlatpakError> {
    let text = output(
        "remote-ls",
        &["remote-ls", "--updates", "--app", "--columns=application"],
    )?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|id| valid_app_id(id).is_ok())
        .map(str::to_owned)
        .collect())
}

/// Start an installed app (a Flatpak game) and return without waiting for
/// it. A thread waits for it instead, so it never lingers as a zombie.
pub fn run(app_id: &str) -> Result<(), HostFlatpakError> {
    let app_id = checked(app_id)?;
    let mut child = flatpak(&["run", app_id])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let waited = std::thread::Builder::new()
        .name("flatpak-run".into())
        .spawn(move || {
            let _ = child.wait();
        });
    if let Err(error) = waited {
        // The game still runs; only its exit goes unreaped until Horizon exits.
        tracing::warn!(%error, "no thread to wait for the Flatpak game");
    }
    Ok(())
}

/// The IDs of the apps running now.
pub fn running() -> Result<BTreeSet<String>, HostFlatpakError> {
    let text = output("ps", &["ps", "--columns=application"])?;
    Ok(parse_running(&text))
}

fn parse_running(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|id| valid_app_id(id).is_ok())
        .map(str::to_owned)
        .collect()
}

/// The Flatpak game source's view of the host.
pub struct HostFlatpak;

impl FlatpakHost for HostFlatpak {
    fn running_apps(&self) -> Result<BTreeSet<String>, String> {
        running().map_err(|error| error.to_string())
    }
}

/// Install from Flathub: system-wide when the system has Flathub (as the
/// software store does), else for this user, adding Flathub there if
/// needed. A system install that's refused (no permission) is tried for
/// this user.
pub fn install(app_id: &str, progress: &mut dyn FnMut(u8)) -> Result<(), HostFlatpakError> {
    let app_id = checked(app_id)?;
    let system_has_flathub = output("remotes", &["remotes", "--system", "--columns=name"])
        .is_ok_and(|text| text.lines().any(|name| name.trim() == REMOTE));
    if system_has_flathub {
        let system = with_progress(
            "install",
            &[
                "install",
                "--system",
                "--noninteractive",
                "-y",
                REMOTE,
                app_id,
            ],
            progress,
        );
        if system.is_ok() {
            return system;
        }
    }
    output(
        "remote-add",
        &[
            "remote-add",
            "--user",
            "--if-not-exists",
            REMOTE,
            REMOTE_URL,
        ],
    )?;
    with_progress(
        "install",
        &[
            "install",
            "--user",
            "--noninteractive",
            "-y",
            REMOTE,
            app_id,
        ],
        progress,
    )
}

pub fn update(
    app_id: &str,
    installation: Installation,
    progress: &mut dyn FnMut(u8),
) -> Result<(), HostFlatpakError> {
    let app_id = checked(app_id)?;
    with_progress(
        "update",
        &[
            "update",
            installation.flag(),
            "--noninteractive",
            "-y",
            app_id,
        ],
        progress,
    )
}

/// Uninstall the app; its data in `~/.var/app` stays.
pub fn uninstall(app_id: &str, installation: Installation) -> Result<(), HostFlatpakError> {
    let app_id = checked(app_id)?;
    if std::env::var("FLATPAK_ID").is_ok_and(|own| own == app_id) {
        return Err(HostFlatpakError::OwnApp);
    }
    output(
        "uninstall",
        &[
            "uninstall",
            installation.flag(),
            "--noninteractive",
            "-y",
            app_id,
        ],
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_apps_are_read_from_flatpak_list() {
        let apps = parse_installed(
            "com.github.flxzt.rnote\tRnote\tsystem\tflathub\n\
             org.example.Game\t\tuser\tflathub\n\
             not an id\tBad\tsystem\tflathub\n\
             \n",
        );
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].name, "Rnote");
        assert_eq!(apps[0].installation, Installation::System);
        assert_eq!(apps[1].name, "org.example.Game");
        assert_eq!(apps[1].installation, Installation::User);
    }

    #[test]
    fn progress_is_the_last_percentage_on_a_line() {
        assert_eq!(percent("Installing 1/2… 45%  3.1 MB/s"), Some(45));
        assert_eq!(percent("Downloading 10% then 100%"), Some(100));
        assert_eq!(percent("Installing org.example.App"), None);
        assert_eq!(percent("300%"), None);
    }

    #[test]
    fn nothing_runs_for_an_unchecked_app_id() {
        let mut ignore = |_| {};
        for bad in ["", "--help", "org.example.App; rm -rf ~", "../x.y.z"] {
            assert!(matches!(
                install(bad, &mut ignore),
                Err(HostFlatpakError::InvalidAppId)
            ));
            assert!(matches!(
                uninstall(bad, Installation::User),
                Err(HostFlatpakError::InvalidAppId)
            ));
            assert!(matches!(run(bad), Err(HostFlatpakError::InvalidAppId)));
        }
    }

    #[test]
    fn running_apps_are_read_from_flatpak_ps() {
        let running = parse_running("org.example.Game\nio.github.Mars7x.Horizon\nnot an id\n\n");
        assert_eq!(
            running.into_iter().collect::<Vec<_>>(),
            vec!["io.github.Mars7x.Horizon", "org.example.Game"]
        );
    }

    #[test]
    fn errors_keep_flatpaks_last_word() {
        assert_eq!(
            last_line("warning\nerror: No remote refs found\n\n"),
            "error: No remote refs found"
        );
        assert_eq!(last_line(""), "no message");
    }
}
