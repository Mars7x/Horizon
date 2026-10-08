//! Controller-first Settings > Third-Party page; Slint displays state only.
use std::{cell::RefCell, rc::Rc, sync::{Arc, atomic::{AtomicU64, Ordering}}};
use slint::ComponentHandle;

use crate::{AppWindow, input::{UiAction, UiActionEvent}, platform::wayland_clipboard::WaylandClipboard, services::settings::{SettingsService, ArtworkPreferences}};

#[derive(Default)]
struct PageState {
    third_party_open: bool,
    selected: i32,
    editing_key: bool,
    editor_target: i32,
    feedback: String,
}

pub struct SettingsController {
    page: RefCell<PageState>,
    service: RefCell<SettingsService>,
    artwork_changed: RefCell<Option<Rc<dyn Fn(ArtworkPreferences)>>>,
    artwork_refresh: RefCell<Option<Rc<dyn Fn(ArtworkPreferences)>>>,
    wayland_clipboard: RefCell<Option<Arc<WaylandClipboard>>>,
    paste_generation: Arc<AtomicU64>,
}

impl SettingsController {
    pub fn new(ui: &AppWindow, service: SettingsService) -> Rc<Self> {
        let controller = Rc::new(Self {
            page: RefCell::new(PageState::default()),
            service: RefCell::new(service),
            artwork_changed: RefCell::new(None),
            artwork_refresh: RefCell::new(None),
            wayland_clipboard: RefCell::new(None),
            paste_generation: Arc::new(AtomicU64::new(0)),
        });
        controller.publish(ui);
        controller.bind_callbacks(ui);
        controller
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
                    page.editor_target = next_editor_target(page.editor_target, action, can_save);
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
                    page.third_party_open,
                    self.service.borrow().has_steamgriddb_key(),
                );
                let delta = if action == UiAction::Up { -1 } else { 1 };
                page.selected = (page.selected + delta + count) % count;
            }
            UiAction::Left | UiAction::Right => {
                if self.page.borrow().third_party_open && self.page.borrow().selected == 1 {
                    let preferred = action == UiAction::Right;
                    self.set_preference(ui, preferred);
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
        ui.on_settings_dismiss_refresh(move || {
            if let Some(ui) = weak.upgrade() { ui.set_settings_refresh_open(false); }
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
        if !self.page.borrow().third_party_open {
            if selected == 0 {
                let mut page = self.page.borrow_mut();
                page.third_party_open = true;
                page.selected = 0;
                page.feedback.clear();
            }
            self.publish(ui);
            return;
        }
        if !(0..=3).contains(&selected) { return; }
        self.page.borrow_mut().selected = selected;
        match selected {
            0 => {
                // A new edit session never inherits feedback from a previous save.
                let mut page = self.page.borrow_mut();
                page.editing_key = true;
                page.editor_target = 0;
                page.feedback.clear();
                drop(page);
                ui.set_settings_key_draft("".into());
            }
            1 => {
                let next = !self.service.borrow().prefer_steamgriddb_artwork();
                self.set_preference(ui, next);
            }
            2 => {
                if self.service.borrow().has_steamgriddb_key() {
                    if let Some(callback) = self.artwork_refresh.borrow().as_ref() {
                        // Open the progress dialog before starting the worker.
                        ui.set_settings_refresh_completed(0);
                        ui.set_settings_refresh_total(0);
                        ui.set_settings_refresh_error("".into());
                        ui.set_settings_refresh_running(true);
                        ui.set_settings_refresh_open(true);
                        self.page.borrow_mut().feedback.clear();
                        callback(self.artwork_preferences());
                    }
                }
            }
            3 => {
                if self.service.borrow().has_steamgriddb_key() {
                    let result = self.service.borrow_mut().remove_steamgriddb_key();
                    if result.is_ok() {
                        self.notify_artwork_changed();
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
            _ => unreachable!(),
        }
        self.publish(ui);
    }

    fn ensure_wayland_clipboard(&self, ui: &AppWindow) -> Option<Arc<WaylandClipboard>> {
        let mut clipboard = self.wayland_clipboard.borrow_mut();
        if clipboard.is_none() {
            if let Ok(reader) = WaylandClipboard::from_window(ui.window()) {
                *clipboard = Some(Arc::new(reader));
            }
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
        if result.is_ok() { self.notify_artwork_changed(); }
        self.page.borrow_mut().feedback = if result.is_ok() {
            "Artwork source preference saved.".into()
        } else {
            "Could not save the artwork preference.".into()
        };
        self.publish(ui);
    }

    fn save_key(&self, ui: &AppWindow, key: &str) {
        if !self.page.borrow().editing_key { return; }
        let result = self.service.borrow_mut().set_steamgriddb_key(key);
        match result {
            Ok(()) => {
                self.notify_artwork_changed();
                self.paste_generation.fetch_add(1, Ordering::Relaxed);
                self.page.borrow_mut().editing_key = false;
                self.page.borrow_mut().feedback = "SteamGridDB API key saved locally.".into();
                ui.set_settings_editing_key(false);
                ui.set_settings_key_draft("".into());
            }
            Err(message) => { self.page.borrow_mut().feedback = message; }
        }
        self.publish(ui);
    }

    fn cancel_key(&self, ui: &AppWindow) {
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
            return true;
        }
        if self.page.borrow().editing_key {
            self.cancel_key(ui);
            return true;
        }
        if !self.page.borrow().third_party_open { return false; }
        let mut page = self.page.borrow_mut();
        page.third_party_open = false;
        page.selected = 0;
        page.feedback.clear();
        drop(page);
        self.publish(ui);
        true
    }

    fn publish(&self, ui: &AppWindow) {
        let page = self.page.borrow();
        let settings = self.service.borrow();
        ui.set_settings_view(i32::from(page.third_party_open));
        ui.set_settings_selection(page.selected);
        ui.set_settings_key_present(settings.has_steamgriddb_key());
        ui.set_settings_prefer_artwork(settings.prefer_steamgriddb_artwork());
        ui.set_settings_editing_key(page.editing_key);
        ui.set_settings_editor_target(page.editor_target);
        ui.set_settings_feedback(page.feedback.clone().into());
    }
}

/// Keep controller focus within visible/enabled Third-Party actions.
fn selection_count(third_party_open: bool, has_key: bool) -> i32 {
    if !third_party_open { 1 } else if has_key { 4 } else { 2 }
}

/// Semantic controller grid for the API-key modal (not a real text cursor).
/// Disabled Save is excluded so every selected action is immediately usable.
fn next_editor_target(target: i32, direction: UiAction, can_save: bool) -> i32 {
    use UiAction::{Down, Left, Right, Up};
    let candidate = match (target, direction) {
        (0, Right) => 1,
        (0, Down) => 2,
        (1, Left) => 0,
        (1, Down) => if can_save { 3 } else { 2 },
        (2, Up) => 0,
        (2, Right) if can_save => 3,
        (3, Left) => 2,
        (3, Up) => 1,
        _ => target,
    };
    candidate
}

#[cfg(test)]
mod editor_navigation_tests {
    use super::next_editor_target;
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
        assert_eq!(super::selection_count(false, false), 1);
        assert_eq!(super::selection_count(true, false), 2);
        assert_eq!(super::selection_count(true, true), 4);
    }

    #[test]
    fn disabled_save_is_skipped() {
        assert_eq!(next_editor_target(1, UiAction::Down, false), 2);
        assert_eq!(next_editor_target(2, UiAction::Right, false), 2);
    }
}
