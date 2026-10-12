//! The on-screen keyboard: when it shows, what it types into, and what its
//! keys do (layout and focus are `navigation::keyboard`).
//!
//! It appears when a text field opens and the last input was a controller
//! or a touch screen, and on request (A on the field, or the field's
//! keyboard button, for a touch screen that arrives as a mouse, as it can
//! under gamescope). It writes straight into the editor's draft, so the
//! field, Save and the editor's own rules see the same text either way.
//! While open it takes controller input: the D-pad moves, A presses, X
//! deletes, L3 or R3 is Shift, + (Start) is Done, and B closes it (the editor
//! stays open). It plays the UI sounds itself (OK for a key, Shift or Done,
//! Back for Delete or Close), for a tap or click as for a button; a move is
//! heard through the navigation layer's focus snapshot.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Instant,
};

use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::{
    AppWindow, KeyboardKeyData,
    audio::UiSoundCue,
    input::{InputDevice, UiAction, UiActionEvent},
    navigation::keyboard::{KeyFocus, KeyKind, Layer, Shift, rows},
    presentation::CallbackSlot,
    services::profile::NAME_LIMIT,
};

/// The text fields it can type into (`keyboard-target` in Slint).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextTarget {
    /// The Settings key and Steam ID editor.
    SettingsKey,
    /// The Profile name editor.
    ProfileName,
    /// The Shop's search field.
    ShopSearch,
}

impl TextTarget {
    fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::SettingsKey),
            1 => Some(Self::ProfileName),
            2 => Some(Self::ShopSearch),
            _ => None,
        }
    }

    fn limit(self) -> usize {
        match self {
            Self::SettingsKey => 512,
            Self::ProfileName => NAME_LIMIT,
            Self::ShopSearch => 100,
        }
    }

    fn text(self, ui: &AppWindow) -> String {
        match self {
            Self::SettingsKey => ui.get_settings_key_draft(),
            Self::ProfileName => ui.get_profile_name_draft(),
            Self::ShopSearch => ui.get_shop_query(),
        }
        .into()
    }

    fn set_text(self, ui: &AppWindow, text: &str) {
        match self {
            Self::SettingsKey => ui.set_settings_key_draft(text.into()),
            Self::ProfileName => ui.set_profile_name_draft(text.into()),
            // The search follows what's typed, as it does from a keyboard.
            Self::ShopSearch => {
                ui.set_shop_query(text.into());
                ui.invoke_shop_search_edited(text.into());
            }
        }
    }

    /// Done: the editor's own save.
    fn done(self, ui: &AppWindow) {
        let text = self.text(ui);
        match self {
            Self::SettingsKey => ui.invoke_settings_save_key(text.into()),
            Self::ProfileName => ui.invoke_profile_save_name(text.into()),
            Self::ShopSearch => ui.invoke_shop_search_finish(),
        }
    }
}

/// Whether the keyboard took an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardFeedback {
    /// Not the keyboard's: it is closed, or the action is global.
    Ignored,
    /// Taken (with its sound played, if it has one).
    Handled,
}

struct State {
    target: Option<TextTarget>,
    layer: Layer,
    shift: Shift,
    focus: KeyFocus,
    /// The field opened with its text selected: the first key replaces it.
    replace: bool,
    /// When Shift was last tapped, to tell a double tap.
    shift_tapped: Option<Instant>,
    /// Where keys type, in characters from the start of the text.
    cursor: usize,
}

pub struct KeyboardController {
    device: Rc<Cell<InputDevice>>,
    sound: CallbackSlot<dyn Fn(UiSoundCue)>,
    state: RefCell<State>,
    keys: Rc<VecModel<KeyboardKeyData>>,
}

impl KeyboardController {
    pub fn new(ui: &AppWindow, device: Rc<Cell<InputDevice>>) -> Rc<Self> {
        let keys = Rc::new(VecModel::<KeyboardKeyData>::default());
        ui.set_keyboard_keys(ModelRc::from(Rc::clone(&keys)));
        let controller = Rc::new(Self {
            device,
            sound: RefCell::new(None),
            state: RefCell::new(State {
                target: None,
                layer: Layer::Letters,
                shift: Shift::Off,
                focus: KeyFocus::default(),
                replace: false,
                shift_tapped: None,
                cursor: 0,
            }),
            keys,
        });
        controller.publish(ui);

        let (weak, keyboard) = (ui.as_weak(), Rc::clone(&controller));
        ui.on_keyboard_key(move |index| {
            if let Some(ui) = weak.upgrade() {
                let rows = rows(keyboard.state.borrow().layer);
                if let Some(focus) = KeyFocus::from_flat(&rows, index.max(0) as usize) {
                    keyboard.state.borrow_mut().focus = focus;
                    keyboard.press(&ui);
                }
            }
        });
        let (weak, keyboard) = (ui.as_weak(), Rc::clone(&controller));
        ui.on_text_entry_changed(move |target, open| {
            if let (Some(ui), Some(target)) = (weak.upgrade(), TextTarget::from_index(target)) {
                if open {
                    if matches!(
                        keyboard.device.get(),
                        InputDevice::Gamepad | InputDevice::Touch
                    ) {
                        keyboard.show(&ui, target);
                    }
                } else if keyboard.state.borrow().target == Some(target) {
                    keyboard.hide(&ui);
                }
            }
        });
        let (weak, keyboard) = (ui.as_weak(), Rc::clone(&controller));
        ui.on_keyboard_show(move |target| {
            if let (Some(ui), Some(target)) = (weak.upgrade(), TextTarget::from_index(target)) {
                keyboard.show(&ui, target);
            }
        });
        controller
    }

    /// The UI sounds (silent when they're off).
    pub fn set_sound(&self, sound: Rc<dyn Fn(UiSoundCue)>) {
        *self.sound.borrow_mut() = Some(sound);
    }

    fn cue(&self, cue: UiSoundCue) {
        if let Some(sound) = self.sound.borrow().as_ref() {
            sound(cue);
        }
    }

    pub fn is_open(&self) -> bool {
        self.state.borrow().target.is_some()
    }

    fn show(&self, ui: &AppWindow, target: TextTarget) {
        let text = target.text(ui);
        // A name or a search opens selected: the first key replaces it.
        let replace =
            matches!(target, TextTarget::ProfileName | TextTarget::ShopSearch) && !text.is_empty();
        *self.state.borrow_mut() = State {
            target: Some(target),
            layer: Layer::Letters,
            shift: Shift::Off,
            // Start in the middle of the letters, on "g".
            focus: KeyFocus { row: 2, key: 4 },
            replace,
            shift_tapped: None,
            cursor: text.chars().count(),
        };
        self.publish(ui);
        // The field's caret goes where keys will type: the end. A selected
        // name keeps its selection until the first key replaces it.
        if !replace {
            self.place_caret(ui, &text);
        }
    }

    fn hide(&self, ui: &AppWindow) {
        self.state.borrow_mut().target = None;
        self.publish(ui);
    }

    /// Input while the keyboard is open.
    pub fn handle_action(&self, ui: &AppWindow, event: UiActionEvent) -> KeyboardFeedback {
        if !self.is_open() {
            return KeyboardFeedback::Ignored;
        }
        let rows = rows(self.state.borrow().layer);
        let mut state = self.state.borrow_mut();
        let moved = match event.action {
            UiAction::Left => state.focus.step_across(&rows, -1, event.repeated),
            UiAction::Right => state.focus.step_across(&rows, 1, event.repeated),
            UiAction::Up => state.focus.step_rows(&rows, -1),
            UiAction::Down => state.focus.step_rows(&rows, 1),
            UiAction::Accept => {
                drop(state);
                self.press(ui);
                return KeyboardFeedback::Handled;
            }
            UiAction::Secondary => {
                drop(state);
                self.cue(UiSoundCue::Back);
                self.backspace(ui);
                return KeyboardFeedback::Handled;
            }
            // LB and RB move the caret.
            UiAction::LeftBumper | UiAction::RightBumper => {
                drop(state);
                let delta = if event.action == UiAction::LeftBumper {
                    -1
                } else {
                    1
                };
                self.move_cursor(ui, delta);
                return KeyboardFeedback::Handled;
            }
            UiAction::StickPress if !event.repeated => {
                drop(state);
                self.cue(UiSoundCue::Ok);
                self.tap_shift();
                self.publish(ui);
                return KeyboardFeedback::Handled;
            }
            // + (Start) finishes, like Done.
            UiAction::Menu if !event.repeated => {
                let target = state.target;
                drop(state);
                self.cue(UiSoundCue::Ok);
                if let Some(target) = target {
                    target.done(ui);
                }
                return KeyboardFeedback::Handled;
            }
            UiAction::Back => {
                drop(state);
                self.cue(UiSoundCue::Back);
                self.hide(ui);
                return KeyboardFeedback::Handled;
            }
            UiAction::Home | UiAction::Forward => {
                drop(state);
                self.hide(ui);
                return KeyboardFeedback::Ignored;
            }
            _ => return KeyboardFeedback::Handled,
        };
        state.focus = moved;
        drop(state);
        self.publish(ui);
        KeyboardFeedback::Handled
    }

    /// Press the focused key (A, or a click or tap on it).
    fn press(&self, ui: &AppWindow) {
        let (target, layer, focus, shift) = {
            let state = self.state.borrow();
            let Some(target) = state.target else {
                return;
            };
            (target, state.layer, state.focus, state.shift)
        };
        let layout = rows(layer);
        let Some(key) = layout.get(focus.row).and_then(|row| row.get(focus.key)) else {
            return;
        };
        // Delete sounds like going back; every other key like OK.
        self.cue(if key.kind == KeyKind::Backspace {
            UiSoundCue::Back
        } else {
            UiSoundCue::Ok
        });
        match key.kind {
            KeyKind::Char(c) => {
                let c = if shift != Shift::Off { upper(c) } else { c };
                self.type_char(ui, target, c);
                let mut state = self.state.borrow_mut();
                state.shift = state.shift.after_letter();
            }
            KeyKind::Space => self.type_char(ui, target, ' '),
            KeyKind::Backspace => self.backspace(ui),
            KeyKind::Shift => self.tap_shift(),
            KeyKind::Layer => {
                let mut state = self.state.borrow_mut();
                state.layer = match state.layer {
                    Layer::Letters => Layer::Symbols,
                    Layer::Symbols => Layer::Letters,
                };
                state.focus = state.focus.clamped(&rows(state.layer));
            }
            KeyKind::Done => {
                // The editor closes, and with it the keyboard.
                target.done(ui);
                return;
            }
        }
        self.publish(ui);
    }

    /// Shift, from its key, L3 or R3 (see `Shift::tap`).
    fn tap_shift(&self) {
        let now = Instant::now();
        let mut state = self.state.borrow_mut();
        let since_last = state.shift_tapped.map(|at| now.duration_since(at));
        state.shift = state.shift.tap(since_last);
        state.shift_tapped = Some(now);
    }

    /// The text to edit and where, taking up a pending replace (a selected
    /// name): the text is then empty, the caret at its start.
    fn editing(&self, ui: &AppWindow, target: TextTarget) -> (String, usize) {
        let mut state = self.state.borrow_mut();
        if std::mem::take(&mut state.replace) {
            state.cursor = 0;
            return (String::new(), 0);
        }
        let text = target.text(ui);
        state.cursor = state.cursor.min(text.chars().count());
        (text, state.cursor)
    }

    fn type_char(&self, ui: &AppWindow, target: TextTarget, c: char) {
        let (text, cursor) = self.editing(ui, target);
        if text.chars().count() >= target.limit() {
            return;
        }
        let (text, cursor) = insert_at(&text, cursor, c);
        self.commit(ui, target, &text, cursor);
    }

    fn backspace(&self, ui: &AppWindow) {
        let Some(target) = self.state.borrow().target else {
            return;
        };
        let replacing = self.state.borrow().replace;
        let (text, cursor) = self.editing(ui, target);
        let (text, cursor) = if replacing {
            (text, cursor)
        } else {
            delete_before(&text, cursor)
        };
        self.commit(ui, target, &text, cursor);
    }

    /// Move the caret a character (LB, RB). Ends a pending replace first.
    fn move_cursor(&self, ui: &AppWindow, delta: i32) {
        let Some(target) = self.state.borrow().target else {
            return;
        };
        let text = target.text(ui);
        let length = text.chars().count();
        let mut state = self.state.borrow_mut();
        let from = if std::mem::take(&mut state.replace) {
            length
        } else {
            state.cursor.min(length)
        };
        let to = from.saturating_add_signed(delta as isize).min(length);
        state.cursor = to;
        drop(state);
        if to != from {
            self.cue(UiSoundCue::Navigation);
        }
        self.place_caret(ui, &text);
    }

    fn commit(&self, ui: &AppWindow, target: TextTarget, text: &str, cursor: usize) {
        target.set_text(ui, text);
        self.state.borrow_mut().cursor = cursor;
        self.place_caret(ui, text);
    }

    /// Show the caret where keys type: the field moves its cursor there
    /// (`keyboard-caret`, a byte offset, applied on each `keyboard-edit`).
    fn place_caret(&self, ui: &AppWindow, text: &str) {
        let cursor = self.state.borrow().cursor;
        ui.set_keyboard_caret(byte_offset(text, cursor) as i32);
        ui.set_keyboard_edit(ui.get_keyboard_edit().wrapping_add(1));
    }

    fn publish(&self, ui: &AppWindow) {
        let state = self.state.borrow();
        let rows = rows(state.layer);
        let capitals = state.shift != Shift::Off && state.layer == Layer::Letters;
        let layer = state.layer;
        let keys: Vec<KeyboardKeyData> = rows
            .iter()
            .enumerate()
            .flat_map(|(row, keys)| {
                keys.iter().map(move |key| {
                    let (label, kind) = match key.kind {
                        KeyKind::Char(c) => {
                            let c = if capitals { upper(c) } else { c };
                            (c.to_string(), 0)
                        }
                        KeyKind::Backspace => (String::new(), 1),
                        KeyKind::Shift => (String::new(), 2),
                        KeyKind::Layer => (
                            match layer {
                                Layer::Letters => "?123",
                                Layer::Symbols => "ABC",
                            }
                            .to_owned(),
                            3,
                        ),
                        KeyKind::Space => ("Space".to_owned(), 4),
                        KeyKind::Done => ("Done".to_owned(), 5),
                    };
                    KeyboardKeyData {
                        label: label.into(),
                        kind,
                        row: row as i32,
                        x: key.x,
                        width: key.width,
                    }
                })
            })
            .collect();
        // Only rebuild the keys when they change (layer or Shift), so a move
        // doesn't redraw every key.
        let same = self.keys.row_count() == keys.len()
            && keys
                .iter()
                .enumerate()
                .all(|(i, key)| self.keys.row_data(i).as_ref() == Some(key));
        if !same {
            self.keys.set_vec(keys);
        }
        ui.set_keyboard_open(state.target.is_some());
        ui.set_keyboard_focused(state.focus.flat(&rows) as i32);
        ui.set_keyboard_shift(match state.shift {
            Shift::Off => 0,
            Shift::Once => 1,
            Shift::Locked => 2,
        });
    }
}

fn upper(c: char) -> char {
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(single), None) => single,
        _ => c,
    }
}

/// `text` with `c` inserted at character `cursor`, and the caret after it.
fn insert_at(text: &str, cursor: usize, c: char) -> (String, usize) {
    let mut text = text.to_owned();
    text.insert(byte_offset(&text, cursor), c);
    (text, cursor + 1)
}

/// `text` without the character before `cursor`, and the caret where it was.
fn delete_before(text: &str, cursor: usize) -> (String, usize) {
    if cursor == 0 {
        return (text.to_owned(), 0);
    }
    let mut text = text.to_owned();
    text.remove(byte_offset(&text, cursor - 1));
    (text, cursor - 1)
}

/// The byte offset of character `index` (the end past the last).
fn byte_offset(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(offset, _)| offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_and_deleting_happen_at_the_caret() {
        assert_eq!(insert_at("Andew", 3, 'r'), ("Andrew".into(), 4));
        assert_eq!(insert_at("", 0, 'é'), ("é".into(), 1));
        assert_eq!(insert_at("éa", 1, 'b'), ("éba".into(), 2));
        assert_eq!(delete_before("Andrew", 4), ("Andew".into(), 3));
        assert_eq!(delete_before("Andrew", 0), ("Andrew".into(), 0));
        assert_eq!(delete_before("éb", 1), ("b".into(), 0));
        assert_eq!(byte_offset("éb", 1), 2);
        assert_eq!(byte_offset("éb", 9), 3);
    }
}
