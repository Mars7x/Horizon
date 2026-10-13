//! The one owner of gamescope control in the session (ADR 0189,
//! docs/GAMESCOPE_SESSION.md). Gamescope is steered with X11 properties: an
//! app ID on each window (`STEAM_GAME`) and, on the root window, which app is
//! shown (`GAMESCOPECTRL_BASELAYER_APPID`).
//!
//! After a launch, [`SessionFront`] brings the game's window to the front
//! and Horizon back when it is gone ([`GameFront`] holds the rules).

use std::{
    cell::RefCell,
    collections::BTreeSet,
    rc::{Rc, Weak},
    time::Duration,
};

use slint::{
    Timer, TimerMode,
    winit_030::{WinitWindowAccessor, winit},
};
use thiserror::Error;
use tracing::{debug, info, warn};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::{
    connection::Connection,
    errors::{ConnectError, ConnectionError, ReplyError},
    protocol::xproto::{AtomEnum, ConnectionExt, MapState, PropMode, Window},
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

use crate::{domain::GameId, services::launch::LaunchListener};

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

    /// The mapped top-level windows and their app IDs. A window that goes
    /// away while it is read is left out.
    pub fn top_level_windows(&self) -> Result<Vec<TopWindow>, GamescopeError> {
        let children = self.connection.query_tree(self.root)?.reply()?.children;
        let cookies = children
            .iter()
            .map(|&window| {
                Ok((
                    window,
                    self.connection.get_window_attributes(window)?,
                    self.connection.get_property(
                        false,
                        window,
                        self.steam_game,
                        AtomEnum::CARDINAL,
                        0,
                        1,
                    )?,
                ))
            })
            .collect::<Result<Vec<_>, ConnectionError>>()?;
        let mut windows = Vec::new();
        for (window, attributes, app_id) in cookies {
            let (Ok(attributes), Ok(app_id)) = (attributes.reply(), app_id.reply()) else {
                continue;
            };
            if attributes.map_state != MapState::VIEWABLE {
                continue;
            }
            windows.push(TopWindow {
                window,
                app_id: app_id.value32().and_then(|mut values| values.next()),
                override_redirect: attributes.override_redirect,
            });
        }
        Ok(windows)
    }
}

/// A mapped window directly under the root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopWindow {
    pub window: Window,
    pub app_id: Option<u32>,
    /// Menus and tooltips; never a game's main window.
    pub override_redirect: bool,
}

/// What [`GameFront`] asks gamescope to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontAction {
    Tag(Window, u32),
    BaseLayer(u32),
}

/// How a launched game's window is recognised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameTarget {
    /// The source tags it (Steam) with this app ID.
    SourceTags(u32),
    /// Horizon tags new untagged windows with this app ID.
    HorizonTags(u32),
}

impl GameTarget {
    fn app_id(self) -> u32 {
        match self {
            Self::SourceTags(app_id) | Self::HorizonTags(app_id) => app_id,
        }
    }
}

/// Polls between window checks.
pub const FRONT_POLL: Duration = Duration::from_millis(250);
/// How long to wait for a launched game's first window (3 minutes).
const WAIT_POLLS: u32 = 720;
/// How long a game may have no window before Horizon returns (2 seconds):
/// games often swap windows while they start.
const GRACE_POLLS: u32 = 8;

/// The rules for which app gamescope shows after a launch: the game once
/// its window appears, Horizon again once the game has had no window for a
/// short while.
#[derive(Debug, Default)]
pub struct GameFront {
    state: FrontState,
}

#[derive(Debug, Default)]
enum FrontState {
    #[default]
    Horizon,
    Waiting {
        target: GameTarget,
        before: BTreeSet<Window>,
        polls: u32,
    },
    Game {
        target: GameTarget,
        before: BTreeSet<Window>,
        missing: u32,
    },
}

impl GameFront {
    pub fn is_horizon(&self) -> bool {
        matches!(self.state, FrontState::Horizon)
    }

    /// A game was launched; `windows` are the windows that existed before,
    /// none of which can be the game's.
    pub fn launched(&mut self, target: GameTarget, windows: &[TopWindow]) {
        self.state = FrontState::Waiting {
            target,
            before: windows.iter().map(|window| window.window).collect(),
            polls: 0,
        };
    }

    pub fn poll(&mut self, windows: &[TopWindow]) -> Vec<FrontAction> {
        let mut actions = Vec::new();
        let (target, before) = match &self.state {
            FrontState::Horizon => return actions,
            FrontState::Waiting { target, before, .. }
            | FrontState::Game { target, before, .. } => (*target, before),
        };
        let app_id = target.app_id();
        let mut present = windows.iter().any(|window| window.app_id == Some(app_id));
        if let GameTarget::HorizonTags(app_id) = target {
            for window in windows {
                if window.app_id.is_none()
                    && !window.override_redirect
                    && !before.contains(&window.window)
                {
                    actions.push(FrontAction::Tag(window.window, app_id));
                    present = true;
                }
            }
        }

        self.state = match std::mem::take(&mut self.state) {
            FrontState::Waiting {
                target,
                before,
                polls,
            } => {
                if present {
                    actions.push(FrontAction::BaseLayer(app_id));
                    FrontState::Game {
                        target,
                        before,
                        missing: 0,
                    }
                } else if polls + 1 >= WAIT_POLLS {
                    FrontState::Horizon
                } else {
                    FrontState::Waiting {
                        target,
                        before,
                        polls: polls + 1,
                    }
                }
            }
            FrontState::Game {
                target,
                before,
                missing,
            } => {
                if present {
                    FrontState::Game {
                        target,
                        before,
                        missing: 0,
                    }
                } else if missing + 1 >= GRACE_POLLS {
                    actions.push(FrontAction::BaseLayer(HORIZON_APP_ID));
                    FrontState::Horizon
                } else {
                    FrontState::Game {
                        target,
                        before,
                        missing: missing + 1,
                    }
                }
            }
            FrontState::Horizon => FrontState::Horizon,
        };
        actions
    }
}

type ShownCallback = Box<dyn Fn(bool)>;

/// Applies [`GameFront`] to gamescope after each launch in the session.
pub struct SessionFront {
    gamescope: Gamescope,
    front: RefCell<GameFront>,
    timer: Timer,
    this: Weak<SessionFront>,
    on_shown: RefCell<Option<ShownCallback>>,
}

impl SessionFront {
    pub fn new(gamescope: Gamescope) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            gamescope,
            front: RefCell::default(),
            timer: Timer::default(),
            this: this.clone(),
            on_shown: RefCell::new(None),
        })
    }

    /// Called with `true` when gamescope shows Horizon again and `false`
    /// when it shows a game instead.
    pub fn on_horizon_shown(&self, callback: impl Fn(bool) + 'static) {
        *self.on_shown.borrow_mut() = Some(Box::new(callback));
    }

    pub fn gamescope(&self) -> &Gamescope {
        &self.gamescope
    }

    fn poll(&self) {
        let windows = match self.gamescope.top_level_windows() {
            Ok(windows) => windows,
            Err(error) => {
                warn!(%error, "couldn't read gamescope's windows");
                return;
            }
        };
        let actions = self.front.borrow_mut().poll(&windows);
        for action in actions {
            let result = match action {
                FrontAction::Tag(window, app_id) => {
                    debug!(window, app_id, "tagging a new window as the game's");
                    self.gamescope.tag_window(window, app_id)
                }
                FrontAction::BaseLayer(app_id) => {
                    info!(app_id, "gamescope shows a new app");
                    if let Some(callback) = &*self.on_shown.borrow() {
                        callback(app_id == HORIZON_APP_ID);
                    }
                    self.gamescope.set_base_layer(app_id)
                }
            };
            if let Err(error) = result {
                warn!(%error, ?action, "gamescope didn't take a change");
            }
        }
        if self.front.borrow().is_horizon() {
            self.timer.stop();
        }
    }
}

impl LaunchListener for SessionFront {
    fn game_launched(&self, game: GameId, window_app_id: Option<u32>) {
        let target = match (window_app_id, game_app_id(game)) {
            (Some(app_id), _) => GameTarget::SourceTags(app_id),
            (None, Some(app_id)) => GameTarget::HorizonTags(app_id),
            (None, None) => {
                warn!(
                    game = game.get(),
                    "no app ID for this game; gamescope won't show it"
                );
                return;
            }
        };
        let windows = self.gamescope.top_level_windows().unwrap_or_else(|error| {
            warn!(%error, "couldn't read gamescope's windows before the launch");
            Vec::new()
        });
        debug!(game = game.get(), ?target, "waiting for the game's window");
        self.front.borrow_mut().launched(target, &windows);
        let this = self.this.clone();
        self.timer.start(TimerMode::Repeated, FRONT_POLL, move || {
            if let Some(front) = this.upgrade() {
                front.poll();
            }
        });
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

    fn window(window: Window, app_id: Option<u32>) -> TopWindow {
        TopWindow {
            window,
            app_id,
            override_redirect: false,
        }
    }

    const HORIZON: Window = 1;

    #[test]
    fn steam_game_is_shown_once_steam_tags_it() {
        let mut front = GameFront::default();
        let horizon = window(HORIZON, Some(HORIZON_APP_ID));
        front.launched(GameTarget::SourceTags(70), &[horizon]);
        assert!(front.poll(&[horizon, window(5, None)]).is_empty());
        assert_eq!(
            front.poll(&[horizon, window(5, Some(70))]),
            [FrontAction::BaseLayer(70)]
        );
        assert!(!front.is_horizon());
    }

    #[test]
    fn horizon_tags_only_new_untagged_windows() {
        let mut front = GameFront::default();
        let horizon = window(HORIZON, Some(HORIZON_APP_ID));
        let older = window(2, None);
        let menu = TopWindow {
            window: 3,
            app_id: None,
            override_redirect: true,
        };
        let steam = window(4, Some(STEAM_APP_ID));
        front.launched(GameTarget::HorizonTags(0x7F00_0005), &[horizon, older]);
        assert_eq!(
            front.poll(&[horizon, older, menu, steam, window(9, None)]),
            [
                FrontAction::Tag(9, 0x7F00_0005),
                FrontAction::BaseLayer(0x7F00_0005)
            ]
        );
        // A second window the game opens later is tagged too.
        assert_eq!(
            front.poll(&[horizon, window(9, Some(0x7F00_0005)), window(10, None)]),
            [FrontAction::Tag(10, 0x7F00_0005)]
        );
    }

    #[test]
    fn horizon_returns_after_the_grace_period() {
        let mut front = GameFront::default();
        let horizon = window(HORIZON, Some(HORIZON_APP_ID));
        front.launched(GameTarget::SourceTags(70), &[horizon]);
        front.poll(&[horizon, window(5, Some(70))]);
        for _ in 1..GRACE_POLLS {
            assert!(front.poll(&[horizon]).is_empty());
        }
        // The game's window coming back inside the grace period keeps it.
        assert!(front.poll(&[horizon, window(6, Some(70))]).is_empty());
        for _ in 1..GRACE_POLLS {
            assert!(front.poll(&[horizon]).is_empty());
        }
        assert_eq!(
            front.poll(&[horizon]),
            [FrontAction::BaseLayer(HORIZON_APP_ID)]
        );
        assert!(front.is_horizon());
    }

    #[test]
    fn waiting_gives_up_without_moving_the_screen() {
        let mut front = GameFront::default();
        let horizon = window(HORIZON, Some(HORIZON_APP_ID));
        front.launched(GameTarget::SourceTags(70), &[horizon]);
        for _ in 0..WAIT_POLLS {
            assert!(front.poll(&[horizon]).is_empty());
        }
        assert!(front.is_horizon());
    }

    #[test]
    fn ids_never_reach_steam_shortcuts() {
        assert_eq!(game_app_id(game(0x0100_0000)), None);
        assert_eq!(game_app_id(game(i64::MAX)), None);
    }
}
