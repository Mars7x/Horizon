//! Rust owns player mode/focus. Slint only presents the latest software frame.
use crate::{
    AppWindow,
    input::{UiAction, UiActionEvent},
    presentation::CallbackSlot,
    services::{
        gba_player::{Command, Event, PlayerPaths, RunningGame},
        launch::{LaunchExecutionError, LaunchExecutor},
    },
    sources::SourceLaunchTarget,
};
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::atomic::Ordering,
    time::Duration,
};

pub struct GbaPlayer {
    ui: slint::Weak<AppWindow>,
    paths: PlayerPaths,
    running: RefCell<Option<RunningGame>>,
    keyboard: Cell<u16>,
    pad_ready: Cell<bool>,
    gamepads: CallbackSlot<dyn Fn() -> u16>,
    timer: Timer,
    closing: Cell<bool>,
}
impl GbaPlayer {
    pub fn new(ui: &AppWindow, paths: PlayerPaths) -> Rc<Self> {
        let player = Rc::new(Self {
            ui: ui.as_weak(),
            paths,
            running: RefCell::new(None),
            keyboard: Cell::new(0),
            pad_ready: Cell::new(false),
            gamepads: RefCell::new(None),
            timer: Timer::default(),
            closing: Cell::new(false),
        });
        let weak = Rc::downgrade(&player);
        ui.on_player_choose(move |index| {
            if let Some(player) = weak.upgrade() {
                player.choose(index);
            }
        });
        let weak = Rc::downgrade(&player);
        player
            .timer
            .start(TimerMode::Repeated, Duration::from_millis(16), move || {
                if let Some(player) = weak.upgrade() {
                    player.tick();
                }
            });
        player.timer.stop();
        player
    }
    pub fn set_gamepads(&self, source: Rc<dyn Fn() -> u16>) {
        *self.gamepads.borrow_mut() = Some(source);
    }
    pub fn is_open(&self) -> bool {
        self.ui.upgrade().is_some_and(|ui| ui.get_player_open())
    }
    pub fn pause(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if !ui.get_player_open() || ui.get_player_failed() {
            return;
        }
        ui.set_player_paused(true);
        ui.set_player_focus(0);
        self.keyboard.set(0);
        self.pad_ready.set(false);
        if let Some(game) = self.running.borrow().as_ref() {
            game.buttons.store(0, Ordering::Release);
            let _ = game.commands.send(Command::Pause(true));
        }
    }
    pub fn keyboard(&self, text: &str, repeated: bool, pressed: bool) -> bool {
        let Some(ui) = self.ui.upgrade() else {
            return false;
        };
        if !ui.get_player_open() {
            return false;
        }
        if pressed
            && !repeated
            && (text == slint::SharedString::from(slint::platform::Key::Escape).as_str()
                || text == slint::SharedString::from(slint::platform::Key::F10).as_str())
        {
            if ui.get_player_busy() {
                self.choose(1);
            } else if ui.get_player_paused() || ui.get_player_failed() {
                self.choose(0);
            } else {
                self.pause();
            }
            return true;
        }
        if ui.get_player_paused() || ui.get_player_failed() {
            if pressed && let Some(action) = crate::input::action_for_key(text, repeated) {
                self.handle_action(UiActionEvent { action, repeated });
            }
            return true;
        }
        if let Some(bit) = keyboard_button(text) {
            let mask = 1 << bit;
            self.keyboard.set(if pressed {
                self.keyboard.get() | mask
            } else {
                self.keyboard.get() & !mask
            });
        }
        true
    }
    pub fn handle_action(&self, event: UiActionEvent) -> bool {
        let Some(ui) = self.ui.upgrade() else {
            return false;
        };
        if !ui.get_player_open() {
            return false;
        }
        if ui.get_player_busy() {
            return true;
        }
        if ui.get_player_failed() {
            if matches!(
                event.action,
                UiAction::Accept | UiAction::Back | UiAction::Home
            ) && !event.repeated
            {
                self.choose(0);
            }
            return true;
        }
        if ui.get_player_paused() {
            match event.action {
                UiAction::Left | UiAction::Up => ui.set_player_focus(0),
                UiAction::Right | UiAction::Down => ui.set_player_focus(1),
                UiAction::Accept if !event.repeated => self.choose(ui.get_player_focus()),
                UiAction::Back | UiAction::Home if !event.repeated => self.choose(0),
                _ => {}
            }
        } else if matches!(event.action, UiAction::Home | UiAction::StickPress) && !event.repeated {
            self.pause();
        }
        // Game B/Start and menu repeats never navigate the shell underneath.
        true
    }
    fn choose(&self, index: i32) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if !ui.get_player_open() {
            return;
        }
        if ui.get_player_failed() {
            self.close(&ui);
            return;
        }
        if ui.get_player_busy() && index == 0 {
            return;
        }
        ui.set_player_focus(index.clamp(0, 1));
        if index == 0 && ui.get_player_paused() {
            self.closing.set(false);
            self.keyboard.set(0);
            self.pad_ready.set(false);
            ui.set_player_paused(false);
            ui.set_player_message("".into());
            if let Some(game) = self.running.borrow().as_ref() {
                let _ = game.commands.send(Command::Pause(false));
            }
        } else if index == 1 {
            ui.set_player_busy(true);
            ui.set_player_message("Saving game…".into());
            if let Some(game) = self.running.borrow().as_ref() {
                let _ = game.commands.send(Command::Stop);
            } else {
                self.close(&ui);
            }
        }
    }
    fn close(&self, ui: &AppWindow) {
        self.timer.stop();
        self.running.borrow_mut().take();
        self.keyboard.set(0);
        self.pad_ready.set(false);
        ui.set_player_open(false);
        ui.set_player_frame(Image::default());
        ui.set_player_has_frame(false);
        if self.closing.get()
            && let Err(error) = slint::quit_event_loop()
        {
            tracing::warn!(%error,"could not close Horizon after saving");
        }
    }
    fn tick(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let mut close = false;
        {
            let running = self.running.borrow();
            let Some(game) = running.as_ref() else {
                return;
            };
            while let Ok(event) = game.events.try_recv() {
                match event {
                    Event::Ready(title) | Event::Started(title) => {
                        ui.set_player_title(super::titles::game_title(&title).into());
                        ui.set_player_busy(false);
                        ui.set_player_message("".into());
                    }
                    Event::Stopped => close = true,
                    Event::Failed(error) => {
                        self.timer.stop();
                        ui.set_player_focus(0);
                        ui.set_player_failed(true);
                        ui.set_player_busy(false);
                        ui.set_player_message(error.into());
                    }
                    Event::SaveFailed(error) => {
                        ui.set_player_paused(true);
                        ui.set_player_busy(false);
                        ui.set_player_message(error.into());
                        self.keyboard.set(0);
                        self.pad_ready.set(false);
                    }
                }
            }
            if game.finished() && !ui.get_player_failed() && !close {
                ui.set_player_failed(true);
                ui.set_player_busy(false);
                ui.set_player_message("The GBA session stopped unexpectedly".into());
            }
            if let Ok(mut frame) = game.frame.lock()
                && let Some(frame) = frame.take()
            {
                ui.set_player_frame(Image::from_rgba8(
                    SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                        &frame.rgba,
                        frame.width,
                        frame.height,
                    ),
                ));
                ui.set_player_has_frame(true);
            }
            let pad = self.gamepads.borrow().as_ref().map_or(0, |source| source());
            if pad == 0 {
                self.pad_ready.set(true);
            }
            let buttons =
                if ui.get_player_paused() || ui.get_player_busy() || ui.get_player_failed() {
                    0
                } else {
                    self.keyboard.get() | if self.pad_ready.get() { pad } else { 0 }
                };
            game.buttons.store(buttons, Ordering::Release);
        }
        if close {
            self.close(&ui);
        }
    }
    pub fn shutdown(&self) -> Result<(), String> {
        if let Some(mut game) = self.running.borrow_mut().take() {
            game.shutdown()
        } else {
            Ok(())
        }
    }
    pub fn request_close(&self) -> slint::CloseRequestResponse {
        if !self.is_open() {
            return slint::CloseRequestResponse::HideWindow;
        }
        self.closing.set(true);
        self.choose(1);
        slint::CloseRequestResponse::KeepWindowShown
    }
}
impl LaunchExecutor for GbaPlayer {
    fn execute(&self, target: &SourceLaunchTarget) -> Result<(), LaunchExecutionError> {
        let SourceLaunchTarget::EmbeddedGba { sha256 } = target else {
            return crate::platform::launcher::PortalLaunchExecutor.execute(target);
        };
        let result = (|| -> Result<(), String> {
            if self.is_open() {
                return Err("Close the current game first".into());
            }
            let Some(ui) = self.ui.upgrade() else {
                return Err("Horizon is closing".into());
            };
            let running = RunningGame::start(self.paths.clone(), sha256.clone())?;
            *self.running.borrow_mut() = Some(running);
            self.timer.restart();
            self.keyboard.set(0);
            self.pad_ready.set(false);
            ui.set_player_title("Starting game…".into());
            ui.set_player_message("".into());
            ui.set_player_busy(true);
            ui.set_player_failed(false);
            ui.set_player_paused(false);
            ui.set_player_focus(0);
            ui.set_player_open(true);
            Ok(())
        })();
        result.map_err(|error| LaunchExecutionError::new(std::io::Error::other(error)))
    }
}
fn keyboard_button(text: &str) -> Option<u32> {
    use slint::{SharedString, platform::Key};
    for (key, bit) in [
        (Key::UpArrow, 4),
        (Key::DownArrow, 5),
        (Key::LeftArrow, 6),
        (Key::RightArrow, 7),
        (Key::Return, 3),
        (Key::Backspace, 2),
    ] {
        if text == SharedString::from(key).as_str() {
            return Some(bit);
        }
    }
    match text.to_ascii_lowercase().as_str() {
        "z" => Some(8),
        "x" => Some(0),
        "a" => Some(10),
        "s" => Some(11),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn game_keys_have_separate_start_select_and_shoulders() {
        assert_eq!(keyboard_button("Z"), Some(8));
        assert_eq!(keyboard_button("x"), Some(0));
        assert_eq!(keyboard_button("a"), Some(10));
        assert_eq!(keyboard_button("s"), Some(11));
        assert_eq!(
            keyboard_button(slint::SharedString::from(slint::platform::Key::Return).as_str()),
            Some(3)
        );
        assert_eq!(
            keyboard_button(slint::SharedString::from(slint::platform::Key::Backspace).as_str()),
            Some(2)
        );
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::*;
    use crate::{
        persistence::SqliteLibraryRepository,
        services::{
            library::LibraryRepository,
            rom_import::{self, RomSetup},
        },
    };
    use std::path::PathBuf;
    #[test]
    #[ignore = "requires Wayland, PipeWire and built mGBA core"]
    fn real_player_window_input_pause_quit_and_relaunch() {
        let directory = PathBuf::from(
            std::env::var_os("HORIZON_GBA_TEST_DIR").expect("set a fresh HORIZON_GBA_TEST_DIR"),
        );
        std::fs::create_dir_all(&directory).unwrap();
        let rom = crate::platform::gba_core::test_rom();
        std::fs::write(directory.join("Horizon homebrew (USA, Europe).gba"), rom).unwrap();
        let database = directory.join("library.sqlite3");
        let mut repository = SqliteLibraryRepository::open(&database).unwrap();
        let scan = rom_import::scan_folder(&directory, None).unwrap();
        repository
            .save_rom_import(&RomSetup::default(), &scan.candidates)
            .unwrap();
        let hash = scan.candidates[0]
            .fingerprint
            .as_ref()
            .unwrap()
            .sha256
            .clone();
        let game_id = repository.list_games().unwrap()[0].game().id();
        let saves = directory.join("saves");
        let ui = AppWindow::new().unwrap();
        let player = GbaPlayer::new(
            &ui,
            PlayerPaths {
                database: database.clone(),
                core: std::env::var_os("HORIZON_GBA_CORE").unwrap().into(),
                saves: saves.clone(),
            },
        );
        let games = repository.list_games().unwrap();
        let catalog = crate::presentation::catalog::Catalog::shared(games);
        let index = catalog.borrow().position(game_id).unwrap();
        let registry = Rc::new(crate::sources::production_source_registry().unwrap());
        let artwork =
            crate::services::artwork::ArtworkService::new(Rc::clone(&registry), None, None);
        let activity = Rc::new(crate::services::activity::ActivityService::new(Rc::new(
            RefCell::new(repository),
        )));
        let launch = Rc::new(crate::services::launch::GameLaunchService::new(
            Rc::clone(&registry),
            player.clone(),
        ));
        let home = crate::presentation::home::HomeController::new(
            &ui,
            catalog,
            &[game_id],
            &artwork,
            &registry,
            launch,
            activity,
        );
        let keys = Rc::clone(&player);
        ui.on_raw_key_input(move |text, repeat| keys.keyboard(&text, repeat, true));
        let keys = Rc::clone(&player);
        ui.on_raw_key_released(move |text| keys.keyboard(&text, false, false));
        ui.show().unwrap();
        home.handle_action(&ui, UiActionEvent::fresh(UiAction::Accept));
        assert!(ui.get_player_open());
        let stage = Rc::new(Cell::new(0));
        let ticks = Rc::new(Cell::new(0));
        let done = Rc::new(Cell::new(false));
        let timed_out = Rc::new(Cell::new(false));
        let timer = Timer::default();
        let (progress, count, finished, expired) = (
            Rc::clone(&stage),
            Rc::clone(&ticks),
            Rc::clone(&done),
            Rc::clone(&timed_out),
        );
        let weak = ui.as_weak();
        let live = Rc::clone(&player);
        timer.start(TimerMode::Repeated, Duration::from_millis(150), move || {
            let ui = weak.upgrade().unwrap();
            count.set(count.get() + 1);
            if count.get() > 100 {
                expired.set(true);
                slint::quit_event_loop().unwrap();
                return;
            }
            assert!(!ui.get_player_failed(), "{}", ui.get_player_message());
            let key = |text: slint::SharedString, pressed| {
                use slint::platform::WindowEvent;
                ui.window()
                    .dispatch_event_with_result(if pressed {
                        WindowEvent::KeyPressed { text }
                    } else {
                        WindowEvent::KeyReleased { text }
                    })
                    .unwrap();
            };
            let tap = |keycode: slint::platform::Key| {
                key(keycode.into(), true);
                key(keycode.into(), false);
            };
            let snapshot = |name: &str| {
                let pixels = ui.window().take_snapshot().unwrap();
                image::save_buffer(
                    directory.join(format!("{name}.png")),
                    pixels.as_bytes(),
                    pixels.width(),
                    pixels.height(),
                    image::ColorType::Rgba8,
                )
                .unwrap();
                let center = pixels.as_slice()
                    [(pixels.height() / 2 * pixels.width() + pixels.width() / 2) as usize];
                [center.r, center.g, center.b]
            };
            match progress.get() {
                0 if ui.get_player_has_frame() && !ui.get_player_busy() => {
                    assert_eq!(ui.get_player_title(), "Horizon homebrew");
                    key("z".into(), true);
                    progress.set(1);
                }
                1 => {
                    let color = snapshot("gba-running");
                    assert!(color[0] > 180 && color[1] < 40, "{color:?}");
                    key("z".into(), false);
                    tap(slint::platform::Key::Escape);
                    assert!(ui.get_player_paused());
                    progress.set(2);
                }
                2 => {
                    snapshot("gba-paused");
                    tap(slint::platform::Key::RightArrow);
                    assert_eq!(ui.get_player_focus(), 1);
                    tap(slint::platform::Key::LeftArrow);
                    assert_eq!(ui.get_player_focus(), 0);
                    tap(slint::platform::Key::Return);
                    assert!(!ui.get_player_paused());
                    progress.set(3);
                }
                3 => {
                    assert_eq!(
                        live.running
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .buttons
                            .load(Ordering::Acquire),
                        0
                    );
                    if ui.get_player_frame().to_rgba8().unwrap().as_slice()[0].g > 180 {
                        // Snapshot capture can return the last presented frame;
                        // allow the green buffer to reach the compositor first.
                        progress.set(7);
                    }
                }
                7 => {
                    let color = snapshot("gba-released");
                    assert!(color[1] > 180 && color[0] < 40, "{color:?}");
                    assert!(live.handle_action(UiActionEvent::fresh(UiAction::Home)));
                    assert!(ui.get_player_paused());
                    live.handle_action(UiActionEvent::fresh(UiAction::Right));
                    live.handle_action(UiActionEvent::fresh(UiAction::Accept));
                    progress.set(4);
                }
                4 if !ui.get_player_open() => {
                    assert!(
                        !std::fs::read(saves.join(format!("{hash}.sav")))
                            .unwrap()
                            .is_empty()
                    );
                    let repository = SqliteLibraryRepository::open(&database).unwrap();
                    assert_eq!(repository.list_games().unwrap()[0].game().id(), game_id);
                    let activity = crate::services::activity::ActivityService::new(Rc::new(
                        RefCell::new(repository),
                    ));
                    assert_eq!(activity.overview(10, 10).unwrap().completed_sessions(), 1);
                    home.launch_from_library(&ui, index);
                    assert!(ui.get_player_open());
                    progress.set(5);
                }
                5 if ui.get_player_has_frame() && !ui.get_player_busy() => {
                    live.request_close();
                    progress.set(6);
                }
                6 if !ui.get_player_open() => {
                    finished.set(true);
                    slint::quit_event_loop().unwrap();
                }
                _ => {}
            }
            // request_close exits the loop as soon as the save completes.
            if progress.get() == 6 {
                finished.set(true);
            }
        });
        slint::run_event_loop().unwrap();
        player.shutdown().unwrap();
        assert!(done.get() && !timed_out.get());
    }
}
