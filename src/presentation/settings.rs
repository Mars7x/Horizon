//! Controller-first Settings pages; Slint displays state only.
use slint::ComponentHandle;
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

pub struct SettingsController {
    page: RefCell<PageState>,
    location_changed: CallbackSlot<dyn Fn(crate::navigation::PageLocation)>,
    published_view: Cell<SettingsView>,
    service: RefCell<SettingsService>,
    steam_account: Rc<RefCell<SteamAccountService>>,
    steam_account_changed: CallbackSlot<dyn Fn()>,
    appearance: AppearanceController,
    controller: Cell<ControllerPreferences>,
    controller_path: PathBuf,
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
        core_path: PathBuf,
    ) -> Rc<Self> {
        let weak_ui = ui.as_weak();
        let (notify, drain) = super::wake::drain_notify(move |controller: &Self| {
            if let Some(ui) = weak_ui.upgrade() {
                controller.drain_cores(&ui);
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
            controller: Cell::new(controller_settings::load_preferences(&controller_path)),
            controller_path,
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
                        "Couldn't load core settings. Check cores.json and restart.".into()
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
        match row {
            0 => next.mgba.audio_smoothing = direction.unwrap_or(!next.mgba.audio_smoothing),
            1 => {
                next.mgba.sunlight =
                    next.mgba
                        .sunlight
                        .step(if direction == Some(false) { -1 } else { 1 })
            }
            2 => next.mgba.video_filter = next.mgba.video_filter.step(direction != Some(false)),
            3 => next.mgba.color_correction = direction.unwrap_or(!next.mgba.color_correction),
            4 => {
                next.mgba.freeze_when_unfocused =
                    direction.unwrap_or(!next.mgba.freeze_when_unfocused)
            }
            _ => return,
        }
        if next == self.core_values.get() {
            return;
        }
        match self.cores.requests.send(next) {
            Ok(()) => {
                self.core_busy.set(true);
                *self.core_feedback.borrow_mut() = "Saving…".into();
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
        let result = controller_settings::save_preferences(&self.controller_path, &next);
        if let Err(error) = &result {
            tracing::warn!(%error, "could not save controller settings");
        } else {
            self.controller.set(next);
            if let Some(callback) = self.controller_changed.borrow().as_ref() {
                callback(next);
            }
            self.cue(UiSoundCue::Ok);
        }
        self.page.borrow_mut().feedback = if result.is_ok() {
            String::new()
        } else {
            "Couldn't save".into()
        };
        self.publish(ui);
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
        page.editing_key || page.confirm.is_some() || ui.get_settings_refresh_open()
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
                } else if page.view == SettingsView::Mgba {
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
                let selected = self.page.borrow().selected;
                self.ask_remove(ui, selected);
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
                core.change_core(&ui, 1, Some(false));
            }
        });
        let weak = ui.as_weak();
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
                if selected != 0 {
                    return;
                }
                let mut page = self.page.borrow_mut();
                page.view = SettingsView::Mgba;
                page.selected = 0;
                drop(page);
                self.cue(UiSoundCue::Ok);
                self.publish(ui);
                return;
            }
            SettingsView::Mgba => {
                self.page.borrow_mut().selected = selected.clamp(0, 2);
                self.change_core(ui, selected, None);
                return;
            }
            SettingsView::Controller => {
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
            _ => "",
        }
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
        if choice == 1 {
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
            SettingsView::Cores => 6,
            SettingsView::Mgba => 7,
        });
        ui.set_settings_core_ready(self.core_ready.get() && !self.core_busy.get());
        ui.set_settings_core_filter(self.core_values.get().mgba.video_filter.label().into());
        ui.set_settings_core_color(self.core_values.get().mgba.color_correction);
        ui.set_settings_core_freeze(self.core_values.get().mgba.freeze_when_unfocused);
        ui.set_settings_core_smoothing(self.core_values.get().mgba.audio_smoothing);
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
        ui.set_settings_feedback(page.feedback.clone().into());
        ui.set_settings_confirm_open(page.confirm.is_some());
        ui.set_settings_confirm_focus(page.confirm_focus);
        let (title, action) = match page.view {
            SettingsView::SteamAccount => ("Disconnect Steam?", "Disconnect"),
            _ => ("Remove your SteamGridDB key?", "Remove"),
        };
        ui.set_settings_confirm_title(title.into());
        ui.set_settings_confirm_action(action.into());
        let secondary =
            if page.editing_key || page.confirm.is_some() || ui.get_settings_refresh_open() {
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
        SettingsView::Appearance => Some((SettingsView::Root, 0)),
        SettingsView::Controller => Some((SettingsView::Root, 1)),
        SettingsView::ThirdParty => Some((SettingsView::Root, 2)),
        SettingsView::SteamGridDb => Some((SettingsView::ThirdParty, 0)),
        SettingsView::SteamAccount => Some((SettingsView::ThirdParty, 1)),
    }
}

/// Keep controller focus within visible/enabled Third-Party actions.
fn selection_count(view: SettingsView) -> i32 {
    match view {
        SettingsView::Root => 4,
        SettingsView::Cores => 1,
        SettingsView::Mgba => 5,
        SettingsView::Appearance => 15,
        SettingsView::Controller => 2,
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
        assert_eq!(super::selection_count(SettingsView::Controller), 2);
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
            directory.join("cores.json"),
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
                if !ui.get_settings_core_ready() {
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
                        assert!(ui.get_settings_core_smoothing());
                        tap(slint::platform::Key::DownArrow);
                        assert_eq!(ui.get_settings_selection(), 1);
                        tap(slint::platform::Key::RightArrow);
                        progress.set(2);
                    }
                    2 => {
                        assert_eq!(ui.get_settings_core_sunlight(), 1);
                        ui.invoke_settings_core_decrease();
                        progress.set(3);
                    }
                    3 => {
                        tap(slint::platform::Key::DownArrow);
                        assert_eq!(ui.get_settings_selection(), 2);
                        tap(slint::platform::Key::RightArrow);
                        progress.set(6);
                    }
                    6 => {
                        assert_eq!(ui.get_settings_core_filter(), "LCD");
                        tap(slint::platform::Key::Return);
                        progress.set(7);
                    }
                    7 => {
                        assert_eq!(ui.get_settings_core_filter(), "Smooth");
                        tap(slint::platform::Key::RightArrow);
                        progress.set(8);
                    }
                    8 => {
                        assert_eq!(ui.get_settings_core_filter(), "Smooth pixel art");
                        tap(slint::platform::Key::RightArrow);
                        progress.set(10);
                    }
                    10 => {
                        assert_eq!(ui.get_settings_core_filter(), "None");
                        tap(slint::platform::Key::LeftArrow);
                        progress.set(9);
                    }
                    9 => {
                        assert_eq!(ui.get_settings_core_filter(), "Smooth pixel art");
                        tap(slint::platform::Key::DownArrow);
                        assert_eq!(ui.get_settings_selection(), 3);
                        tap(slint::platform::Key::RightArrow);
                        progress.set(11);
                    }
                    11 => {
                        assert!(ui.get_settings_core_color());
                        assert!(ui.get_settings_core_freeze());
                        tap(slint::platform::Key::DownArrow);
                        assert_eq!(ui.get_settings_selection(), 4);
                        tap(slint::platform::Key::LeftArrow);
                        progress.set(12);
                    }
                    12 => {
                        assert!(!ui.get_settings_core_freeze());
                        assert_eq!(ui.get_settings_core_sunlight(), 0);
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
                        tap(slint::platform::Key::Escape);
                        assert_eq!(
                            (ui.get_settings_view(), ui.get_settings_selection()),
                            (0, 3)
                        );
                        // A failed write must retain the already saved in-memory value.
                        std::fs::rename(output.join("cores.json"), output.join("saved-cores.json"))
                            .unwrap();
                        std::fs::create_dir(output.join("cores.json")).unwrap();
                        live.activate(&ui, 3);
                        live.activate(&ui, 0);
                        live.activate(&ui, 0);
                        progress.set(4);
                    }
                    4 => {
                        assert!(ui.get_settings_core_feedback().starts_with("Couldn't save"));
                        assert!(ui.get_settings_core_smoothing());
                        progress.set(5);
                        slint::quit_event_loop().unwrap();
                    }
                    _ => {}
                }
            },
        );
        slint::run_event_loop().unwrap();
        assert_eq!(stage.get(), 5);
        let saved: crate::domain::core_settings::CorePreferences =
            SettingsStore::new(directory.join("saved-cores.json"))
                .load_value()
                .unwrap();
        assert!(saved.mgba.audio_smoothing);
        assert_eq!(saved.mgba.sunlight.level(), 0);
        assert_eq!(
            saved.mgba.video_filter,
            crate::domain::core_settings::VideoFilter::Xbrz
        );
        assert!(saved.mgba.color_correction);
        assert!(!saved.mgba.freeze_when_unfocused);
    }
}
