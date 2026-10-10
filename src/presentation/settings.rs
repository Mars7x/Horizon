//! Controller-first Settings > Third-Party page; Slint displays state only.
use std::{cell::RefCell, rc::Rc, sync::{Arc, atomic::{AtomicU64, Ordering}}};
use slint::ComponentHandle;

use crate::{AppWindow,
    audio::UiSoundCue,
    appearance::{ThemePreference, ACCENT_PRESETS},
    input::{UiAction, UiActionEvent},
    navigation::step_with_edge_wrap,
    platform::wayland_clipboard::WaylandClipboard,
    presentation::{CallbackSlot, appearance::AppearanceController},
    services::{settings::{SettingsService, ArtworkPreferences}, steam_account::SteamAccountService}};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SettingsView { #[default] Root, Appearance, ThirdParty, SteamAccount }

// SteamGridDB is a separate service; the shared prefix is not redundant.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum EditorKind { #[default] SteamGridDb, SteamId, SteamWebKey }

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
}

pub struct SettingsController {
    page: RefCell<PageState>,
    service: RefCell<SettingsService>,
    steam_account: Rc<RefCell<SteamAccountService>>,
    steam_account_changed: CallbackSlot<dyn Fn()>,
    appearance: AppearanceController,
    artwork_changed: CallbackSlot<dyn Fn(ArtworkPreferences)>,
    artwork_refresh: CallbackSlot<dyn Fn(ArtworkPreferences)>,
    sound_changed: CallbackSlot<dyn Fn(bool)>,
    action_sound: CallbackSlot<dyn Fn(UiSoundCue)>,
    wayland_clipboard: RefCell<Option<Arc<WaylandClipboard>>>,
    paste_generation: Arc<AtomicU64>,
}

impl SettingsController {
    pub fn new(ui: &AppWindow, service: SettingsService, appearance: AppearanceController, steam_account: Rc<RefCell<SteamAccountService>>) -> Rc<Self> {
        let controller = Rc::new(Self {
            page: RefCell::new(PageState::default()),
            service: RefCell::new(service),
            steam_account,
            steam_account_changed: RefCell::new(None),
            appearance,
            artwork_changed: RefCell::new(None),
            artwork_refresh: RefCell::new(None),
            sound_changed: RefCell::new(None),
            action_sound: RefCell::new(None),
            wayland_clipboard: RefCell::new(None),
            paste_generation: Arc::new(AtomicU64::new(0)),
        });
        controller.publish(ui);
        controller.bind_callbacks(ui);
        controller
    }

    pub fn set_steam_account_changed(&self, changed: Rc<dyn Fn()>) {
        *self.steam_account_changed.borrow_mut() = Some(changed);
    }

    fn notify_steam_account_changed(&self) {
        if let Some(callback) = self.steam_account_changed.borrow().as_ref() { callback(); }
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

    pub fn set_sound_changed(&self, callback: Rc<dyn Fn(bool)>) {
        *self.sound_changed.borrow_mut() = Some(callback);
    }

    pub fn set_action_sound(&self, callback: Rc<dyn Fn(UiSoundCue)>) {
        *self.action_sound.borrow_mut() = Some(callback);
    }

    fn cue(&self, cue: UiSoundCue) {
        if let Some(callback) = self.action_sound.borrow().as_ref() { callback(cue); }
    }

    fn set_ui_sounds_enabled(&self, ui: &AppWindow, enabled: bool) {
        if self.appearance.preferences().ui_sounds_enabled == enabled { return; }
        let result = self.appearance.set_ui_sounds_enabled(ui, enabled);
        if result.is_ok() {
            if let Some(callback) = self.sound_changed.borrow().as_ref() { callback(enabled); }
            // Off mutes immediately; On can confirm with an OK cue.
            self.cue(UiSoundCue::Ok);
        }
        self.page.borrow_mut().feedback = if result.is_ok() {
            String::new()
        } else { "Could not save UI sound preference.".into() };
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

    pub fn on_enter(&self, ui: &AppWindow) {
        self.reset(ui);
        // Register wl_data_device before the first user paste. The native
        // clipboard worker then has time to observe the current selection.
        // Failure is non-fatal: try again on the actual Ctrl+V gesture.
        self.ensure_wayland_clipboard(ui);
    }
    pub fn on_leave(&self, ui: &AppWindow) { self.reset(ui); }

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
                self.cue(if action == UiAction::Back { UiSoundCue::Back } else { UiSoundCue::Ok });
            } else if action == UiAction::Home {
                ui.set_settings_refresh_open(false);
                return false;
            }
            return true;
        }
        if self.page.borrow().editing_key {
            match action {
                UiAction::Back => self.cancel_key(ui),
                UiAction::Accept if !event.repeated => {
                    let target = self.page.borrow().editor_target;
                    if target == 0 {
                        // Accept on the field confirms the typed key.
                        self.save_key(ui, ui.get_settings_key_draft().as_str());
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
                    } else { next_editor_target(page.editor_target, action, can_save) };
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
                // An unavailable remove action is never included in focus wrap.
                let count = selection_count(
                    page.view,
                    self.service.borrow().has_steamgriddb_key(),
                    self.steam_account.borrow().saved(),
                );
                let delta = if action == UiAction::Up { -1 } else { 1 };
                page.selected = if page.view == SettingsView::Appearance {
                    appearance_step_vertical(page.selected, action, page.last_accent_selection.unwrap_or(3), event.repeated)
                } else if page.view == SettingsView::ThirdParty && !self.service.borrow().has_steamgriddb_key() {
                    third_party_step_without_key(page.selected, delta, event.repeated)
                } else { step_with_edge_wrap(page.selected, 0, count - 1, delta, event.repeated) };
                if page.view == SettingsView::Appearance && (3..=12).contains(&page.selected) {
                    page.last_accent_selection = Some(page.selected);
                }
            }
            UiAction::Left | UiAction::Right => {
                let page = self.page.borrow();
                if page.view == SettingsView::ThirdParty && page.selected == 1 {
                    let preferred = action == UiAction::Right;
                    drop(page);
                    self.set_preference(ui, preferred);
                } else if page.view == SettingsView::Appearance && page.selected == 13 {
                    drop(page);
                    self.set_ui_sounds_enabled(ui, action == UiAction::Right);
                } else if page.view == SettingsView::Appearance {
                    drop(page);
                    let mut page = self.page.borrow_mut();
                    page.selected = appearance_step_horizontal(page.selected, action, event.repeated);
                    if (3..=12).contains(&page.selected) {
                        page.last_accent_selection = Some(page.selected);
                    }
                }
            }
            UiAction::Accept if !event.repeated => {
                let selected = self.page.borrow().selected;
                self.activate(ui, selected);
            }
            UiAction::Home | UiAction::Menu => return false,
            _ => {}
        }
        self.publish(ui);
        true
    }

    fn bind_callbacks(self: &Rc<Self>, ui: &AppWindow) {
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_settings_activate(move |index| {
            if let Some(ui) = weak.upgrade() { controller.activate(&ui, index); }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_settings_back(move || {
            if let Some(ui) = weak.upgrade() { controller.back_inside(&ui); }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_settings_dismiss_refresh(move || {
            if let Some(ui) = weak.upgrade() { controller.back_inside(&ui); }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_settings_save_key(move |key| {
            if let Some(ui) = weak.upgrade() { controller.save_key(&ui, key.as_str()); }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_settings_cancel_key(move || {
            if let Some(ui) = weak.upgrade() { controller.cancel_key(&ui); }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_settings_request_wayland_paste(move || {
            if let Some(ui) = weak.upgrade() { controller.paste_from_wayland(&ui); }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_settings_paste_failed(move || {
            if let Some(ui) = weak.upgrade() {
                controller.page.borrow_mut().feedback =
                    "Unable to read clipboard text. Check the copied key and retry.".into();
                controller.publish(&ui);
            }
        });
    }

    fn activate(&self, ui: &AppWindow, selected: i32) {
        if self.page.borrow().editing_key { return; }
        let view = self.page.borrow().view;
        match view {
            SettingsView::Root => {
                if !(0..=1).contains(&selected) { return; }
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
                if !(0..=13).contains(&selected) { return; }
                {
                    let mut page = self.page.borrow_mut();
                    page.selected = selected;
                    if (3..=12).contains(&selected) {
                        page.last_accent_selection = Some(selected);
                    }
                }
                if selected == 13 {
                    let enabled = !self.appearance.preferences().ui_sounds_enabled;
                    self.set_ui_sounds_enabled(ui, enabled);
                    return;
                }
                let result = match selected {
                    0 => self.appearance.set_theme_preference(ui, ThemePreference::System),
                    1 => self.appearance.set_theme_preference(ui, ThemePreference::Light),
                    2 => self.appearance.set_theme_preference(ui, ThemePreference::Dark),
                    3 => self.appearance.use_system_accent(ui),
                    _ => self.appearance.set_custom_accent(ui, ACCENT_PRESETS[(selected - 4) as usize].1),
                };
                self.page.borrow_mut().feedback = if result.is_ok() {
                    String::new()
                } else {
                    "Could not save appearance settings.".into()
                };
                if result.is_ok() { self.cue(UiSoundCue::Ok); }
                self.publish(ui);
                return;
            }
            SettingsView::ThirdParty | SettingsView::SteamAccount => {}
        }
        if view == SettingsView::SteamAccount {
            if !(0..=2).contains(&selected) { return; }
            self.page.borrow_mut().selected = selected;
            match selected {
                0 => self.open_editor(ui, EditorKind::SteamId),
                1 => self.open_editor(ui, EditorKind::SteamWebKey),
                2 => {
                    let has_saved_account = self.steam_account.borrow().saved();
                    if has_saved_account {
                        let result = self.steam_account.borrow_mut().remove();
                        let ok = result.is_ok();
                        self.page.borrow_mut().feedback = if ok {
                            "Saved Steam account removed locally.".into()
                        } else { "Unable to remove Steam account settings.".into() };
                        if ok { self.notify_steam_account_changed(); self.cue(UiSoundCue::Ok); }
                    }
                }
                _ => {}
            }
            self.publish(ui);
            return;
        }
        if !(0..=4).contains(&selected) { return; }
        if (selected == 2 || selected == 3)
            && !self.service.borrow().has_steamgriddb_key() { return; }
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
                    && let Some(callback) = self.artwork_refresh.borrow().as_ref() {
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
            3 => {
                if self.service.borrow().has_steamgriddb_key() {
                    let result = self.service.borrow_mut().remove_steamgriddb_key();
                    if result.is_ok() {
                        self.notify_artwork_changed();
                        self.cue(UiSoundCue::Ok);
                        // Move focus to a usable control after removing the key.
                        self.page.borrow_mut().selected = 0;
                    }
                    self.page.borrow_mut().feedback = if result.is_ok() {
                        "Saved API key removed.".into()
                    } else {
                        "Could not remove the API key.".into()
                    };
                }
            }
            4 => {
                let mut page = self.page.borrow_mut();
                page.view = SettingsView::SteamAccount;
                page.selected = 0;
                page.feedback.clear();
                self.cue(UiSoundCue::Ok);
            }
            _ => {}
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
            self.steam_account.borrow().identity().unwrap_or("").to_owned()
        } else { String::new() };
        ui.set_settings_key_draft(draft.into());
        self.cue(UiSoundCue::Ok);
    }

    fn ensure_wayland_clipboard(&self, ui: &AppWindow) -> Option<Arc<WaylandClipboard>> {
        let mut clipboard = self.wayland_clipboard.borrow_mut();
        if clipboard.is_none()
            && let Ok(reader) = WaylandClipboard::from_window(ui.window()) {
            *clipboard = Some(Arc::new(reader));
        }
        clipboard.as_ref().cloned()
    }

    fn paste_from_wayland(&self, ui: &AppWindow) {
        if !self.page.borrow().editing_key { return; }
        // Keep the entire clipboard operation off the Slint event loop.
        let Some(clipboard) = self.ensure_wayland_clipboard(ui) else {
            self.page.borrow_mut().feedback =
                "Wayland clipboard is unavailable for this window.".into();
            self.publish(ui);
            return;
        };
        let initial_draft = ui.get_settings_key_draft().to_string();
        let generation = self.paste_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let epoch = Arc::clone(&self.paste_generation);
        let weak_ui = ui.as_weak();
        std::thread::spawn(move || {
            let text = clipboard.read_key();
            let _ = weak_ui.upgrade_in_event_loop(move |ui| {
                if !ui.get_settings_editing_key() ||
                    ui.get_settings_editor_target() != 0 ||
                    epoch.load(Ordering::Relaxed) != generation ||
                    ui.get_settings_key_draft().as_str() != initial_draft {
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
                            let _ = window.dispatch_event_with_result(
                                WindowEvent::KeyReleased { text: control.into() }
                            );
                        }
                        for ch in text.chars() {
                            let key: slint::SharedString = ch.to_string().into();
                            let _ = window.dispatch_event_with_result(
                                WindowEvent::KeyPressed { text: key.clone() }
                            );
                            let _ = window.dispatch_event_with_result(
                                WindowEvent::KeyReleased { text: key }
                            );
                        }
                        if held & 1 != 0 {
                            let _ = window.dispatch_event_with_result(
                                WindowEvent::KeyPressed { text: Key::Control.into() }
                            );
                        }
                        if held & 2 != 0 {
                            let _ = window.dispatch_event_with_result(
                                WindowEvent::KeyPressed { text: Key::ControlR.into() }
                            );
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
        let result = self.service.borrow_mut().set_prefer_steamgriddb_artwork(preferred);
        if result.is_ok() {
            self.notify_artwork_changed();
            self.cue(UiSoundCue::Ok);
        }
        self.page.borrow_mut().feedback = if result.is_ok() {
            "Artwork source preference saved.".into()
        } else {
            "Could not save the artwork preference.".into()
        };
        self.publish(ui);
    }

    fn save_key(&self, ui: &AppWindow, key: &str) {
        if !self.page.borrow().editing_key { return; }
        let kind = self.page.borrow().editor_kind;
        let result = match kind {
            EditorKind::SteamGridDb => self.service.borrow_mut().set_steamgriddb_key(key),
            EditorKind::SteamId => self.steam_account.borrow_mut().set_steam_id64(key).map_err(str::to_owned),
            EditorKind::SteamWebKey => self.steam_account.borrow_mut().set_web_api_key(key).map_err(str::to_owned),
        };
        match result {
            Ok(()) => {
                if kind == EditorKind::SteamGridDb { self.notify_artwork_changed(); }
                else { self.notify_steam_account_changed(); }
                self.paste_generation.fetch_add(1, Ordering::Relaxed);
                self.page.borrow_mut().editing_key = false;
                self.page.borrow_mut().feedback = match kind {
                    EditorKind::SteamGridDb => "SteamGridDB API key saved locally.",
                    EditorKind::SteamId => if self.steam_account.borrow().connected() {
                        "SteamID64 updated."
                    } else { "SteamID64 entered. Add the Web API key to connect." },
                    EditorKind::SteamWebKey => "Steam Web API key saved locally.",
                }.into();
                ui.set_settings_editing_key(false);
                ui.set_settings_key_draft("".into());
                self.cue(UiSoundCue::Ok);
            }
            Err(message) => { self.page.borrow_mut().feedback = message; }
        }
        self.publish(ui);
    }

    fn cancel_key(&self, ui: &AppWindow) {
        if !self.page.borrow().editing_key { return; }
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
            SettingsView::Root => 0, SettingsView::Appearance => 1, SettingsView::ThirdParty => 2, SettingsView::SteamAccount => 3
        });
        let prefs = self.appearance.preferences();
        ui.set_settings_theme_index(prefs.theme_index());
        ui.set_settings_accent_index(prefs.accent_index());
        ui.set_settings_ui_sounds_enabled(prefs.ui_sounds_enabled);
        ui.set_settings_appearance_feedback(if page.view == SettingsView::Appearance {
            page.feedback.clone().into()
        } else { "".into() });
        ui.set_settings_selection(page.selected);
        ui.set_settings_key_present(settings.has_steamgriddb_key());
        let account = self.steam_account.borrow();
        ui.set_settings_steam_identity(account.identity().unwrap_or("Not configured").into());
        ui.set_settings_steam_connected(account.connected());
        ui.set_settings_steam_saved(account.saved());
        ui.set_settings_editor_kind(match page.editor_kind {
            EditorKind::SteamGridDb => 0, EditorKind::SteamId => 1, EditorKind::SteamWebKey => 2,
        });
        ui.set_settings_prefer_artwork(settings.prefer_steamgriddb_artwork());
        ui.set_settings_editing_key(page.editing_key);
        ui.set_settings_editor_target(page.editor_target);
        ui.set_settings_feedback(page.feedback.clone().into());
    }
}

/// Intra-Settings Back restores the row that opened the child page.
/// A brand-new visit uses PageState::default() and starts at Appearance.
fn settings_parent(view: SettingsView) -> Option<(SettingsView, i32)> {
    match view {
        SettingsView::Root => None,
        SettingsView::Appearance => Some((SettingsView::Root, 0)),
        SettingsView::ThirdParty => Some((SettingsView::Root, 1)),
        SettingsView::SteamAccount => Some((SettingsView::ThirdParty, 4)),
    }
}

/// Keep controller focus within visible/enabled Third-Party actions.
fn selection_count(view: SettingsView, _has_key: bool, steam_saved: bool) -> i32 {
    match view {
        SettingsView::Root => 2,
        SettingsView::Appearance => 14,
        SettingsView::ThirdParty => 5,
        SettingsView::SteamAccount => if steam_saved { 3 } else { 2 },
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

fn third_party_step_without_key(selected: i32, delta: i32, repeated: bool) -> i32 {
    let selectable = [0, 1, 4];
    let index = selectable.iter().position(|&item| item == selected).unwrap_or(0) as i32;
    let next = step_with_edge_wrap(index, 0, selectable.len() as i32 - 1, delta, repeated);
    selectable[next as usize]
}

/// A root category's explicit choice is authoritative. Pointer clicks pass
/// their row index directly, while controller Accept uses the Rust focus index.
fn root_destination(index: i32) -> Option<SettingsView> {
    match index {
        0 => Some(SettingsView::Appearance),
        1 => Some(SettingsView::ThirdParty),
        _ => None,
    }
}

// Two horizontal focus rows match the visible Appearance layout:
// Theme (System, Light, Dark), then Accent (System + nine swatches).
// The UI Sounds switch is a separate full-width row. Do not separate the
// System accent from the swatches: they share the same y coordinate.
fn appearance_step_horizontal(index: i32, direction: UiAction, repeated: bool) -> i32 {
    let (first, last) = match index {
        0..=2 => (0, 2),
        3..=12 => (3, 12),
        _ => return index, // UI Sounds uses Left/Right to switch Off/On.
    };
    let delta = if direction == UiAction::Left { -1 } else { 1 };
    step_with_edge_wrap(index, first, last, delta, repeated)
}

// Vertical movement follows approximate horizontal positions of the visual
// controls, preserving the last accent on a return trip. The accent layout is
// fixed: System + Red/Orange align to Theme System, Yellow through Blue to
// Theme Light, and Purple/Pink/White to Theme Dark.
fn appearance_step_vertical(index: i32, direction: UiAction, last_accent: i32, repeated: bool) -> i32 {
    let last_accent = if (3..=12).contains(&last_accent) { last_accent } else { 3 };
    match (index, direction) {
        (0, UiAction::Down) => if (3..=5).contains(&last_accent) { last_accent } else { 3 },
        (1, UiAction::Down) => if (6..=9).contains(&last_accent) { last_accent } else { 8 },
        (2, UiAction::Down) => if (10..=12).contains(&last_accent) { last_accent } else { 12 },
        (0..=2, UiAction::Up) => if repeated { index } else { 13 },
        (3..=5, UiAction::Up) => 0,
        (6..=9, UiAction::Up) => 1,
        (10..=12, UiAction::Up) => 2,
        (3..=12, UiAction::Down) => 13,
        (13, UiAction::Up) => last_accent,
        (13, UiAction::Down) => if repeated { 13 } else { 0 },
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
        (1, Down) => if can_save { 3 } else { 2 },
        (2, Up) => 0,
        (2, Right) if can_save => 3,
        (3, Left) => 2,
        (3, Up) => 1,
        _ => target,
    }
}

#[cfg(test)]
mod editor_navigation_tests {
    use super::{next_editor_target, SettingsView, appearance_step_horizontal, appearance_step_vertical};
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
        assert_eq!(super::selection_count(SettingsView::Root, false, false), 2);
        assert_eq!(super::selection_count(SettingsView::Appearance, false, false), 14);
        assert_eq!(super::selection_count(SettingsView::ThirdParty, false, false), 5);
        assert_eq!(super::third_party_step_without_key(1, 1, false), 4);
        assert_eq!(super::selection_count(SettingsView::ThirdParty, true, false), 5);
        assert_eq!(super::selection_count(SettingsView::SteamAccount, true, true), 3);
        assert_eq!(super::selection_count(SettingsView::SteamAccount, true, false), 2);
    }

    #[test]
    fn appearance_navigation_respects_visual_rows() {
        use UiAction::{Down, Left, Right, Up};
        assert_eq!(appearance_step_horizontal(0, Left, false), 2);
        assert_eq!(appearance_step_horizontal(2, Right, false), 0);

        // System accent is in the SAME horizontal row as the nine swatches.
        let mut focus = 3;
        for expected in 4..=12 {
            focus = appearance_step_horizontal(focus, Right, false);
            assert_eq!(focus, expected);
        }
        assert_eq!(appearance_step_horizontal(12, Right, false), 3);
        assert_eq!(appearance_step_horizontal(3, Left, false), 12);
        assert_eq!(appearance_step_horizontal(4, Left, false), 3);
        for accent in 3..=12 {
            let next = appearance_step_horizontal(accent, Right, false);
            assert_eq!(appearance_step_horizontal(next, Left, false), accent);
        }

        // Up/Down uses spatial columns and reverses when returning to a swatch.
        for accent in 3..=12 {
            let theme = appearance_step_vertical(accent, Up, accent, false);
            let expected_theme = if accent <= 5 { 0 } else if accent <= 9 { 1 } else { 2 };
            assert_eq!(theme, expected_theme);
            assert_eq!(appearance_step_vertical(theme, Down, accent, false), accent);
            assert_eq!(appearance_step_vertical(accent, Down, accent, false), 13);
            assert_eq!(appearance_step_vertical(13, Up, accent, false), accent);
        }
        assert_eq!(appearance_step_vertical(0, Down, 8, false), 3);
        assert_eq!(appearance_step_vertical(1, Down, 3, false), 8);
        assert_eq!(appearance_step_vertical(2, Down, 3, false), 12);
        assert_eq!(appearance_step_vertical(13, Up, 0, false), 3);
        assert_eq!(appearance_step_vertical(13, Down, 8, false), 0);
        assert_eq!(appearance_step_horizontal(13, Left, false), 13);
        assert_eq!(appearance_step_horizontal(13, Right, false), 13);
    }

    #[test]
    fn held_navigation_never_wraps_in_settings() {
        use UiAction::{Down, Left, Right, Up};
        assert_eq!(appearance_step_horizontal(2, Right, true), 2);
        assert_eq!(appearance_step_horizontal(0, Left, true), 0);
        assert_eq!(appearance_step_horizontal(12, Right, true), 12);
        assert_eq!(appearance_step_horizontal(3, Left, true), 3);
        assert_eq!(appearance_step_horizontal(11, Right, true), 12);
        assert_eq!(appearance_step_vertical(0, Up, 3, true), 0);
        assert_eq!(appearance_step_vertical(13, Down, 3, true), 13);
        assert_eq!(appearance_step_vertical(0, Up, 3, false), 13);
        assert_eq!(appearance_step_vertical(13, Down, 3, false), 0);
    }

    #[test]
    fn disabled_save_is_skipped() {
        assert_eq!(next_editor_target(1, UiAction::Down, false), 2);
        assert_eq!(next_editor_target(2, UiAction::Right, false), 2);
    }
}

#[cfg(test)]
mod root_activation_regressions {
    use super::{root_destination, settings_parent, PageState, SettingsView};

    #[test]
    fn back_inside_settings_restores_the_parent_row() {
        assert_eq!(settings_parent(SettingsView::Root), None);
        assert_eq!(settings_parent(SettingsView::Appearance), Some((SettingsView::Root, 0)));
        assert_eq!(settings_parent(SettingsView::ThirdParty), Some((SettingsView::Root, 1)));
        assert_eq!(settings_parent(SettingsView::SteamAccount), Some((SettingsView::ThirdParty, 4)));
        let fresh = PageState::default();
        assert_eq!(fresh.view, SettingsView::Root);
        assert_eq!(fresh.selected, 0);
    }

    #[test]
    fn root_category_clicks_open_the_corresponding_page() {
        assert_eq!(root_destination(0), Some(SettingsView::Appearance));
        assert_eq!(root_destination(1), Some(SettingsView::ThirdParty));
        assert_eq!(root_destination(-1), None);
        assert_eq!(root_destination(2), None);
    }
}
