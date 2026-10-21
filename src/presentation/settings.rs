//! Controller-first Settings pages; Slint displays state only.
use slint::{ComponentHandle, Model};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{
    AppWindow,
    appearance::{ACCENT_PRESETS, ThemePreference},
    audio::UiSoundCue,
    input::{UiAction, UiActionEvent},
    navigation::step_with_edge_wrap,
    platform::wayland_clipboard::WaylandClipboard,
    presentation::{CallbackSlot, appearance::AppearanceController},
    services::{
        controller_settings::{self, ControllerPreferences},
        settings::{ArtworkPreferences, SettingsService},
        steam_account::SteamAccountService,
    },
};

use crate::navigation::SettingsPage as SettingsView;

// SteamGridDB is a separate service; the shared prefix is not redundant.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum EditorKind {
    #[default]
    SteamGridDb,
    SteamId,
    SteamWebKey,
}

#[derive(Default)]
struct PageState {
    view: SettingsView,
    selected: i32,
    // Restore the same swatch when returning from UI Sounds or the Theme row.
    last_accent_selection: Option<i32>,
    editing_key: bool,
    editor_kind: EditorKind,
    editor_target: i32,
    feedback: String,
    /// The row whose Remove / Disconnect awaits confirmation, and the
    /// dialog's focus (0 Cancel, 1 the action).
    confirm: Option<i32>,
    confirm_focus: i32,
}

struct BindingCapture {
    row: usize,
    /// The page capturing: Game Boy mapping, DS mapping or shortcuts.
    target: SettingsView,
    /// Shortcuts: every button held so far; set once all are let go.
    combo: u16,
    since: std::time::Instant,
    armed: bool,
    device: Option<u64>,
}

pub struct SettingsController {
    page: RefCell<PageState>,
    location_changed: CallbackSlot<dyn Fn(crate::navigation::PageLocation)>,
    published_view: Cell<SettingsView>,
    service: RefCell<SettingsService>,
    steam_account: Rc<RefCell<SteamAccountService>>,
    steam_account_changed: CallbackSlot<dyn Fn()>,
    appearance: AppearanceController,
    controller: Cell<ControllerPreferences>,
    controller_io: controller_settings::ControllerSettings,
    controller_ready: Cell<bool>,
    controller_busy: Cell<bool>,
    controller_feedback: RefCell<String>,
    mapping_source: CallbackSlot<dyn Fn() -> Option<(u16, u64)>>,
    capture: RefCell<Option<BindingCapture>>,
    capture_timer: slint::Timer,
    binding_model: Rc<slint::VecModel<crate::ControllerBindingRow>>,
    cores: crate::services::core_settings::CoreSettings,
    core_values: Cell<crate::domain::core_settings::CorePreferences>,
    core_ready: Cell<bool>,
    core_busy: Cell<bool>,
    core_feedback: RefCell<String>,
    controller_changed: CallbackSlot<dyn Fn(ControllerPreferences)>,
    artwork_changed: CallbackSlot<dyn Fn(ArtworkPreferences)>,
    artwork_refresh: CallbackSlot<dyn Fn(ArtworkPreferences)>,
    sound_changed: CallbackSlot<dyn Fn(bool)>,
    action_sound: CallbackSlot<dyn Fn(UiSoundCue)>,
    wayland_clipboard: RefCell<Option<Arc<WaylandClipboard>>>,
    paste_generation: Arc<AtomicU64>,
}

impl SettingsController {
    pub fn new(
        ui: &AppWindow,
        service: SettingsService,
        appearance: AppearanceController,
        steam_account: Rc<RefCell<SteamAccountService>>,
        controller_path: PathBuf,
        core_path: crate::platform::data_paths::CoreSettingsPaths,
    ) -> Rc<Self> {
        let weak_ui = ui.as_weak();
        let (notify, drain) = super::wake::drain_notify(move |controller: &Self| {
            if let Some(ui) = weak_ui.upgrade() {
                controller.drain_cores(&ui);
                controller.drain_controller(&ui);
            }
        });
        let controller = Rc::new(Self {
            page: RefCell::new(PageState::default()),
            location_changed: RefCell::new(None),
            published_view: Cell::new(SettingsView::Root),
            service: RefCell::new(service),
            steam_account,
            steam_account_changed: RefCell::new(None),
            appearance,
            controller: Cell::default(),
            controller_io: controller_settings::ControllerSettings::new(
                controller_path,
                notify.clone(),
            ),
            controller_ready: Cell::new(false),
            controller_busy: Cell::new(false),
            controller_feedback: RefCell::new("Loading…".into()),
            mapping_source: RefCell::new(None),
            capture: RefCell::new(None),
            capture_timer: slint::Timer::default(),
            binding_model: Rc::new(slint::VecModel::default()),
            cores: crate::services::core_settings::CoreSettings::new(core_path, notify),
            core_values: Cell::default(),
            core_ready: Cell::new(false),
            core_busy: Cell::new(false),
            core_feedback: RefCell::new("Loading…".into()),
            controller_changed: RefCell::new(None),
            artwork_changed: RefCell::new(None),
            artwork_refresh: RefCell::new(None),
            sound_changed: RefCell::new(None),
            action_sound: RefCell::new(None),
            wayland_clipboard: RefCell::new(None),
            paste_generation: Arc::new(AtomicU64::new(0)),
        });
        drain.bind(&controller);
        controller.publish(ui);
        controller.bind_callbacks(ui);
        let weak = Rc::downgrade(&controller);
        let weak_ui = ui.as_weak();
        controller.capture_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(16),
            move || {
                if let (Some(controller), Some(ui)) = (weak.upgrade(), weak_ui.upgrade()) {
                    controller.poll_binding(&ui);
                }
            },
        );
        controller
    }

    fn drain_cores(&self, ui: &AppWindow) {
        for result in self.cores.results.try_iter() {
            self.core_busy.set(false);
            match result {
                Ok(values) => {
                    let was_ready = self.core_ready.replace(true);
                    self.core_values.set(values);
                    self.core_feedback.borrow_mut().clear();
                    if was_ready {
                        self.cue(UiSoundCue::Ok);
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "core preferences unavailable");
                    *self.core_feedback.borrow_mut() = if self.core_ready.get() {
                        "Couldn't save. Previous settings retained.".into()
                    } else {
                        "Couldn't load playback settings. Check saved settings and restart.".into()
                    };
                }
            }
        }
        self.publish(ui);
    }

    fn change_core(&self, ui: &AppWindow, row: i32, direction: Option<bool>) {
        if !self.core_ready.get() || self.core_busy.get() {
            return;
        }
        let mut next = self.core_values.get();
        match (self.page.borrow().view, row) {
            (SettingsView::Cores, 2) => {
                next.freeze_when_unfocused = direction.unwrap_or(!next.freeze_when_unfocused)
            }
            (SettingsView::Cores, 3) => {
                next.resume_last_state = direction.unwrap_or(!next.resume_last_state)
            }
            (SettingsView::MelonDs, 0) => {
                next.ds.layout = next.ds.layout.step(direction != Some(false))
            }
            (SettingsView::MelonDs, 1) => {
                next.ds.video_filter = next.ds.video_filter.step(direction != Some(false))
            }
            (SettingsView::MelonDs, 2) => {
                next.ds.resolution = next.ds.resolution.step(direction != Some(false))
            }
            (SettingsView::Mgba, 0) => {
                next.mgba.sunlight =
                    next.mgba
                        .sunlight
                        .step(if direction == Some(false) { -1 } else { 1 })
            }
            (SettingsView::Mgba, 1) => {
                next.mgba.video_filter = next.mgba.video_filter.step(direction != Some(false))
            }
            (SettingsView::Mgba, 2) => {
                next.mgba.color_correction = direction.unwrap_or(!next.mgba.color_correction)
            }
            (SettingsView::Mgba, 3) => {
                next.mgba.game_boy_palette =
                    next.mgba.game_boy_palette.step(direction != Some(false))
            }
            (SettingsView::Mgba, 4) => {
                next.mgba.sgb_border = direction.unwrap_or(!next.mgba.sgb_border)
            }
            _ => return,
        }
        if next == self.core_values.get() {
            return;
        }
        ui.set_settings_core_value_forward(direction != Some(false));
        match self.cores.requests.send(next) {
            Ok(()) => {
                self.core_busy.set(true);
                self.core_feedback.borrow_mut().clear();
            }
            Err(_) => *self.core_feedback.borrow_mut() = "Couldn't save core settings".into(),
        }
        self.publish(ui);
    }

    pub fn set_steam_account_changed(&self, changed: Rc<dyn Fn()>) {
        *self.steam_account_changed.borrow_mut() = Some(changed);
    }

    fn notify_steam_account_changed(&self) {
        if let Some(callback) = self.steam_account_changed.borrow().as_ref() {
            callback();
        }
    }

    /// Settings persist first; notify Home only on committed changes.
    pub fn set_artwork_changed(&self, callback: Rc<dyn Fn(ArtworkPreferences)>) {
        *self.artwork_changed.borrow_mut() = Some(callback);
    }

    /// Explicit refresh must bypass the lookup cache; ordinary preference
    /// changes continue to reuse valid cached artwork.
    pub fn set_artwork_refresh(&self, callback: Rc<dyn Fn(ArtworkPreferences)>) {
        *self.artwork_refresh.borrow_mut() = Some(callback);
    }

    pub fn controller_preferences(&self) -> ControllerPreferences {
        self.controller.get()
    }

    /// Called once a controller preference is saved.
    pub fn set_controller_changed(&self, callback: Rc<dyn Fn(ControllerPreferences)>) {
        *self.controller_changed.borrow_mut() = Some(callback);
    }

    /// Row 0 swaps select and back; row 1 whether hints swap with them.
    fn set_controller_switch(&self, ui: &AppWindow, row: i32, on: bool) {
        let current = self.controller.get();
        let next = match row {
            0 => ControllerPreferences {
                swap_confirm: on,
                ..current
            },
            1 => ControllerPreferences {
                swap_hints: on,
                ..current
            },
            _ => return,
        };
        if next == current {
            return;
        }
        self.save_controller(ui, next);
    }
    fn save_controller(&self, ui: &AppWindow, next: ControllerPreferences) {
        if !self.controller_ready.get() || self.controller_busy.get() {
            self.publish(ui);
            return;
        }
        if self.controller_io.requests.send(next).is_ok() {
            self.controller_busy.set(true);
            self.controller_feedback.borrow_mut().clear();
        } else {
            *self.controller_feedback.borrow_mut() = "Couldn't save controller settings".into();
        }
        self.publish(ui);
    }
    fn drain_controller(&self, ui: &AppWindow) {
        for result in self.controller_io.results.try_iter() {
            let saving = self.controller_busy.replace(false);
            match result {
                Ok(values) => {
                    self.controller.set(values);
                    self.controller_ready.set(true);
                    self.controller_feedback.borrow_mut().clear();
                    if let Some(callback) = self.controller_changed.borrow().as_ref() {
                        callback(values);
                    }
                    if saving {
                        self.cue(UiSoundCue::Ok);
                    }
                }
                Err(_) => {
                    *self.controller_feedback.borrow_mut() = if saving {
                        "Couldn't save. Previous bindings retained."
                    } else {
                        "Couldn't load controller settings. Check the saved file and restart."
                    }
                    .into();
                }
            }
        }
        self.publish(ui);
    }
    pub fn set_mapping_source(&self, source: Rc<dyn Fn() -> Option<(u16, u64)>>) {
        *self.mapping_source.borrow_mut() = Some(source);
    }
    pub fn cancel_binding(&self, ui: &AppWindow) {
        if self.capture.borrow_mut().take().is_some() {
            self.capture_timer.stop();
            self.cue(UiSoundCue::Back);
            self.publish(ui);
        }
    }
    fn begin_binding(&self, ui: &AppWindow, row: usize) {
        if !self.controller_ready.get() || self.controller_busy.get() {
            return;
        }
        *self.capture.borrow_mut() = Some(BindingCapture {
            row,
            target: self.page.borrow().view,
            combo: 0,
            since: std::time::Instant::now(),
            armed: false,
            device: None,
        });
        self.capture_timer.restart();
        self.controller_feedback.borrow_mut().clear();
        self.publish(ui);
    }
    fn poll_binding(&self, ui: &AppWindow) {
        if self.capture.borrow().is_none() {
            self.capture_timer.stop();
            return;
        }
        if ui.get_current_route() != crate::AppRouteView::Settings {
            self.cancel_binding(ui);
            return;
        }
        let snapshot = self
            .mapping_source
            .borrow()
            .as_ref()
            .and_then(|source| source());
        let mut capture = self.capture.borrow_mut();
        let Some(current) = capture.as_mut() else {
            return;
        };
        if current.since.elapsed() >= std::time::Duration::from_secs(10) {
            capture.take();
            drop(capture);
            *self.controller_feedback.borrow_mut() = "No button received. Try again.".into();
            self.publish(ui);
            return;
        }
        let Some((buttons, device)) = snapshot else {
            if current.device.is_some() {
                capture.take();
                drop(capture);
                *self.controller_feedback.borrow_mut() =
                    "Controller disconnected. Try again.".into();
                self.publish(ui);
            }
            return;
        };
        if current.device.is_some_and(|previous| previous != device) {
            capture.take();
            drop(capture);
            *self.controller_feedback.borrow_mut() = "Controller changed. Try again.".into();
            self.publish(ui);
            return;
        }
        current.device = Some(device);
        let valid = crate::domain::controller::SOURCES
            .iter()
            .fold(0u16, |all, bit| all | 1 << bit);
        if current.target == SettingsView::Shortcuts {
            // One or two buttons, held together, then let go.
            if !current.armed {
                current.armed = buttons == 0;
                return;
            }
            if buttons != 0 {
                current.combo |= buttons & valid;
                return;
            }
            let (row, combo) = (current.row, std::mem::take(&mut current.combo));
            if combo == 0 {
                return;
            }
            if combo.count_ones() > 2 {
                *self.controller_feedback.borrow_mut() = "Use one or two buttons.".into();
                self.publish(ui);
                return;
            }
            capture.take();
            drop(capture);
            let mut values = self.controller.get();
            values.shortcuts = values.shortcuts.assign(row, combo);
            self.save_controller(ui, values);
            return;
        }
        if buttons == 0 {
            current.armed = true;
            return;
        }
        if !current.armed {
            return;
        }
        let pressed: Vec<_> = crate::domain::controller::SOURCES
            .into_iter()
            .filter(|source| buttons & (1 << source) != 0)
            .collect();
        if pressed.len() != 1 {
            current.armed = false;
            return;
        }
        let (row, target) = (current.row, current.target);
        let source = pressed[0];
        capture.take();
        drop(capture);
        let mut values = self.controller.get();
        match target {
            SettingsView::DsBindings => {
                values.ds_bindings = values.ds_bindings.assign(row, source, values.swap_confirm)
            }
            _ => values.gba_bindings = values.gba_bindings.assign(row, source, values.swap_confirm),
        }
        self.save_controller(ui, values);
    }

    pub fn set_sound_changed(&self, callback: Rc<dyn Fn(bool)>) {
        *self.sound_changed.borrow_mut() = Some(callback);
    }

    pub fn set_action_sound(&self, callback: Rc<dyn Fn(UiSoundCue)>) {
        *self.action_sound.borrow_mut() = Some(callback);
    }

    fn cue(&self, cue: UiSoundCue) {
        if let Some(callback) = self.action_sound.borrow().as_ref() {
            callback(cue);
        }
    }

    fn set_ui_sounds_enabled(&self, ui: &AppWindow, enabled: bool) {
        if self.appearance.preferences().ui_sounds_enabled == enabled {
            return;
        }
        let result = self.appearance.set_ui_sounds_enabled(ui, enabled);
        if result.is_ok() {
            if let Some(callback) = self.sound_changed.borrow().as_ref() {
                callback(enabled);
            }
            // Off mutes immediately; On can confirm with an OK cue.
            self.cue(UiSoundCue::Ok);
        }
        self.page.borrow_mut().feedback = if result.is_ok() {
            String::new()
        } else {
            "Couldn't save".into()
        };
        self.publish(ui);
    }

    pub fn artwork_preferences(&self) -> ArtworkPreferences {
        self.service.borrow().artwork_preferences()
    }

    fn notify_artwork_changed(&self) {
        if let Some(callback) = self.artwork_changed.borrow().as_ref() {
            callback(self.artwork_preferences());
        }
    }

    pub fn set_location_changed(&self, callback: Rc<dyn Fn(crate::navigation::PageLocation)>) {
        *self.location_changed.borrow_mut() = Some(callback);
    }
    pub fn has_transient(&self, ui: &AppWindow) -> bool {
        let page = self.page.borrow();
        self.capture.borrow().is_some()
            || page.editing_key
            || page.confirm.is_some()
            || ui.get_settings_refresh_open()
    }
    pub fn restore_page(&self, ui: &AppWindow, view: SettingsView) {
        let mut page = self.page.borrow_mut();
        let selected = settings_parent(page.view)
            .filter(|(parent, _)| *parent == view)
            .map_or(0, |(_, row)| row);
        page.view = view;
        page.selected = selected;
        page.feedback.clear();
        drop(page);
        self.publish(ui);
    }

    pub fn on_enter(&self, ui: &AppWindow) {
        self.reset(ui);
        // Register wl_data_device before the first user paste. The native
        // clipboard worker then has time to observe the current selection.
        // Failure is non-fatal: try again on the actual Ctrl+V gesture.
        self.ensure_wayland_clipboard(ui);
    }
    pub fn on_leave(&self, ui: &AppWindow) {
        self.reset(ui);
    }

    fn reset(&self, ui: &AppWindow) {
        self.capture.borrow_mut().take();
        self.capture_timer.stop();
        self.paste_generation.fetch_add(1, Ordering::Relaxed);
        *self.page.borrow_mut() = PageState::default();
        ui.set_settings_editing_key(false);
        ui.set_settings_refresh_open(false);
        ui.set_settings_key_draft("".into());
        self.publish(ui);
    }

    /// Returns true if the Settings page owns this semantic action.
    /// Back from the category root is left to the normal Navigator.
    pub fn handle_action(&self, ui: &AppWindow, event: UiActionEvent) -> bool {
        let action = event.action;
        if self.capture.borrow().is_some() {
            let keyboard =
                ui.global::<crate::InputGlyphs>().get_family() == crate::GlyphFamily::Keyboard;
            if action == UiAction::Home
                || action.any_stick_press()
                || (keyboard && matches!(action, UiAction::Back | UiAction::Accept))
            {
                self.cancel_binding(ui);
            }
            return true;
        }
        // A modal refresh owns navigation until dismissed. Hide does not cancel
        // the worker; existing covers continue updating in the background.
        if ui.get_settings_refresh_open() {
            if action == UiAction::Back || (action == UiAction::Accept && !event.repeated) {
                ui.set_settings_refresh_open(false);
                self.cue(if action == UiAction::Back {
                    UiSoundCue::Back
                } else {
                    UiSoundCue::Ok
                });
            } else if action == UiAction::Home {
                ui.set_settings_refresh_open(false);
                return false;
            }
            return true;
        }
        if self.page.borrow().confirm.is_some() {
            match action {
                UiAction::Left | UiAction::Up | UiAction::Right | UiAction::Down => {
                    self.page.borrow_mut().confirm_focus =
                        i32::from(matches!(action, UiAction::Right | UiAction::Down));
                    self.publish(ui);
                }
                UiAction::Accept if !event.repeated => {
                    let focus = self.page.borrow().confirm_focus;
                    self.choose_confirm(ui, focus);
                }
                UiAction::Back => self.choose_confirm(ui, 0),
                UiAction::Home => {
                    self.page.borrow_mut().confirm = None;
                    self.publish(ui);
                    return false;
                }
                _ => {}
            }
            return true;
        }
        if self.page.borrow().editing_key {
            match action {
                UiAction::Back => self.cancel_key(ui),
                UiAction::Accept if !event.repeated => {
                    let target = self.page.borrow().editor_target;
                    if target == 0 {
                        // A on the field brings up the on-screen keyboard
                        // (Enter on a physical keyboard goes to the field).
                        ui.invoke_keyboard_show(0);
                    } else {
                        ui.invoke_settings_activate_editor_target(target);
                    }
                }
                UiAction::Up | UiAction::Down | UiAction::Left | UiAction::Right => {
                    // Logical 2x2 layout: input | eye / Cancel | Save.
                    // Inapplicable Save is skipped until text is provided.
                    let can_save = !ui.get_settings_key_draft().is_empty();
                    let mut page = self.page.borrow_mut();
                    page.editor_target = if page.editor_kind == EditorKind::SteamId {
                        next_id_editor_target(page.editor_target, action, can_save)
                    } else {
                        next_editor_target(page.editor_target, action, can_save)
                    };
                    let target = page.editor_target;
                    drop(page);
                    ui.set_settings_editor_target(target);
                }
                UiAction::Home => {
                    self.cancel_key(ui);
                    return false; // preserve global Home behavior
                }
                _ => {}
            }
            return true;
        }

        match action {
            UiAction::Back => return self.back_inside(ui),
            UiAction::Up | UiAction::Down => {
                let mut page = self.page.borrow_mut();
                let count = selection_count(page.view);
                let delta = if action == UiAction::Up { -1 } else { 1 };
                page.selected = if page.view == SettingsView::Appearance {
                    appearance_step_vertical(
                        page.selected,
                        action,
                        page.last_accent_selection.unwrap_or(4),
                        event.repeated,
                    )
                } else if page.view == SettingsView::SteamGridDb
                    && !self.service.borrow().has_steamgriddb_key()
                {
                    steamgriddb_step_without_key(page.selected, delta, event.repeated)
                } else {
                    step_with_edge_wrap(page.selected, 0, count - 1, delta, event.repeated)
                };
                if page.view == SettingsView::Appearance && (4..=13).contains(&page.selected) {
                    page.last_accent_selection = Some(page.selected);
                }
            }
            UiAction::Left | UiAction::Right => {
                let page = self.page.borrow();
                if page.view == SettingsView::SteamGridDb && page.selected == 1 {
                    let preferred = action == UiAction::Right;
                    drop(page);
                    self.set_preference(ui, preferred);
                } else if page.view == SettingsView::Appearance && page.selected == 14 {
                    drop(page);
                    self.set_ui_sounds_enabled(ui, action == UiAction::Right);
                } else if matches!(page.view, SettingsView::Mgba | SettingsView::MelonDs)
                    || (page.view == SettingsView::Cores && matches!(page.selected, 2 | 3))
                {
                    let row = page.selected;
                    drop(page);
                    self.change_core(ui, row, Some(action == UiAction::Right));
                } else if page.view == SettingsView::Controller {
                    let row = page.selected;
                    drop(page);
                    self.set_controller_switch(ui, row, action == UiAction::Right);
                } else if page.view == SettingsView::Appearance {
                    drop(page);
                    let mut page = self.page.borrow_mut();
                    page.selected =
                        appearance_step_horizontal(page.selected, action, event.repeated);
                    if (4..=13).contains(&page.selected) {
                        page.last_accent_selection = Some(page.selected);
                    }
                }
            }
            UiAction::Accept if !event.repeated => {
                let selected = self.page.borrow().selected;
                self.activate(ui, selected);
            }
            UiAction::Secondary if !event.repeated => {
                let (view, selected) = {
                    let page = self.page.borrow();
                    (page.view, page.selected)
                };
                if view == SettingsView::Shortcuts {
                    self.clear_shortcut(ui, selected);
                } else {
                    self.ask_remove(ui, selected);
                }
                return true;
            }
            UiAction::Home | UiAction::Menu => return false,
            _ => {}
        }
        self.publish(ui);
        true
    }

    fn bind_callbacks(self: &Rc<Self>, ui: &AppWindow) {
        let core = Rc::downgrade(self);
        let weak = ui.as_weak();
        ui.on_settings_core_decrease(move || {
            if let (Some(core), Some(ui)) = (core.upgrade(), weak.upgrade())
                && core.page.borrow().view == SettingsView::Mgba
            {
                core.change_core(&ui, 0, Some(false));
            }
        });
        let weak = ui.as_weak();
        let capture_self = Rc::clone(self);
        let capture_ui = ui.as_weak();
        ui.on_settings_cancel_binding(move || {
            if let Some(ui) = capture_ui.upgrade() {
                capture_self.cancel_binding(&ui);
            }
        });
        let controller = Rc::clone(self);
        ui.on_settings_activate(move |index| {
            if let Some(ui) = weak.upgrade() {
                controller.activate(&ui, index);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_settings_back(move || {
            if let Some(ui) = weak.upgrade() {
                controller.back_inside(&ui);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_settings_dismiss_refresh(move || {
            if let Some(ui) = weak.upgrade() {
                controller.back_inside(&ui);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_settings_save_key(move |key| {
            if let Some(ui) = weak.upgrade() {
                controller.save_key(&ui, key.as_str());
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_settings_cancel_key(move || {
            if let Some(ui) = weak.upgrade() {
                controller.cancel_key(&ui);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_settings_request_wayland_paste(move || {
            if let Some(ui) = weak.upgrade() {
                controller.paste_from_wayland(&ui);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_settings_remove(move |row| {
            if let Some(ui) = weak.upgrade() {
                controller.ask_remove(&ui, row);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_settings_confirm(move |choice| {
            if let Some(ui) = weak.upgrade() {
                controller.choose_confirm(&ui, choice);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_settings_paste_failed(move || {
            if let Some(ui) = weak.upgrade() {
                controller.page.borrow_mut().feedback = "Couldn't paste".into();
                controller.publish(&ui);
            }
        });
    }

    fn activate(&self, ui: &AppWindow, selected: i32) {
        if self.page.borrow().editing_key {
            return;
        }
        let view = self.page.borrow().view;
        match view {
            SettingsView::Root => {
                if root_destination(selected).is_none() {
                    return;
                }
                let mut page = self.page.borrow_mut();
                page.view = root_destination(selected).expect("root selection checked above");
                page.selected = 0;
                page.feedback.clear();
                drop(page);
                self.cue(UiSoundCue::Ok);
                self.publish(ui);
                return;
            }
            SettingsView::Appearance => {
                if !(0..=14).contains(&selected) {
                    return;
                }
                {
                    let mut page = self.page.borrow_mut();
                    page.selected = selected;
                    if (4..=13).contains(&selected) {
                        page.last_accent_selection = Some(selected);
                    }
                }
                if selected == 14 {
                    let enabled = !self.appearance.preferences().ui_sounds_enabled;
                    self.set_ui_sounds_enabled(ui, enabled);
                    return;
                }
                let result = match selected {
                    0 => self
                        .appearance
                        .set_theme_preference(ui, ThemePreference::System),
                    1 => self
                        .appearance
                        .set_theme_preference(ui, ThemePreference::Light),
                    2 => self
                        .appearance
                        .set_theme_preference(ui, ThemePreference::Dark),
                    3 => self
                        .appearance
                        .set_theme_preference(ui, ThemePreference::Black),
                    4 => self.appearance.use_system_accent(ui),
                    _ => self
                        .appearance
                        .set_custom_accent(ui, ACCENT_PRESETS[(selected - 5) as usize].1),
                };
                self.page.borrow_mut().feedback = if result.is_ok() {
                    String::new()
                } else {
                    "Couldn't save".into()
                };
                if result.is_ok() {
                    self.cue(UiSoundCue::Ok);
                }
                self.publish(ui);
                return;
            }
            SettingsView::Cores => {
                if matches!(selected, 2 | 3) {
                    self.page.borrow_mut().selected = selected;
                    self.change_core(ui, selected, None);
                    return;
                }
                let child = match selected {
                    0 => SettingsView::Mgba,
                    1 => SettingsView::MelonDs,
                    _ => return,
                };
                let mut page = self.page.borrow_mut();
                page.view = child;
                page.selected = 0;
                drop(page);
                self.cue(UiSoundCue::Ok);
                self.publish(ui);
                return;
            }
            SettingsView::Mgba => {
                self.page.borrow_mut().selected = selected.clamp(0, 4);
                self.change_core(ui, selected, None);
                return;
            }
            SettingsView::MelonDs => {
                self.page.borrow_mut().selected = selected.clamp(0, 2);
                self.change_core(ui, selected, None);
                return;
            }
            SettingsView::GameBindings | SettingsView::DsBindings | SettingsView::Shortcuts => {
                if !self.controller_ready.get() || self.controller_busy.get() {
                    return;
                }
                if self.capture.borrow().is_some() {
                    return;
                }
                // The last row restores the defaults.
                let reset = selection_count(view) - 1;
                if (0..reset).contains(&selected) {
                    self.page.borrow_mut().selected = selected;
                    self.begin_binding(ui, selected as usize);
                } else if selected == reset {
                    let mut page = self.page.borrow_mut();
                    page.selected = reset;
                    page.confirm = Some(reset);
                    page.confirm_focus = 0;
                    drop(page);
                    self.publish(ui);
                }
                return;
            }
            SettingsView::MappingCores => {
                let child = match selected {
                    0 => SettingsView::GameBindings,
                    1 => SettingsView::DsBindings,
                    _ => return,
                };
                let mut page = self.page.borrow_mut();
                page.view = child;
                page.selected = 0;
                drop(page);
                self.cue(UiSoundCue::Ok);
                self.publish(ui);
                return;
            }
            SettingsView::Controller => {
                if matches!(selected, 2 | 3) {
                    let mut page = self.page.borrow_mut();
                    page.view = if selected == 2 {
                        SettingsView::MappingCores
                    } else {
                        SettingsView::Shortcuts
                    };
                    page.selected = 0;
                    drop(page);
                    self.cue(UiSoundCue::Ok);
                    self.publish(ui);
                    return;
                }
                let current = self.controller.get();
                let on = match selected {
                    0 => !current.swap_confirm,
                    1 => !current.swap_hints,
                    _ => return,
                };
                self.page.borrow_mut().selected = selected;
                self.set_controller_switch(ui, selected, on);
                return;
            }
            SettingsView::ThirdParty => {
                let destination = match selected {
                    0 => SettingsView::SteamGridDb,
                    1 => SettingsView::SteamAccount,
                    _ => return,
                };
                let mut page = self.page.borrow_mut();
                page.view = destination;
                page.selected = 0;
                page.feedback.clear();
                drop(page);
                self.cue(UiSoundCue::Ok);
                self.publish(ui);
                return;
            }
            SettingsView::SteamGridDb | SettingsView::SteamAccount => {}
        }
        if view == SettingsView::SteamAccount {
            if !(0..=1).contains(&selected) {
                return;
            }
            self.page.borrow_mut().selected = selected;
            match selected {
                0 => self.open_editor(ui, EditorKind::SteamId),
                _ => self.open_editor(ui, EditorKind::SteamWebKey),
            }
            self.publish(ui);
            return;
        }
        if !(0..=2).contains(&selected) {
            return;
        }
        if selected == 2 && !self.service.borrow().has_steamgriddb_key() {
            return;
        }
        self.page.borrow_mut().selected = selected;
        match selected {
            0 => {
                self.open_editor(ui, EditorKind::SteamGridDb);
            }
            1 => {
                let next = !self.service.borrow().prefer_steamgriddb_artwork();
                self.set_preference(ui, next);
            }
            2 => {
                if self.service.borrow().has_steamgriddb_key()
                    && let Some(callback) = self.artwork_refresh.borrow().as_ref()
                {
                    // Open the progress dialog before starting the worker.
                    ui.set_settings_refresh_completed(0);
                    ui.set_settings_refresh_total(0);
                    ui.set_settings_refresh_error("".into());
                    ui.set_settings_refresh_running(true);
                    ui.set_settings_refresh_open(true);
                    self.page.borrow_mut().feedback.clear();
                    callback(self.artwork_preferences());
                    self.cue(UiSoundCue::Ok);
                }
            }
            _ => {}
        }
        self.publish(ui);
    }

    /// What X (or a row's Remove button) removes on the row, if anything:
    /// the SteamGridDB key on its row, the whole Steam account on the Steam
    /// ID row. Its label, for the hints.
    fn secondary_label(&self, view: SettingsView, row: i32) -> &'static str {
        match (view, row) {
            (SettingsView::SteamGridDb, 0) if self.service.borrow().has_steamgriddb_key() => {
                "Remove"
            }
            (SettingsView::SteamAccount, 0) if self.steam_account.borrow().saved() => "Disconnect",
            (SettingsView::Shortcuts, 0..=3)
                if self.controller.get().shortcuts.is_set(row as usize) =>
            {
                "Clear"
            }
            _ => "",
        }
    }

    /// A shortcut no longer has a button (X on its row): nothing to confirm.
    fn clear_shortcut(&self, ui: &AppWindow, row: i32) {
        let mut values = self.controller.get();
        if self.capture.borrow().is_some()
            || !self.controller_ready.get()
            || self.controller_busy.get()
            || !values.shortcuts.is_set(row as usize)
        {
            return;
        }
        values.shortcuts = values.shortcuts.clear(row as usize);
        self.cue(UiSoundCue::Back);
        self.save_controller(ui, values);
    }

    /// Ask before removing what a row sets (X, or the row's Remove button).
    fn ask_remove(&self, ui: &AppWindow, row: i32) {
        let view = self.page.borrow().view;
        let mut page = self.page.borrow_mut();
        if page.editing_key || page.confirm.is_some() || self.secondary_label(view, row).is_empty()
        {
            return;
        }
        page.selected = row;
        page.confirm = Some(row);
        page.confirm_focus = 0;
        drop(page);
        self.cue(UiSoundCue::Ok);
        self.publish(ui);
    }

    /// The confirmation's answer: 0 Cancel, 1 remove.
    fn choose_confirm(&self, ui: &AppWindow, choice: i32) {
        let Some(row) = self.page.borrow_mut().confirm.take() else {
            return;
        };
        let view = self.page.borrow().view;
        if choice == 1
            && matches!(
                view,
                SettingsView::GameBindings | SettingsView::DsBindings | SettingsView::Shortcuts
            )
        {
            let mut next = self.controller.get();
            match view {
                SettingsView::DsBindings => next.ds_bindings = Default::default(),
                SettingsView::Shortcuts => next.shortcuts = Default::default(),
                _ => next.gba_bindings = Default::default(),
            }
            self.save_controller(ui, next);
        } else if choice == 1 {
            self.remove(ui, row);
        } else {
            self.cue(UiSoundCue::Back);
            self.publish(ui);
        }
    }

    /// Remove what a row sets, once confirmed.
    fn remove(&self, ui: &AppWindow, row: i32) {
        let view = self.page.borrow().view;
        if self.secondary_label(view, row).is_empty() {
            return;
        }
        let removed = match view {
            SettingsView::SteamGridDb => {
                let result = self.service.borrow_mut().remove_steamgriddb_key();
                if result.is_ok() {
                    self.notify_artwork_changed();
                }
                result.is_ok()
            }
            SettingsView::SteamAccount => {
                let ok = self.steam_account.borrow_mut().remove().is_ok();
                if ok {
                    self.notify_steam_account_changed();
                }
                ok
            }
            _ => return,
        };
        self.page.borrow_mut().feedback = if removed {
            String::new()
        } else {
            "Couldn't remove".into()
        };
        if removed {
            self.cue(UiSoundCue::Ok);
        }
        self.publish(ui);
    }

    fn open_editor(&self, ui: &AppWindow, kind: EditorKind) {
        let mut page = self.page.borrow_mut();
        page.editing_key = true;
        page.editor_kind = kind;
        page.editor_target = 0;
        page.feedback.clear();
        drop(page);
        let draft = if kind == EditorKind::SteamId {
            self.steam_account
                .borrow()
                .identity()
                .unwrap_or("")
                .to_owned()
        } else {
            String::new()
        };
        ui.set_settings_key_draft(draft.into());
        self.cue(UiSoundCue::Ok);
    }

    fn ensure_wayland_clipboard(&self, ui: &AppWindow) -> Option<Arc<WaylandClipboard>> {
        let mut clipboard = self.wayland_clipboard.borrow_mut();
        if clipboard.is_none()
            && let Ok(reader) = WaylandClipboard::from_window(ui.window())
        {
            *clipboard = Some(Arc::new(reader));
        }
        clipboard.as_ref().cloned()
    }

    fn paste_from_wayland(&self, ui: &AppWindow) {
        if !self.page.borrow().editing_key {
            return;
        }
        // Keep the entire clipboard operation off the Slint event loop.
        let Some(clipboard) = self.ensure_wayland_clipboard(ui) else {
            self.page.borrow_mut().feedback = "Couldn't paste".into();
            self.publish(ui);
            return;
        };
        let initial_draft = ui.get_settings_key_draft().to_string();
        let generation = self.paste_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let epoch = Arc::clone(&self.paste_generation);
        let weak_ui = ui.as_weak();
        crate::services::worker::spawn("settings-paste", move || {
            let text = clipboard.read_key();
            let _ = weak_ui.upgrade_in_event_loop(move |ui| {
                if !ui.get_settings_editing_key()
                    || ui.get_settings_editor_target() != 0
                    || epoch.load(Ordering::Relaxed) != generation
                    || ui.get_settings_key_draft().as_str() != initial_draft
                {
                    // Text was edited or this modal was closed while the
                    // compositor supplied the selection; never overwrite it.
                    return;
                }
                match text {
                    Some(text) => {
                        // Insert through the focused Slint TextInput to retain
                        // caret/selection behavior. Modifier keys must be
                        // temporarily released, because otherwise Slint treats
                        // the inserted characters as Ctrl shortcuts. The previous
                        // code never restored held Control keys, breaking the
                        // next Ctrl+V until the user released/re-pressed Ctrl.
                        //
                        // Snapshot PHYSICAL left/right Ctrl state before the
                        // synthetic events, then restore only the keys still
                        // physically held when this async read completed.
                        // Slint suppresses physical-state tracking while we
                        // dispatch synthetic modifier events.
                        ui.set_settings_feedback("".into());
                        ui.invoke_settings_apply_wayland_paste("".into());
                        use slint::platform::{Key, WindowEvent};
                        let held = ui.get_settings_physical_control_mask();
                        ui.set_settings_synthetic_key_insertion(true);
                        let window = ui.window();
                        for control in [Key::Control, Key::ControlR] {
                            let _ = window.dispatch_event_with_result(WindowEvent::KeyReleased {
                                text: control.into(),
                            });
                        }
                        for ch in text.chars() {
                            let key: slint::SharedString = ch.to_string().into();
                            let _ = window.dispatch_event_with_result(WindowEvent::KeyPressed {
                                text: key.clone(),
                            });
                            let _ = window
                                .dispatch_event_with_result(WindowEvent::KeyReleased { text: key });
                        }
                        if held & 1 != 0 {
                            let _ = window.dispatch_event_with_result(WindowEvent::KeyPressed {
                                text: Key::Control.into(),
                            });
                        }
                        if held & 2 != 0 {
                            let _ = window.dispatch_event_with_result(WindowEvent::KeyPressed {
                                text: Key::ControlR.into(),
                            });
                        }
                        ui.set_settings_synthetic_key_insertion(false);
                    }
                    None => ui.invoke_settings_paste_failed(),
                }
            });
        });
    }

    fn set_preference(&self, ui: &AppWindow, preferred: bool) {
        // Left/right repeat must not trigger redundant disk writes or resets.
        if self.service.borrow().prefer_steamgriddb_artwork() == preferred {
            return;
        }
        let result = self
            .service
            .borrow_mut()
            .set_prefer_steamgriddb_artwork(preferred);
        if result.is_ok() {
            self.notify_artwork_changed();
            self.cue(UiSoundCue::Ok);
        }
        self.page.borrow_mut().feedback = if result.is_ok() {
            String::new()
        } else {
            "Couldn't save".into()
        };
        self.publish(ui);
    }

    fn save_key(&self, ui: &AppWindow, key: &str) {
        if !self.page.borrow().editing_key {
            return;
        }
        let kind = self.page.borrow().editor_kind;
        let result = match kind {
            EditorKind::SteamGridDb => self.service.borrow_mut().set_steamgriddb_key(key),
            EditorKind::SteamId => self
                .steam_account
                .borrow_mut()
                .set_steam_id64(key)
                .map_err(str::to_owned),
            EditorKind::SteamWebKey => self
                .steam_account
                .borrow_mut()
                .set_web_api_key(key)
                .map_err(str::to_owned),
        };
        match result {
            Ok(()) => {
                if kind == EditorKind::SteamGridDb {
                    self.notify_artwork_changed();
                } else {
                    self.notify_steam_account_changed();
                }
                self.paste_generation.fetch_add(1, Ordering::Relaxed);
                self.page.borrow_mut().editing_key = false;
                self.page.borrow_mut().feedback = match kind {
                    EditorKind::SteamId if !self.steam_account.borrow().connected() => {
                        "Now add your Web API key"
                    }
                    _ => "",
                }
                .into();
                ui.set_settings_editing_key(false);
                ui.set_settings_key_draft("".into());
                self.cue(UiSoundCue::Ok);
            }
            Err(message) => {
                self.page.borrow_mut().feedback = message;
            }
        }
        self.publish(ui);
    }

    fn cancel_key(&self, ui: &AppWindow) {
        if !self.page.borrow().editing_key {
            return;
        }
        self.cue(UiSoundCue::Back);
        self.paste_generation.fetch_add(1, Ordering::Relaxed);
        let mut page = self.page.borrow_mut();
        page.editing_key = false;
        page.feedback.clear();
        drop(page);
        ui.set_settings_editing_key(false);
        ui.set_settings_key_draft("".into());
        self.publish(ui);
    }

    fn back_inside(&self, ui: &AppWindow) -> bool {
        if self.capture.borrow().is_some() {
            self.cancel_binding(ui);
            return true;
        }
        if ui.get_settings_refresh_open() {
            ui.set_settings_refresh_open(false);
            self.cue(UiSoundCue::Back);
            return true;
        }
        if self.page.borrow().editing_key {
            self.cancel_key(ui);
            return true;
        }
        let Some((parent_view, parent_index)) = settings_parent(self.page.borrow().view) else {
            return false;
        };
        let mut page = self.page.borrow_mut();
        page.view = parent_view;
        page.selected = parent_index;
        page.feedback.clear();
        drop(page);
        self.cue(UiSoundCue::Back);
        self.publish(ui);
        true
    }

    fn publish(&self, ui: &AppWindow) {
        let page = self.page.borrow();
        let settings = self.service.borrow();
        ui.set_settings_view(match page.view {
            SettingsView::Root => 0,
            SettingsView::Appearance => 1,
            SettingsView::ThirdParty => 2,
            SettingsView::SteamAccount => 3,
            SettingsView::SteamGridDb => 4,
            SettingsView::Controller => 5,
            SettingsView::GameBindings => 8,
            SettingsView::DsBindings => 10,
            SettingsView::Shortcuts => 11,
            SettingsView::MappingCores => 12,
            SettingsView::Cores => 6,
            SettingsView::Mgba => 7,
            SettingsView::MelonDs => 9,
        });
        ui.set_settings_binding_open(self.capture.borrow().is_some());
        ui.set_settings_binding_title(
            self.capture
                .borrow()
                .as_ref()
                .map(|capture| {
                    let names: &[&str] = match capture.target {
                        SettingsView::DsBindings => &crate::domain::controller::DS_ACTION_NAMES,
                        SettingsView::Shortcuts => &crate::domain::controller::SHORTCUT_NAMES,
                        _ => &crate::domain::controller::ACTION_NAMES,
                    };
                    format!("Assign {}", names.get(capture.row).copied().unwrap_or(""))
                })
                .unwrap_or_default()
                .into(),
        );
        let values = self.controller.get();
        let family = ui.global::<crate::InputGlyphs>().get_family();
        let rows: Vec<_> = if page.view == SettingsView::Shortcuts {
            crate::domain::controller::SHORTCUT_NAMES
                .into_iter()
                .enumerate()
                .map(|(row, title)| crate::ControllerBindingRow {
                    title: if row == crate::domain::controller::Shortcuts::FAST_FORWARD {
                        "Fast-forward (hold)".into()
                    } else {
                        title.into()
                    },
                    value: {
                        let buttons = values.shortcuts.buttons(row);
                        if buttons.is_empty() {
                            "Not set".into()
                        } else {
                            buttons
                                .into_iter()
                                .map(|source| binding_label(Some(source), family))
                                .collect::<Vec<_>>()
                                .join(" + ")
                                .into()
                        }
                    },
                })
                .collect()
        } else if page.view == SettingsView::DsBindings {
            crate::domain::controller::DS_ACTION_NAMES
                .into_iter()
                .enumerate()
                .map(|(row, title)| crate::ControllerBindingRow {
                    title: match title {
                        "Start" | "Select" => title.into(),
                        "Microphone" => "Microphone (hold to blow)".into(),
                        _ => format!("{title} button").into(),
                    },
                    value: binding_label(
                        values.ds_bindings.source(row, values.swap_confirm),
                        family,
                    )
                    .into(),
                })
                .collect()
        } else {
            crate::domain::controller::ACTION_NAMES
                .into_iter()
                .enumerate()
                .map(|(row, title)| crate::ControllerBindingRow {
                    // Game Boy games have no shoulder buttons.
                    title: match title {
                        "Start" | "Select" => title.into(),
                        "L" | "R" => format!("{title} button (GBA)").into(),
                        _ => format!("{title} button").into(),
                    },
                    value: binding_label(
                        values.gba_bindings.source(row, values.swap_confirm),
                        family,
                    )
                    .into(),
                })
                .collect()
        };
        // The DS page has more rows than the Game Boy page.
        while self.binding_model.row_count() > rows.len() {
            self.binding_model
                .remove(self.binding_model.row_count() - 1);
        }
        for (index, row) in rows.into_iter().enumerate() {
            if let Some(previous) = self.binding_model.row_data(index) {
                if previous.title != row.title || previous.value != row.value {
                    self.binding_model.set_row_data(index, row);
                }
            } else {
                self.binding_model.push(row);
            }
        }
        ui.set_settings_binding_rows(self.binding_model.clone().into());
        ui.set_settings_controller_ready(self.controller_ready.get());
        ui.set_settings_core_ready(self.core_ready.get());
        ui.set_settings_core_filter(self.core_values.get().mgba.video_filter.label().into());
        ui.set_settings_core_color(self.core_values.get().mgba.color_correction);
        ui.set_settings_core_palette(self.core_values.get().mgba.game_boy_palette.label().into());
        ui.set_settings_core_border(self.core_values.get().mgba.sgb_border);
        ui.set_settings_ds_layout(self.core_values.get().ds.layout.label().into());
        ui.set_settings_ds_filter(self.core_values.get().ds.video_filter.label().into());
        ui.set_settings_ds_resolution(self.core_values.get().ds.resolution.label().into());
        ui.set_settings_core_freeze(self.core_values.get().freeze_when_unfocused);
        ui.set_settings_core_resume(self.core_values.get().resume_last_state);
        ui.set_settings_core_sunlight(i32::from(self.core_values.get().mgba.sunlight.level()));
        ui.set_settings_core_feedback(self.core_feedback.borrow().as_str().into());
        let prefs = self.appearance.preferences();
        ui.set_settings_theme_index(prefs.theme_index());
        ui.set_settings_accent_index(prefs.accent_index());
        ui.set_settings_ui_sounds_enabled(prefs.ui_sounds_enabled);
        let controller = self.controller.get();
        ui.set_settings_swap_confirm(controller.swap_confirm);
        ui.set_settings_swap_hints(controller.swap_hints);
        ui.set_settings_appearance_feedback(if page.view == SettingsView::Appearance {
            page.feedback.clone().into()
        } else {
            "".into()
        });
        ui.set_settings_selection(page.selected);
        ui.set_settings_key_present(settings.has_steamgriddb_key());
        let account = self.steam_account.borrow();
        ui.set_settings_steam_identity(account.identity().unwrap_or("Not set").into());
        ui.set_settings_steam_connected(account.connected());
        ui.set_settings_steam_saved(account.saved());
        ui.set_settings_editor_kind(match page.editor_kind {
            EditorKind::SteamGridDb => 0,
            EditorKind::SteamId => 1,
            EditorKind::SteamWebKey => 2,
        });
        ui.set_settings_prefer_artwork(settings.prefer_steamgriddb_artwork());
        ui.set_settings_editing_key(page.editing_key);
        ui.set_settings_editor_target(page.editor_target);
        ui.set_settings_feedback(
            if matches!(
                page.view,
                SettingsView::Controller
                    | SettingsView::GameBindings
                    | SettingsView::DsBindings
                    | SettingsView::Shortcuts
            ) {
                self.controller_feedback.borrow().clone()
            } else {
                page.feedback.clone()
            }
            .into(),
        );
        ui.set_settings_confirm_open(page.confirm.is_some());
        ui.set_settings_confirm_focus(page.confirm_focus);
        let (title, action) = match page.view {
            SettingsView::GameBindings | SettingsView::DsBindings | SettingsView::Shortcuts => {
                ("Restore default controls?", "Restore")
            }
            SettingsView::SteamAccount => ("Disconnect Steam?", "Disconnect"),
            _ => ("Remove your SteamGridDB key?", "Remove"),
        };
        ui.set_settings_confirm_title(title.into());
        ui.set_settings_confirm_action(action.into());
        let secondary = if self.capture.borrow().is_some()
            || page.editing_key
            || page.confirm.is_some()
            || ui.get_settings_refresh_open()
        {
            ""
        } else {
            self.secondary_label(page.view, page.selected)
        };
        ui.set_settings_secondary_label(secondary.into());
        let view = page.view;
        drop(page);
        drop(settings);
        if self.published_view.replace(view) != view
            && let Some(callback) = self.location_changed.borrow().as_ref()
        {
            callback(if view == SettingsView::Root {
                crate::navigation::PageLocation::Root
            } else {
                crate::navigation::PageLocation::Settings(view)
            });
        }
    }
}

/// Intra-Settings Back restores the row that opened the child page.
/// A brand-new visit uses PageState::default() and starts at Appearance.
fn settings_parent(view: SettingsView) -> Option<(SettingsView, i32)> {
    match view {
        SettingsView::Root => None,
        SettingsView::Cores => Some((SettingsView::Root, 3)),
        SettingsView::Mgba => Some((SettingsView::Cores, 0)),
        SettingsView::MelonDs => Some((SettingsView::Cores, 1)),
        SettingsView::Appearance => Some((SettingsView::Root, 0)),
        SettingsView::Controller => Some((SettingsView::Root, 1)),
        SettingsView::MappingCores => Some((SettingsView::Controller, 2)),
        SettingsView::GameBindings => Some((SettingsView::MappingCores, 0)),
        SettingsView::DsBindings => Some((SettingsView::MappingCores, 1)),
        SettingsView::Shortcuts => Some((SettingsView::Controller, 3)),
        SettingsView::ThirdParty => Some((SettingsView::Root, 2)),
        SettingsView::SteamGridDb => Some((SettingsView::ThirdParty, 0)),
        SettingsView::SteamAccount => Some((SettingsView::ThirdParty, 1)),
    }
}

/// Keep controller focus within visible/enabled Third-Party actions.
fn selection_count(view: SettingsView) -> i32 {
    match view {
        SettingsView::Root => 4,
        SettingsView::Cores => 4,
        SettingsView::Mgba => 5,
        SettingsView::MelonDs => 3,
        SettingsView::Appearance => 15,
        SettingsView::Controller => 4,
        SettingsView::MappingCores => 2,
        SettingsView::GameBindings => 7,
        SettingsView::DsBindings => 10,
        SettingsView::Shortcuts => 5,
        SettingsView::ThirdParty => 2,
        SettingsView::SteamGridDb => 3,
        SettingsView::SteamAccount => 2,
    }
}

fn next_id_editor_target(target: i32, direction: UiAction, can_save: bool) -> i32 {
    use UiAction::{Down, Left, Right, Up};
    match (target, direction) {
        (0, Down | Right) => 2,
        (2, Up | Left) => 0,
        (2, Right) if can_save => 3,
        (3, Left) => 2,
        (3, Up) => 0,
        _ => target,
    }
}

/// Without a key, Refresh is unavailable and skipped by focus.
fn steamgriddb_step_without_key(selected: i32, delta: i32, repeated: bool) -> i32 {
    let selectable = [0, 1];
    let index = selectable
        .iter()
        .position(|&item| item == selected)
        .unwrap_or(0) as i32;
    let next = step_with_edge_wrap(index, 0, selectable.len() as i32 - 1, delta, repeated);
    selectable[next as usize]
}

/// A root category's explicit choice is authoritative. Pointer clicks pass
/// their row index directly, while controller Accept uses the Rust focus index.
fn root_destination(index: i32) -> Option<SettingsView> {
    match index {
        0 => Some(SettingsView::Appearance),
        1 => Some(SettingsView::Controller),
        2 => Some(SettingsView::ThirdParty),
        3 => Some(SettingsView::Cores),
        _ => None,
    }
}

// Two horizontal focus rows match the visible Appearance layout:
// Theme (System, Light, Dark, Black), then Accent (System + nine swatches).
// The UI Sounds switch is a separate full-width row. Do not separate the
// System accent from the swatches: they share the same y coordinate.
fn appearance_step_horizontal(index: i32, direction: UiAction, repeated: bool) -> i32 {
    let (first, last) = match index {
        0..=3 => (0, 3),
        4..=13 => (4, 13),
        _ => return index, // UI Sounds uses Left/Right to switch Off/On.
    };
    let delta = if direction == UiAction::Left { -1 } else { 1 };
    step_with_edge_wrap(index, first, last, delta, repeated)
}

/// The accents under each theme choice, by horizontal position: System and
/// Red under System, Orange to Green under Light, Teal to Purple under Dark,
/// Pink and White under Black. The second value is where Down lands first.
fn accent_column(theme: i32) -> (std::ops::RangeInclusive<i32>, i32) {
    match theme {
        0 => (4..=5, 4),
        1 => (6..=8, 7),
        2 => (9..=11, 10),
        _ => (12..=13, 13),
    }
}

// Vertical movement follows approximate horizontal positions of the visual
// controls, preserving the last accent on a return trip.
fn appearance_step_vertical(
    index: i32,
    direction: UiAction,
    last_accent: i32,
    repeated: bool,
) -> i32 {
    let last_accent = if (4..=13).contains(&last_accent) {
        last_accent
    } else {
        4
    };
    match (index, direction) {
        (0..=3, UiAction::Down) => {
            let (column, first) = accent_column(index);
            if column.contains(&last_accent) {
                last_accent
            } else {
                first
            }
        }
        (0..=3, UiAction::Up) => {
            if repeated {
                index
            } else {
                14
            }
        }
        (4..=13, UiAction::Up) => (0..=3)
            .find(|&theme| accent_column(theme).0.contains(&index))
            .unwrap_or(0),
        (4..=13, UiAction::Down) => 14,
        (14, UiAction::Up) => last_accent,
        (14, UiAction::Down) => {
            if repeated {
                14
            } else {
                0
            }
        }
        _ => index,
    }
}

/// Semantic controller grid for the API-key modal (not a real text cursor).
/// Disabled Save is excluded so every selected action is immediately usable.
fn next_editor_target(target: i32, direction: UiAction, can_save: bool) -> i32 {
    use UiAction::{Down, Left, Right, Up};

    match (target, direction) {
        (0, Right) => 1,
        (0, Down) => 2,
        (1, Left) => 0,
        (1, Down) => {
            if can_save {
                3
            } else {
                2
            }
        }
        (2, Up) => 0,
        (2, Right) if can_save => 3,
        (3, Left) => 2,
        (3, Up) => 1,
        _ => target,
    }
}

#[cfg(test)]
mod editor_navigation_tests {
    use super::{
        SettingsView, appearance_step_horizontal, appearance_step_vertical, next_editor_target,
    };
    use crate::input::UiAction;

    #[test]
    fn controller_reaches_all_editor_controls_with_a_key() {
        assert_eq!(next_editor_target(0, UiAction::Right, true), 1);
        assert_eq!(next_editor_target(1, UiAction::Down, true), 3);
        assert_eq!(next_editor_target(3, UiAction::Left, true), 2);
        assert_eq!(next_editor_target(2, UiAction::Up, true), 0);
    }

    #[test]
    fn refresh_row_is_included_only_with_saved_key() {
        assert_eq!(super::selection_count(SettingsView::Root), 4);
        assert_eq!(super::selection_count(SettingsView::Controller), 4);
        assert_eq!(super::selection_count(SettingsView::Cores), 4);
        assert_eq!(super::selection_count(SettingsView::DsBindings), 10);
        assert_eq!(
            super::settings_parent(SettingsView::MelonDs),
            Some((SettingsView::Cores, 1))
        );
        assert_eq!(
            super::settings_parent(SettingsView::DsBindings),
            Some((SettingsView::MappingCores, 1))
        );
        assert_eq!(
            super::settings_parent(SettingsView::MappingCores),
            Some((SettingsView::Controller, 2))
        );
        assert_eq!(super::selection_count(SettingsView::Appearance), 15);
        assert_eq!(super::selection_count(SettingsView::ThirdParty), 2);
        // Removing a key is X on its row, not a row of its own.
        assert_eq!(super::selection_count(SettingsView::SteamGridDb), 3);
        // Without a key, focus moves only between API key and the preference.
        assert_eq!(super::steamgriddb_step_without_key(1, 1, false), 0);
        assert_eq!(super::steamgriddb_step_without_key(0, 1, false), 1);
        assert_eq!(super::selection_count(SettingsView::SteamAccount), 2);
    }

    #[test]
    fn appearance_navigation_respects_visual_rows() {
        use UiAction::{Down, Left, Right, Up};
        assert_eq!(appearance_step_horizontal(0, Left, false), 3);
        assert_eq!(appearance_step_horizontal(3, Right, false), 0);

        // System accent is in the SAME horizontal row as the nine swatches.
        let mut focus = 4;
        for expected in 5..=13 {
            focus = appearance_step_horizontal(focus, Right, false);
            assert_eq!(focus, expected);
        }
        assert_eq!(appearance_step_horizontal(13, Right, false), 4);
        assert_eq!(appearance_step_horizontal(4, Left, false), 13);
        for accent in 4..=13 {
            let next = appearance_step_horizontal(accent, Right, false);
            assert_eq!(appearance_step_horizontal(next, Left, false), accent);
        }

        // Up/Down uses spatial columns and reverses when returning to a swatch.
        for accent in 4..=13 {
            let theme = appearance_step_vertical(accent, Up, accent, false);
            let expected_theme = match accent {
                4..=5 => 0,
                6..=8 => 1,
                9..=11 => 2,
                _ => 3,
            };
            assert_eq!(theme, expected_theme);
            assert_eq!(appearance_step_vertical(theme, Down, accent, false), accent);
            assert_eq!(appearance_step_vertical(accent, Down, accent, false), 14);
            assert_eq!(appearance_step_vertical(14, Up, accent, false), accent);
        }
        assert_eq!(appearance_step_vertical(0, Down, 8, false), 4);
        assert_eq!(appearance_step_vertical(1, Down, 4, false), 7);
        assert_eq!(appearance_step_vertical(2, Down, 4, false), 10);
        assert_eq!(appearance_step_vertical(3, Down, 4, false), 13);
        assert_eq!(appearance_step_vertical(14, Up, 0, false), 4);
        assert_eq!(appearance_step_vertical(14, Down, 8, false), 0);
        assert_eq!(appearance_step_horizontal(14, Left, false), 14);
        assert_eq!(appearance_step_horizontal(14, Right, false), 14);
    }

    #[test]
    fn held_navigation_never_wraps_in_settings() {
        use UiAction::{Down, Left, Right, Up};
        assert_eq!(appearance_step_horizontal(3, Right, true), 3);
        assert_eq!(appearance_step_horizontal(0, Left, true), 0);
        assert_eq!(appearance_step_horizontal(13, Right, true), 13);
        assert_eq!(appearance_step_horizontal(4, Left, true), 4);
        assert_eq!(appearance_step_horizontal(12, Right, true), 13);
        assert_eq!(appearance_step_vertical(0, Up, 4, true), 0);
        assert_eq!(appearance_step_vertical(14, Down, 4, true), 14);
        assert_eq!(appearance_step_vertical(0, Up, 4, false), 14);
        assert_eq!(appearance_step_vertical(14, Down, 4, false), 0);
    }

    #[test]
    fn disabled_save_is_skipped() {
        assert_eq!(next_editor_target(1, UiAction::Down, false), 2);
        assert_eq!(next_editor_target(2, UiAction::Right, false), 2);
    }
}

#[cfg(test)]
mod root_activation_regressions {
    use super::{PageState, SettingsView, root_destination, settings_parent};

    #[test]
    fn back_inside_settings_restores_the_parent_row() {
        assert_eq!(settings_parent(SettingsView::Root), None);
        assert_eq!(
            settings_parent(SettingsView::Appearance),
            Some((SettingsView::Root, 0))
        );
        assert_eq!(
            settings_parent(SettingsView::Controller),
            Some((SettingsView::Root, 1))
        );
        assert_eq!(
            settings_parent(SettingsView::ThirdParty),
            Some((SettingsView::Root, 2))
        );
        assert_eq!(
            settings_parent(SettingsView::SteamGridDb),
            Some((SettingsView::ThirdParty, 0))
        );
        assert_eq!(
            settings_parent(SettingsView::SteamAccount),
            Some((SettingsView::ThirdParty, 1))
        );
        let fresh = PageState::default();
        assert_eq!(fresh.view, SettingsView::Root);
        assert_eq!(fresh.selected, 0);
    }

    #[test]
    fn root_category_clicks_open_the_corresponding_page() {
        assert_eq!(root_destination(0), Some(SettingsView::Appearance));
        assert_eq!(root_destination(1), Some(SettingsView::Controller));
        assert_eq!(root_destination(2), Some(SettingsView::ThirdParty));
        assert_eq!(root_destination(-1), None);
        assert_eq!(root_destination(3), Some(SettingsView::Cores));
        assert_eq!(root_destination(4), None);
        assert_eq!(
            settings_parent(SettingsView::Mgba),
            Some((SettingsView::Cores, 0))
        );
        assert_eq!(
            settings_parent(SettingsView::Cores),
            Some((SettingsView::Root, 3))
        );
    }
}

#[cfg(test)]
mod core_runtime_tests {
    use super::*;
    use crate::{appearance::store::AppearanceStore, persistence::settings::SettingsStore};
    #[test]
    #[ignore = "requires Wayland and a fresh HORIZON_CORE_SETTINGS_TEST_DIR"]
    fn core_settings_navigation_save_and_restore() {
        let directory = PathBuf::from(std::env::var_os("HORIZON_CORE_SETTINGS_TEST_DIR").unwrap());
        std::fs::create_dir_all(&directory).unwrap();
        let ui = AppWindow::new().unwrap();
        ui.set_current_route(crate::AppRouteView::Settings);
        ui.global::<crate::Motion>().set_reduced_motion(true);
        let appearance =
            AppearanceController::new(&ui, AppearanceStore::new(directory.join("appearance.json")))
                .unwrap();
        let controller = SettingsController::new(
            &ui,
            SettingsService::load(SettingsStore::new(directory.join("artwork.json"))).unwrap(),
            appearance,
            Rc::new(RefCell::new(
                SteamAccountService::load(directory.join("steam.json")).unwrap(),
            )),
            directory.join("controller.json"),
            crate::platform::data_paths::CoreSettingsPaths {
                playback: directory.join("playback.json"),
                mgba: directory.join("cores/mgba.json"),
                ds: directory.join("cores/melonds-ds.json"),
            },
        );
        let weak = Rc::downgrade(&controller);
        let weak_ui = ui.as_weak();
        ui.on_raw_key_input(move |text, repeated| {
            let (Some(controller), Some(ui)) = (weak.upgrade(), weak_ui.upgrade()) else {
                return false;
            };
            crate::input::action_for_key(&text, repeated).is_some_and(|action| {
                controller.handle_action(&ui, UiActionEvent { action, repeated })
            })
        });
        ui.on_raw_key_released(|_| true);
        ui.show().unwrap();
        ui.invoke_settings_activate(3); // Pointer activation opens the requested category.
        assert_eq!(ui.get_settings_view(), 6);
        controller.handle_action(&ui, UiActionEvent::fresh(UiAction::Accept));
        assert_eq!(ui.get_settings_view(), 7);
        let stage = Rc::new(Cell::new(0));
        let ticks = Rc::new(Cell::new(0));
        let timer = slint::Timer::default();
        let (progress, count) = (Rc::clone(&stage), Rc::clone(&ticks));
        let weak_ui = ui.as_weak();
        let live = Rc::clone(&controller);
        let output = directory.clone();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(150),
            move || {
                let ui = weak_ui.upgrade().unwrap();
                count.set(count.get() + 1);
                assert!(
                    count.get() < 100,
                    "core settings timed out: {}",
                    ui.get_settings_core_feedback()
                );
                if !ui.get_settings_core_ready() || live.core_busy.get() {
                    return;
                }
                let tap = |key: slint::platform::Key| {
                    ui.window()
                        .dispatch_event_with_result(slint::platform::WindowEvent::KeyPressed {
                            text: key.into(),
                        })
                        .unwrap();
                    ui.window()
                        .dispatch_event_with_result(slint::platform::WindowEvent::KeyReleased {
                            text: key.into(),
                        })
                        .unwrap();
                };
                match progress.get() {
                    0 => {
                        tap(slint::platform::Key::RightArrow);
                        progress.set(1);
                    }
                    1 => {
                        assert_eq!(ui.get_settings_core_sunlight(), 1);
                        ui.invoke_settings_core_decrease();
                        progress.set(2);
                    }
                    2 => {
                        assert_eq!(ui.get_settings_core_sunlight(), 0);
                        tap(slint::platform::Key::DownArrow);
                        assert_eq!(ui.get_settings_selection(), 1);
                        tap(slint::platform::Key::RightArrow);
                        progress.set(3);
                    }
                    3 => {
                        assert_eq!(ui.get_settings_core_filter(), "LCD");
                        tap(slint::platform::Key::Return);
                        progress.set(6);
                    }
                    6 => {
                        assert_eq!(ui.get_settings_core_filter(), "Smooth");
                        tap(slint::platform::Key::RightArrow);
                        progress.set(7);
                    }
                    7 => {
                        assert_eq!(
                            ui.get_settings_core_filter(),
                            "Smooth pixel art",
                            "feedback={} sel={} view={}",
                            ui.get_settings_core_feedback(),
                            ui.get_settings_selection(),
                            ui.get_settings_view()
                        );
                        tap(slint::platform::Key::RightArrow);
                        progress.set(8);
                    }
                    8 => {
                        assert_eq!(ui.get_settings_core_filter(), "None");
                        tap(slint::platform::Key::LeftArrow);
                        progress.set(9);
                    }
                    9 => {
                        assert_eq!(ui.get_settings_core_filter(), "Smooth pixel art");
                        tap(slint::platform::Key::DownArrow);
                        assert_eq!(ui.get_settings_selection(), 2);
                        tap(slint::platform::Key::RightArrow);
                        progress.set(10);
                    }
                    10 => {
                        assert!(ui.get_settings_core_color());
                        let pixels = ui.window().take_snapshot().unwrap();
                        image::save_buffer(
                            output.join("mgba-settings.png"),
                            pixels.as_bytes(),
                            pixels.width(),
                            pixels.height(),
                            image::ColorType::Rgba8,
                        )
                        .unwrap();
                        tap(slint::platform::Key::Escape);
                        assert_eq!(
                            (ui.get_settings_view(), ui.get_settings_selection()),
                            (6, 0)
                        );
                        tap(slint::platform::Key::DownArrow);
                        assert_eq!(ui.get_settings_selection(), 1);
                        tap(slint::platform::Key::Return);
                        assert_eq!(ui.get_settings_view(), 9);
                        assert_eq!(ui.get_settings_ds_layout(), "Side by side");
                        tap(slint::platform::Key::RightArrow);
                        progress.set(14);
                    }
                    14 => {
                        assert_eq!(ui.get_settings_ds_layout(), "Stacked");
                        tap(slint::platform::Key::DownArrow);
                        assert_eq!(ui.get_settings_selection(), 1);
                        tap(slint::platform::Key::RightArrow);
                        progress.set(15);
                    }
                    15 => {
                        assert_eq!(ui.get_settings_ds_filter(), "LCD");
                        tap(slint::platform::Key::DownArrow);
                        assert_eq!(ui.get_settings_selection(), 2);
                        assert_eq!(ui.get_settings_ds_resolution(), "Native");
                        tap(slint::platform::Key::RightArrow);
                        progress.set(16);
                    }
                    16 => {
                        assert_eq!(ui.get_settings_ds_resolution(), "2× (384p)");
                        let pixels = ui.window().take_snapshot().unwrap();
                        image::save_buffer(
                            output.join("melonds-settings.png"),
                            pixels.as_bytes(),
                            pixels.width(),
                            pixels.height(),
                            image::ColorType::Rgba8,
                        )
                        .unwrap();
                        tap(slint::platform::Key::Escape);
                        assert_eq!(
                            (ui.get_settings_view(), ui.get_settings_selection()),
                            (6, 1)
                        );
                        tap(slint::platform::Key::DownArrow);
                        assert_eq!(ui.get_settings_selection(), 2);
                        assert!(ui.get_settings_core_freeze());
                        tap(slint::platform::Key::LeftArrow);
                        progress.set(11);
                    }
                    11 => {
                        assert!(!ui.get_settings_core_freeze());
                        tap(slint::platform::Key::DownArrow);
                        assert_eq!(ui.get_settings_selection(), 3);
                        assert!(ui.get_settings_core_resume());
                        tap(slint::platform::Key::LeftArrow);
                        progress.set(13);
                    }
                    13 => {
                        assert!(!ui.get_settings_core_resume());
                        let pixels = ui.window().take_snapshot().unwrap();
                        image::save_buffer(
                            output.join("shared-settings.png"),
                            pixels.as_bytes(),
                            pixels.width(),
                            pixels.height(),
                            image::ColorType::Rgba8,
                        )
                        .unwrap();
                        tap(slint::platform::Key::Escape);
                        assert_eq!(
                            (ui.get_settings_view(), ui.get_settings_selection()),
                            (0, 3)
                        );
                        // Failed writes retain the last committed shared value.
                        std::fs::rename(
                            output.join("playback.json"),
                            output.join("saved-playback.json"),
                        )
                        .unwrap();
                        std::fs::create_dir(output.join("playback.json")).unwrap();
                        live.activate(&ui, 3);
                        live.activate(&ui, 2);
                        progress.set(12);
                    }
                    12 => {
                        assert!(ui.get_settings_core_feedback().starts_with("Couldn't save"));
                        assert!(!ui.get_settings_core_freeze());
                        progress.set(5);
                        slint::quit_event_loop().unwrap();
                    }
                    _ => {}
                }
            },
        );
        slint::run_event_loop().unwrap();
        assert_eq!(stage.get(), 5);
        let saved = crate::persistence::core_settings::CoreSettingsStore::new(
            crate::platform::data_paths::CoreSettingsPaths {
                playback: directory.join("saved-playback.json"),
                mgba: directory.join("cores/mgba.json"),
                ds: directory.join("cores/melonds-ds.json"),
            },
        )
        .load()
        .unwrap();
        assert!(!saved.resume_last_state);
        assert_eq!(saved.mgba.sunlight.level(), 0);
        assert_eq!(
            saved.mgba.video_filter,
            crate::domain::core_settings::VideoFilter::Xbrz
        );
        assert!(saved.mgba.color_correction);
        assert!(!saved.freeze_when_unfocused);
        assert_eq!(
            saved.ds.layout,
            crate::domain::core_settings::DsLayout::Stacked
        );
        assert_eq!(
            saved.ds.video_filter,
            crate::domain::core_settings::VideoFilter::Lcd
        );
        assert_eq!(saved.ds.resolution.scale(), 2);
    }
}

fn binding_label(source: Option<u8>, family: crate::GlyphFamily) -> &'static str {
    let ps = family == crate::GlyphFamily::Playstation;
    match source {
        Some(8) => {
            if ps {
                "Cross"
            } else {
                "A"
            }
        }
        Some(0) => {
            if ps {
                "Circle"
            } else {
                "B"
            }
        }
        Some(9) => {
            if ps {
                "Square"
            } else {
                "X"
            }
        }
        Some(1) => {
            if ps {
                "Triangle"
            } else {
                "Y"
            }
        }
        Some(10) => match family {
            crate::GlyphFamily::Xbox => "LB",
            crate::GlyphFamily::Nintendo => "L",
            crate::GlyphFamily::Keyboard => "Left shoulder",
            _ => "L1",
        },
        Some(11) => match family {
            crate::GlyphFamily::Xbox => "RB",
            crate::GlyphFamily::Nintendo => "R",
            crate::GlyphFamily::Keyboard => "Right shoulder",
            _ => "R1",
        },
        Some(12) => match family {
            crate::GlyphFamily::Nintendo => "ZL",
            crate::GlyphFamily::Xbox => "LT",
            crate::GlyphFamily::Keyboard => "Left trigger",
            _ => "L2",
        },
        Some(13) => match family {
            crate::GlyphFamily::Nintendo => "ZR",
            crate::GlyphFamily::Xbox => "RT",
            crate::GlyphFamily::Keyboard => "Right trigger",
            _ => "R2",
        },
        Some(3) => match family {
            crate::GlyphFamily::Playstation => "Options",
            crate::GlyphFamily::Nintendo => "+",
            _ => "Start",
        },
        Some(2) => match family {
            crate::GlyphFamily::Playstation => "Share",
            crate::GlyphFamily::Nintendo => "−",
            _ => "Select",
        },
        _ => "Unavailable",
    }
}

#[cfg(test)]
mod mapping_runtime_tests {
    use super::*;
    use crate::{appearance::store::AppearanceStore, persistence::settings::SettingsStore};
    #[test]
    #[ignore = "requires Wayland and a fresh HORIZON_MAPPING_TEST_DIR"]
    fn controller_mapping_capture_persist_cancel_and_restore() {
        let directory = PathBuf::from(std::env::var_os("HORIZON_MAPPING_TEST_DIR").unwrap());
        std::fs::create_dir_all(&directory).unwrap();
        let ui = AppWindow::new().unwrap();
        ui.window().set_size(slint::PhysicalSize::new(1280, 720));
        ui.set_current_route(crate::AppRouteView::Settings);
        let appearance =
            AppearanceController::new(&ui, AppearanceStore::new(directory.join("appearance.json")))
                .unwrap();
        let theme = match std::env::var("HORIZON_MAPPING_THEME").as_deref() {
            Ok("light") => ThemePreference::Light,
            Ok("dark") => ThemePreference::Dark,
            _ => ThemePreference::Black,
        };
        appearance.set_theme_preference(&ui, theme).unwrap();
        ui.global::<crate::Motion>()
            .set_reduced_motion(std::env::var_os("HORIZON_MAPPING_REDUCED").is_some());
        let controller = SettingsController::new(
            &ui,
            SettingsService::load(SettingsStore::new(directory.join("artwork.json"))).unwrap(),
            appearance,
            Rc::new(RefCell::new(
                SteamAccountService::load(directory.join("steam.json")).unwrap(),
            )),
            directory.join("controller.json"),
            crate::platform::data_paths::CoreSettingsPaths {
                playback: directory.join("playback.json"),
                mgba: directory.join("cores/mgba.json"),
                ds: directory.join("cores/melonds-ds.json"),
            },
        );
        let snapshot = Rc::new(Cell::new(Some((1 << 8, 7))));
        let source = snapshot.clone();
        controller.set_mapping_source(Rc::new(move || source.get()));
        ui.global::<crate::InputGlyphs>()
            .set_family(crate::GlyphFamily::Xbox);
        ui.show().unwrap();
        let stage = Rc::new(Cell::new(0));
        let progress = stage.clone();
        let count = Cell::new(0);
        let timer = slint::Timer::default();
        let weak_ui = ui.as_weak();
        let live = controller.clone();
        let output = directory.clone();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(150),
            move || {
                let ui = weak_ui.upgrade().unwrap();
                count.set(count.get() + 1);
                assert!(
                    count.get() < 100,
                    "mapping timed out: {}",
                    ui.get_settings_feedback()
                );
                if count.get() < 10 || !live.controller_ready.get() || live.controller_busy.get() {
                    return;
                }
                let shot = |name: &str| {
                    let pixels = ui.window().take_snapshot().unwrap();
                    image::save_buffer(
                        output.join(format!("{name}.png")),
                        pixels.as_bytes(),
                        pixels.width(),
                        pixels.height(),
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                };
                match progress.get() {
                    0 => {
                        ui.invoke_settings_activate(1);
                        assert_eq!(ui.get_settings_view(), 5);
                        // Button mapping lists the cores; each opens its own.
                        ui.invoke_settings_activate(2);
                        assert_eq!(ui.get_settings_view(), 12);
                        ui.invoke_settings_activate(0);
                        assert_eq!(ui.get_settings_view(), 8);
                        progress.set(100);
                    }
                    100 => {
                        progress.set(101);
                    }
                    101 => {
                        shot("mapping-defaults");
                        ui.invoke_settings_activate(0);
                        live.poll_binding(&ui);
                        assert!(!live.capture.borrow().as_ref().unwrap().armed);
                        snapshot.set(Some((0, 7)));
                        live.poll_binding(&ui);
                        assert!(live.capture.borrow().as_ref().unwrap().armed);
                        progress.set(102);
                    }
                    102 => {
                        progress.set(103);
                    }
                    103 => {
                        shot("mapping-capture");
                        live.handle_action(&ui, UiActionEvent::fresh(UiAction::Back));
                        assert!(ui.get_settings_binding_open());
                        snapshot.set(Some((1, 7)));
                        live.poll_binding(&ui);
                        assert!(!ui.get_settings_binding_open());
                        progress.set(1);
                    }
                    1 => {
                        assert_eq!(live.controller.get().gba_bindings.source(0, false), Some(0));
                        assert_eq!(live.controller.get().gba_bindings.source(1, false), Some(8));
                        assert_eq!(
                            SettingsStore::new(output.join("controller.json"))
                                .load_value::<ControllerPreferences>()
                                .unwrap(),
                            live.controller.get()
                        );
                        snapshot.set(Some((0, 7)));
                        ui.invoke_settings_activate(1);
                        live.poll_binding(&ui);
                        snapshot.set(Some(((1 << 9) | (1 << 10), 7)));
                        live.poll_binding(&ui);
                        assert!(ui.get_settings_binding_open());
                        snapshot.set(Some((0, 7)));
                        live.poll_binding(&ui);
                        snapshot.set(Some((1 << 9, 7)));
                        live.poll_binding(&ui);
                        progress.set(2);
                    }
                    2 => {
                        assert_eq!(live.controller.get().gba_bindings.source(1, false), Some(9));
                        shot("mapping-custom");
                        live.set_controller_switch(&ui, 0, true);
                        progress.set(3);
                    }
                    3 => {
                        assert_eq!(live.controller.get().gba_bindings.source(0, true), Some(9));
                        let saved = live.controller.get();
                        snapshot.set(Some((0, 7)));
                        ui.invoke_settings_activate(0);
                        live.poll_binding(&ui);
                        live.handle_action(&ui, UiActionEvent::fresh(UiAction::StickPress));
                        assert!(!ui.get_settings_binding_open());
                        assert_eq!(saved, live.controller.get());
                        ui.invoke_settings_activate(0);
                        live.poll_binding(&ui);
                        snapshot.set(None);
                        live.poll_binding(&ui);
                        assert!(!ui.get_settings_binding_open());
                        assert!(ui.get_settings_feedback().contains("disconnected"));
                        snapshot.set(Some((0, 7)));
                        ui.invoke_settings_activate(0);
                        live.poll_binding(&ui);
                        snapshot.set(Some((1, 8)));
                        live.poll_binding(&ui);
                        assert!(!ui.get_settings_binding_open());
                        assert!(ui.get_settings_feedback().contains("changed"));
                        snapshot.set(Some((0, 7)));
                        ui.invoke_settings_activate(0);
                        live.capture.borrow_mut().as_mut().unwrap().since -=
                            std::time::Duration::from_secs(11);
                        live.poll_binding(&ui);
                        assert!(!ui.get_settings_binding_open());
                        ui.invoke_settings_activate(6);
                        assert_eq!(ui.get_settings_confirm_focus(), 0);
                        live.choose_confirm(&ui, 0);
                        assert_eq!(saved, live.controller.get());
                        ui.invoke_settings_activate(6);
                        progress.set(104);
                    }
                    104 => {
                        progress.set(105);
                    }
                    105 => {
                        shot("mapping-restore");
                        live.choose_confirm(&ui, 1);
                        progress.set(106);
                    }
                    106 => {
                        // The DS page maps all four face buttons.
                        assert!(live.back_inside(&ui));
                        assert_eq!(
                            (ui.get_settings_view(), ui.get_settings_selection()),
                            (12, 0)
                        );
                        ui.invoke_settings_activate(1);
                        assert_eq!(ui.get_settings_view(), 10);
                        assert_eq!(ui.get_settings_binding_rows().row_count(), 9);
                        snapshot.set(Some((0, 7)));
                        ui.invoke_settings_activate(3);
                        assert_eq!(ui.get_settings_binding_title(), "Assign Y");
                        live.poll_binding(&ui);
                        snapshot.set(Some((1 << 9, 7)));
                        live.poll_binding(&ui);
                        progress.set(107);
                    }
                    107 => {
                        let ds = live.controller.get().ds_bindings;
                        assert_eq!((ds.source(3, true), ds.source(2, true)), (Some(9), Some(1)));
                        shot("mapping-ds");
                        for _ in 0..6 {
                            live.handle_action(&ui, UiActionEvent::fresh(UiAction::Down));
                        }
                        assert_eq!(ui.get_settings_selection(), 9);
                        progress.set(108);
                    }
                    108 => {
                        progress.set(109);
                    }
                    109 => {
                        shot("mapping-ds-restore");
                        assert!(live.back_inside(&ui));
                        assert_eq!(
                            (ui.get_settings_view(), ui.get_settings_selection()),
                            (12, 1)
                        );
                        progress.set(120);
                    }
                    120 => progress.set(121),
                    121 => progress.set(122),
                    122 => {
                        shot("mapping-cores");
                        assert!(live.back_inside(&ui));
                        assert_eq!(
                            (ui.get_settings_view(), ui.get_settings_selection()),
                            (5, 2)
                        );
                        // Shortcuts: same page, four rows, X clears one.
                        ui.invoke_settings_activate(3);
                        assert_eq!(ui.get_settings_view(), 11);
                        assert_eq!(ui.get_settings_binding_rows().row_count(), 4);
                        // A pair: both held, then let go.
                        snapshot.set(Some((0, 7)));
                        ui.invoke_settings_activate(1);
                        assert_eq!(ui.get_settings_binding_title(), "Assign Quick save");
                        live.poll_binding(&ui);
                        snapshot.set(Some((1 << 10, 7)));
                        live.poll_binding(&ui);
                        snapshot.set(Some((1 << 10 | 1 << 9, 7)));
                        live.poll_binding(&ui);
                        assert!(ui.get_settings_binding_open());
                        snapshot.set(Some((0, 7)));
                        live.poll_binding(&ui);
                        progress.set(110);
                    }
                    110 => {
                        let shortcuts = live.controller.get().shortcuts;
                        assert_eq!(shortcuts.buttons(1), vec![9, 10]);
                        assert_eq!(shortcuts.buttons(0), vec![12, 13]);
                        shot("mapping-shortcuts");
                        live.page.borrow_mut().selected = 1;
                        live.handle_action(&ui, UiActionEvent::fresh(UiAction::Secondary));
                        progress.set(111);
                    }
                    111 => {
                        assert!(!live.controller.get().shortcuts.is_set(1));
                        assert!(live.back_inside(&ui));
                        assert_eq!(
                            (ui.get_settings_view(), ui.get_settings_selection()),
                            (5, 3)
                        );
                        ui.invoke_settings_activate(2);
                        ui.invoke_settings_activate(0);
                        assert_eq!(ui.get_settings_binding_rows().row_count(), 6);
                        progress.set(4);
                    }
                    4 => {
                        assert_eq!(live.controller.get().gba_bindings, Default::default());
                        assert!(live.controller.get().swap_confirm);
                        assert!(live.back_inside(&ui));
                        assert!(live.back_inside(&ui));
                        assert_eq!(
                            (ui.get_settings_view(), ui.get_settings_selection()),
                            (5, 2)
                        );
                        std::fs::rename(
                            output.join("controller.json"),
                            output.join("preserved.json"),
                        )
                        .unwrap();
                        std::fs::create_dir(output.join("controller.json")).unwrap();
                        let mut changed = live.controller.get();
                        changed.gba_bindings = changed.gba_bindings.assign(0, 9, false);
                        live.save_controller(&ui, changed);
                        progress.set(5);
                    }
                    5 => {
                        assert_eq!(live.controller.get().gba_bindings, Default::default());
                        assert!(ui.get_settings_feedback().contains("retained"));
                        std::fs::remove_dir(output.join("controller.json")).unwrap();
                        std::fs::rename(
                            output.join("preserved.json"),
                            output.join("controller.json"),
                        )
                        .unwrap();
                        assert_eq!(
                            SettingsStore::new(output.join("controller.json"))
                                .load_value::<ControllerPreferences>()
                                .unwrap(),
                            live.controller.get()
                        );
                        progress.set(6);
                        slint::quit_event_loop().unwrap();
                    }
                    _ => {}
                }
            },
        );
        slint::run_event_loop().unwrap();
        assert_eq!(stage.get(), 6);
    }
}

#[cfg(test)]
mod value_motion_runtime_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    #[ignore = "requires Wayland and a fresh HORIZON_VALUE_MOTION_TEST_DIR"]
    fn menu_values_animate_and_settle_without_moving_rows() {
        let output = PathBuf::from(std::env::var_os("HORIZON_VALUE_MOTION_TEST_DIR").unwrap());
        std::fs::create_dir_all(&output).unwrap();
        let ui = AppWindow::new().unwrap();
        ui.window().set_size(slint::PhysicalSize::new(1280, 720));
        let appearance = AppearanceController::new(
            &ui,
            crate::appearance::store::AppearanceStore::new(output.join("appearance.json")),
        )
        .unwrap();
        appearance
            .set_theme_preference(
                &ui,
                if std::env::var_os("HORIZON_VALUE_MOTION_LIGHT").is_some() {
                    ThemePreference::Light
                } else {
                    ThemePreference::Black
                },
            )
            .unwrap();
        let reduced = std::env::var_os("HORIZON_VALUE_MOTION_REDUCED").is_some();
        ui.global::<crate::Motion>().set_reduced_motion(reduced);
        ui.set_current_route(crate::AppRouteView::Settings);
        ui.set_settings_view(7);
        ui.set_settings_core_ready(true);
        ui.set_settings_selection(1);
        ui.show().unwrap();
        let step = Rc::new(Cell::new(0));
        let progress = step.clone();
        let middle = RefCell::new(Vec::new());
        let before = RefCell::new(Vec::new());
        let captures = Rc::new(RefCell::new(Vec::new()));
        let frames = captures.clone();
        let weak = ui.as_weak();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(60),
            move || {
                let ui = weak.upgrade().unwrap();
                progress.set(progress.get() + 1);
                let tick = progress.get();
                let capture = |name: &str| {
                    let pixels = ui.window().take_snapshot().unwrap();
                    let image = image::RgbaImage::from_raw(
                        pixels.width(),
                        pixels.height(),
                        pixels.as_bytes().to_vec(),
                    )
                    .unwrap();
                    frames.borrow_mut().push((name.to_owned(), image.clone()));
                    image
                };
                // Compare text ink rather than hover-dependent row colours.
                let value = |image: &image::RgbaImage| {
                    image::imageops::crop_imm(image, 1016, 222, 196, 30)
                        .to_image()
                        .pixels()
                        .map(|pixel| {
                            i16::from(pixel[2]) - i16::from(pixel[0]) > 30
                                && i16::from(pixel[2]) - i16::from(pixel[1]) > 20
                        })
                        .collect::<Vec<_>>()
                };
                let label = |image: &image::RgbaImage| {
                    let light = std::env::var_os("HORIZON_VALUE_MOTION_LIGHT").is_some();
                    image::imageops::crop_imm(image, 64, 218, 300, 54)
                        .to_image()
                        .enumerate_pixels()
                        .filter(
                            |(_, _, pixel)| {
                                if light { pixel[0] < 70 } else { pixel[0] > 200 }
                            },
                        )
                        .fold(vec![300, 54, 0, 0], |bounds, (x, y, _)| {
                            vec![
                                bounds[0].min(x),
                                bounds[1].min(y),
                                bounds[2].max(x),
                                bounds[3].max(y),
                            ]
                        })
                };
                match tick {
                    14 => *before.borrow_mut() = label(&capture("core-before")),
                    15 => {
                        ui.set_settings_core_value_forward(true);
                        ui.set_settings_core_filter("LCD".into());
                    }
                    16 => *middle.borrow_mut() = value(&capture("core-forward-middle")),
                    21 => {
                        let settled = capture("core-forward-settled");
                        assert_eq!(
                            *before.borrow(),
                            label(&settled),
                            "row label moved during value change"
                        );
                        assert_eq!(
                            *middle.borrow() == value(&settled),
                            reduced,
                            "value must animate only when motion is enabled"
                        );
                    }
                    22 => {
                        ui.set_settings_core_value_forward(false);
                        ui.set_settings_core_filter("None".into());
                    }
                    23 => *middle.borrow_mut() = value(&capture("core-backward-middle")),
                    27 => {
                        let settled = capture("core-backward-settled");
                        assert_eq!(*middle.borrow() == value(&settled), reduced);
                        ui.set_settings_core_filter("LCD".into());
                    }
                    28 => ui.set_settings_core_filter("Smooth".into()),
                    29 => ui.set_settings_core_filter("Smooth pixel art".into()),
                    35 => {
                        *middle.borrow_mut() = value(&capture("core-rapid-settled"));
                    }
                    39 => {
                        assert!(
                            *middle.borrow() == value(&capture("core-idle")),
                            "value must settle after rapid changes"
                        );
                        ui.set_current_route(crate::AppRouteView::Library);
                    }
                    50 => ui.set_library_sort_text("Recently played".into()),
                    51 => {
                        capture("header-middle");
                    }
                    56 => {
                        capture("header-settled");
                        ui.set_player_open(true);
                        ui.set_player_phase(2);
                        ui.set_player_title("Menu motion preview".into());
                        ui.set_player_has_frame(true);
                        ui.set_player_paused(true);
                        ui.set_player_focus(1);
                    }
                    67 => ui.set_player_speed(2),
                    68 => {
                        capture("pause-middle");
                    }
                    73 => {
                        capture("pause-settled");
                        slint::quit_event_loop().unwrap();
                    }
                    _ => {}
                }
            },
        );
        slint::run_event_loop().unwrap();
        assert_eq!(step.get(), 73);
        // PNG encoding stays outside animation sampling so capture I/O cannot
        // consume the transition we are trying to inspect.
        for (name, image) in captures.borrow().iter() {
            image.save(output.join(format!("{name}.png"))).unwrap();
        }
    }
}
