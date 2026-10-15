//! Rust owns player mode/focus. Slint only presents the latest software frame.
use crate::{
    AppWindow,
    audio::UiSoundCue,
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
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlayerStage {
    Waiting,
    Entering,
    Playing,
    Exiting,
}

const RETURN_TO_HORIZON: i32 = 6;
const PAUSE_ACTIONS: [i32; 5] = [0, 1, 4, 5, RETURN_TO_HORIZON];
fn pause_step(current: i32, down: bool, repeated: bool) -> i32 {
    let index = PAUSE_ACTIONS
        .iter()
        .position(|&action| action == current)
        .unwrap_or(0);
    let next = crate::navigation::step_with_edge_wrap(
        index as i32,
        0,
        PAUSE_ACTIONS.len() as i32 - 1,
        if down { 1 } else { -1 },
        repeated,
    );
    PAUSE_ACTIONS[next as usize]
}

pub struct GbaPlayer {
    ui: slint::Weak<AppWindow>,
    paths: PlayerPaths,
    running: RefCell<Option<RunningGame>>,
    keyboard: Cell<u16>,
    pad_ready: Cell<bool>,
    gamepads: CallbackSlot<dyn Fn() -> u16>,
    sound: CallbackSlot<dyn Fn(UiSoundCue)>,
    timer: Timer,
    closing: Cell<bool>,
    foreground: Cell<bool>,
    freeze_when_unfocused: Cell<bool>,
    worker_paused: Cell<bool>,
    stage: Cell<PlayerStage>,
    stage_since: Cell<Instant>,
    first_frame_at: Cell<Option<Instant>>,
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
            sound: RefCell::new(None),
            timer: Timer::default(),
            closing: Cell::new(false),
            foreground: Cell::new(true),
            freeze_when_unfocused: Cell::new(true),
            worker_paused: Cell::new(false),
            stage: Cell::new(PlayerStage::Waiting),
            stage_since: Cell::new(Instant::now()),
            first_frame_at: Cell::new(None),
        });
        let weak = Rc::downgrade(&player);
        ui.on_player_choose(move |index| {
            if let Some(player) = weak.upgrade() {
                player.choose(index);
            }
        });
        let weak = Rc::downgrade(&player);
        ui.on_player_restart_answer(move |choice| {
            if let Some(player) = weak.upgrade() {
                player.restart_answer(choice);
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
    pub fn set_sound(&self, callback: Rc<dyn Fn(UiSoundCue)>) {
        *self.sound.borrow_mut() = Some(callback);
    }
    fn cue(&self, cue: UiSoundCue) {
        if let Some(sound) = self.sound.borrow().as_ref() {
            sound(cue);
        }
    }
    pub fn set_gamepads(&self, source: Rc<dyn Fn() -> u16>) {
        *self.gamepads.borrow_mut() = Some(source);
    }
    pub fn is_open(&self) -> bool {
        self.ui.upgrade().is_some_and(|ui| ui.get_player_open())
    }
    pub fn set_active(&self, active: bool) {
        self.foreground.set(active);
        self.keyboard.set(0);
        self.pad_ready.set(false);
        if let Some(ui) = self.ui.upgrade() {
            if let Some(game) = self.running.borrow().as_ref() {
                game.buttons.store(0, Ordering::Release);
            }
            self.sync_pause(&ui);
        }
    }
    fn sync_pause(&self, ui: &AppWindow) {
        let frozen = !self.foreground.get() && self.freeze_when_unfocused.get();
        ui.set_player_frozen(frozen);
        // Startup still needs one frame before the worker can safely freeze.
        if ui.get_player_open() && ui.get_player_has_frame() {
            let paused = ui.get_player_paused() || frozen;
            if paused != self.worker_paused.get()
                && let Some(game) = self.running.borrow().as_ref()
                && game.commands.send(Command::Pause(paused)).is_ok()
            {
                self.worker_paused.set(paused);
            }
        }
    }
    pub fn pause(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if !ui.get_player_open() || ui.get_player_failed() {
            return;
        }
        ui.set_player_paused(true);
        ui.set_player_restart_confirm(false);
        ui.set_player_focus(0);
        self.keyboard.set(0);
        self.pad_ready.set(false);
        if let Some(game) = self.running.borrow().as_ref() {
            game.buttons.store(0, Ordering::Release);
        }
        self.sync_pause(&ui);
    }

    pub fn keyboard(&self, text: &str, repeated: bool, pressed: bool) -> bool {
        let Some(ui) = self.ui.upgrade() else {
            return false;
        };
        if !ui.get_player_open() {
            return false;
        }
        if !self.foreground.get() || self.stage.get() == PlayerStage::Exiting {
            return true;
        }
        if pressed
            && !repeated
            && (text == slint::SharedString::from(slint::platform::Key::Escape).as_str()
                || text == slint::SharedString::from(slint::platform::Key::F10).as_str())
        {
            if ui.get_player_busy() {
                if !ui.get_player_paused() {
                    self.choose(RETURN_TO_HORIZON);
                }
            } else if ui.get_player_restart_confirm() {
                self.restart_answer(0);
            } else if ui.get_player_paused() || ui.get_player_failed() {
                self.choose(0);
            } else {
                self.pause();
            }
            return true;
        }
        if self.stage.get() != PlayerStage::Playing {
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
        let before = self
            .ui
            .upgrade()
            .map(|ui| (ui.get_player_focus(), ui.get_player_restart_focus()));
        let handled = self.handle_action_inner(event);
        if handled
            && matches!(
                event.action,
                UiAction::Up | UiAction::Down | UiAction::Left | UiAction::Right
            )
            && self.ui.upgrade().is_some_and(|ui| {
                Some((ui.get_player_focus(), ui.get_player_restart_focus())) != before
            })
        {
            self.cue(UiSoundCue::Navigation);
        }
        handled
    }
    fn handle_action_inner(&self, event: UiActionEvent) -> bool {
        let Some(ui) = self.ui.upgrade() else {
            return false;
        };
        if !ui.get_player_open() {
            return false;
        }
        if !self.foreground.get()
            || self.stage.get() != PlayerStage::Playing
            || ui.get_player_busy()
        {
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
            if ui.get_player_restart_confirm() {
                match event.action {
                    UiAction::Left | UiAction::Up => ui.set_player_restart_focus(0),
                    UiAction::Right | UiAction::Down => ui.set_player_restart_focus(1),
                    UiAction::Accept if !event.repeated => {
                        self.restart_answer(ui.get_player_restart_focus())
                    }
                    UiAction::Back | UiAction::Home if !event.repeated => self.restart_answer(0),
                    _ => {}
                }
                return true;
            }
            match event.action {
                UiAction::Up | UiAction::Down => ui.set_player_focus(pause_step(
                    ui.get_player_focus(),
                    event.action == UiAction::Down,
                    event.repeated,
                )),
                UiAction::Left | UiAction::Right
                    if ui.get_player_focus() == 1 && !event.repeated =>
                {
                    self.set_speed(&ui, event.action == UiAction::Right)
                }
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
            self.begin_exit(&ui);
            return;
        }
        if !PAUSE_ACTIONS.contains(&index) || (ui.get_player_busy() && index != RETURN_TO_HORIZON) {
            return;
        }
        ui.set_player_focus(index);
        if index != 1 {
            self.cue(if index == 0 {
                UiSoundCue::Back
            } else {
                UiSoundCue::Ok
            });
        }
        match index {
            0 if ui.get_player_paused() && self.foreground.get() => {
                self.closing.set(false);
                self.keyboard.set(0);
                self.pad_ready.set(false);
                ui.set_player_restart_confirm(false);
                ui.set_player_paused(false);
                ui.set_player_message("".into());
                self.sync_pause(&ui);
            }
            1 => self.set_speed(&ui, !ui.get_player_fast_forward()),
            4 => self.send_action(&ui, Command::Screenshot, "Taking screenshot"),
            5 => {
                ui.set_player_restart_confirm(true);
                ui.set_player_restart_focus(0);
                ui.set_player_message("".into());
            }
            RETURN_TO_HORIZON => {
                ui.set_player_busy(true);
                ui.set_player_message("Saving game".into());
                if let Some(game) = self.running.borrow().as_ref() {
                    let _ = game.commands.send(Command::Stop);
                } else {
                    self.begin_exit(&ui);
                }
            }
            _ => {}
        }
    }
    fn set_speed(&self, ui: &AppWindow, fast: bool) {
        if fast != ui.get_player_fast_forward() && !ui.get_player_busy() {
            self.cue(UiSoundCue::Ok);
            self.send_action(ui, Command::FastForward(fast), "");
        }
    }
    fn send_action(&self, ui: &AppWindow, command: Command, message: &str) {
        if ui.get_player_busy() {
            return;
        }
        let sent = self
            .running
            .borrow()
            .as_ref()
            .is_some_and(|game| game.commands.send(command).is_ok());
        ui.set_player_busy(sent);
        ui.set_player_message(
            if sent {
                message
            } else {
                "The game is no longer available"
            }
            .into(),
        );
    }
    fn restart_answer(&self, choice: i32) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if !ui.get_player_restart_confirm() || ui.get_player_busy() {
            return;
        }
        self.cue(if choice == 1 {
            UiSoundCue::Ok
        } else {
            UiSoundCue::Back
        });
        if choice == 1 {
            self.send_action(&ui, Command::Restart, "Restarting game");
        } else {
            ui.set_player_restart_confirm(false);
            ui.set_player_focus(5);
        }
    }
    fn release_card(ui: &AppWindow) {
        ui.set_launching_game_index(-1);
        ui.set_library_launching_index(-1);
    }
    fn set_stage(&self, ui: &AppWindow, stage: PlayerStage) {
        self.stage.set(stage);
        self.stage_since.set(Instant::now());
        ui.set_player_phase(match stage {
            PlayerStage::Waiting => 0,
            PlayerStage::Entering => 1,
            PlayerStage::Playing => 2,
            PlayerStage::Exiting => 3,
        });
    }
    fn zoom_duration(ui: &AppWindow) -> Duration {
        Duration::from_millis(
            ui.global::<crate::Motion>()
                .get_player_zoom_duration()
                .max(0) as u64,
        )
    }
    fn begin_exit(&self, ui: &AppWindow) {
        if self.stage.get() == PlayerStage::Exiting {
            return;
        }
        self.keyboard.set(0);
        self.pad_ready.set(false);
        if self.closing.get() || self.stage.get() == PlayerStage::Waiting {
            self.finish_close(ui);
            return;
        }
        if ui.get_current_route() == crate::AppRouteView::Library {
            ui.set_library_launching_index(ui.get_library_selected_index());
        } else {
            ui.set_launching_game_index(ui.get_selected_index());
        }
        ui.set_player_origin(ui.invoke_player_capture_origin());
        ui.set_player_paused(false);
        self.set_stage(ui, PlayerStage::Exiting);
        self.timer.restart();
        if Self::zoom_duration(ui).is_zero() {
            self.finish_close(ui);
        }
    }
    fn finish_close(&self, ui: &AppWindow) {
        self.timer.stop();
        Self::release_card(ui);
        self.set_stage(ui, PlayerStage::Waiting);
        self.running.borrow_mut().take();
        self.keyboard.set(0);
        self.pad_ready.set(false);
        ui.set_player_open(false);
        ui.set_player_frozen(false);
        self.worker_paused.set(false);
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
        if self.stage.get() == PlayerStage::Exiting {
            if self.stage_since.get().elapsed()
                >= Self::zoom_duration(&ui) + Duration::from_millis(16)
            {
                self.finish_close(&ui);
            }
            return;
        }
        let mut close = false;
        {
            let running = self.running.borrow();
            let Some(game) = running.as_ref() else {
                return;
            };
            while let Ok(event) = game.events.try_recv() {
                match event {
                    Event::Ready {
                        title,
                        freeze_when_unfocused,
                    } => {
                        self.freeze_when_unfocused.set(freeze_when_unfocused);
                        self.sync_pause(&ui);
                        ui.set_player_title(super::titles::game_title(&title).into());
                    }
                    Event::Started(title) => {
                        ui.set_player_title(super::titles::game_title(&title).into());
                        ui.set_player_busy(false);
                        ui.set_player_message("".into());
                    }
                    Event::Stopped => close = true,
                    Event::Failed(error) => {
                        ui.set_player_focus(0);
                        ui.set_player_failed(true);
                        ui.set_player_busy(false);
                        ui.set_player_message(error.into());
                    }
                    Event::SpeedChanged(fast) => {
                        ui.set_player_fast_forward(fast);
                        ui.set_player_busy(false);
                        ui.set_player_message("".into());
                    }
                    Event::ScreenshotSaved => {
                        ui.set_player_busy(false);
                        ui.set_player_message("Screenshot saved to Album".into());
                    }
                    Event::Restarted => {
                        ui.set_player_busy(false);
                        ui.set_player_restart_confirm(false);
                        ui.set_player_paused(false);
                        self.worker_paused.set(false); // Restart resumes the core worker.
                        self.sync_pause(&ui);
                        ui.set_player_message("".into());
                        self.keyboard.set(0);
                        self.pad_ready.set(false);
                    }
                    Event::ActionFailed(error) => {
                        ui.set_player_busy(false);
                        ui.set_player_restart_confirm(false);
                        ui.set_player_message(error.into());
                    }
                    Event::SaveFailed(error) => {
                        ui.set_player_restart_confirm(false);
                        ui.set_player_paused(true);
                        self.worker_paused.set(true); // Save failures pause in the worker.
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
                if self.first_frame_at.get().is_none() {
                    self.first_frame_at.set(Some(Instant::now()));
                }
                ui.set_player_smooth(frame.smooth);
                ui.set_player_frame(Image::from_rgba8(
                    SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                        &frame.rgba,
                        frame.width,
                        frame.height,
                    ),
                ));
                ui.set_player_has_frame(true);
                self.sync_pause(&ui);
            }
            let pad = self.gamepads.borrow().as_ref().map_or(0, |source| source());
            if self.foreground.get() && pad == 0 {
                self.pad_ready.set(true);
            }
            let buttons = if !self.foreground.get()
                || self.stage.get() != PlayerStage::Playing
                || ui.get_player_paused()
                || ui.get_player_busy()
                || ui.get_player_failed()
            {
                0
            } else {
                self.keyboard.get() | if self.pad_ready.get() { pad } else { 0 }
            };
            game.buttons.store(buttons, Ordering::Release);
        }
        if close {
            self.begin_exit(&ui);
            return;
        }
        if self.stage.get() == PlayerStage::Waiting {
            let elapsed = self.stage_since.get().elapsed();
            let hold = Duration::from_millis(
                ui.global::<crate::Motion>().get_player_press_hold().max(0) as u64,
            );
            let prewarmed = self.first_frame_at.get().is_some_and(|first| {
                Self::zoom_duration(&ui).is_zero() || first.elapsed() >= Duration::from_millis(16)
            });
            if (ui.get_player_has_frame() && prewarmed && elapsed >= hold)
                || ui.get_player_failed()
                || elapsed >= Duration::from_millis(500)
            {
                // Capture the live camera position at the rebound, not at Accept.
                ui.set_player_origin(ui.invoke_player_capture_origin());
                Self::release_card(&ui);
                self.set_stage(&ui, PlayerStage::Entering);
            }
        }
        if self.stage.get() == PlayerStage::Entering
            && (Self::zoom_duration(&ui).is_zero()
                || self.stage_since.get().elapsed()
                    >= Self::zoom_duration(&ui) + Duration::from_millis(16))
        {
            self.keyboard.set(0);
            self.pad_ready.set(false);
            self.set_stage(&ui, PlayerStage::Playing);
        }
        if self.stage.get() == PlayerStage::Playing && ui.get_player_failed() {
            self.timer.stop();
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
        if self.stage.get() == PlayerStage::Exiting {
            if let Some(ui) = self.ui.upgrade() {
                self.finish_close(&ui);
            }
        } else {
            self.choose(RETURN_TO_HORIZON);
        }
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
            ui.set_player_origin(ui.invoke_player_capture_origin());
            self.set_stage(&ui, PlayerStage::Waiting);
            self.closing.set(false);
            self.first_frame_at.set(None);
            self.timer.restart();
            self.keyboard.set(0);
            self.pad_ready.set(false);
            ui.set_player_title("Starting game".into());
            ui.set_player_message("".into());
            ui.set_player_busy(true);
            ui.set_player_failed(false);
            ui.set_player_paused(false);
            ui.set_player_frozen(false);
            self.worker_paused.set(false);
            ui.set_player_focus(0);
            ui.set_player_restart_confirm(false);
            ui.set_player_restart_focus(0);
            ui.set_player_fast_forward(false);
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
    fn pause_navigation_skips_placeholders_and_does_not_wrap_on_repeat() {
        assert_eq!(pause_step(1, true, false), 4);
        assert_eq!(pause_step(4, false, false), 1);
        assert_eq!(pause_step(0, false, true), 0);
        assert_eq!(pause_step(6, true, true), 6);
        assert_eq!(pause_step(6, true, false), 0);
    }
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
        let scrolled = std::env::var_os("HORIZON_TEST_SCROLLED_LIBRARY").is_some();
        if scrolled {
            for n in 1..=32u8 {
                let mut variant = crate::platform::gba_core::test_rom();
                variant[1023] = n;
                std::fs::write(directory.join(format!("z demo {n:02}.gba")), variant).unwrap();
            }
        }
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
        let filter: crate::domain::core_settings::VideoFilter =
            serde_json::from_value(serde_json::Value::String(
                std::env::var("HORIZON_TEST_VIDEO_FILTER").unwrap_or("none".into()),
            ))
            .unwrap();
        let mut preferences = crate::domain::core_settings::CorePreferences::default();
        preferences.mgba.video_filter = filter;
        preferences.mgba.color_correction =
            std::env::var_os("HORIZON_TEST_COLOR_CORRECTION").is_some();
        preferences.mgba.freeze_when_unfocused =
            std::env::var_os("HORIZON_TEST_BACKGROUND_CONTINUES").is_none();
        let freeze = preferences.mgba.freeze_when_unfocused;
        let corrected = preferences.mgba.color_correction;
        crate::persistence::settings::SettingsStore::new(directory.join("cores.json"))
            .save_value(&preferences)
            .unwrap();
        let ui = AppWindow::new().unwrap();
        let reduced = std::env::var_os("HORIZON_TEST_REDUCED_MOTION").is_some();
        ui.global::<crate::Motion>().set_reduced_motion(reduced);
        if let Ok(width) = std::env::var("HORIZON_PLAYER_TEST_WIDTH") {
            ui.window()
                .set_size(slint::PhysicalSize::new(width.parse().unwrap(), 720));
        }
        let player = GbaPlayer::new(
            &ui,
            PlayerPaths {
                database: database.clone(),
                core: std::env::var_os("HORIZON_GBA_CORE").unwrap().into(),
                saves: saves.clone(),
                settings: directory.join("cores.json"),
                album: directory.join("album"),
            },
        );
        let games = repository.list_games().unwrap();
        let catalog = crate::presentation::catalog::Catalog::shared(games);
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
            Rc::clone(&catalog),
            &[game_id],
            &artwork,
            &registry,
            launch,
            activity.clone(),
        );
        let library = crate::presentation::library::LibraryController::new(
            &ui,
            catalog,
            Rc::clone(&registry),
            Rc::clone(&home),
            activity,
        );
        let keys = Rc::clone(&player);
        ui.on_raw_key_input(move |text, repeat| keys.keyboard(&text, repeat, true));
        let keys = Rc::clone(&player);
        ui.on_raw_key_released(move |text| keys.keyboard(&text, false, false));
        ui.show().unwrap();
        home.handle_action(&ui, UiActionEvent::fresh(UiAction::Accept));
        assert!(ui.get_player_open());
        if std::env::var_os("HORIZON_TEST_DEACTIVATE_LAUNCH").is_some() {
            player.set_active(false);
        }
        let stage = Rc::new(Cell::new(0));
        let ticks = Rc::new(Cell::new(0));
        let done = Rc::new(Cell::new(false));
        let timed_out = Rc::new(Cell::new(false));
        let motion = Rc::new(Cell::new(0u8));
        let blends = Rc::new(Cell::new(0u8));
        let motion_captures = Rc::new(RefCell::new(Vec::new()));
        let captures_directory = directory.clone();
        let captured_blends = Rc::clone(&blends);
        let captures = Rc::clone(&motion_captures);
        let motion_timer = Timer::default();
        let recorded = Rc::clone(&motion);
        let weak_ui = ui.as_weak();
        let motion_player = Rc::clone(&player);
        motion_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            let ui = weak_ui.upgrade().unwrap();
            let phase = ui.get_player_phase();
            let progress = ui.get_player_visual_progress();
            if phase == 3 && progress > 0.04 && progress < 0.35 {
                let library = ui.get_current_route() == crate::AppRouteView::Library;
                let bit = if library { 2 } else { 1 };
                if captured_blends.get() & bit == 0 {
                    assert!(ui.get_player_picture_opacity() < 0.55);
                    captures.borrow_mut().push((
                        format!(
                            "return-blend-{}.png",
                            if library { "library" } else { "home" }
                        ),
                        ui.window().take_snapshot().unwrap(),
                    ));
                    captured_blends.set(captured_blends.get() | bit);
                }
            }
            if (phase == 1 || phase == 3) && progress > 0.1 && progress < 0.9 {
                let library = ui.get_current_route() == crate::AppRouteView::Library;
                let bit = if library {
                    if phase == 1 { 4 } else { 8 }
                } else if phase == 1 {
                    1
                } else {
                    2
                };
                if recorded.get() & bit == 0 {
                    assert!(ui.get_player_has_frame());
                    assert!(motion_player.handle_action(UiActionEvent::fresh(UiAction::Accept)));
                    assert!(ui.get_player_open());
                    let pixels = ui.window().take_snapshot().unwrap();
                    captures
                        .borrow_mut()
                        .push((format!("transition-{bit}.png"), pixels));
                    recorded.set(recorded.get() | bit);
                }
            }
        });
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
                0 if ui.get_player_has_frame()
                    && !ui.get_player_busy()
                    && ui.get_player_phase() == 2 =>
                {
                    assert_eq!(ui.get_player_title(), "Horizon homebrew");
                    if !live.foreground.get() {
                        assert!(!ui.get_player_paused());
                        assert_eq!(ui.get_player_frozen(), freeze);
                        snapshot("gba-background-start");
                        live.set_active(true);
                        assert!(!ui.get_player_paused());
                        assert!(!ui.get_player_frozen());
                    }
                    key("z".into(), true);
                    assert_ne!(live.keyboard.get() & (1 << 8), 0);
                    progress.set(18);
                }
                18 => {
                    let first = ui.get_player_frame().to_rgba8().unwrap().as_slice()[0];
                    if first.r > 180 && first.r > first.g {
                        progress.set(1);
                    }
                }
                1 => {
                    let color = snapshot("gba-running");
                    assert!(
                        color[0] > 180
                            && if corrected {
                                color[0] > color[1].saturating_add(70)
                            } else {
                                color[1] < 40
                            },
                        "{color:?}"
                    );
                    key("z".into(), false);
                    tap(slint::platform::Key::Escape);
                    assert!(ui.get_player_paused());
                    progress.set(2);
                }
                2 => {
                    // An explicit pause survives deactivation/reactivation.
                    live.set_active(false);
                    assert!(ui.get_player_paused());
                    live.set_active(true);
                    assert!(ui.get_player_paused());
                    snapshot("gba-paused");
                    tap(slint::platform::Key::DownArrow);
                    assert_eq!(ui.get_player_focus(), 1);
                    tap(slint::platform::Key::RightArrow);
                    progress.set(10);
                }
                10 if !ui.get_player_busy() => {
                    assert!(ui.get_player_fast_forward());
                    assert!(ui.get_player_message().is_empty());
                    snapshot("gba-speed");
                    tap(slint::platform::Key::DownArrow);
                    assert_eq!(ui.get_player_focus(), 4); // skips save/load placeholders
                    ui.invoke_player_choose(2);
                    ui.invoke_player_choose(3);
                    assert_eq!(ui.get_player_focus(), 4);
                    assert!(!ui.get_player_busy());
                    tap(slint::platform::Key::Return);
                    progress.set(11);
                }
                11 if !ui.get_player_busy() => {
                    assert!(ui.get_player_message().contains("saved to Album"));
                    let captures =
                        crate::services::album::catalog::HorizonAlbum::new(directory.join("album"))
                            .scan();
                    assert_eq!(captures.len(), 1);
                    assert_eq!(
                        captures[0].game().unwrap().external_id().as_str(),
                        format!("sha256:{hash}")
                    );
                    let shot = image::open(captures[0].path()).unwrap().to_rgba8();
                    assert_eq!(shot.dimensions(), (240, 160));
                    let first = shot.get_pixel(0, 0);
                    assert!(shot.pixels().all(|p| p == first)); // native pixels, no panel/filter/badge
                    tap(slint::platform::Key::DownArrow);
                    assert_eq!(ui.get_player_focus(), 5);
                    tap(slint::platform::Key::Return);
                    assert!(ui.get_player_restart_confirm());
                    assert_eq!(ui.get_player_restart_focus(), 0);
                    progress.set(12);
                }
                12 => {
                    snapshot("gba-restart-confirm");
                    tap(slint::platform::Key::Escape);
                    assert!(!ui.get_player_restart_confirm());
                    assert_eq!(ui.get_player_focus(), 5);
                    tap(slint::platform::Key::Return);
                    tap(slint::platform::Key::RightArrow);
                    tap(slint::platform::Key::Return);
                    if std::env::var_os("HORIZON_TEST_DEACTIVATE_LAUNCH").is_some() {
                        live.set_active(false);
                    }
                    progress.set(13);
                }
                13 if !ui.get_player_busy() => {
                    assert!(!ui.get_player_restart_confirm());
                    if !live.foreground.get() {
                        assert!(!ui.get_player_paused());
                        assert_eq!(ui.get_player_frozen(), freeze);
                        live.set_active(true);
                        assert!(!ui.get_player_frozen());
                    }
                    assert!(!ui.get_player_paused());
                    assert!(ui.get_player_fast_forward());
                    snapshot("gba-restarted");
                    live.handle_action(UiActionEvent::fresh(UiAction::Home));
                    tap(slint::platform::Key::DownArrow);
                    tap(slint::platform::Key::LeftArrow);
                    progress.set(14);
                }
                14 if !ui.get_player_busy() => {
                    assert!(!ui.get_player_fast_forward());
                    tap(slint::platform::Key::UpArrow);
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
                    let first = ui.get_player_frame().to_rgba8().unwrap().as_slice()[0];
                    if if corrected {
                        first.g > first.r.saturating_add(30)
                    } else {
                        first.g > 180
                    } {
                        // Snapshot capture can return the last presented frame;
                        // allow the green buffer to reach the compositor first.
                        if std::env::var_os("HORIZON_TEST_FOCUS_CYCLE").is_some() {
                            live.timer.stop();
                            live.set_active(false);
                            assert!(!ui.get_player_paused());
                            assert_eq!(ui.get_player_frozen(), freeze);
                            key("z".into(), true);
                            assert_eq!(live.keyboard.get(), 0);
                            assert!(live.handle_action(UiActionEvent::fresh(UiAction::Home)));
                            assert!(!ui.get_player_paused());
                            progress.set(16);
                        } else {
                            progress.set(7);
                        }
                    }
                }
                16 => {
                    // Let the worker process its pause command; discard any prior frame.
                    live.running
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .frame
                        .lock()
                        .unwrap()
                        .take();
                    progress.set(17);
                }
                17 => {
                    assert_eq!(
                        live.running
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .frame
                            .lock()
                            .unwrap()
                            .is_some(),
                        !freeze
                    );
                    assert!(!ui.get_player_paused());
                    snapshot("gba-background-frozen");
                    live.set_active(true);
                    assert!(!ui.get_player_frozen());
                    assert!(!ui.get_player_paused());
                    live.timer.restart();
                    progress.set(7);
                }
                7 => {
                    let color = snapshot("gba-released");
                    assert!(
                        if corrected {
                            color[1] > 100 && color[1] > color[0].saturating_add(40)
                        } else {
                            color[1] > 180 && color[0] < 40
                        },
                        "{color:?}"
                    );
                    assert!(live.handle_action(UiActionEvent::fresh(UiAction::Home)));
                    assert!(ui.get_player_paused());
                    // A save failure must stay paused, without beginning collapse.
                    std::fs::create_dir(saves.join(format!("{hash}.tmp"))).unwrap();
                    for _ in 0..4 {
                        live.handle_action(UiActionEvent::fresh(UiAction::Down));
                    }
                    assert_eq!(ui.get_player_focus(), RETURN_TO_HORIZON);
                    live.handle_action(UiActionEvent::fresh(UiAction::Accept));
                    progress.set(9);
                }
                9 if !ui.get_player_busy() => {
                    assert!(ui.get_player_paused());
                    assert_eq!(ui.get_player_phase(), 2);
                    assert!(!ui.get_player_message().is_empty());
                    std::fs::remove_dir(saves.join(format!("{hash}.tmp"))).unwrap();
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
                    ui.set_current_route(crate::AppRouteView::Library);
                    library.on_enter(&ui);
                    if scrolled {
                        library.handle_action(&ui, UiActionEvent::fresh(UiAction::Down));
                        library.handle_action(&ui, UiActionEvent::fresh(UiAction::Down));
                        assert!(ui.get_library_scroll_row() > 0);
                    }
                    progress.set(8);
                }
                8 => {
                    library.handle_action(&ui, UiActionEvent::fresh(UiAction::Accept));
                    assert!(ui.get_player_open());
                    let origin = ui.get_player_origin();
                    assert!(origin.width > 0.0 && origin.width < 1.0);
                    progress.set(5);
                }
                5 if ui.get_player_has_frame()
                    && !ui.get_player_busy()
                    && ui.get_player_phase() == 2 =>
                {
                    live.handle_action(UiActionEvent::fresh(UiAction::Home));
                    for _ in 0..4 {
                        live.handle_action(UiActionEvent::fresh(UiAction::Down));
                    }
                    live.handle_action(UiActionEvent::fresh(UiAction::Accept));
                    progress.set(6);
                }
                6 if !ui.get_player_open() => {
                    finished.set(true);
                    slint::quit_event_loop().unwrap();
                }
                _ => {}
            }
        });
        slint::run_event_loop().unwrap();
        player.shutdown().unwrap();
        assert!(
            done.get() && !timed_out.get(),
            "stage {}: paused={}, frozen={}, worker_paused={}, keys={}",
            stage.get(),
            ui.get_player_paused(),
            ui.get_player_frozen(),
            player.worker_paused.get(),
            player.keyboard.get()
        );
        assert_eq!(motion.get(), if reduced { 0 } else { 15 });
        assert_eq!(blends.get(), if reduced { 0 } else { 3 });
        // Encode after the event loop so snapshot I/O cannot skip animation phases.
        for (name, pixels) in motion_captures.borrow().iter() {
            image::save_buffer(
                captures_directory.join(name),
                pixels.as_bytes(),
                pixels.width(),
                pixels.height(),
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
}

#[cfg(test)]
mod pause_preview_tests {
    use super::*;
    #[test]
    #[ignore = "requires Wayland and a fresh HORIZON_PAUSE_PREVIEW_DIR"]
    fn quiet_pause_panel_visual_preview() {
        let directory =
            std::path::PathBuf::from(std::env::var_os("HORIZON_PAUSE_PREVIEW_DIR").unwrap());
        std::fs::create_dir_all(&directory).unwrap();
        let ui = AppWindow::new().unwrap();
        let appearance = crate::presentation::appearance::AppearanceController::new(
            &ui,
            crate::appearance::store::AppearanceStore::new(directory.join("appearance.json")),
        )
        .unwrap();
        appearance
            .set_theme_preference(
                &ui,
                if std::env::var_os("HORIZON_PAUSE_PREVIEW_BLACK").is_some() {
                    crate::appearance::ThemePreference::Black
                } else if std::env::var_os("HORIZON_PAUSE_PREVIEW_DARK").is_some() {
                    crate::appearance::ThemePreference::Dark
                } else {
                    crate::appearance::ThemePreference::Light
                },
            )
            .unwrap();
        ui.global::<crate::Motion>().set_reduced_motion(true);
        let mut rgba = Vec::with_capacity(240 * 160 * 4);
        for y in 0..160u32 {
            for x in 0..240u32 {
                rgba.extend_from_slice(&[(x * 180 / 240) as u8, (y * 170 / 160) as u8, 95, 255]);
            }
        }
        ui.set_player_frame(Image::from_rgba8(
            SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, 240, 160),
        ));
        ui.set_player_title("Pokemon - Emerald Version".into());
        ui.set_player_open(true);
        ui.set_player_phase(2);
        ui.set_player_has_frame(true);
        ui.set_player_fast_forward(true);
        ui.set_player_paused(true);
        ui.set_player_focus(1);
        ui.set_player_restart_confirm(std::env::var_os("HORIZON_PAUSE_PREVIEW_RESTART").is_some());
        ui.show().unwrap();
        let timer = Timer::default();
        let weak = ui.as_weak();
        timer.start(
            TimerMode::SingleShot,
            Duration::from_millis(400),
            move || {
                let ui = weak.upgrade().unwrap();
                let pixels = ui.window().take_snapshot().unwrap();
                if std::env::var_os("HORIZON_PAUSE_PREVIEW_BLACK").is_some() {
                    let interior = pixels.as_slice()[(100 * pixels.width() + 52) as usize];
                    assert_eq!([interior.r, interior.g, interior.b], [0, 0, 0]);
                }
                image::save_buffer(
                    directory.join("pause-preview.png"),
                    pixels.as_bytes(),
                    pixels.width(),
                    pixels.height(),
                    image::ColorType::Rgba8,
                )
                .unwrap();
                slint::quit_event_loop().unwrap();
            },
        );
        slint::run_event_loop().unwrap();
    }
}
