//! Whether Horizon runs as a desktop app or as the gamescope session
//! (`--session`, docs/GAMESCOPE_SESSION.md), and what that session offers.
//!
//! Pages ask [`SessionCapabilities`] what exists; they never check the mode
//! themselves.

use std::ffi::OsStr;

use thiserror::Error;

pub const SESSION_ARG: &str = "--session";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionMode {
    /// A window on a desktop, as Horizon has always run.
    #[default]
    Desktop,
    /// The whole screen, as gamescope's main app.
    Gamescope,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ArgsError {
    #[error("unknown argument {0:?}; the only option is {SESSION_ARG}")]
    Unknown(String),
}

impl SessionMode {
    /// Reads the command-line arguments after the program name.
    /// `--session` is the only switch; anything else is refused rather than
    /// guessed at.
    pub fn from_args<I, S>(args: I) -> Result<Self, ArgsError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut mode = Self::Desktop;
        for arg in args {
            let arg = arg.as_ref();
            if arg == SESSION_ARG {
                mode = Self::Gamescope;
            } else {
                return Err(ArgsError::Unknown(arg.to_string_lossy().into_owned()));
            }
        }
        Ok(mode)
    }
}

/// What the running session offers. Each answer is added with the feature
/// that needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionCapabilities {
    mode: SessionMode,
}

impl SessionCapabilities {
    pub fn new(mode: SessionMode) -> Self {
        Self { mode }
    }

    /// Horizon decides what gamescope shows (window tags, base layer).
    pub fn controls_gamescope(self) -> bool {
        self.mode == SessionMode::Gamescope
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_arguments_is_the_desktop() {
        assert_eq!(
            SessionMode::from_args(Vec::<&str>::new()),
            Ok(SessionMode::Desktop)
        );
    }

    #[test]
    fn session_switch_selects_gamescope() {
        assert_eq!(
            SessionMode::from_args(["--session"]),
            Ok(SessionMode::Gamescope)
        );
        assert_eq!(
            SessionMode::from_args(["--session", "--session"]),
            Ok(SessionMode::Gamescope)
        );
    }

    #[test]
    fn unknown_arguments_are_refused() {
        assert_eq!(
            SessionMode::from_args(["--sesion"]),
            Err(ArgsError::Unknown("--sesion".into()))
        );
        assert_eq!(
            SessionMode::from_args(["--session", "steam://rungameid/70"]),
            Err(ArgsError::Unknown("steam://rungameid/70".into()))
        );
    }

    #[test]
    fn only_the_session_controls_gamescope() {
        assert!(!SessionCapabilities::new(SessionMode::Desktop).controls_gamescope());
        assert!(SessionCapabilities::new(SessionMode::Gamescope).controls_gamescope());
    }
}
