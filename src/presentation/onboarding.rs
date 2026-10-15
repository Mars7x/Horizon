//! First-run setup owns input and persistence until the user reaches Home.
use crate::{
    AppWindow,
    audio::UiSoundCue,
    input::{UiAction, UiActionEvent},
    navigation::onboarding::{Direction, SetupStep, move_focus},
    persistence::SqliteLibraryRepository,
    presentation::{
        keyboard::{KeyboardController, KeyboardFeedback},
        profile::ProfileController,
        rom_setup::RomSetupController,
        wake::drain_notify,
    },
    services::{
        profile::{self, ProfilePreferences, ProfileSource},
        worker::{WorkerSender, spawn, worker_channel},
    },
};
use slint::ComponentHandle;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::mpsc::Receiver,
};

/// The shared folder importer; setup does not own a separate scan path.
pub trait OnboardingImport {
    fn open(&self);
    fn set_finished(&self, callback: Rc<dyn Fn(bool)>);
}

impl OnboardingImport for RomSetupController {
    fn open(&self) {
        RomSetupController::open(self);
    }
    fn set_finished(&self, callback: Rc<dyn Fn(bool)>) {
        RomSetupController::set_finished(self, callback);
    }
}

pub struct OnboardingParts {
    pub database: PathBuf,
    pub profile_path: PathBuf,
    pub progress: (i32, bool),
    pub profile: Rc<ProfileController>,
    pub importer: Rc<dyn OnboardingImport>,
    pub keyboard: Rc<KeyboardController>,
    pub sound: Rc<dyn Fn(UiSoundCue)>,
}
struct Saved {
    step: SetupStep,
    completed: bool,
    profile: Option<ProfilePreferences>,
}

pub struct OnboardingController {
    ui: slint::Weak<AppWindow>,
    parts: OnboardingParts,
    step: Cell<SetupStep>,
    avatar: Cell<u8>,
    avatar_changed: Cell<bool>,
    busy: Cell<bool>,
    sender: WorkerSender<Result<Saved, String>>,
    receiver: RefCell<Receiver<Result<Saved, String>>>,
}

impl OnboardingController {
    pub fn new(ui: &AppWindow, parts: OnboardingParts) -> Rc<Self> {
        let step = SetupStep::from_index(parts.progress.0).unwrap_or_default();
        let preferences = parts.profile.preferences();
        let avatar = match preferences.source {
            ProfileSource::Placeholder { index } => index.min(3),
            _ => 0,
        };
        let (notify, drain) = drain_notify(|controller: &Self| controller.drain());
        let (sender, receiver) = worker_channel(notify);
        ui.set_onboarding_open(!parts.progress.1);
        ui.set_onboarding_name_draft(preferences.name.into());
        let controller = Rc::new(Self {
            ui: ui.as_weak(),
            parts,
            step: Cell::new(step),
            avatar: Cell::new(avatar),
            avatar_changed: Cell::new(false),
            busy: Cell::new(false),
            sender,
            receiver: RefCell::new(receiver),
        });
        drain.bind(&controller);
        let weak = Rc::downgrade(&controller);
        ui.on_onboarding_choose(move |index| {
            if let Some(controller) = weak.upgrade() {
                controller.choose(index);
            }
        });
        let weak = Rc::downgrade(&controller);
        ui.on_onboarding_name_done(move || {
            if let Some(controller) = weak.upgrade() {
                controller.name_done();
            }
        });
        let weak = Rc::downgrade(&controller);
        ui.on_onboarding_navigate(move |index| {
            if let Some(controller) = weak.upgrade()
                && let Some(ui) = controller.ui.upgrade()
            {
                let action = match index {
                    0 => UiAction::Up,
                    1 => UiAction::Down,
                    2 => UiAction::Back,
                    _ => return,
                };
                controller.handle_action(&ui, UiActionEvent::fresh(action));
            }
        });
        let weak = Rc::downgrade(&controller);
        ui.on_onboarding_name_focused(move || {
            if let Some(controller) = weak.upgrade()
                && let Some(ui) = controller.ui.upgrade()
                && ui.get_onboarding_open()
                && controller.step.get() == SetupStep::Profile
                && !controller.busy.get()
            {
                ui.set_onboarding_focus(0);
            }
        });
        let weak = Rc::downgrade(&controller);
        ui.on_onboarding_name_edited(move || {
            if let Some(controller) = weak.upgrade()
                && let Some(ui) = controller.ui.upgrade()
            {
                let limited: String = ui
                    .get_onboarding_name_draft()
                    .chars()
                    .take(profile::NAME_LIMIT)
                    .collect();
                ui.set_onboarding_name_draft(limited.into());
                controller.publish(&ui);
            }
        });
        let weak = Rc::downgrade(&controller);
        controller
            .parts
            .importer
            .set_finished(Rc::new(move |saved| {
                if let Some(controller) = weak.upgrade()
                    && let Some(ui) = controller.ui.upgrade()
                    && ui.get_onboarding_open()
                    && controller.step.get() == SetupStep::Library
                    && saved
                {
                    controller.save_step(SetupStep::Ready, false, false);
                }
            }));
        controller.publish(ui);
        controller
    }

    pub fn handle_action(&self, ui: &AppWindow, event: UiActionEvent) -> bool {
        if !ui.get_onboarding_open() {
            return false;
        }
        if self.busy.get() {
            return true;
        }
        if self.parts.keyboard.is_open() {
            let before = ui.get_keyboard_focused();
            let handled = self.parts.keyboard.handle_action(ui, event);
            if handled == KeyboardFeedback::Handled && before != ui.get_keyboard_focused() {
                (self.parts.sound)(UiSoundCue::Navigation);
            }
            // Even keyboard-ignored global shortcuts cannot bypass setup.
            return true;
        }
        let direction = match event.action {
            UiAction::Up => Some(Direction::Up),
            UiAction::Down => Some(Direction::Down),
            UiAction::Left => Some(Direction::Left),
            UiAction::Right => Some(Direction::Right),
            _ => None,
        };
        if let Some(direction) = direction {
            let focus = move_focus(
                self.step.get(),
                ui.get_onboarding_focus(),
                self.avatar.get(),
                direction,
            );
            if focus != ui.get_onboarding_focus() {
                (self.parts.sound)(UiSoundCue::Navigation);
                ui.set_onboarding_focus(focus);
            }
        } else if !event.repeated {
            match event.action {
                UiAction::Accept => self.choose(ui.get_onboarding_focus()),
                UiAction::Back if self.step.get() != SetupStep::Welcome => self.back(),
                _ => {}
            }
        }
        true
    }

    fn publish(&self, ui: &AppWindow) {
        let draft = ui.get_onboarding_name_draft();
        let name = if draft.trim().is_empty() {
            "Player"
        } else {
            draft.as_str()
        };
        ui.set_onboarding_initial(profile::monogram(name).into());
        ui.set_onboarding_monogram(
            profile::placeholder_monogram(
                &ProfileSource::Placeholder {
                    index: self.avatar.get(),
                },
                name,
            )
            .into(),
        );
        ui.set_onboarding_step(self.step.get() as i32);
        ui.set_onboarding_avatar(i32::from(self.avatar.get()));
        ui.set_onboarding_avatar_changed(self.avatar_changed.get());
        ui.set_onboarding_busy(self.busy.get());
    }

    fn name_done(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if !ui.get_onboarding_open() || self.step.get() != SetupStep::Profile || self.busy.get() {
            return;
        }
        ui.set_onboarding_name_draft(profile::clean_name(&ui.get_onboarding_name_draft()).into());
        ui.invoke_text_entry_changed(2, false);
        ui.set_onboarding_focus(6);
        self.publish(&ui);
    }

    fn back(&self) {
        (self.parts.sound)(UiSoundCue::Back);
        self.save_step(self.step.get().back(), false, false);
    }

    fn choose(&self, index: i32) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if !ui.get_onboarding_open() || self.busy.get() {
            return;
        }
        ui.set_onboarding_message("".into());
        match self.step.get() {
            SetupStep::Welcome if index == 0 => {
                (self.parts.sound)(UiSoundCue::Ok);
                self.save_step(SetupStep::Profile, false, false);
            }
            SetupStep::Profile => match index {
                0 => {
                    ui.set_onboarding_focus(0);
                    ui.invoke_keyboard_show(2);
                }
                1..=4 => {
                    self.avatar.set((index - 1) as u8);
                    self.avatar_changed.set(true);
                    ui.set_onboarding_focus(index);
                    (self.parts.sound)(UiSoundCue::Ok);
                    self.publish(&ui);
                }
                5 => self.back(),
                6 => {
                    ui.invoke_text_entry_changed(2, false);
                    (self.parts.sound)(UiSoundCue::Ok);
                    self.save_step(SetupStep::Library, false, true);
                }
                _ => {}
            },
            SetupStep::Library => match index {
                0 => {
                    (self.parts.sound)(UiSoundCue::Ok);
                    self.parts.importer.open();
                }
                1 => {
                    (self.parts.sound)(UiSoundCue::Ok);
                    self.save_step(SetupStep::Ready, false, false);
                }
                2 => self.back(),
                _ => {}
            },
            SetupStep::Ready => match index {
                0 => {
                    (self.parts.sound)(UiSoundCue::Ok);
                    self.save_step(SetupStep::Ready, true, false);
                }
                1 => self.back(),
                _ => {}
            },
            _ => {}
        }
    }

    fn save_step(&self, step: SetupStep, completed: bool, save_profile: bool) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let preferences = save_profile.then(|| {
            let mut preferences = self.parts.profile.preferences();
            preferences.name = profile::clean_name(&ui.get_onboarding_name_draft());
            if preferences.name.is_empty() {
                preferences.name = "Player".into();
            }
            if self.avatar_changed.get() {
                preferences.source = ProfileSource::Placeholder {
                    index: self.avatar.get(),
                };
            }
            preferences
        });
        let database = self.parts.database.clone();
        let path = self.parts.profile_path.clone();
        let sender = self.sender.clone();
        self.busy.set(true);
        self.publish(&ui);
        if !spawn("onboarding-save", move || {
            let result = (|| {
                if let Some(preferences) = &preferences {
                    profile::save_preferences(&path, preferences).map_err(|e| e.to_string())?;
                }
                let repository =
                    SqliteLibraryRepository::open(database).map_err(|e| e.to_string())?;
                repository
                    .save_onboarding_progress(step as i32, completed)
                    .map_err(|e| e.to_string())?;
                Ok(Saved {
                    step,
                    completed,
                    profile: preferences,
                })
            })();
            let _ = sender.send(result);
        }) {
            self.busy.set(false);
            ui.set_onboarding_message("Could not start setup worker; try again".into());
            self.publish(&ui);
        }
    }

    fn drain(&self) {
        while let Ok(result) = self.receiver.borrow().try_recv() {
            self.busy.set(false);
            let Some(ui) = self.ui.upgrade() else {
                return;
            };
            match result {
                Ok(saved) => {
                    if let Some(preferences) = saved.profile {
                        ui.set_onboarding_name_draft(preferences.name.clone().into());
                        self.parts.profile.apply_saved_preferences(&ui, preferences);
                    }
                    ui.invoke_text_entry_changed(2, false);
                    self.step.set(saved.step);
                    ui.set_onboarding_focus(0);
                    ui.set_onboarding_message("".into());
                    ui.set_onboarding_open(!saved.completed);
                }
                Err(error) => {
                    ui.set_onboarding_message(format!("Could not save setup: {error}").into())
                }
            }
            self.publish(&ui);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{input::InputDevice, services::steam_account::SteamAccountService};
    use std::time::Duration;

    #[derive(Default)]
    struct ImportStub {
        opened: Cell<bool>,
        finished: crate::presentation::CallbackSlot<dyn Fn(bool)>,
    }
    impl OnboardingImport for ImportStub {
        fn open(&self) {
            self.opened.set(true);
        }
        fn set_finished(&self, callback: Rc<dyn Fn(bool)>) {
            *self.finished.borrow_mut() = Some(callback);
        }
    }

    /// Needs a Wayland session. Run as the only test in a process with isolated
    /// XDG directories; no physical controller is required for semantic actions.
    #[test]
    #[ignore = "requires a Wayland session and isolated XDG directories"]
    fn controller_setup_keyboard_and_restart_smoke() {
        let data = std::env::var_os("HORIZON_SETUP_TEST_DIR")
            .map(PathBuf::from)
            .expect("set HORIZON_SETUP_TEST_DIR to an empty test directory");
        std::fs::create_dir_all(&data).unwrap();
        let database = data.join("library.sqlite3");
        let path = data.join("profile.json");
        let repository = SqliteLibraryRepository::open(&database).unwrap();
        let ui = AppWindow::new().unwrap();
        if std::env::var_os("HORIZON_SETUP_TEST_DARK").is_some() {
            let appearance = crate::presentation::appearance::AppearanceController::new(
                &ui,
                crate::appearance::store::AppearanceStore::new(data.join("appearance.json")),
            )
            .unwrap();
            appearance
                .set_theme_preference(&ui, crate::appearance::ThemePreference::Dark)
                .unwrap();
        }
        if std::env::var_os("HORIZON_SETUP_TEST_REDUCED").is_some() {
            ui.global::<crate::Motion>().set_reduced_motion(true);
        }
        let last_device = Rc::new(Cell::new(InputDevice::Gamepad));
        let keyboard = KeyboardController::new(&ui, Rc::clone(&last_device));
        let profile = ProfileController::new(
            &ui,
            Rc::new(RefCell::new(
                SteamAccountService::load(data.join("steam.json")).unwrap(),
            )),
        );
        let importer = Rc::new(ImportStub::default());
        let controller = OnboardingController::new(
            &ui,
            OnboardingParts {
                database: database.clone(),
                profile_path: path.clone(),
                progress: repository.onboarding_progress().unwrap(),
                profile: Rc::clone(&profile),
                importer: importer.clone(),
                keyboard: Rc::clone(&keyboard),
                sound: Rc::new(|_| {}),
            },
        );
        // Dispatch physical key events through Slint focus/bubbling, rather
        // than testing only controller actions that bypass native focus.
        let (key_controller, key_ui, key_device) = (
            Rc::clone(&controller),
            ui.as_weak(),
            Rc::clone(&last_device),
        );
        ui.on_raw_key_input(move |text, repeated| {
            let Some(action) = crate::input::action_for_key(&text, repeated) else {
                return false;
            };
            let Some(ui) = key_ui.upgrade() else {
                return false;
            };
            key_device.set(InputDevice::Keyboard);
            key_controller.handle_action(&ui, UiActionEvent { action, repeated })
        });
        ui.on_raw_key_released(|text| crate::input::action_for_key(&text, false).is_some());
        ui.show().unwrap();
        let phase = Rc::new(Cell::new(0));
        let ticks = Rc::new(Cell::new(0));
        let timeout = Rc::new(Cell::new(false));
        let done = Rc::new(Cell::new(false));
        let timer = slint::Timer::default();
        let (phase_loop, ticks_loop, timeout_loop, done_loop) = (
            Rc::clone(&phase),
            Rc::clone(&ticks),
            Rc::clone(&timeout),
            Rc::clone(&done),
        );
        let live_ui = ui.as_weak();
        let live = Rc::clone(&controller);
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(350),
            move || {
                let ui = live_ui.upgrade().unwrap();
                ticks_loop.set(ticks_loop.get() + 1);
                if ticks_loop.get() > 60 {
                    timeout_loop.set(true);
                    slint::quit_event_loop().unwrap();
                    return;
                }
                if live.busy.get() {
                    return;
                }
                let act = |action| {
                    last_device.set(InputDevice::Gamepad);
                    assert!(live.handle_action(&ui, UiActionEvent::fresh(action)));
                };
                let key = |key: slint::platform::Key| {
                    use slint::platform::WindowEvent;
                    ui.window()
                        .dispatch_event_with_result(WindowEvent::KeyPressed { text: key.into() })
                        .unwrap();
                    ui.window()
                        .dispatch_event_with_result(WindowEvent::KeyReleased { text: key.into() })
                        .unwrap();
                };
                let snapshot = |name: &str| {
                    let buffer = ui.window().take_snapshot().unwrap();
                    image::save_buffer(
                        data.join(format!("{name}.png")),
                        buffer.as_bytes(),
                        buffer.width(),
                        buffer.height(),
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                };
                match phase_loop.get() {
                    0 => {
                        assert!(ui.get_onboarding_open());
                        assert_eq!(live.step.get(), SetupStep::Welcome);
                        snapshot("welcome");
                        act(UiAction::Home);
                        assert!(ui.get_onboarding_open());
                        act(UiAction::Accept);
                        phase_loop.set(1);
                    }
                    1 => {
                        assert_eq!(live.step.get(), SetupStep::Profile);
                        act(UiAction::Accept);
                        assert!(keyboard.is_open());
                        act(UiAction::Accept);
                        assert_eq!(ui.get_onboarding_name_draft(), "g");
                        phase_loop.set(2);
                    }
                    2 => {
                        snapshot("keyboard");
                        act(UiAction::Back);
                        assert!(!keyboard.is_open());
                        assert_eq!(live.step.get(), SetupStep::Profile);
                        ui.set_onboarding_name_draft("  Ariété   Player  ".into());
                        ui.invoke_onboarding_name_edited();
                        ui.invoke_keyboard_show(2);
                        act(UiAction::Menu);
                        assert!(!keyboard.is_open());
                        assert_eq!(ui.get_onboarding_focus(), 6);
                        assert_eq!(ui.get_onboarding_name_draft(), "Ariété Player");
                        act(UiAction::Up);
                        act(UiAction::Right);
                        act(UiAction::Right);
                        act(UiAction::Accept);
                        assert_eq!(live.avatar.get(), 2);
                        phase_loop.set(3);
                    }
                    3 => {
                        snapshot("profile");
                        act(UiAction::Down);
                        key(slint::platform::Key::Return);
                        phase_loop.set(4);
                    }
                    4 => {
                        assert_eq!(live.step.get(), SetupStep::Library);
                        last_device.set(InputDevice::Gamepad);
                        ui.window().request_redraw();
                        phase_loop.set(5);
                    }
                    5 => {
                        snapshot("library");
                        assert_eq!(ui.get_onboarding_focus(), 0);
                        key(slint::platform::Key::LeftArrow);
                        assert_eq!(last_device.get(), InputDevice::Keyboard);
                        assert_eq!(ui.get_onboarding_focus(), 1);
                        key(slint::platform::Key::LeftArrow);
                        assert_eq!(ui.get_onboarding_focus(), 2);
                        key(slint::platform::Key::LeftArrow);
                        assert_eq!(ui.get_onboarding_focus(), 2);
                        // Switch to controller while Back is selected, then
                        // return to keyboard without resetting semantic focus.
                        act(UiAction::Right);
                        assert_eq!(ui.get_onboarding_focus(), 1);
                        act(UiAction::Right);
                        assert_eq!(ui.get_onboarding_focus(), 0);
                        key(slint::platform::Key::LeftArrow);
                        assert_eq!(ui.get_onboarding_focus(), 1);
                        act(UiAction::Right);
                        assert_eq!(ui.get_onboarding_focus(), 0);
                        assert_eq!(profile.preferences().name, "Ariété Player");
                        assert_eq!(
                            profile.preferences().source,
                            ProfileSource::Placeholder { index: 2 }
                        );
                        act(UiAction::Accept);
                        assert!(importer.opened.get());
                        if let Some(callback) = importer.finished.borrow().as_ref() {
                            callback(false);
                        }
                        assert_eq!(live.step.get(), SetupStep::Library);
                        assert_eq!(ui.get_onboarding_focus(), 0);
                        key(slint::platform::Key::LeftArrow);
                        assert_eq!(ui.get_onboarding_focus(), 1);
                        key(slint::platform::Key::Return);
                        phase_loop.set(6);
                    }
                    6 => {
                        assert_eq!(live.step.get(), SetupStep::Ready);
                        snapshot("ready");
                        key(slint::platform::Key::LeftArrow);
                        assert_eq!(ui.get_onboarding_focus(), 1);
                        act(UiAction::Right);
                        assert_eq!(ui.get_onboarding_focus(), 0);
                        act(UiAction::Home);
                        assert!(ui.get_onboarding_open());
                        act(UiAction::Accept);
                        phase_loop.set(7);
                    }
                    7 => {
                        assert!(!ui.get_onboarding_open());
                        done_loop.set(true);
                        slint::quit_event_loop().unwrap();
                    }
                    _ => {}
                }
            },
        );
        slint::run_event_loop().unwrap();
        assert!(!timeout.get(), "setup timed out");
        assert!(done.get(), "setup did not finish");
        assert_eq!(
            SqliteLibraryRepository::open(database)
                .unwrap()
                .onboarding_progress()
                .unwrap(),
            (3, true)
        );
        let preferences = profile::load_preferences(&path);
        assert_eq!(preferences.name, "Ariété Player");
        assert_eq!(preferences.source, ProfileSource::Placeholder { index: 2 });
        ui.hide().unwrap();
    }
}
