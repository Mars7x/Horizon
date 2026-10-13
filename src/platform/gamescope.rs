//! The one owner of gamescope control in the session (ADR 0189,
//! docs/GAMESCOPE_SESSION.md). Gamescope is steered with X11 properties: an
//! app ID on each window (`STEAM_GAME`) and, on the root window, which app is
//! shown (`GAMESCOPECTRL_BASELAYER_APPID`).

use slint::winit_030::{WinitWindowAccessor, winit};
use thiserror::Error;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::{
    connection::Connection,
    errors::{ConnectError, ConnectionError, ReplyError},
    protocol::xproto::{AtomEnum, ConnectionExt, PropMode, Window},
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

use crate::domain::GameId;

/// Horizon's own app ID: the base of a block reserved for Horizon, far above
/// real Steam app IDs and below Steam's shortcut range (2^31 and up). 769 is
/// Steam's own and is never Horizon's.
pub const HORIZON_APP_ID: u32 = 0x7F00_0000;
/// Steam's app ID for its own windows.
pub const STEAM_APP_ID: u32 = 769;
const STEAM_SHORTCUT_RANGE: u32 = 0x8000_0000;

/// The app ID Horizon gives a Heroic or Flatpak game's window: the base plus
/// the game's database ID. `None` if that would leave the reserved block.
pub fn game_app_id(game: GameId) -> Option<u32> {
    let id = u32::try_from(game.get()).ok()?;
    HORIZON_APP_ID
        .checked_add(id)
        .filter(|app_id| *app_id < STEAM_SHORTCUT_RANGE)
}

#[derive(Debug, Error)]
pub enum GamescopeError {
    #[error("can't connect to gamescope's X11 display: {0}")]
    Connect(#[from] ConnectError),
    #[error("gamescope's X11 connection failed: {0}")]
    Connection(#[from] ConnectionError),
    #[error("gamescope's X11 display refused a request: {0}")]
    Reply(#[from] ReplyError),
}

/// A connection to gamescope's X11 display (`DISPLAY`).
pub struct Gamescope {
    connection: RustConnection,
    root: Window,
    steam_game: u32,
    base_layer: u32,
}

impl Gamescope {
    pub fn connect() -> Result<Self, GamescopeError> {
        let (connection, screen) = x11rb::connect(None)?;
        let root = connection.setup().roots[screen].root;
        let steam_game = intern(&connection, b"STEAM_GAME")?;
        let base_layer = intern(&connection, b"GAMESCOPECTRL_BASELAYER_APPID")?;
        Ok(Self {
            connection,
            root,
            steam_game,
            base_layer,
        })
    }

    /// Gives a window its app ID. Gamescope never shows an untagged window.
    pub fn tag_window(&self, window: Window, app_id: u32) -> Result<(), GamescopeError> {
        self.connection.change_property32(
            PropMode::REPLACE,
            window,
            self.steam_game,
            AtomEnum::CARDINAL,
            &[app_id],
        )?;
        self.connection.flush()?;
        Ok(())
    }

    /// Shows the app with this ID; nothing is shown while it has no window.
    pub fn set_base_layer(&self, app_id: u32) -> Result<(), GamescopeError> {
        self.connection.change_property32(
            PropMode::REPLACE,
            self.root,
            self.base_layer,
            AtomEnum::CARDINAL,
            &[app_id],
        )?;
        self.connection.flush()?;
        Ok(())
    }

    /// Makes Horizon's window the one gamescope shows.
    pub fn show_horizon(&self, window: Window) -> Result<(), GamescopeError> {
        self.tag_window(window, HORIZON_APP_ID)?;
        self.set_base_layer(HORIZON_APP_ID)
    }
}

fn intern(connection: &RustConnection, name: &[u8]) -> Result<u32, GamescopeError> {
    Ok(connection.intern_atom(false, name)?.reply()?.atom)
}

/// The X11 window behind a Slint window, once winit has created it.
pub fn x11_window(window: &slint::Window) -> Option<Window> {
    window
        .with_winit_window(|window| match window.window_handle().ok()?.as_raw() {
            RawWindowHandle::Xlib(handle) => Window::try_from(handle.window).ok(),
            RawWindowHandle::Xcb(handle) => Some(handle.window.get()),
            _ => None,
        })
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: i64) -> GameId {
        GameId::new(id).unwrap()
    }

    #[test]
    fn horizon_is_not_steam() {
        assert_ne!(HORIZON_APP_ID, STEAM_APP_ID);
    }

    #[test]
    fn games_follow_horizon_in_its_block() {
        assert_eq!(game_app_id(game(1)), Some(0x7F00_0001));
        assert_eq!(game_app_id(game(42)), Some(0x7F00_002A));
        assert_eq!(game_app_id(game(0x00FF_FFFF)), Some(0x7FFF_FFFF));
    }

    #[test]
    fn ids_never_reach_steam_shortcuts() {
        assert_eq!(game_app_id(game(0x0100_0000)), None);
        assert_eq!(game_app_id(game(i64::MAX)), None);
    }
}
