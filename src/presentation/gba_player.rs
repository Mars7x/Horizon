//! Rust owns player mode/focus. Slint only presents the latest software frame.
use crate::domain::core_settings::PlaybackSpeed;
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PlayerStage {
    Waiting,
    Entering,
    Playing,
    Exiting,
}

const RETURN_TO_HORIZON: i32 = 6;
/// DS only, shown after Playback speed. Their own numbers keep the others'.
const SWAP_SCREENS: i32 = 7;
const SCREEN_LAYOUT: i32 = 8;
/// Closes or opens the DS lid, then continues so the game sees it.
const LID: i32 = 9;
const REWIND: i32 = 10;
const PAUSE_ACTIONS: [i32; 8] = [0, 1, REWIND, 2, 3, 4, 5, RETURN_TO_HORIZON];
const DS_PAUSE_ACTIONS: [i32; 11] = [
    0,
    1,
    REWIND,
    SCREEN_LAYOUT,
    SWAP_SCREENS,
    LID,
    2,
    3,
    4,
    5,
    RETURN_TO_HORIZON,
];
fn pause_actions(ds: bool, three_ds: bool) -> &'static [i32] {
    if three_ds {
        // No rewind (a 3DS state takes about a second) and no lid.
        &[
            0,
            1,
            SCREEN_LAYOUT,
            SWAP_SCREENS,
            2,
            3,
            4,
            5,
            RETURN_TO_HORIZON,
        ]
    } else if ds {
        &DS_PAUSE_ACTIONS
    } else {
        &PAUSE_ACTIONS
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
enum StateMode {
    Menu = 0,
    Save = 1,
    Load = 2,
    Overwrite = 3,
}
fn pause_step(current: i32, down: bool, repeated: bool, ds: bool, three_ds: bool) -> i32 {
    let actions = pause_actions(ds, three_ds);
    let index = actions
        .iter()
        .position(|&action| action == current)
        .unwrap_or(0);
    let next = crate::navigation::step_with_edge_wrap(
        index as i32,
        0,
        actions.len() as i32 - 1,
        if down { 1 } else { -1 },
        repeated,
    );
    actions[next as usize]
}

pub struct GbaPlayer {
    ui: slint::Weak<AppWindow>,
    paths: PlayerPaths,
    running: RefCell<Option<RunningGame>>,
    screenshot_until: Cell<Option<Instant>>,
    state_mode: Cell<StateMode>,
    state_focus: Cell<i32>,
    state_entries: RefCell<Vec<crate::services::save_states::Entry>>,
    rewind_rows: RefCell<Vec<crate::RewindRow>>,
    rewind_requested: Cell<bool>,
    rewind_from_menu: Cell<bool>,
    keyboard: Cell<u16>,
    pad_ready: Cell<bool>,
    gamepads: CallbackSlot<dyn Fn() -> u16>,
    /// Shortcut buttons held now, their assignments and the last reading
    /// (presses act once); Tab held on the keyboard; the speed fast-forward
    /// returns to; a quick load in progress; when the notice hides.
    shortcut_buttons: CallbackSlot<dyn Fn() -> u16>,
    shortcuts: Cell<crate::domain::controller::Shortcuts>,
    shortcuts_held: Cell<u16>,
    shortcuts_ready: Cell<bool>,
    keyboard_fast_forward: Cell<bool>,
    speed_before_fast_forward: Cell<Option<PlaybackSpeed>>,
    quick_loading: Cell<bool>,
    notice_until: Cell<Option<Instant>>,
    ds_gamepad: CallbackSlot<dyn Fn() -> u16>,
    three_ds_gamepad: CallbackSlot<dyn Fn() -> (u16, [i16; 4])>,
    three_ds_keyboard: Cell<u8>,
    /// A DS game's screen layout (None for other systems), and whether its
    /// screens are swapped for this session.
    ds_layout: Cell<Option<crate::domain::core_settings::DsLayout>>,
    ds_swapped: Cell<bool>,
    selected_game: CallbackSlot<dyn Fn() -> Option<i64>>,
    before_return: CallbackSlot<dyn Fn()>,
    /// The game whose card launched this session.
    launched_game: Cell<Option<i64>>,
    sound: CallbackSlot<dyn Fn(UiSoundCue)>,
    timer: Timer,
    closing: Cell<bool>,
    foreground: Cell<bool>,
    freeze_when_unfocused: Cell<bool>,
    worker_paused: Cell<bool>,
    stage: Cell<PlayerStage>,
    stage_since: Cell<Instant>,
    /// When Return to Horizon started saving, until the collapse begins.
    exit_since: Cell<Option<Instant>>,
    /// Called by the game worker when a frame is ready (see `show_frame`).
    frame_ready: std::cell::OnceCell<crate::services::worker::Notify>,
    /// When the last game frame was handed to the screen. Frames are shown
    /// at most once per display refresh (`frame_period`); one arriving
    /// sooner waits on `retry`.
    last_shown: Cell<Option<Instant>>,
    frame_period: Cell<Duration>,
    retry: Timer,
    this: RefCell<std::rc::Weak<Self>>,
    /// Game frames shown in total, and the total at the instant of the last
    /// once-a-second debug report.
    shown: Cell<u64>,
    shown_report: Cell<(Instant, u64)>,
}
impl GbaPlayer {
    pub fn new(ui: &AppWindow, paths: PlayerPaths) -> Rc<Self> {
        let player = Rc::new(Self {
            ui: ui.as_weak(),
            paths,
            running: RefCell::new(None),
            screenshot_until: Cell::new(None),
            state_mode: Cell::new(StateMode::Menu),
            state_focus: Cell::new(-1),
            state_entries: RefCell::new(Vec::new()),
            rewind_rows: RefCell::new(Vec::new()),
            rewind_requested: Cell::new(false),
            rewind_from_menu: Cell::new(false),
            keyboard: Cell::new(0),
            pad_ready: Cell::new(false),
            gamepads: RefCell::new(None),
            shortcut_buttons: RefCell::new(None),
            shortcuts: Cell::default(),
            shortcuts_held: Cell::new(0),
            shortcuts_ready: Cell::new(false),
            keyboard_fast_forward: Cell::new(false),
            speed_before_fast_forward: Cell::new(None),
            quick_loading: Cell::new(false),
            notice_until: Cell::new(None),
            ds_gamepad: RefCell::new(None),
            three_ds_gamepad: RefCell::new(None),
            three_ds_keyboard: Cell::new(0),
            ds_layout: Cell::new(None),
            ds_swapped: Cell::new(false),
            selected_game: RefCell::new(None),
            before_return: RefCell::new(None),
            launched_game: Cell::new(None),
            sound: RefCell::new(None),
            timer: Timer::default(),
            closing: Cell::new(false),
            foreground: Cell::new(true),
            freeze_when_unfocused: Cell::new(true),
            worker_paused: Cell::new(false),
            stage: Cell::new(PlayerStage::Waiting),
            stage_since: Cell::new(Instant::now()),
            exit_since: Cell::new(None),
            frame_ready: std::cell::OnceCell::new(),
            last_shown: Cell::new(None),
            frame_period: Cell::new(Duration::from_micros(16_667)),
            retry: Timer::default(),
            this: RefCell::new(std::rc::Weak::new()),
            shown: Cell::new(0),
            shown_report: Cell::new((Instant::now(), 0)),
        });
        let weak = Rc::downgrade(&player);
        let _ = player.frame_ready.set(super::wake::ui_notify(move || {
            if let Some(player) = weak.upgrade()
                && let Some(ui) = player.ui.upgrade()
            {
                player.show_frame(&ui);
            }
        }));
        let weak = Rc::downgrade(&player);
        *player.this.borrow_mut() = weak;
        let weak = Rc::downgrade(&player);
        ui.on_player_touch(move |x, y, pressed| {
            if let Some(player) = weak.upgrade()
                && player.foreground.get()
                && player.stage.get() == PlayerStage::Playing
                && let Some(ui) = player.ui.upgrade()
                && !ui.get_player_paused()
                && !ui.get_player_busy()
                && !ui.get_player_failed()
                && let Some(game) = player.running.borrow().as_ref()
                && let Some(extra) = &game.ds_input
            {
                let coordinate =
                    |v: f32| ((v.clamp(0.0, 1.0) * 65534.0 - 32767.0).round() as i16) as u16;
                // Azahar's frame has the 320-wide touch screen centred in 400.
                let x = if game.three_ds {
                    0.1 + 0.8 * x.clamp(0.0, 1.0)
                } else {
                    x
                };
                extra.pointer.store(
                    u64::from(coordinate(x))
                        | (u64::from(coordinate(y)) << 16)
                        | (u64::from(pressed) << 32),
                    Ordering::Release,
                );
            }
        });
        let weak = Rc::downgrade(&player);
        ui.on_player_choose(move |index| {
            if let Some(player) = weak.upgrade() {
                player.choose(index);
            }
        });
        let weak = Rc::downgrade(&player);
        ui.on_player_state_choose(move |slot| {
            if let Some(player) = weak.upgrade() {
                player.choose_state(slot);
            }
        });
        let weak = Rc::downgrade(&player);
        ui.on_player_rewind_select(move |index| {
            if let Some(player) = weak.upgrade() {
                player.select_rewind(index);
            }
        });
        let weak = Rc::downgrade(&player);
        ui.on_player_rewind_scrub(move |position| {
            if let Some(player) = weak.upgrade()
                && position.is_finite()
            {
                let last = player.rewind_rows.borrow().len().saturating_sub(1);
                player.select_rewind((position.clamp(0.0, 1.0) * last as f32).round() as i32);
            }
        });
        let weak = Rc::downgrade(&player);
        ui.on_player_rewind_answer(move |resume| {
            if let Some(player) = weak.upgrade() {
                player.answer_rewind(resume);
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
    pub fn set_ds_gamepad(&self, source: Rc<dyn Fn() -> u16>) {
        *self.ds_gamepad.borrow_mut() = Some(source);
    }
    pub fn set_three_ds_gamepad(&self, source: Rc<dyn Fn() -> (u16, [i16; 4])>) {
        *self.three_ds_gamepad.borrow_mut() = Some(source);
    }
    pub fn set_shortcut_buttons(&self, source: Rc<dyn Fn() -> u16>) {
        *self.shortcut_buttons.borrow_mut() = Some(source);
    }
    pub fn set_shortcuts(&self, shortcuts: crate::domain::controller::Shortcuts) {
        self.shortcuts.set(shortcuts);
    }
    pub fn set_gamepads(&self, source: Rc<dyn Fn() -> u16>) {
        *self.gamepads.borrow_mut() = Some(source);
    }
    /// The game whose card is selected on the current page, so a return can
    /// tell whether the card it launched from is still there.
    pub fn set_selected_game(&self, source: Rc<dyn Fn() -> Option<i64>>) {
        *self.selected_game.borrow_mut() = Some(source);
    }
    /// Settle the underlying page before capturing the return destination.
    pub fn set_before_return(&self, callback: Rc<dyn Fn()>) {
        *self.before_return.borrow_mut() = Some(callback);
    }
    fn selected_game(&self) -> Option<i64> {
        self.selected_game
            .borrow()
            .as_ref()
            .and_then(|source| source())
    }
    pub fn is_open(&self) -> bool {
        self.ui.upgrade().is_some_and(|ui| ui.get_player_open())
    }
    pub fn set_active(&self, active: bool) {
        let leaving = self.foreground.replace(active) && !active;
        self.keyboard.set(0);
        self.pad_ready.set(false);
        self.shortcuts_ready.set(false);
        if let Some(ui) = self.ui.upgrade() {
            if let Some(game) = self.running.borrow().as_ref() {
                game.buttons.store(0, Ordering::Release);
                // Leaving Horizon is a likely moment for something else to
                // go wrong: refresh the automatic resume point.
                if leaving && self.stage.get() == PlayerStage::Playing {
                    let _ = game.commands.send(Command::Autosave);
                }
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
        if !ui.get_player_open()
            || ui.get_player_failed()
            || ui.get_player_paused()
            || !self.foreground.get()
            || self.stage.get() != PlayerStage::Playing
            || ui.get_player_busy()
        {
            return;
        }
        ui.set_player_paused(true);
        self.shortcuts_ready.set(false);
        self.set_state_mode(&ui, StateMode::Menu);
        ui.set_player_restart_confirm(false);
        ui.set_player_focus(0);
        self.keyboard.set(0);
        self.pad_ready.set(false);
        if let Some(game) = self.running.borrow().as_ref() {
            game.buttons.store(0, Ordering::Release);
        }
        self.sync_pause(&ui);
        self.cue(UiSoundCue::QuickMenuOpen);
    }

    fn clear_screenshot(&self, ui: &AppWindow) {
        self.screenshot_until.set(None);
        ui.set_player_screenshot_visible(false);
    }
    fn close_pause(&self, ui: &AppWindow) {
        self.clear_screenshot(ui);
        let was_paused = ui.get_player_paused();
        ui.set_player_paused(false);
        if was_paused && !self.closing.get() && self.foreground.get() && !ui.get_player_failed() {
            self.cue(UiSoundCue::QuickMenuClose);
        }
    }

    fn open_rewind(&self, ui: &AppWindow) {
        if ui.get_player_busy()
            || ui.get_player_rewind_open()
            || ui.get_player_failed()
            || !self.foreground.get()
            || self.stage.get() != PlayerStage::Playing
        {
            return;
        }
        self.rewind_from_menu.set(ui.get_player_paused());
        self.rewind_requested.set(true);
        self.shortcuts_ready.set(false);
        self.rewind_rows.borrow_mut().clear();
        ui.set_player_rewind_rows(Default::default());
        ui.set_player_rewind_position("".into());
        ui.set_player_rewind_duration("".into());
        self.clear_screenshot(ui);
        self.keyboard.set(0);
        self.keyboard_fast_forward.set(false);
        self.fast_forward(ui, false);
        self.pad_ready.set(false);
        ui.set_player_paused(true);
        ui.set_player_rewind_open(true);
        ui.set_player_rewind_preview(ui.get_player_frame());
        ui.set_player_busy(true);
        ui.set_player_message("".into());
        self.sync_pause(ui);
        if let Some(game) = self.running.borrow().as_ref() {
            game.buttons.store(0, Ordering::Release);
            if let Some(extra) = &game.ds_input {
                extra.pointer.store(0, Ordering::Release);
            }
            if game.commands.send(Command::OpenRewind).is_err() {
                self.rewind_requested.set(false);
                ui.set_player_rewind_open(false);
                ui.set_player_busy(false);
                ui.set_player_failed(true);
                ui.set_player_message("The game stopped unexpectedly".into());
            }
        }
    }

    fn publish_rewind(
        &self,
        ui: &AppWindow,
        points: Vec<crate::services::gba_player::RewindPoint>,
    ) {
        if !self.rewind_requested.replace(false) || self.closing.get() {
            return;
        }
        let duration = points.first().map_or(Duration::ZERO, |point| point.behind);
        let rows: Vec<_> = points
            .into_iter()
            .enumerate()
            .map(|(index, point)| crate::RewindRow {
                index: index as i32,
                label: rewind_label(point.behind).into(),
                preview: Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                    &point.preview.rgba,
                    point.preview.width,
                    point.preview.height,
                )),
            })
            .collect();
        let selected = rows.len().saturating_sub(1);
        if let Some(row) = rows.get(selected) {
            ui.set_player_rewind_preview(row.preview.clone());
            ui.set_player_rewind_position(row.label.clone());
        }
        ui.set_player_rewind_selected(selected as i32);
        ui.set_player_rewind_duration(
            format!("{:.1} seconds available", duration.as_secs_f64()).into(),
        );
        ui.set_player_rewind_rows(Rc::new(slint::VecModel::from(rows.clone())).into());
        *self.rewind_rows.borrow_mut() = rows;
        ui.set_player_busy(false);
        self.worker_paused.set(true);
        self.cue(UiSoundCue::RewindOpen);
    }

    fn select_rewind(&self, index: i32) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if !ui.get_player_rewind_open() || ui.get_player_busy() || !self.foreground.get() {
            return;
        }
        let rows = self.rewind_rows.borrow();
        let Some(row) = usize::try_from(index)
            .ok()
            .and_then(|index| rows.get(index))
        else {
            return;
        };
        let changed = index != ui.get_player_rewind_selected();
        ui.set_player_rewind_selected(index);
        ui.set_player_rewind_preview(row.preview.clone());
        ui.set_player_rewind_position(row.label.clone());
        if changed {
            self.cue(UiSoundCue::Navigation);
        }
    }

    fn answer_rewind(&self, resume: bool) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if !ui.get_player_rewind_open() || ui.get_player_busy() || !self.foreground.get() {
            return;
        }
        if resume
            && (ui.get_player_rewind_selected() as usize) + 1 < self.rewind_rows.borrow().len()
        {
            ui.set_player_busy(true);
            ui.set_player_message("".into());
            if let Some(game) = self.running.borrow().as_ref() {
                let _ = game
                    .commands
                    .send(Command::RewindTo(ui.get_player_rewind_selected() as usize));
            }
        } else {
            self.leave_rewind(&ui, resume);
        }
    }

    fn leave_rewind(&self, ui: &AppWindow, resumed: bool) {
        self.rewind_requested.set(false);
        ui.set_player_rewind_open(false);
        ui.set_player_busy(false);
        ui.set_player_paused(false);
        ui.set_player_message("".into());
        self.keyboard.set(0);
        self.pad_ready.set(false);
        self.shortcuts_ready.set(false);
        self.set_state_mode(ui, StateMode::Menu);
        self.sync_pause(ui);
        self.cue(if resumed {
            UiSoundCue::Ok
        } else {
            UiSoundCue::Back
        });
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
        if ui.get_player_three_ds() && !ui.get_player_paused() {
            let bit = match text.to_ascii_lowercase().as_str() {
                "i" => 1,
                "k" => 2,
                "j" => 4,
                "l" => 8,
                _ => 0,
            };
            if bit != 0 {
                let held = self.three_ds_keyboard.get();
                self.three_ds_keyboard
                    .set(if pressed { held | bit } else { held & !bit });
                return true;
            }
        }
        if pressed
            && !repeated
            && (text == slint::SharedString::from(slint::platform::Key::Escape).as_str()
                || text == slint::SharedString::from(slint::platform::Key::F10).as_str())
        {
            if ui.get_player_rewind_open() {
                self.answer_rewind(false);
                return true;
            }
            if ui.get_player_busy() {
                if !ui.get_player_paused() {
                    self.choose(RETURN_TO_HORIZON);
                }
            } else if ui.get_player_restart_confirm()
                || self.state_mode.get() == StateMode::Overwrite
            {
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
        // Shortcuts: Tab held fast-forwards; F5/F8/F12 quick save, quick
        // load and screenshot.
        if text == slint::SharedString::from(slint::platform::Key::Tab).as_str() {
            self.keyboard_fast_forward.set(pressed);
            return true;
        }
        for (key, row) in [
            (
                slint::platform::Key::F6,
                crate::domain::controller::Shortcuts::REWIND,
            ),
            (
                slint::platform::Key::F5,
                crate::domain::controller::Shortcuts::QUICK_SAVE,
            ),
            (
                slint::platform::Key::F8,
                crate::domain::controller::Shortcuts::QUICK_LOAD,
            ),
            (
                slint::platform::Key::F12,
                crate::domain::controller::Shortcuts::SCREENSHOT,
            ),
        ] {
            if text == slint::SharedString::from(key).as_str() {
                if pressed && !repeated {
                    self.shortcut(&ui, row);
                }
                return true;
            }
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
        let before = self.ui.upgrade().map(|ui| {
            (
                ui.get_player_focus(),
                ui.get_player_restart_focus(),
                ui.get_player_state_focus(),
            )
        });
        let handled = self.handle_action_inner(event);
        if handled
            && matches!(
                event.action,
                UiAction::Up | UiAction::Down | UiAction::Left | UiAction::Right
            )
            && self.ui.upgrade().is_some_and(|ui| {
                Some((
                    ui.get_player_focus(),
                    ui.get_player_restart_focus(),
                    ui.get_player_state_focus(),
                )) != before
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
        if ui.get_player_rewind_open() {
            match event.action {
                UiAction::Left | UiAction::Right => {
                    let last = self.rewind_rows.borrow().len().saturating_sub(1) as i32;
                    let delta = if event.action == UiAction::Left {
                        -1
                    } else {
                        1
                    };
                    self.select_rewind((ui.get_player_rewind_selected() + delta).clamp(0, last));
                }
                UiAction::Accept if !event.repeated => self.answer_rewind(true),
                UiAction::Back | UiAction::Home if !event.repeated => self.answer_rewind(false),
                _ => {}
            }
            return true;
        }
        if ui.get_player_paused() {
            if self.state_mode.get() != StateMode::Menu {
                match event.action {
                    UiAction::Back | UiAction::Home if !event.repeated => self.choose(0),
                    UiAction::Accept if !event.repeated => {
                        if self.state_mode.get() == StateMode::Overwrite {
                            self.restart_answer(ui.get_player_restart_focus());
                        } else {
                            self.choose_state(self.state_focus.get());
                        }
                    }
                    UiAction::Up | UiAction::Down | UiAction::Left | UiAction::Right
                        if self.state_mode.get() == StateMode::Overwrite =>
                    {
                        ui.set_player_restart_focus(
                            if matches!(event.action, UiAction::Down | UiAction::Right) {
                                1
                            } else {
                                0
                            },
                        )
                    }
                    UiAction::Up | UiAction::Down => {
                        let slots: Vec<_> = self
                            .state_entries
                            .borrow()
                            .iter()
                            .filter(|entry| {
                                if self.state_mode.get() == StateMode::Save {
                                    entry.slot > 0
                                } else {
                                    entry.metadata.is_some()
                                }
                            })
                            .map(|entry| i32::from(entry.slot))
                            .collect();
                        if let Some(index) = slots
                            .iter()
                            .position(|slot| *slot == self.state_focus.get())
                        {
                            let index = crate::navigation::step_with_edge_wrap(
                                index as i32,
                                0,
                                slots.len() as i32 - 1,
                                if event.action == UiAction::Down {
                                    1
                                } else {
                                    -1
                                },
                                event.repeated,
                            );
                            self.state_focus.set(slots[index as usize]);
                            ui.set_player_state_focus(self.state_focus.get());
                        }
                    }
                    _ => {}
                }
                return true;
            }
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
                    self.ds_layout.get().is_some(),
                    ui.get_player_three_ds(),
                )),
                UiAction::Left | UiAction::Right
                    if ui.get_player_focus() == 1 && !event.repeated =>
                {
                    self.set_speed(
                        &ui,
                        current_speed(&ui).step(event.action == UiAction::Right),
                    )
                }
                UiAction::Left | UiAction::Right
                    if ui.get_player_focus() == SCREEN_LAYOUT && !event.repeated =>
                {
                    self.step_layout(&ui, event.action == UiAction::Right)
                }
                UiAction::Accept if !event.repeated => self.choose(ui.get_player_focus()),
                UiAction::Back | UiAction::Home if !event.repeated => self.choose(0),
                _ => {}
            }
        } else if event.action.any_stick_press() {
            // Stick clicks never open the quick menu (Guide and Escape do);
            // in a DS game L3 swaps the screens without pausing.
            if event.action == UiAction::StickPress
                && !event.repeated
                && self.ds_layout.get().is_some()
            {
                self.swap_screens(&ui);
            }
        } else if event.action == UiAction::Home && !event.repeated {
            self.pause();
        }
        // Game B/Start and menu repeats never navigate the shell underneath.
        true
    }
    fn set_state_mode(&self, ui: &AppWindow, mode: StateMode) {
        if mode != StateMode::Menu {
            self.clear_screenshot(ui);
        }
        self.state_mode.set(mode);
        ui.set_player_state_mode(mode as i32);
    }
    fn publish_states(&self, ui: &AppWindow) {
        let saving = self.state_mode.get() != StateMode::Load;
        let entries = self.state_entries.borrow();
        let allowed: Vec<_> = entries
            .iter()
            .filter(|entry| {
                if saving {
                    entry.slot > 0
                } else {
                    entry.metadata.is_some()
                }
            })
            .collect();
        if !allowed
            .iter()
            .any(|entry| i32::from(entry.slot) == self.state_focus.get())
        {
            self.state_focus.set(
                allowed
                    .iter()
                    .max_by_key(|entry| {
                        if saving {
                            -i64::from(entry.slot)
                        } else {
                            entry.metadata.as_ref().map(|m| m.saved_at).unwrap_or(0)
                        }
                    })
                    .map_or(-1, |entry| i32::from(entry.slot)),
            );
        }
        let rows = entries
            .iter()
            .map(|entry| {
                let preview = entry.preview.as_ref().map_or_else(Image::default, |frame| {
                    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                        &frame.rgba,
                        frame.width,
                        frame.height,
                    ))
                });
                let detail = entry
                    .metadata
                    .as_ref()
                    .and_then(|meta| chrono::DateTime::from_timestamp_millis(meta.saved_at))
                    .map(|date| {
                        date.with_timezone(&chrono::Local)
                            .format("%b %-d, %Y · %H:%M")
                            .to_string()
                    })
                    .unwrap_or_else(|| {
                        if entry.issue.is_some() {
                            "Unavailable".into()
                        } else {
                            "Empty slot".into()
                        }
                    });
                crate::SaveStateRow {
                    slot: i32::from(entry.slot),
                    title: if entry.slot == 0 {
                        "Automatic resume".into()
                    } else {
                        if entry.slot == crate::services::gba_player::QUICK_SLOT {
                            format!("Slot {} · Quick", entry.slot).into()
                        } else {
                            format!("Slot {}", entry.slot).into()
                        }
                    },
                    detail: detail.into(),
                    preview,
                    available: entry.metadata.is_some(),
                    occupied: entry.metadata.is_some(),
                }
            })
            .collect::<Vec<_>>();
        ui.set_player_state_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
        ui.set_player_state_focus(self.state_focus.get());
    }
    fn choose_state(&self, slot: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        if !ui.get_player_paused() || ui.get_player_busy() || !self.foreground.get() {
            return;
        }
        let Some(entry) = self
            .state_entries
            .borrow()
            .iter()
            .find(|entry| i32::from(entry.slot) == slot)
            .cloned()
        else {
            return;
        };
        self.state_focus.set(slot);
        ui.set_player_state_focus(slot);
        if self.state_mode.get() == StateMode::Save && slot > 0 {
            if entry.metadata.is_some() || entry.issue.is_some() {
                self.set_state_mode(&ui, StateMode::Overwrite);
                ui.set_player_restart_focus(0);
                self.cue(UiSoundCue::Ok);
            } else {
                self.send_action(
                    &ui,
                    Command::SaveState(entry.slot),
                    slow_state(&ui, "Saving"),
                );
            }
        } else if self.state_mode.get() == StateMode::Load && entry.metadata.is_some() {
            self.send_action(
                &ui,
                Command::LoadState {
                    slot: entry.slot,
                    quick: false,
                },
                slow_state(&ui, "Loading"),
            );
        }
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
        if self.state_mode.get() != StateMode::Menu && index == 0 {
            if self.state_mode.get() == StateMode::Overwrite {
                self.restart_answer(0);
            } else {
                let old = self.state_mode.get();
                self.set_state_mode(&ui, StateMode::Menu);
                ui.set_player_focus(if old == StateMode::Save { 2 } else { 3 });
                ui.set_player_message("".into());
                self.cue(UiSoundCue::Back);
            }
            return;
        }
        if !pause_actions(self.ds_layout.get().is_some(), ui.get_player_three_ds()).contains(&index)
            || (ui.get_player_busy() && index != RETURN_TO_HORIZON)
        {
            return;
        }
        ui.set_player_focus(index);
        if ![0, 1, 4, REWIND, RETURN_TO_HORIZON].contains(&index) {
            self.cue(UiSoundCue::Ok);
        }
        match index {
            0 if ui.get_player_paused() && self.foreground.get() => {
                self.closing.set(false);
                self.keyboard.set(0);
                self.pad_ready.set(false);
                ui.set_player_restart_confirm(false);
                self.close_pause(&ui);
                ui.set_player_message("".into());
                self.sync_pause(&ui);
            }
            1 => self.set_speed(&ui, current_speed(&ui).cycle()),
            REWIND => self.open_rewind(&ui),
            2 | 3 => {
                self.set_state_mode(
                    &ui,
                    if index == 2 {
                        StateMode::Save
                    } else {
                        StateMode::Load
                    },
                );
                self.state_focus.set(if index == 2 { 1 } else { -1 });
                self.publish_states(&ui);
                self.send_action(&ui, Command::ListStates, "");
            }
            4 => {
                self.clear_screenshot(&ui);
                self.send_action(&ui, Command::Screenshot, "");
            }
            5 => {
                self.clear_screenshot(&ui);
                ui.set_player_restart_confirm(true);
                ui.set_player_restart_focus(0);
                ui.set_player_message("".into());
            }
            SWAP_SCREENS => self.swap_screens(&ui),
            SCREEN_LAYOUT => self.step_layout(&ui, true),
            LID => {
                let closed = !ui.get_player_ds_lid_closed();
                if let Some(game) = self.running.borrow().as_ref()
                    && game.commands.send(Command::Lid(closed)).is_ok()
                {
                    ui.set_player_ds_lid_closed(closed);
                    // The game reacts to the lid as it runs.
                    self.choose(0);
                }
            }
            RETURN_TO_HORIZON => {
                self.rewind_requested.set(false);
                ui.set_player_rewind_open(false);
                self.set_state_mode(&ui, StateMode::Menu);
                ui.set_player_busy(true);
                self.exit_since.set(Some(Instant::now()));
                ui.set_player_exiting(true);
                // Saving is quick and quiet; only a failure is shown.
                ui.set_player_message("".into());
                if let Some(game) = self.running.borrow().as_ref() {
                    let _ = game.commands.send(Command::Stop);
                } else {
                    self.begin_exit(&ui);
                }
            }
            _ => {}
        }
    }
    /// Controller shortcuts during play: presses act once; fast-forward
    /// lasts while its button (or Tab) is held. Anything else (the quick
    /// menu, losing focus) ends fast-forward.
    fn poll_shortcuts(&self, ui: &AppWindow) {
        use crate::domain::controller::Shortcuts;
        let raw = self
            .shortcut_buttons
            .borrow()
            .as_ref()
            .map_or(0, |source| source());
        let shortcuts = self.shortcuts.get();
        // Rows whose buttons are all down now; a press is a row newly held.
        let held = (0..crate::domain::controller::SHORTCUT_NAMES.len()).fold(0u16, |rows, row| {
            rows | u16::from(shortcuts.held(row, raw)) << row
        });
        let pressed = held & !self.shortcuts_held.replace(held);
        let playing = self.foreground.get()
            && self.stage.get() == PlayerStage::Playing
            && !ui.get_player_paused()
            && !ui.get_player_busy()
            && !ui.get_player_failed();
        if !playing {
            self.shortcuts_ready.set(false);
            self.keyboard_fast_forward
                .set(self.keyboard_fast_forward.get() && self.foreground.get());
            self.fast_forward(ui, false);
            return;
        }
        // A button that closed a menu or returned focus must be released
        // before it can also trigger a gameplay shortcut (including Rewind).
        if !self.shortcuts_ready.get() {
            if raw == 0 {
                self.shortcuts_ready.set(true);
            } else {
                self.fast_forward(ui, false);
                return;
            }
        }
        for row in [
            Shortcuts::QUICK_SAVE,
            Shortcuts::QUICK_LOAD,
            Shortcuts::SCREENSHOT,
            Shortcuts::REWIND,
        ] {
            if pressed & (1 << row) != 0 {
                self.shortcut(ui, row);
            }
        }
        self.fast_forward(
            ui,
            !ui.get_player_rewind_open()
                && (shortcuts.held(Shortcuts::FAST_FORWARD, raw)
                    || self.keyboard_fast_forward.get()),
        );
    }

    /// Quick save, quick load or screenshot, during play only.
    fn shortcut(&self, ui: &AppWindow, row: usize) {
        use crate::domain::controller::Shortcuts;
        if !self.foreground.get()
            || self.stage.get() != PlayerStage::Playing
            || ui.get_player_paused()
            || ui.get_player_busy()
            || ui.get_player_failed()
        {
            return;
        }
        let Some(game) = self
            .running
            .borrow()
            .as_ref()
            .map(|game| game.commands.clone())
        else {
            return;
        };
        let command = match row {
            Shortcuts::QUICK_SAVE => Command::QuickSave,
            Shortcuts::QUICK_LOAD => {
                self.quick_loading.set(true);
                Command::LoadState {
                    slot: crate::services::gba_player::QUICK_SLOT,
                    quick: true,
                }
            }
            Shortcuts::SCREENSHOT => Command::Screenshot,
            Shortcuts::REWIND => {
                self.open_rewind(ui);
                return;
            }
            _ => return,
        };
        let _ = game.send(command);
    }

    /// Hold to play at 2×; let go to return to the chosen speed. Quiet: no
    /// sound, and the quick menu's own speed is what it returns to.
    fn fast_forward(&self, ui: &AppWindow, on: bool) {
        let before = self.speed_before_fast_forward.get();
        let target = match (on, before) {
            (true, None) => {
                self.speed_before_fast_forward.set(Some(current_speed(ui)));
                PlaybackSpeed::Double
            }
            (false, Some(speed)) => {
                self.speed_before_fast_forward.set(None);
                speed
            }
            _ => return,
        };
        if target != current_speed(ui)
            && let Some(game) = self.running.borrow().as_ref()
        {
            let _ = game.commands.send(Command::Speed(target));
        }
    }

    /// A short message over the game (quick save, load, screenshot).
    fn notice(&self, ui: &AppWindow, text: &str) {
        ui.set_player_notice(text.into());
        ui.set_player_notice_shown(true);
        self.notice_until
            .set(Some(Instant::now() + Duration::from_millis(1600)));
    }

    fn set_speed(&self, ui: &AppWindow, speed: PlaybackSpeed) {
        if speed != current_speed(ui) && !ui.get_player_busy() {
            self.cue(UiSoundCue::Ok);
            self.send_action(ui, Command::Speed(speed), "");
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
        if self.state_mode.get() == StateMode::Overwrite {
            if ui.get_player_busy() {
                return;
            }
            if choice == 1 {
                self.send_action(
                    &ui,
                    Command::SaveState(self.state_focus.get() as u8),
                    slow_state(&ui, "Saving"),
                );
            } else {
                self.set_state_mode(&ui, StateMode::Save);
                self.cue(UiSoundCue::Back);
            }
            return;
        }
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
        if let Some(before_return) = self.before_return.borrow().as_ref() {
            before_return();
        }
        // The game can leave the library while it runs (its file deleted):
        // then fade out in place rather than shrink into another game's card.
        let card_remains =
            self.launched_game.get().is_some() && self.selected_game() == self.launched_game.get();
        if !card_remains {
            ui.set_player_origin(crate::CardRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            });
        } else {
            if ui.get_current_route() == crate::AppRouteView::Library {
                ui.set_library_launching_index(ui.get_library_selected_index());
            } else {
                ui.set_launching_game_index(ui.get_selected_index());
            }
            ui.set_player_origin(ui.invoke_player_capture_origin());
        }
        self.close_pause(ui);
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
        ui.set_player_loading_frame(Image::default());
        ui.set_player_loading_picture(false);
        ui.set_player_saving(false);
        ui.set_player_exiting(false);
        self.exit_since.set(None);
        self.speed_before_fast_forward.set(None);
        self.keyboard_fast_forward.set(false);
        self.notice_until.set(None);
        ui.set_player_notice_shown(false);
        self.rewind_requested.set(false);
        self.rewind_rows.borrow_mut().clear();
        ui.set_player_rewind_open(false);
        ui.set_player_rewind_rows(Default::default());
        ui.set_player_rewind_preview(Image::default());
        ui.set_player_ds(false);
        self.ds_layout.set(None);
        if self.closing.get()
            && let Err(error) = slint::quit_event_loop()
        {
            tracing::warn!(%error,"could not close Horizon after saving");
        }
    }
    /// Shows the worker's newest frame, if one is waiting. Runs as soon as
    /// the worker reports it (`frame_ready`), and from the tick as a fallback.
    fn show_frame(&self, ui: &AppWindow) {
        if self.stage.get() == PlayerStage::Exiting {
            return;
        }
        // A first frame that arrives mid-zoom waits for the zoom to end (the
        // tick shows it), so uploading it never costs the animation a frame.
        if self.stage.get() == PlayerStage::Entering && !ui.get_player_has_frame() {
            return;
        }
        // At most one frame per display refresh: a frame set sooner would be
        // replaced before it is drawn, wasting its filtering.
        let wait = self.last_shown.get().map_or(Duration::ZERO, |at| {
            self.frame_period.get().saturating_sub(at.elapsed())
        });
        if wait > Duration::from_micros(500) {
            if !self.retry.running() {
                let weak = self.this.borrow().clone();
                self.retry.start(TimerMode::SingleShot, wait, move || {
                    if let Some(player) = weak.upgrade()
                        && let Some(ui) = player.ui.upgrade()
                    {
                        player.show_frame(&ui);
                    }
                });
            }
            return;
        }
        let running = self.running.borrow();
        let Some(frame) = running
            .as_ref()
            .and_then(|game| game.frame.lock().ok()?.take())
        else {
            return;
        };
        publish_display(ui, running.as_ref());
        if let Some([top, bottom]) = frame.screens {
            let size = |value: u32| i32::try_from(value).unwrap_or(i32::MAX);
            ui.set_player_ds_top_clip_width(size(top.0));
            ui.set_player_ds_top_clip_height(size(top.1));
            ui.set_player_ds_bottom_clip_width(size(bottom.0));
            ui.set_player_ds_bottom_clip_height(size(bottom.1));
        }
        drop(running);
        self.publish_ds(ui);
        let running = self.running.borrow();
        ui.set_player_smooth(frame.smooth);
        // xBRZ's output is sized in physical pixels, shown 1:1.
        let (width, height) = frame.exact.unwrap_or_default();
        ui.set_player_exact_width(width as i32);
        ui.set_player_exact_height(height as i32);
        // The worker built Slint's buffer: no copy here.
        ui.set_player_frame(Image::from_rgba8(frame.pixels));
        ui.set_player_has_frame(true);
        self.last_shown.set(Some(Instant::now()));
        self.shown.set(self.shown.get() + 1);
        let (since, before) = self.shown_report.get();
        if since.elapsed() >= Duration::from_secs(1) {
            let frames_per_second =
                (self.shown.get() - before) as f64 / since.elapsed().as_secs_f64();
            tracing::debug!(frames_per_second, "game frames shown");
            self.shown_report.set((Instant::now(), self.shown.get()));
            // The window may only learn its monitor after it appears, or move.
            self.frame_period.set(display_period(ui));
        }
        // Filter the next while this one is drawn.
        if let Some(game) = running.as_ref() {
            game.stage.want();
        }
        self.sync_pause(ui);
    }
    fn tick(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if self
            .screenshot_until
            .get()
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.clear_screenshot(&ui);
        }
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
                        ds_layout,
                        ds_swapped,
                    } => {
                        self.freeze_when_unfocused.set(freeze_when_unfocused);
                        self.ds_layout.set(ds_layout);
                        self.ds_swapped.set(ds_swapped);
                        ui.set_player_ds_lid_closed(false);
                        ui.set_player_ds(ds_layout.is_some());
                        self.publish_ds(&ui);
                        self.sync_pause(&ui);
                        ui.set_player_title(super::titles::game_title(&title).into());
                    }
                    Event::Loading {
                        preview,
                        ds_layout,
                        ds_swapped,
                    } => {
                        if ds_layout.is_some() {
                            self.ds_layout.set(ds_layout);
                            self.ds_swapped.set(ds_swapped);
                            ui.set_player_ds(true);
                            self.publish_ds(&ui);
                        }
                        let picture = preview.is_some();
                        if let Some(frame) = preview {
                            ui.set_player_loading_frame(Image::from_rgba8(SharedPixelBuffer::<
                                Rgba8Pixel,
                            >::clone_from_slice(
                                &frame.rgba,
                                frame.width,
                                frame.height,
                            )));
                        }
                        ui.set_player_loading_picture(picture);
                    }
                    Event::Started(title) => {
                        ui.set_player_title(super::titles::game_title(&title).into());
                        ui.set_player_busy(false);
                        ui.set_player_message("".into());
                    }
                    Event::Stopped => close = true,
                    Event::RewindOpened(points) => self.publish_rewind(&ui, points),
                    Event::Rewound { lid_closed } => {
                        ui.set_player_ds_lid_closed(lid_closed);
                        if !self.closing.get() {
                            self.leave_rewind(&ui, true);
                        }
                    }
                    Event::Failed(error) => {
                        self.rewind_requested.set(false);
                        ui.set_player_rewind_open(false);
                        ui.set_player_focus(0);
                        ui.set_player_failed(true);
                        ui.set_player_busy(false);
                        ui.set_player_message(error.into());
                    }
                    Event::SpeedChanged(speed) => {
                        ui.set_player_speed(i32::from(speed.factor()));
                        if !self.rewind_requested.get() {
                            ui.set_player_busy(false);
                        }
                        ui.set_player_message("".into());
                    }
                    Event::States(entries) => {
                        *self.state_entries.borrow_mut() = entries;
                        if self.state_mode.get() != StateMode::Menu && !self.closing.get() {
                            self.publish_states(&ui);
                            ui.set_player_busy(false);
                        }
                    }
                    Event::StateSaved => {
                        if !self.closing.get() {
                            self.set_state_mode(&ui, StateMode::Save);
                            ui.set_player_busy(false);
                            self.publish_states(&ui);
                            self.cue(UiSoundCue::Ok);
                        }
                    }
                    Event::QuickSaved(result) => match result {
                        Ok(()) => {
                            self.cue(UiSoundCue::Ok);
                            self.notice(
                                &ui,
                                &format!(
                                    "Saved to slot {}",
                                    crate::services::gba_player::QUICK_SLOT
                                ),
                            );
                        }
                        Err(error) => self.notice(&ui, &error),
                    },
                    Event::ScreenshotFailed(error) => {
                        if ui.get_player_paused() {
                            ui.set_player_message(error.into());
                        } else {
                            self.notice(&ui, &error);
                        }
                    }
                    Event::Notice(text) => {
                        self.quick_loading.set(false);
                        self.notice(&ui, &text);
                    }
                    Event::StateLoaded => {
                        if self.quick_loading.replace(false) {
                            self.notice(
                                &ui,
                                &format!("Loaded slot {}", crate::services::gba_player::QUICK_SLOT),
                            );
                        }
                        ui.set_player_ds_lid_closed(false);
                        if self.closing.get() {
                            continue;
                        }
                        self.set_state_mode(&ui, StateMode::Menu);
                        ui.set_player_busy(false);
                        self.close_pause(&ui);
                        self.sync_pause(&ui);
                        ui.set_player_message("".into());
                        self.keyboard.set(0);
                        self.pad_ready.set(false);
                    }
                    Event::ScreenshotSaved(frame) => {
                        if self.closing.get() {
                            continue;
                        }
                        ui.set_player_busy(false);
                        ui.set_player_message("".into());
                        if ui.get_player_paused() && self.state_mode.get() == StateMode::Menu {
                            ui.set_player_screenshot_preview(Image::from_rgba8(
                                SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                                    &frame.rgba,
                                    frame.width,
                                    frame.height,
                                ),
                            ));
                            ui.set_player_screenshot_sequence(
                                ui.get_player_screenshot_sequence().wrapping_add(1),
                            );
                            ui.set_player_screenshot_visible(true);
                            let motion = ui.global::<crate::Motion>();
                            self.screenshot_until.set(Some(
                                Instant::now()
                                    + Duration::from_millis(
                                        (motion.get_value_swap_duration()
                                            + motion.get_screenshot_hold_duration())
                                        .max(0) as u64,
                                    ),
                            ));
                            self.cue(UiSoundCue::Screenshot);
                        } else if !ui.get_player_paused() {
                            // Taken with a shortcut during play.
                            self.cue(UiSoundCue::Screenshot);
                            self.notice(&ui, "Screenshot saved");
                        }
                    }
                    Event::Restarted => {
                        ui.set_player_ds_lid_closed(false);
                        ui.set_player_busy(false);
                        ui.set_player_restart_confirm(false);
                        self.close_pause(&ui);
                        self.worker_paused.set(false); // Restart resumes the core worker.
                        self.sync_pause(&ui);
                        ui.set_player_message("".into());
                        self.keyboard.set(0);
                        self.pad_ready.set(false);
                    }
                    Event::ActionFailed(error) => {
                        if self.rewind_requested.replace(false) && !self.closing.get() {
                            ui.set_player_rewind_open(false);
                            if !self.rewind_from_menu.get() {
                                ui.set_player_paused(false);
                                self.sync_pause(&ui);
                                self.notice(&ui, &error);
                            }
                        }
                        if self.state_mode.get() == StateMode::Overwrite {
                            self.set_state_mode(&ui, StateMode::Save);
                        }
                        ui.set_player_busy(false);
                        ui.set_player_restart_confirm(false);
                        ui.set_player_message(error.into());
                    }
                    Event::Exiting(frame) => {
                        // Shown only if saving outlasts EXIT_PICTURE_DELAY.
                        ui.set_player_loading_frame(Image::from_rgba8(SharedPixelBuffer::<
                            Rgba8Pixel,
                        >::clone_from_slice(
                            &frame.rgba,
                            frame.width,
                            frame.height,
                        )));
                    }
                    Event::SaveFailed(error) => {
                        self.exit_since.set(None);
                        ui.set_player_saving(false);
                        ui.set_player_exiting(false);
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
                self.rewind_requested.set(false);
                ui.set_player_rewind_open(false);
                ui.set_player_failed(true);
                ui.set_player_busy(false);
                ui.set_player_message("The game stopped unexpectedly".into());
            }
            self.show_frame(&ui);
            let active = self.foreground.get()
                && self.stage.get() == PlayerStage::Playing
                && !ui.get_player_paused()
                && !ui.get_player_busy()
                && !ui.get_player_failed();
            if !active {
                self.three_ds_keyboard.set(0);
            }
            let mut axes = [0; 4];
            let pad = if game.three_ds {
                let (buttons, analog) = self
                    .three_ds_gamepad
                    .borrow()
                    .as_ref()
                    .map_or((0, [0; 4]), |source| source());
                axes = analog;
                let keys = self.three_ds_keyboard.get();
                if axes[0] == 0 {
                    axes[0] = (i16::from(keys & 8 != 0) - i16::from(keys & 4 != 0)) * 32767;
                }
                if axes[1] == 0 {
                    axes[1] = (i16::from(keys & 2 != 0) - i16::from(keys & 1 != 0)) * 32767;
                }
                buttons
            } else if let Some(extra) = &game.ds_input {
                // The touchscreen takes the mouse only.
                if !active {
                    extra.pointer.store(0, Ordering::Release);
                }
                self.ds_gamepad
                    .borrow()
                    .as_ref()
                    .map_or(0, |source| source())
            } else {
                self.gamepads.borrow().as_ref().map_or(0, |source| source())
            };
            if self.foreground.get() && pad == 0 && axes == [0; 4] {
                self.pad_ready.set(true);
            }
            if game.three_ds
                && let Some(extra) = &game.ds_input
            {
                let packed = if active && self.pad_ready.get() {
                    axes.into_iter()
                        .enumerate()
                        .fold(0, |packed, (index, value)| {
                            packed | (u64::from(value as u16) << (index * 16))
                        })
                } else {
                    extra.pointer.store(0, Ordering::Release);
                    0
                };
                extra.analog.store(packed, Ordering::Release);
            }
            let buttons = if !self.foreground.get()
                || self.stage.get() != PlayerStage::Playing
                || ui.get_player_paused()
                || ui.get_player_busy()
                || ui.get_player_failed()
            {
                0
            } else {
                // Azahar reads L3 as the 3DS Home button and R3 as a touch;
                // the keyboard's microphone key (L3 for DS) mustn't reach it.
                let keyboard = if game.three_ds {
                    self.keyboard.get() & !(1 << 14 | 1 << 15)
                } else {
                    self.keyboard.get()
                };
                keyboard | if self.pad_ready.get() { pad } else { 0 }
            };
            game.buttons.store(buttons, Ordering::Release);
        }
        self.poll_shortcuts(&ui);
        if self
            .notice_until
            .get()
            .is_some_and(|until| Instant::now() >= until)
        {
            self.notice_until.set(None);
            ui.set_player_notice_shown(false);
        }
        if close {
            self.exit_since.set(None);
            // The saving picture fades as the player collapses.
            ui.set_player_saving(false);
            ui.set_player_exiting(false);
            self.begin_exit(&ui);
            return;
        }
        // A save that takes a moment shows the last frame blurred, with a
        // spinner, instead of a menu that seems stuck; quick ones never do.
        if self
            .exit_since
            .get()
            .is_some_and(|since| since.elapsed() >= EXIT_PICTURE_DELAY)
            && !ui.get_player_saving()
            && !ui.get_player_failed()
        {
            ui.set_player_loading_picture(true);
            ui.set_player_saving(true);
        }
        if self.stage.get() == PlayerStage::Waiting {
            let elapsed = self.stage_since.get().elapsed();
            let hold = Duration::from_millis(
                ui.global::<crate::Motion>().get_player_press_hold().max(0) as u64,
            );
            // The zoom never waits for the game: it starts when the press
            // ends and opens onto the loading layer (the resumed state's
            // picture, or black, under a spinner) until the first frame.
            if elapsed >= hold || ui.get_player_failed() {
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
            // A frame held back during the zoom appears now.
            self.show_frame(&ui);
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
/// How long saving on exit may take before the player shows it.
const EXIT_PICTURE_DELAY: Duration = Duration::from_millis(200);

/// The display's refresh interval, a little short so a frame due right at a
/// refresh is not pushed to the next; 60 Hz when the display does not say.
/// (Pacing by Slint's rendering notifier is not possible: it is never called
/// under Skia on Vulkan.)
const PERIOD_SLACK: f64 = 0.95;
fn display_period(ui: &AppWindow) -> Duration {
    use slint::winit_030::WinitWindowAccessor;
    let millihertz = ui
        .window()
        .with_winit_window(|window| {
            window
                .current_monitor()
                .and_then(|monitor| monitor.refresh_rate_millihertz())
        })
        .flatten()
        .filter(|rate| (24_000..=500_000).contains(rate))
        .unwrap_or(60_000);
    Duration::from_secs_f64(1000.0 / f64::from(millihertz) * PERIOD_SLACK)
}

/// Tell the game worker the player's size; it follows window resizes with
/// the next frame shown.
impl GbaPlayer {
    /// Place a DS game's screens for its layout, swap and the window's shape,
    /// and tell the filter thread the sizes it should filter them at.
    fn publish_ds(&self, ui: &AppWindow) {
        let Some(layout) = self.ds_layout.get() else {
            return;
        };
        let swapped = self.ds_swapped.get();
        if let Some(input) = self
            .running
            .borrow()
            .as_ref()
            .and_then(|game| game.ds_input.as_ref())
        {
            input.view.store(
                crate::platform::gba_core::pack_ds_view(layout, swapped),
                Ordering::Release,
            );
        }
        let size = ui.window().size();
        let area = (f64::from(size.width.max(1)), f64::from(size.height.max(1)));
        let sizes = if ui.get_player_three_ds() {
            crate::domain::core_settings::DsLayout::THREE_DS_SCREENS
        } else {
            crate::domain::core_settings::DsLayout::DS_SCREENS
        };
        let [top, bottom] = layout.place(swapped, area, sizes);
        let fraction = |rect: crate::domain::core_settings::ScreenRect| crate::CardRect {
            x: (rect.x / area.0) as f32,
            y: (rect.y / area.1) as f32,
            width: (rect.width / area.0) as f32,
            height: (rect.height / area.1) as f32,
        };
        ui.set_player_ds_layout(layout.label().into());
        ui.set_player_ds_top(fraction(top));
        ui.set_player_ds_bottom(fraction(bottom));
    }

    /// The next or previous DS layout, for this session; Settings keeps the
    /// layout each launch starts with. The screens move there animated.
    fn step_layout(&self, ui: &AppWindow, forward: bool) {
        let Some(layout) = self.ds_layout.get() else {
            return;
        };
        self.ds_layout.set(Some(layout.step(forward)));
        ui.set_player_ds_layout_forward(forward);
        self.publish_ds(ui);
        self.remember_view();
    }

    /// The game is shown this way next time too.
    fn remember_view(&self) {
        if let Some(game) = self.running.borrow().as_ref() {
            let _ = game.commands.send(Command::RememberView(
                crate::services::game_storage::GameView {
                    ds_layout: self.ds_layout.get(),
                    ds_swapped: self.ds_swapped.get(),
                },
            ));
        }
    }

    /// The DS screens trade places, animated (instant with Reduced Motion).
    fn swap_screens(&self, ui: &AppWindow) {
        if self.ds_layout.get().is_none() {
            return;
        }
        self.ds_swapped.set(!self.ds_swapped.get());
        self.publish_ds(ui);
        self.remember_view();
    }
}

fn publish_display(ui: &AppWindow, running: Option<&RunningGame>) {
    if let Some(running) = running {
        let size = ui.window().size();
        running.display.store(
            u64::from(size.width) << 32 | u64::from(size.height),
            std::sync::atomic::Ordering::Release,
        );
    }
}

impl LaunchExecutor for GbaPlayer {
    fn execute(&self, target: &SourceLaunchTarget) -> Result<(), LaunchExecutionError> {
        let SourceLaunchTarget::EmbeddedCartridge { system, sha256 } = target else {
            return crate::platform::launcher::PortalLaunchExecutor.execute(target);
        };
        let result = (|| -> Result<(), String> {
            if self.is_open() {
                return Err("Close the current game first".into());
            }
            let Some(ui) = self.ui.upgrade() else {
                return Err("Horizon is closing".into());
            };
            let frame_ready = self
                .frame_ready
                .get()
                .cloned()
                .ok_or("The player is not ready")?;
            let running =
                RunningGame::start(self.paths.clone(), *system, sha256.clone(), frame_ready)?;
            ui.set_player_three_ds(running.three_ds);
            self.three_ds_keyboard.set(0);
            publish_display(&ui, Some(&running));
            *self.running.borrow_mut() = Some(running);
            self.launched_game.set(self.selected_game());
            ui.set_player_origin(ui.invoke_player_capture_origin());
            self.set_stage(&ui, PlayerStage::Waiting);
            self.closing.set(false);
            ui.set_player_loading_picture(false);
            self.last_shown.set(None);
            self.frame_period.set(display_period(&ui));
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
            ui.set_player_speed(1);
            self.clear_screenshot(&ui);
            ui.set_player_screenshot_preview(Image::default());
            self.set_state_mode(&ui, StateMode::Menu);
            self.state_entries.borrow_mut().clear();
            self.state_focus.set(-1);
            ui.set_player_state_rows(slint::ModelRc::default());
            ui.set_player_open(true);
            // Feedback belongs to the accepted request, not a first-frame or
            // resume-preview wait that can add hundreds of milliseconds.
            self.cue(UiSoundCue::Launch);
            Ok(())
        })();
        result.map_err(|error| LaunchExecutionError::new(std::io::Error::other(error)))
    }
}
/// A 3DS state takes about a second to save or load: say so meanwhile.
/// Other systems' are instant and show nothing.
fn slow_state(ui: &AppWindow, doing: &'static str) -> &'static str {
    match (ui.get_player_three_ds(), doing) {
        (true, "Saving") => "Saving state…",
        (true, _) => "Loading state…",
        _ => "",
    }
}
fn current_speed(ui: &AppWindow) -> PlaybackSpeed {
    u8::try_from(ui.get_player_speed())
        .ok()
        .and_then(|value| PlaybackSpeed::try_from(value).ok())
        .unwrap_or_default()
}

fn rewind_label(behind: Duration) -> String {
    if behind.is_zero() {
        "Now".into()
    } else {
        format!("−{:.1} s", behind.as_secs_f64())
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
        "c" => Some(9),
        "v" => Some(1),
        "a" => Some(10),
        "s" => Some(11),
        // DS: held, the microphone hears a blow.
        "b" => Some(14),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn three_ds_menu_has_states_but_no_rewind_or_lid() {
        assert_eq!(
            super::pause_actions(false, true),
            &[0, 1, 8, 7, 2, 3, 4, 5, 6]
        );
        assert_eq!(super::pause_step(1, true, false, false, true), 8);
        assert_eq!(super::pause_step(7, true, false, false, true), 2);
        assert_eq!(super::pause_step(4, false, false, false, true), 3);
        assert_eq!(super::pause_step(6, true, false, false, true), 0);
    }

    use super::*;
    fn walk(directory: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(directory).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(walk(&path));
            } else {
                found.push(path);
            }
        }
        found
    }

    #[test]
    #[ignore = "requires Wayland, PipeWire, HORIZON_DS_CORE and fresh HORIZON_DS_TEST_DIR"]
    fn real_ds_player_import_pause_resume_and_exit() {
        use crate::{
            persistence::SqliteLibraryRepository,
            services::rom_import::{self, RomSetup},
        };
        use std::path::PathBuf;
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();
        let directory = PathBuf::from(std::env::var_os("HORIZON_DS_TEST_DIR").unwrap());
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("Horizon DS.nds"),
            crate::platform::gba_core::test_ds_rom(),
        )
        .unwrap();
        let database = directory.join("library.sqlite3");
        let mut repository = SqliteLibraryRepository::open(&database).unwrap();
        let scan = rom_import::scan_folder(&directory, None).unwrap();
        assert_eq!(scan.candidates.len(), 1);
        repository
            .save_rom_import(&RomSetup::default(), &scan.candidates)
            .unwrap();
        let target = SourceLaunchTarget::EmbeddedCartridge {
            system: crate::services::rom_import::RomSystem::NintendoDs,
            sha256: scan.candidates[0]
                .fingerprint
                .as_ref()
                .unwrap()
                .sha256
                .clone(),
        };
        let ui = AppWindow::new().unwrap();
        ui.window().set_size(slint::PhysicalSize::new(1280, 720));
        let player = GbaPlayer::new(
            &ui,
            PlayerPaths {
                database,
                core: PathBuf::new(),
                ds_core: std::env::var_os("HORIZON_DS_CORE").unwrap().into(),
                three_ds_core: crate::platform::data_paths::three_ds_core_path(),
                saves: directory.join("saves"),
                album: directory.join("album"),
                settings: crate::platform::data_paths::CoreSettingsPaths {
                    playback: directory.join("playback.json"),
                    mgba: directory.join("mgba.json"),
                    ds: directory.join("melonds-ds.json"),
                    three_ds: directory.join("azahar.json"),
                },
            },
        );
        let keys = Rc::clone(&player);
        ui.on_raw_key_input(move |text, repeat| keys.keyboard(&text, repeat, true));
        let keys = Rc::clone(&player);
        ui.on_raw_key_released(move |text| keys.keyboard(&text, false, false));
        ui.show().unwrap();
        player.execute(&target).unwrap();
        let weak = ui.as_weak();
        let live = Rc::clone(&player);
        let phase = Rc::new(Cell::new(0));
        let step = Rc::clone(&phase);
        let started = Instant::now();
        let paused_at = Cell::new(None::<Instant>);
        let pictures = Rc::new(RefCell::new(Vec::new()));
        let shots = Rc::clone(&pictures);
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
            let ui = weak.upgrade().unwrap();
            assert!(!ui.get_player_failed(), "{}", ui.get_player_message());
            assert!(
                started.elapsed() < Duration::from_secs(25),
                "DS player timed out: {}",
                ui.get_player_message()
            );
            match step.get() {
                0 if live.stage.get() == PlayerStage::Playing
                    && started.elapsed() > Duration::from_secs(2) =>
                {
                    assert!(ui.get_player_has_frame());
                    shots
                        .borrow_mut()
                        .push(ui.window().take_snapshot().unwrap());
                    // A press on the touchscreen where the layout shows it
                    // (side by side: the right half), as a mouse or a
                    // touchscreen delivers it.
                    let at = slint::LogicalPosition::new(960.0, 360.0);
                    ui.window()
                        .dispatch_event(slint::platform::WindowEvent::PointerPressed {
                            position: at,
                            button: slint::platform::PointerEventButton::Left,
                        });
                    assert_ne!(
                        live.running
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .ds_input
                            .as_ref()
                            .unwrap()
                            .pointer
                            .load(Ordering::Acquire)
                            >> 32,
                        0
                    );
                    ui.window()
                        .dispatch_event(slint::platform::WindowEvent::PointerReleased {
                            position: at,
                            button: slint::platform::PointerEventButton::Left,
                        });
                    live.pause();
                    assert!(ui.get_player_paused());
                    paused_at.set(Some(Instant::now()));
                    step.set(5);
                }
                5 if paused_at
                    .get()
                    .is_some_and(|at| at.elapsed() > Duration::from_millis(500)) =>
                {
                    assert!(ui.get_player_paused());
                    // The quick menu steps the layout for this session.
                    assert_eq!(ui.get_player_ds_layout(), "Side by side");
                    ui.set_player_focus(SCREEN_LAYOUT);
                    live.handle_action(UiActionEvent::fresh(UiAction::Right));
                    assert_eq!(ui.get_player_ds_layout(), "Stacked");
                    assert!(ui.get_player_paused());
                    shots
                        .borrow_mut()
                        .push(ui.window().take_snapshot().unwrap());
                    // The last action scrolls into view at 720p.
                    ui.set_player_focus(RETURN_TO_HORIZON);
                    paused_at.set(Some(Instant::now()));
                    step.set(7);
                }
                7 if paused_at
                    .get()
                    .is_some_and(|at| at.elapsed() > Duration::from_millis(400)) =>
                {
                    shots
                        .borrow_mut()
                        .push(ui.window().take_snapshot().unwrap());
                    live.choose(0);
                    assert!(!ui.get_player_paused());
                    // L3 swaps the screens without pausing; R3 doesn't pause.
                    live.handle_action(UiActionEvent::fresh(UiAction::RightStickPress));
                    assert!(!ui.get_player_paused());
                    let before = ui.get_player_ds_top();
                    live.handle_action(UiActionEvent::fresh(UiAction::StickPress));
                    assert!(!ui.get_player_paused());
                    let after = ui.get_player_ds_bottom();
                    assert_eq!((after.x, after.y), (before.x, before.y));
                    paused_at.set(Some(Instant::now()));
                    step.set(6);
                }
                6 if paused_at
                    .get()
                    .is_some_and(|at| at.elapsed() > Duration::from_millis(600)) =>
                {
                    shots
                        .borrow_mut()
                        .push(ui.window().take_snapshot().unwrap());
                    // Close lid continues the game, which then sees the lid.
                    live.pause();
                    live.choose(LID);
                    assert!(ui.get_player_ds_lid_closed());
                    assert!(!ui.get_player_paused());
                    paused_at.set(Some(Instant::now()));
                    step.set(9);
                }
                9 if paused_at
                    .get()
                    .is_some_and(|at| at.elapsed() > Duration::from_millis(300)) =>
                {
                    // Shortcuts during play: quick save, then quick load.
                    live.shortcut(&ui, crate::domain::controller::Shortcuts::QUICK_SAVE);
                    step.set(10);
                }
                10 if ui.get_player_notice_shown() => {
                    assert_eq!(ui.get_player_notice(), "Saved to slot 1");
                    assert!(!ui.get_player_paused());
                    live.shortcut(&ui, crate::domain::controller::Shortcuts::QUICK_LOAD);
                    step.set(11);
                }
                11 if ui.get_player_notice() == "Loaded slot 1" => {
                    // Holding fast-forward plays at 2×, letting go returns.
                    ui.window()
                        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                            text: slint::platform::Key::Tab.into(),
                        });
                    step.set(12);
                }
                12 if ui.get_player_speed() == 2 => {
                    ui.window()
                        .dispatch_event(slint::platform::WindowEvent::KeyReleased {
                            text: slint::platform::Key::Tab.into(),
                        });
                    step.set(13);
                }
                13 if ui.get_player_speed() == 1 => {
                    // A screenshot is confirmed at once, as the screen shows.
                    live.shortcut(&ui, crate::domain::controller::Shortcuts::SCREENSHOT);
                    paused_at.set(Some(Instant::now()));
                    step.set(14);
                }
                14 if ui.get_player_notice() == "Screenshot saved" => {
                    assert!(
                        paused_at.get().unwrap().elapsed() < Duration::from_millis(300),
                        "screenshot confirmed late"
                    );
                    live.running
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .commands
                        .send(Command::Stop)
                        .unwrap();
                    step.set(1);
                }
                1 if !live.is_open() => {
                    live.execute(&target).unwrap();
                    step.set(2);
                }
                2 if live.stage.get() == PlayerStage::Playing => {
                    assert!(ui.get_player_has_frame());
                    // A new launch keeps the layout and swap this game was
                    // last shown with.
                    assert_eq!(ui.get_player_ds_layout(), "Stacked");
                    assert!(live.ds_swapped.get());
                    // A slow exit save shows the last frame blurred, with
                    // a spinner, instead of a dialog.
                    // SAFETY: this test's own variable, read by its worker.
                    unsafe { std::env::set_var("HORIZON_TEST_EXIT_DELAY_MS", "700") };
                    live.choose(RETURN_TO_HORIZON);
                    paused_at.set(Some(Instant::now()));
                    step.set(8);
                }
                8 if paused_at
                    .get()
                    .is_some_and(|at| at.elapsed() > Duration::from_millis(450)) =>
                {
                    assert!(ui.get_player_saving());
                    assert!(ui.get_player_exiting());
                    shots
                        .borrow_mut()
                        .push(ui.window().take_snapshot().unwrap());
                    step.set(3);
                }
                3 if !live.is_open() => {
                    assert!(!ui.get_player_saving());
                    // SAFETY: as above.
                    unsafe { std::env::remove_var("HORIZON_TEST_EXIT_DELAY_MS") };
                    step.set(4);
                    slint::quit_event_loop().unwrap();
                }
                _ => {}
            }
        });
        slint::run_event_loop().unwrap();
        assert_eq!(phase.get(), 4);
        // The screenshot: stacked and swapped, as shown (touchscreen first).
        let shot = walk(&directory.join("album"))
            .into_iter()
            .find(|path| path.extension().is_some_and(|e| e == "jxl"))
            .expect("a screenshot in Album");
        let picture = crate::services::image::decode_file(&shot)
            .unwrap()
            .into_rgba8();
        assert_eq!(picture.dimensions(), (256, 384));
        assert!(picture.get_pixel(128, 96)[2] > picture.get_pixel(128, 96)[0]);
        assert!(picture.get_pixel(128, 300)[0] > picture.get_pixel(128, 300)[2]);
        for (i, pixels) in pictures.borrow().iter().enumerate() {
            image::save_buffer(
                directory.join(format!("ds-{i}.png")),
                pixels.as_bytes(),
                pixels.width(),
                pixels.height(),
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }

    #[test]
    #[ignore = "measurement; requires Wayland, PipeWire, HORIZON_DS_CORE, a real HORIZON_DS_ROM and fresh HORIZON_DS_TEST_DIR"]
    fn real_ds_player_fast_forward_audio_report() {
        real_player_audio_report(false);
    }

    #[test]
    #[ignore = "requires Wayland, PipeWire, Azahar, HORIZON_3DS_ROM and fresh HORIZON_3DS_TEST_DIR"]
    fn real_3ds_player_audio_report() {
        real_player_audio_report(true);
    }

    fn real_player_audio_report(three_ds: bool) {
        use crate::{
            persistence::SqliteLibraryRepository,
            services::rom_import::{self, RomSetup},
        };
        use std::path::PathBuf;
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();
        let directory = PathBuf::from(
            std::env::var_os(if three_ds {
                "HORIZON_3DS_TEST_DIR"
            } else {
                "HORIZON_DS_TEST_DIR"
            })
            .unwrap(),
        );
        std::fs::create_dir_all(&directory).unwrap();
        // Kept between runs in the same folder, so a second launch can reuse
        // what the first verified.
        let game = directory.join(if three_ds { "Game.3ds" } else { "Game.nds" });
        if !game.exists() {
            std::fs::copy(
                std::env::var_os(if three_ds {
                    "HORIZON_3DS_ROM"
                } else {
                    "HORIZON_DS_ROM"
                })
                .unwrap(),
                &game,
            )
            .unwrap();
        }
        let database = directory.join("library.sqlite3");
        let mut repository = SqliteLibraryRepository::open(&database).unwrap();
        let scan = rom_import::scan_folder(&directory, None).unwrap();
        repository
            .save_rom_import(&RomSetup::default(), &scan.candidates)
            .unwrap();
        let target = SourceLaunchTarget::EmbeddedCartridge {
            system: if three_ds {
                crate::services::rom_import::RomSystem::Nintendo3ds
            } else {
                crate::services::rom_import::RomSystem::NintendoDs
            },
            sha256: scan.candidates[0]
                .fingerprint
                .as_ref()
                .unwrap()
                .sha256
                .clone(),
        };
        // HORIZON_DS_FILTER=xbrz and HORIZON_TEST_WINDOW=3440x1440 match a
        // demanding real setup.
        if let Ok(scale) = std::env::var("HORIZON_3DS_SCALE") {
            std::fs::write(
                directory.join("azahar.json"),
                format!(r#"{{"resolution":{scale}}}"#),
            )
            .unwrap();
        }
        if let Ok(filter) = std::env::var("HORIZON_DS_FILTER") {
            std::fs::write(
                directory.join("melonds-ds.json"),
                format!(
                    r#"{{"layout":"side_by_side","video_filter":"{filter}","resolution":{}}}"#,
                    std::env::var("HORIZON_DS_RESOLUTION").unwrap_or_else(|_| "1".into())
                ),
            )
            .unwrap();
        }
        let (width, height) = std::env::var("HORIZON_TEST_WINDOW")
            .ok()
            .and_then(|size| {
                let (w, h) = size.split_once('x')?;
                Some((w.parse().ok()?, h.parse().ok()?))
            })
            .unwrap_or((1280, 720));
        let ui = AppWindow::new().unwrap();
        ui.window()
            .set_size(slint::PhysicalSize::new(width, height));
        let player = GbaPlayer::new(
            &ui,
            PlayerPaths {
                database,
                core: PathBuf::new(),
                ds_core: std::env::var_os("HORIZON_DS_CORE").unwrap().into(),
                three_ds_core: std::env::var_os("HORIZON_3DS_CORE")
                    .map(PathBuf::from)
                    .unwrap_or_else(crate::platform::data_paths::three_ds_core_path),
                saves: directory.join("saves"),
                album: directory.join("album"),
                settings: crate::platform::data_paths::CoreSettingsPaths {
                    playback: directory.join("playback.json"),
                    mgba: directory.join("mgba.json"),
                    ds: directory.join("melonds-ds.json"),
                    three_ds: directory.join("azahar.json"),
                },
            },
        );
        ui.show().unwrap();
        // Let the new window draw its first frames first: those aren't the launch.
        Timer::single_shot(Duration::from_millis(500), || {
            let _ = slint::quit_event_loop();
        });
        slint::run_event_loop().unwrap();
        player.execute(&target).unwrap();
        let weak = ui.as_weak();
        let live = Rc::clone(&player);
        let started = Instant::now();
        // The launch must never stall the UI: a 4 ms timer records the
        // longest wait between its ticks for the first two seconds.
        let longest = Rc::new(Cell::new(Duration::ZERO));
        let watch = Timer::default();
        {
            let (longest, last) = (Rc::clone(&longest), Cell::new(Instant::now()));
            watch.start(TimerMode::Repeated, Duration::from_millis(4), move || {
                let gap = last.get().elapsed();
                if started.elapsed() < Duration::from_secs(2) {
                    longest.set(longest.get().max(gap));
                    if gap > Duration::from_millis(20) {
                        eprintln!("DS launch: UI waited {gap:?} at {:?}", started.elapsed());
                    }
                }
                last.set(Instant::now());
            });
        }
        let shots = Cell::new(0u32);
        let shot_directory = directory.clone();
        let normal_only = std::env::var_os("HORIZON_DS_NORMAL_ONLY").is_some();
        let run_seconds = std::env::var("HORIZON_DS_RUN_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(29);
        let three_ds_shot = Cell::new(0);
        let timer = Timer::default();
        // Eight seconds at normal speed, then 2× for twenty; the worker logs
        // audio gaps every five seconds.
        timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
            let ui = weak.upgrade().unwrap();
            // Pictures of the launch: card press, zoom, loading, playing.
            // (Snapshots stall the UI themselves: they're taken only with
            // HORIZON_LAUNCH_SHOTS, and the stall measurement is for runs
            // without them.)
            let at: &[u64] =
                if std::env::var_os("HORIZON_LAUNCH_SHOTS").is_some_and(|v| !v.is_empty()) {
                    &[150, 250, 450, 700, 1150, 1200, 1250, 1300, 1500]
                } else {
                    &[]
                };
            if let Some(&due) = at.get(shots.get() as usize)
                && started.elapsed() >= Duration::from_millis(due)
            {
                let pixels = ui.window().take_snapshot().unwrap();
                image::save_buffer(
                    shot_directory.join(format!("launch-{due}.png")),
                    pixels.as_bytes(),
                    pixels.width(),
                    pixels.height(),
                    image::ColorType::Rgba8,
                )
                .unwrap();
                shots.set(shots.get() + 1);
            }
            if started.elapsed() > Duration::from_secs(2)
                && started.elapsed() < Duration::from_millis(2100)
            {
                eprintln!(
                    "DS launch: longest UI wait {:?} (stage {:?})",
                    longest.get(),
                    live.stage.get()
                );
            }
            assert!(!ui.get_player_failed(), "{}", ui.get_player_message());
            let elapsed = started.elapsed();
            if three_ds {
                let stage = three_ds_shot.get();
                let snapshot = |name: &str| {
                    let pixels = ui.window().take_snapshot().unwrap();
                    image::save_buffer(
                        shot_directory.join(name),
                        pixels.as_bytes(),
                        pixels.width(),
                        pixels.height(),
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                };
                if stage == 0 && elapsed > Duration::from_secs(18) {
                    assert!(ui.get_player_three_ds());
                    assert!(ui.get_player_has_frame());
                    snapshot("3ds-playing.png");
                    assert!(live.keyboard("i", false, true));
                    three_ds_shot.set(1);
                } else if stage == 1 && elapsed > Duration::from_secs(19) {
                    let running = live.running.borrow();
                    let packed = running
                        .as_ref()
                        .unwrap()
                        .ds_input
                        .as_ref()
                        .unwrap()
                        .analog
                        .load(Ordering::Acquire);
                    assert_eq!((packed >> 16) as u16 as i16, -32767);
                    drop(running);
                    assert!(live.keyboard("i", false, false));
                    live.keyboard(
                        slint::SharedString::from(slint::platform::Key::Escape).as_str(),
                        false,
                        true,
                    );
                    three_ds_shot.set(2);
                } else if stage == 2 && elapsed > Duration::from_secs(20) {
                    assert!(ui.get_player_paused());
                    snapshot("3ds-quick-menu.png");
                    assert_eq!(pause_step(1, true, false, false, true), SCREEN_LAYOUT);
                    live.choose(0);
                    three_ds_shot.set(3);
                } else if stage == 3 && elapsed > Duration::from_secs(21) {
                    // A touch on the bottom screen's left edge reaches
                    // Azahar 40 pixels into its 400-wide frame.
                    ui.invoke_player_touch(0.0, 0.75, true);
                    let pointer = live
                        .running
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .ds_input
                        .as_ref()
                        .unwrap()
                        .pointer
                        .load(Ordering::Acquire);
                    assert_eq!(pointer as u16 as i16, -26214);
                    ui.invoke_player_touch(0.0, 0.75, false);
                    three_ds_shot.set(4);
                } else if (4..=7).contains(&stage)
                    && elapsed > Duration::from_millis(21_000 + 600 * (stage - 3) as u64)
                {
                    // Each layout, then the first one swapped; the view ends
                    // as it began.
                    snapshot(&format!(
                        "3ds-{}{}.png",
                        ui.get_player_ds_layout().to_lowercase().replace(' ', "-"),
                        if live.ds_swapped.get() {
                            "-swapped"
                        } else {
                            ""
                        }
                    ));
                    match stage {
                        4 | 5 => live.step_layout(&ui, true),
                        6 => {
                            live.step_layout(&ui, true);
                            live.swap_screens(&ui);
                        }
                        _ => live.swap_screens(&ui),
                    }
                    three_ds_shot.set(stage + 1);
                } else if stage == 8 && elapsed > Duration::from_millis(24_000) {
                    // A manual state: saved into slot 1, then loaded back.
                    live.send_action(&ui, Command::SaveState(1), slow_state(&ui, "Saving"));
                    assert!(ui.get_player_busy());
                    assert_eq!(ui.get_player_message(), "Saving state…");
                    three_ds_shot.set(9);
                } else if stage == 9 && !ui.get_player_busy() {
                    assert!(!ui.get_player_failed(), "{}", ui.get_player_message());
                    eprintln!("3DS state saved at {elapsed:?}");
                    live.send_action(
                        &ui,
                        Command::LoadState {
                            slot: 1,
                            quick: false,
                        },
                        slow_state(&ui, "Loading"),
                    );
                    three_ds_shot.set(10);
                } else if stage == 10 && !ui.get_player_busy() {
                    assert!(!ui.get_player_failed(), "{}", ui.get_player_message());
                    eprintln!("3DS state loaded at {elapsed:?}");
                    three_ds_shot.set(11);
                }
            }
            if !normal_only
                && elapsed > Duration::from_secs(8)
                && current_speed(&ui) == PlaybackSpeed::Normal
            {
                eprintln!("DS fast-forward: 2x");
                live.set_speed(&ui, PlaybackSpeed::Double);
            }
            if elapsed > Duration::from_secs(run_seconds)
                && live.is_open()
                && let Some(game) = live.running.borrow().as_ref()
            {
                let _ = game.commands.send(Command::Stop);
            }
            if elapsed > Duration::from_secs(run_seconds) && !live.is_open() {
                slint::quit_event_loop().unwrap();
            }
        });
        slint::run_event_loop().unwrap();
    }

    #[test]
    fn pause_navigation_includes_states_and_does_not_wrap_on_repeat() {
        assert_eq!(pause_step(1, true, false, false, false), REWIND);
        assert_eq!(pause_step(REWIND, true, false, false, false), 2);
        assert_eq!(pause_step(4, false, false, false, false), 3);
        assert_eq!(pause_step(0, false, true, false, false), 0);
        assert_eq!(pause_step(6, true, true, false, false), 6);
        assert_eq!(pause_step(6, true, false, false, false), 0);
        // DS: Screen layout, Swap screens and the lid sit between Playback
        // speed and Save state.
        assert_eq!(pause_step(1, true, false, true, false), REWIND);
        assert_eq!(pause_step(REWIND, true, false, true, false), SCREEN_LAYOUT);
        assert_eq!(
            pause_step(SCREEN_LAYOUT, true, false, true, false),
            SWAP_SCREENS
        );
        assert_eq!(pause_step(SWAP_SCREENS, true, false, true, false), LID);
        assert_eq!(pause_step(LID, true, false, true, false), 2);
        assert_eq!(pause_step(2, false, false, true, false), LID);
    }
    #[test]
    fn rewind_positions_show_the_actual_available_time() {
        assert_eq!(rewind_label(Duration::ZERO), "Now");
        assert_eq!(rewind_label(Duration::from_millis(1500)), "−1.5 s");
    }
    #[test]
    #[ignore = "requires the Slint Wayland backend"]
    fn rewind_timeline_owns_input_cancels_and_plays_its_opening_cue_once() {
        let ui = AppWindow::new().unwrap();
        let player = GbaPlayer::new(
            &ui,
            PlayerPaths {
                database: Default::default(),
                core: Default::default(),
                ds_core: Default::default(),
                three_ds_core: crate::platform::data_paths::three_ds_core_path(),
                saves: Default::default(),
                album: Default::default(),
                settings: crate::platform::data_paths::CoreSettingsPaths {
                    playback: Default::default(),
                    mgba: Default::default(),
                    ds: Default::default(),
                    three_ds: Default::default(),
                },
            },
        );
        let sounds = Rc::new(RefCell::new(Vec::new()));
        let heard = Rc::clone(&sounds);
        player.set_sound(Rc::new(move |cue| heard.borrow_mut().push(cue)));
        ui.set_player_open(true);
        ui.set_player_paused(true);
        ui.set_player_rewind_open(true);
        player.set_stage(&ui, PlayerStage::Playing);
        player.rewind_requested.set(true);
        let points = (0..3)
            .map(|index| crate::services::gba_player::RewindPoint {
                behind: Duration::from_secs(2 - index),
                preview: crate::platform::gba_core::Frame {
                    width: 2,
                    height: 2,
                    rgba: [index as u8, 0, 0, 255].repeat(4),
                    smooth: false,
                },
            })
            .collect();
        player.publish_rewind(&ui, points);
        assert_eq!(ui.get_player_rewind_selected(), 2);
        assert_eq!(sounds.borrow().as_slice(), &[UiSoundCue::RewindOpen]);
        player.publish_rewind(&ui, Vec::new());
        assert_eq!(sounds.borrow().len(), 1);
        player.handle_action(UiActionEvent::fresh(UiAction::Left));
        assert_eq!(ui.get_player_rewind_selected(), 1);
        assert_eq!(ui.get_player_rewind_position(), "−1.0 s");
        player.set_active(false);
        player.handle_action(UiActionEvent::fresh(UiAction::Right));
        assert_eq!(ui.get_player_rewind_selected(), 1);
        assert!(ui.get_player_paused());
        player.set_active(true);
        player.handle_action(UiActionEvent::fresh(UiAction::Back));
        assert!(!ui.get_player_rewind_open());
        assert!(!ui.get_player_paused());
        let held = Rc::new(Cell::new(1u16));
        let source = Rc::clone(&held);
        player.set_shortcuts(
            crate::domain::controller::Shortcuts::default()
                .assign(crate::domain::controller::Shortcuts::REWIND, 1),
        );
        player.set_shortcut_buttons(Rc::new(move || source.get()));
        player.poll_shortcuts(&ui);
        assert!(!player.shortcuts_ready.get());
        held.set(0);
        player.poll_shortcuts(&ui);
        assert!(player.shortcuts_ready.get());
        assert_eq!(sounds.borrow().last(), Some(&UiSoundCue::Back));
        assert_eq!(player.rewind_rows.borrow().len(), 3);
        assert_eq!(
            sounds
                .borrow()
                .iter()
                .filter(|cue| **cue == UiSoundCue::RewindOpen)
                .count(),
            1
        );
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
    /// The playing game's file is deleted and the library retires it: Return
    /// to Horizon fades out in place instead of shrinking into another card.
    #[test]
    #[ignore = "requires Wayland, PipeWire, built mGBA core and HORIZON_GBA_TEST_DIR"]
    fn real_player_returns_in_place_when_its_game_left_the_library() {
        let directory = PathBuf::from(std::env::var_os("HORIZON_GBA_TEST_DIR").unwrap());
        std::fs::create_dir_all(&directory).unwrap();
        let playing = directory.join("Playing.gba");
        std::fs::write(&playing, crate::platform::gba_core::test_rom()).unwrap();
        let mut other = crate::platform::gba_core::test_rom();
        other[1023] = 1;
        std::fs::write(directory.join("Other.gba"), other).unwrap();
        let database = directory.join("library.sqlite3");
        let mut repository = SqliteLibraryRepository::open(&database).unwrap();
        let scan = rom_import::scan_folder(&directory, None).unwrap();
        repository
            .save_rom_import(&RomSetup::default(), &scan.candidates)
            .unwrap();
        let games = repository.list_games().unwrap();
        let launched = games
            .iter()
            .find(|g| g.game().title().as_str() == "Playing")
            .unwrap()
            .game()
            .id();
        let core_paths = crate::platform::data_paths::CoreSettingsPaths {
            playback: directory.join("playback.json"),
            mgba: directory.join("cores/mgba.json"),
            ds: directory.join("cores/melonds-ds.json"),
            three_ds: directory.join("cores/azahar.json"),
        };
        let preferences = crate::domain::core_settings::CorePreferences {
            resume_last_state: false,
            ..Default::default()
        };
        crate::persistence::core_settings::CoreSettingsStore::new(core_paths.clone())
            .write_fixture(preferences);
        let ui = AppWindow::new().unwrap();
        let player = GbaPlayer::new(
            &ui,
            PlayerPaths {
                database: database.clone(),
                core: std::env::var_os("HORIZON_GBA_CORE").unwrap().into(),
                ds_core: crate::platform::data_paths::ds_core_path(),
                three_ds_core: crate::platform::data_paths::three_ds_core_path(),
                saves: directory.join("saves"),
                settings: core_paths,
                album: directory.join("album"),
            },
        );
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
            &[launched],
            &artwork,
            &registry,
            launch,
            activity,
        );
        let selected = Rc::clone(&home);
        player.set_selected_game(Rc::new(move || selected.selected_game_id()));
        ui.show().unwrap();
        assert_eq!(home.selected_game_id(), Some(launched.get()));
        home.handle_action(&ui, UiActionEvent::fresh(UiAction::Accept));
        assert!(ui.get_player_open());
        let step = Rc::new(Cell::new(0));
        let started = Instant::now();
        let shot = Rc::new(RefCell::new(None));
        let (live, progress, capture) = (Rc::clone(&player), Rc::clone(&step), Rc::clone(&shot));
        let folder = directory.clone();
        let weak = ui.as_weak();
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
            let ui = weak.upgrade().unwrap();
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "stuck at {}",
                progress.get()
            );
            match progress.get() {
                0 if ui.get_player_phase() == 2 && ui.get_player_has_frame() => {
                    // Deleted while playing; the next folder check retires it.
                    std::fs::remove_file(&playing).unwrap();
                    let mut repository = SqliteLibraryRepository::open(&database).unwrap();
                    let seen = [folder.join("Other.gba")].into_iter().collect();
                    assert_eq!(
                        repository.retire_missing_rom_files(&folder, &seen).unwrap(),
                        1
                    );
                    let changes = catalog.borrow_mut().merge(repository.list_games().unwrap());
                    assert_eq!(changes.removed.len(), 1);
                    home.catalog_changed(
                        &ui,
                        &changes,
                        &artwork,
                        &registry,
                        crate::services::settings::ArtworkPreferences {
                            api_key: None,
                            prefer_steamgriddb: false,
                        },
                        &[],
                    );
                    assert_ne!(home.selected_game_id(), Some(launched.get()));
                    live.choose(RETURN_TO_HORIZON);
                    progress.set(1);
                }
                1 if ui.get_player_phase() == 3 => {
                    let origin = ui.get_player_origin();
                    assert_eq!(
                        (origin.x, origin.y, origin.width, origin.height),
                        (0.0, 0.0, 1.0, 1.0)
                    );
                    assert_eq!(ui.get_launching_game_index(), -1);
                    progress.set(2);
                }
                2 if ui.get_player_visual_progress() < 0.6 && capture.borrow().is_none() => {
                    *capture.borrow_mut() = Some(ui.window().take_snapshot().unwrap());
                }
                2 if !ui.get_player_open() => {
                    progress.set(3);
                    slint::quit_event_loop().unwrap();
                }
                _ => {}
            }
        });
        slint::run_event_loop().unwrap();
        assert_eq!(step.get(), 3);
        if let Some(pixels) = shot.borrow().as_ref() {
            image::save_buffer(
                directory.join("fading-out.png"),
                pixels.as_bytes(),
                pixels.width(),
                pixels.height(),
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }

    /// A short launch from a later Home slot must return to the same game's
    /// new first slot, without waiting for the three-second history poll.
    #[test]
    #[ignore = "requires Wayland, PipeWire, built mGBA core and HORIZON_GBA_TEST_DIR"]
    fn real_player_returns_to_settled_recent_card() {
        let directory = PathBuf::from(std::env::var_os("HORIZON_GBA_TEST_DIR").unwrap());
        std::fs::create_dir_all(&directory).unwrap();
        let playing = directory.join("Playing.gba");
        std::fs::write(&playing, crate::platform::gba_core::test_rom()).unwrap();
        let mut other = crate::platform::gba_core::test_rom();
        other[1023] = 1;
        std::fs::write(directory.join("Other.gba"), other).unwrap();
        let database = directory.join("library.sqlite3");
        let mut repository = SqliteLibraryRepository::open(&database).unwrap();
        let scan = rom_import::scan_folder(&directory, None).unwrap();
        repository
            .save_rom_import(&RomSetup::default(), &scan.candidates)
            .unwrap();
        let games = repository.list_games().unwrap();
        let launched = games
            .iter()
            .find(|g| g.game().title().as_str() == "Playing")
            .unwrap()
            .game()
            .id();
        let core_paths = crate::platform::data_paths::CoreSettingsPaths {
            playback: directory.join("playback.json"),
            mgba: directory.join("cores/mgba.json"),
            ds: directory.join("cores/melonds-ds.json"),
            three_ds: directory.join("cores/azahar.json"),
        };
        let preferences = crate::domain::core_settings::CorePreferences {
            resume_last_state: false,
            ..Default::default()
        };
        crate::persistence::core_settings::CoreSettingsStore::new(core_paths.clone())
            .write_fixture(preferences);
        let ui = AppWindow::new().unwrap();
        let player = GbaPlayer::new(
            &ui,
            PlayerPaths {
                database: database.clone(),
                core: std::env::var_os("HORIZON_GBA_CORE").unwrap().into(),
                ds_core: crate::platform::data_paths::ds_core_path(),
                three_ds_core: crate::platform::data_paths::three_ds_core_path(),
                saves: directory.join("saves"),
                settings: core_paths,
                album: directory.join("album"),
            },
        );
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
            &[],
            &artwork,
            &registry,
            launch,
            activity.clone(),
        );
        let selected = Rc::clone(&home);
        player.set_selected_game(Rc::new(move || selected.selected_game_id()));
        let returned = Rc::new(Cell::new(false));
        let refreshed = Rc::clone(&returned);
        let recent_home = Rc::clone(&home);
        let weak = ui.as_weak();
        player.set_before_return(Rc::new(move || {
            let ui = weak.upgrade().unwrap();
            let ids = activity.recent_game_ids(15).unwrap();
            assert_eq!(ids.first(), Some(&launched));
            recent_home.refresh_recent_games(&ui, &ids);
            refreshed.set(true);
        }));
        ui.show().unwrap();
        home.handle_action(&ui, UiActionEvent::fresh(UiAction::Right));
        assert_eq!(ui.get_selected_index(), 1);
        assert_eq!(home.selected_game_id(), Some(launched.get()));
        home.handle_action(&ui, UiActionEvent::fresh(UiAction::Accept));
        let original = ui.get_player_origin();
        let started = Instant::now();
        let step = Rc::new(Cell::new(0));
        let progress = Rc::clone(&step);
        let live = Rc::clone(&player);
        let weak = ui.as_weak();
        let folder = directory.clone();
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
            let ui = weak.upgrade().unwrap();
            assert!(started.elapsed() < Duration::from_secs(20));
            assert!(!ui.get_player_failed(), "{}", ui.get_player_message());
            match progress.get() {
                0 if ui.get_player_phase() == 2 && ui.get_player_has_frame() => {
                    assert!(!returned.get());
                    assert_eq!(ui.get_selected_index(), 1);
                    live.choose(RETURN_TO_HORIZON);
                    progress.set(1);
                }
                1 if ui.get_player_phase() == 3 => {
                    assert!(returned.get());
                    assert_eq!(home.selected_game_id(), Some(launched.get()));
                    assert_eq!(ui.get_selected_index(), 0);
                    assert_eq!(ui.get_launching_game_index(), 0);
                    assert_eq!(ui.get_home_reorder_sequence(), 0);
                    let destination = ui.get_player_origin();
                    assert!(destination.x < original.x);
                    assert!(destination.width > 0.0 && destination.width < 1.0);
                    home.reveal_row_changes(&ui);
                    assert_eq!(ui.get_home_reorder_sequence(), 0);
                    progress.set(2);
                }
                2 if !ui.get_player_open() => {
                    assert_eq!(ui.get_selected_index(), 0);
                    assert_eq!(home.selected_game_id(), Some(launched.get()));
                    let pixels = ui.window().take_snapshot().unwrap();
                    image::save_buffer(
                        folder.join("home-return.png"),
                        pixels.as_bytes(),
                        pixels.width(),
                        pixels.height(),
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                    progress.set(3);
                    slint::quit_event_loop().unwrap();
                }
                _ => {}
            }
        });
        slint::run_event_loop().unwrap();
        assert_eq!(step.get(), 3);
    }

    /// Frames shown per second at 1× and 2×, in a real window (default
    /// 3440×1440) with the chosen filter (default Smooth pixel art).
    #[test]
    #[ignore = "requires Wayland, PipeWire, built mGBA core and HORIZON_GBA_TEST_DIR"]
    fn real_player_frame_rate_report() {
        let directory = PathBuf::from(std::env::var_os("HORIZON_GBA_TEST_DIR").unwrap());
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("Rate.gba"),
            crate::platform::gba_core::test_rom(),
        )
        .unwrap();
        let database = directory.join("library.sqlite3");
        let mut repository = SqliteLibraryRepository::open(&database).unwrap();
        let scan = rom_import::scan_folder(&directory, None).unwrap();
        repository
            .save_rom_import(&RomSetup::default(), &scan.candidates)
            .unwrap();
        let sha256 = scan.candidates[0]
            .fingerprint
            .as_ref()
            .unwrap()
            .sha256
            .clone();
        let core_paths = crate::platform::data_paths::CoreSettingsPaths {
            playback: directory.join("playback.json"),
            mgba: directory.join("cores/mgba.json"),
            ds: directory.join("cores/melonds-ds.json"),
            three_ds: directory.join("cores/azahar.json"),
        };
        let mut preferences = crate::domain::core_settings::CorePreferences::default();
        preferences.mgba.video_filter = serde_json::from_value(serde_json::Value::String(
            std::env::var("HORIZON_TEST_VIDEO_FILTER").unwrap_or("xbrz".into()),
        ))
        .unwrap();
        preferences.resume_last_state = false;
        crate::persistence::core_settings::CoreSettingsStore::new(core_paths.clone())
            .write_fixture(preferences);
        let ui = AppWindow::new().unwrap();
        let size = |name, default: u32| {
            std::env::var(name)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        };
        ui.window().set_size(slint::PhysicalSize::new(
            size("HORIZON_PLAYER_TEST_WIDTH", 3440),
            size("HORIZON_PLAYER_TEST_HEIGHT", 1440),
        ));
        let player = GbaPlayer::new(
            &ui,
            PlayerPaths {
                database,
                core: std::env::var_os("HORIZON_GBA_CORE").unwrap().into(),
                ds_core: crate::platform::data_paths::ds_core_path(),
                three_ds_core: crate::platform::data_paths::three_ds_core_path(),
                saves: directory.join("saves"),
                settings: core_paths,
                album: directory.join("album"),
            },
        );
        ui.show().unwrap();
        player
            .execute(&SourceLaunchTarget::EmbeddedCartridge {
                system: crate::services::rom_import::RomSystem::GameBoyAdvance,
                sha256,
            })
            .unwrap();
        let started = Instant::now();
        let mark = Cell::new(None::<(Instant, u64)>);
        let results = Rc::new(RefCell::new(Vec::new()));
        let (live, out) = (Rc::clone(&player), Rc::clone(&results));
        let weak = ui.as_weak();
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
            let ui = weak.upgrade().unwrap();
            let elapsed = started.elapsed();
            let measure = |label: &str| {
                if let Some((since, before)) = mark.take() {
                    out.borrow_mut().push(format!(
                        "{label}: {:.1} frames/s shown",
                        (live.shown.get() - before) as f64 / since.elapsed().as_secs_f64()
                    ));
                }
            };
            match elapsed.as_millis() {
                // Let the zoom finish, then measure 3 s at each speed.
                1500..1600 => mark.set(Some((Instant::now(), live.shown.get()))),
                4500..4600 => {
                    measure("1x");
                    live.set_speed(&ui, PlaybackSpeed::Double);
                }
                5000..5100 => mark.set(Some((Instant::now(), live.shown.get()))),
                8000..8100 => {
                    measure("2x");
                    slint::quit_event_loop().unwrap();
                }
                _ => {}
            }
        });
        slint::run_event_loop().unwrap();
        eprintln!(
            "RATE display: {:.1} Hz",
            PERIOD_SLACK / player.frame_period.get().as_secs_f64()
        );
        for line in results.borrow().iter() {
            eprintln!("RATE {line}");
        }
        drop(player);
    }

    #[test]
    #[ignore = "requires Wayland, PipeWire and built mGBA core"]
    fn real_player_window_input_pause_quit_and_relaunch() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_test_writer()
            .try_init();
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
        preferences.freeze_when_unfocused =
            std::env::var_os("HORIZON_TEST_BACKGROUND_CONTINUES").is_none();
        let freeze = preferences.freeze_when_unfocused;
        let corrected = preferences.mgba.color_correction;
        let core_paths = crate::platform::data_paths::CoreSettingsPaths {
            playback: directory.join("playback.json"),
            mgba: directory.join("cores/mgba.json"),
            ds: directory.join("cores/melonds-ds.json"),
            three_ds: directory.join("cores/azahar.json"),
        };
        crate::persistence::core_settings::CoreSettingsStore::new(core_paths.clone())
            .write_fixture(preferences);
        let storage_game = crate::domain::SourceGameRef::new(
            crate::domain::SourceId::new("gba").unwrap(),
            crate::domain::ExternalGameId::new(format!("sha256:{hash}")).unwrap(),
        );
        let save_file = crate::services::game_storage::GameFolder::prepare(
            &saves,
            &storage_game,
            "Horizon homebrew",
        )
        .unwrap()
        .save_path();
        let ui = AppWindow::new().unwrap();
        let _appearance = std::env::var_os("HORIZON_TEST_BLACK").map(|_| {
            let appearance = crate::presentation::appearance::AppearanceController::new(
                &ui,
                crate::appearance::store::AppearanceStore::new(directory.join("appearance.json")),
            )
            .unwrap();
            appearance
                .set_theme_preference(&ui, crate::appearance::ThemePreference::Black)
                .unwrap();
            appearance
        });
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
                ds_core: crate::platform::data_paths::ds_core_path(),
                three_ds_core: crate::platform::data_paths::three_ds_core_path(),
                saves: saves.clone(),
                settings: core_paths,
                album: directory.join("album"),
            },
        );
        let sounds = Rc::new(RefCell::new(Vec::new()));
        let recorded_sounds = Rc::clone(&sounds);
        let mut audio_feedback =
            std::env::var_os("HORIZON_TEST_MENU_AUDIO").map(|_| crate::audio::UiSounds::new(true));
        if let Some(sound) = audio_feedback.as_mut() {
            sound.prepare();
        }
        let audio_feedback = RefCell::new(audio_feedback);
        player.set_sound(Rc::new(move |cue| {
            recorded_sounds.borrow_mut().push(cue);
            if let Some(sound) = audio_feedback.borrow_mut().as_mut() {
                sound.play(cue);
            }
        }));
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
        assert_eq!(player.stage.get(), PlayerStage::Waiting);
        assert_eq!(
            sounds
                .borrow()
                .iter()
                .filter(|cue| **cue == UiSoundCue::Launch)
                .count(),
            1,
            "launch feedback must precede the first-frame/animation wait"
        );
        if std::env::var_os("HORIZON_TEST_DEACTIVATE_LAUNCH").is_some() {
            player.set_active(false);
        }
        let final_sounds = Rc::clone(&sounds);
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
        // Capture value and nested-view motion before it settles. Encode after
        // the event loop so PNG writes cannot interrupt the animation clock.
        let menu_key = Cell::new((-1, 1, false));
        let menu_since = Cell::new(Instant::now());
        let menu_frames = Cell::new(0u8);
        let menu_sequence = Cell::new(0u32);
        motion_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            let ui = weak_ui.upgrade().unwrap();
            let phase = ui.get_player_phase();
            let progress = ui.get_player_visual_progress();
            if !reduced && phase == 2 && ui.get_player_paused() {
                let key = (
                    ui.get_player_state_mode(),
                    ui.get_player_speed(),
                    ui.get_player_restart_confirm(),
                );
                if menu_key.replace(key) != key {
                    menu_since.set(Instant::now());
                    menu_frames.set(0);
                    menu_sequence.set(menu_sequence.get() + 1);
                }
                for (bit, milliseconds) in [(1, 40), (2, 100)] {
                    if menu_frames.get() & bit == 0
                        && menu_since.get().elapsed() >= Duration::from_millis(milliseconds)
                    {
                        captures.borrow_mut().push((
                            format!(
                                "menu-view-{}-speed-{}-{}-{milliseconds}.png",
                                key.0,
                                key.1,
                                menu_sequence.get()
                            ),
                            ui.window().take_snapshot().unwrap(),
                        ));
                        menu_frames.set(menu_frames.get() | bit);
                    }
                }
            }

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
                    assert_eq!(sounds.borrow().last(), Some(&UiSoundCue::QuickMenuOpen));
                    let sound_count = sounds.borrow().len();
                    live.pause(); // Repeated open requests cannot replay the cue.
                    assert_eq!(sounds.borrow().len(), sound_count);
                    progress.set(2);
                }
                2 => {
                    // An explicit pause survives deactivation/reactivation silently.
                    let sound_count = sounds.borrow().len();
                    live.set_active(false);
                    assert!(ui.get_player_paused());
                    live.set_active(true);
                    assert!(ui.get_player_paused());
                    assert_eq!(sounds.borrow().len(), sound_count);
                    snapshot("gba-paused");
                    tap(slint::platform::Key::DownArrow);
                    assert_eq!(ui.get_player_focus(), 1);
                    let before = ui
                        .global::<crate::Motion>()
                        .get_reduced_motion()
                        .then(|| ui.window().take_snapshot().unwrap());
                    tap(slint::platform::Key::RightArrow);
                    assert!(ui.get_player_busy());
                    if let Some(before) = before {
                        let during = ui.window().take_snapshot().unwrap();
                        assert_eq!(
                            before.as_bytes(),
                            during.as_bytes(),
                            "pause panel restyled while changing speed"
                        );
                    }
                    progress.set(10);
                }
                10 if !ui.get_player_busy() => {
                    assert_eq!(ui.get_player_speed(), 2);
                    tap(slint::platform::Key::RightArrow);
                    progress.set(40);
                }
                40 if !ui.get_player_busy() => {
                    assert_eq!(ui.get_player_speed(), 2);
                    snapshot("gba-speed-2-clamped");
                    tap(slint::platform::Key::RightArrow);
                    progress.set(41);
                }
                41 if !ui.get_player_busy() => {
                    assert_eq!(ui.get_player_speed(), 2);
                    tap(slint::platform::Key::RightArrow);
                    assert!(!ui.get_player_busy());
                    tap(slint::platform::Key::Return);
                    progress.set(42);
                }
                42 if !ui.get_player_busy() => {
                    assert_eq!(ui.get_player_speed(), 1);
                    tap(slint::platform::Key::RightArrow);
                    progress.set(43);
                }
                43 if !ui.get_player_busy() => {
                    assert_eq!(ui.get_player_speed(), 2);
                    assert!(ui.get_player_message().is_empty());
                    snapshot("gba-speed");
                    tap(slint::platform::Key::DownArrow);
                    assert_eq!(ui.get_player_focus(), REWIND);
                    tap(slint::platform::Key::DownArrow);
                    assert_eq!(ui.get_player_focus(), 2);
                    tap(slint::platform::Key::Return);
                    progress.set(20);
                }
                20 if !ui.get_player_busy() => {
                    assert_eq!(ui.get_player_state_mode(), StateMode::Save as i32);
                    assert_eq!(ui.get_player_state_focus(), 1);
                    assert_eq!(
                        live.state_entries
                            .borrow()
                            .iter()
                            .filter(|e| e.metadata.is_some())
                            .count(),
                        0
                    );
                    snapshot("states-empty");
                    tap(slint::platform::Key::Return);
                    progress.set(21);
                }
                21 if !ui.get_player_busy() => {
                    assert!(live.state_entries.borrow()[1].metadata.is_some());
                    snapshot("states-saved");
                    tap(slint::platform::Key::Return);
                    assert_eq!(ui.get_player_state_mode(), StateMode::Overwrite as i32);
                    assert_eq!(ui.get_player_restart_focus(), 0);
                    progress.set(22);
                }
                22 => {
                    snapshot("states-replace-confirm");
                    tap(slint::platform::Key::Escape);
                    assert_eq!(ui.get_player_state_mode(), StateMode::Save as i32);
                    tap(slint::platform::Key::Return);
                    assert_eq!(ui.get_player_state_mode(), StateMode::Overwrite as i32);
                    tap(slint::platform::Key::RightArrow);
                    tap(slint::platform::Key::Return);
                    progress.set(29);
                }
                29 if !ui.get_player_busy() => {
                    assert_eq!(ui.get_player_state_mode(), StateMode::Save as i32);
                    tap(slint::platform::Key::DownArrow);
                    assert_eq!(ui.get_player_state_focus(), 2);
                    tap(slint::platform::Key::Return);
                    progress.set(23);
                }
                23 if !ui.get_player_busy() => {
                    assert!(live.state_entries.borrow()[2].metadata.is_some());
                    tap(slint::platform::Key::Escape);
                    assert_eq!(ui.get_player_focus(), 2);
                    tap(slint::platform::Key::DownArrow);
                    tap(slint::platform::Key::Return);
                    progress.set(24);
                }
                24 if !ui.get_player_busy() => {
                    assert_eq!(ui.get_player_state_mode(), StateMode::Load as i32);
                    assert_eq!(ui.get_player_state_focus(), 2);
                    snapshot("states-load");
                    tap(slint::platform::Key::UpArrow);
                    assert_eq!(ui.get_player_state_focus(), 1);
                    tap(slint::platform::Key::Return);
                    progress.set(25);
                }
                25 if !ui.get_player_busy() => {
                    assert!(!ui.get_player_paused());
                    live.pause();
                    std::fs::write(directory.join("album"), b"capture obstruction").unwrap();
                    ui.invoke_player_choose(4);
                    assert!(!ui.get_player_screenshot_visible());
                    assert!(ui.get_player_message().is_empty());
                    progress.set(31);
                }
                31 if !ui.get_player_busy() => {
                    assert!(ui.get_player_paused());
                    assert!(!ui.get_player_message().is_empty());
                    assert!(!ui.get_player_screenshot_visible());
                    assert!(!sounds.borrow().contains(&UiSoundCue::Screenshot));
                    snapshot("gba-screenshot-failed");
                    std::fs::remove_file(directory.join("album")).unwrap();
                    ui.invoke_player_choose(4);
                    progress.set(11);
                }
                11 if !ui.get_player_busy() => {
                    assert!(ui.get_player_message().is_empty());
                    assert!(ui.get_player_screenshot_visible());
                    assert_eq!(sounds.borrow().last(), Some(&UiSoundCue::Screenshot));
                    let preview = ui.get_player_screenshot_preview().to_rgba8().unwrap();
                    assert_eq!((preview.width(), preview.height()), (240, 160));
                    snapshot("gba-screenshot-confirmation");
                    let captures =
                        crate::services::album::catalog::HorizonAlbum::new(directory.join("album"))
                            .scan();
                    assert_eq!(captures.len(), 1);
                    assert_eq!(
                        captures[0].game().unwrap().external_id().as_str(),
                        format!("sha256:{hash}")
                    );
                    assert_eq!(captures[0].path().extension().unwrap(), "jxl");
                    let shot = crate::services::image::decode_file(captures[0].path())
                        .unwrap()
                        .to_rgba8();
                    assert_eq!(shot.dimensions(), (240, 160));
                    assert_eq!(shot.as_raw(), preview.as_bytes());
                    let first = shot.get_pixel(0, 0);
                    assert!(shot.pixels().all(|p| p == first)); // native pixels, no panel/filter/badge
                    let first_sequence = ui.get_player_screenshot_sequence();
                    ui.invoke_player_choose(4);
                    assert!(!ui.get_player_screenshot_visible());
                    assert_eq!(ui.get_player_screenshot_sequence(), first_sequence);
                    progress.set(32);
                }
                32 if !ui.get_player_busy() => {
                    assert!(ui.get_player_screenshot_visible());
                    assert_eq!(ui.get_player_screenshot_sequence(), 2);
                    assert_eq!(
                        sounds
                            .borrow()
                            .iter()
                            .filter(|cue| **cue == UiSoundCue::Screenshot)
                            .count(),
                        2
                    );
                    assert_eq!(
                        crate::services::album::catalog::HorizonAlbum::new(directory.join("album"))
                            .scan()
                            .len(),
                        2
                    );
                    progress.set(30);
                }
                30 if !ui.get_player_screenshot_visible() => {
                    assert!(ui.get_player_paused());
                    assert_eq!(ui.get_player_focus(), 4);
                    assert!(ui.get_player_message().is_empty());
                    snapshot("gba-screenshot-cleared");
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
                    assert_eq!(ui.get_player_speed(), 2);
                    snapshot("gba-restarted");
                    // Stick clicks never open the quick menu; Guide does.
                    live.handle_action(UiActionEvent::fresh(UiAction::StickPress));
                    live.handle_action(UiActionEvent::fresh(UiAction::RightStickPress));
                    assert!(!ui.get_player_paused());
                    live.handle_action(UiActionEvent::fresh(UiAction::Home));
                    tap(slint::platform::Key::DownArrow);
                    tap(slint::platform::Key::LeftArrow);
                    progress.set(14);
                }
                14 if !ui.get_player_busy() => {
                    assert_eq!(ui.get_player_speed(), 1);
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
                    std::fs::write(
                        crate::platform::data_paths::state_slot_directory(
                            &crate::platform::data_paths::state_core_directory(
                                save_file.parent().unwrap(),
                            ),
                            0,
                        ),
                        b"exit-state obstruction",
                    )
                    .unwrap();
                    for _ in 1..pause_actions(false, false).len() {
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
                    std::fs::remove_file(crate::platform::data_paths::state_slot_directory(
                        &crate::platform::data_paths::state_core_directory(
                            save_file.parent().unwrap(),
                        ),
                        0,
                    ))
                    .unwrap();
                    live.handle_action(UiActionEvent::fresh(UiAction::Accept));
                    progress.set(4);
                }
                4 if !ui.get_player_open() => {
                    assert!(!std::fs::read(&save_file).unwrap().is_empty());
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
                    live.pause();
                    ui.invoke_player_choose(2);
                    progress.set(26);
                }
                26 if !ui.get_player_busy() => {
                    assert!(live.state_entries.borrow()[0].metadata.is_some());
                    ui.invoke_player_state_choose(3);
                    progress.set(27);
                }
                27 if !ui.get_player_busy() => {
                    let folder = crate::services::game_storage::GameFolder::prepare(
                        &saves,
                        &storage_game,
                        "Horizon homebrew",
                    )
                    .unwrap();
                    let core = crate::platform::data_paths::state_core_directory(&folder.directory);
                    let read_counter = |slot| {
                        let files = crate::platform::data_paths::state_bundle_paths(
                            &crate::platform::data_paths::state_slot_directory(&core, slot),
                            &folder.manifest.file_stem,
                            slot,
                        );
                        let bytes = std::fs::read(files.state).unwrap();
                        u32::from_le_bytes(bytes[0x1fc..0x200].try_into().unwrap())
                    };
                    assert!(
                        read_counter(3) > read_counter(0),
                        "Launch did not resume the automatic machine state"
                    );
                    tap(slint::platform::Key::Escape);
                    ui.invoke_player_choose(3);
                    progress.set(28);
                }
                28 if !ui.get_player_busy() => {
                    snapshot("states-with-automatic");
                    tap(slint::platform::Key::Escape);
                    ui.invoke_player_choose(6);
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
        // Each successful launch (the first and the relaunches) plays it.
        assert!(
            final_sounds
                .borrow()
                .iter()
                .filter(|cue| **cue == UiSoundCue::Launch)
                .count()
                >= 2,
            "{:?}",
            final_sounds.borrow()
        );
        let transitions: Vec<_> = final_sounds
            .borrow()
            .iter()
            .copied()
            .filter(|cue| matches!(cue, UiSoundCue::QuickMenuOpen | UiSoundCue::QuickMenuClose))
            .collect();
        let mut expected = vec![
            UiSoundCue::QuickMenuOpen,
            UiSoundCue::QuickMenuClose,
            UiSoundCue::QuickMenuOpen,
        ];
        // Restart completion while inactive must stay silent.
        if std::env::var_os("HORIZON_TEST_DEACTIVATE_LAUNCH").is_none() {
            expected.push(UiSoundCue::QuickMenuClose);
        }
        expected.extend([
            UiSoundCue::QuickMenuOpen,
            UiSoundCue::QuickMenuClose, // Continue
            UiSoundCue::QuickMenuOpen,
            UiSoundCue::QuickMenuClose, // Saved Home return
            UiSoundCue::QuickMenuOpen,
            UiSoundCue::QuickMenuClose, // Library return
        ]);
        assert_eq!(transitions, expected);
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
        assert_eq!(motion.get(), if reduced { 0 } else { 15 });
        assert_eq!(blends.get(), if reduced { 0 } else { 3 });
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
        if let Ok(width) = std::env::var("HORIZON_PLAYER_TEST_WIDTH") {
            let height =
                std::env::var("HORIZON_PLAYER_TEST_HEIGHT").unwrap_or_else(|_| "720".into());
            ui.window().set_size(slint::PhysicalSize::new(
                width.parse().unwrap(),
                height.parse().unwrap(),
            ));
        }
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
        ui.set_player_speed(2);
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
                let background = ui.global::<crate::Theme>().get_background();
                let expected = [background.red(), background.green(), background.blue()];
                let scale = (pixels.width() as f32 / 1280.0).min(pixels.height() as f32 / 720.0);
                let inside_right =
                    ((ui.global::<crate::Metrics>().get_pause_panel_width() - 2.0) * scale) as u32;
                // The drawer reaches both screen edges with square corners,
                // at normal, scaled and ultrawide viewport sizes.
                for (x, y) in [
                    (0, 0),
                    (inside_right, 0),
                    (0, pixels.height() - 1),
                    (inside_right, pixels.height() - 1),
                ] {
                    let corner = pixels.as_slice()[(y * pixels.width() + x) as usize];
                    assert_eq!([corner.r, corner.g, corner.b], expected);
                }
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
