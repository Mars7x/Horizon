use std::{cell::Cell, rc::Rc, time::Instant};

use tracing::warn;

use super::{
    actions::{ControllerFamily, GlyphStyle, InputDevice, UiAction, UiActionEvent},
    keyboard,
    sdl::{
        ControllerStatus, DIGITAL_INITIAL_REPEAT_DELAY, DIGITAL_REPEAT_INTERVAL, GamepadActionSink,
        SdlGamepadInput,
    },
};

// The OS may emit keyboard repeat at 20-50+ Hz, much faster than SDL's
// deliberate controller navigation pace. Track each held direction and cap
// OS-generated repeats to the controller's initial delay + repeat interval.
#[derive(Clone, Copy)]
struct KeyboardRepeat {
    action: UiAction,
    pressed_at: Instant,
    last_emitted_at: Instant,
}

/// Coordinates Horizon input adapters and publishes only semantic actions.
///
/// The manager deliberately has no knowledge of screens, game selection, or
/// other presentation state. Those concerns live above the input layer.
pub struct InputManager {
    action_sink: Rc<dyn Fn(UiActionEvent)>,
    gamepad_input: Option<SdlGamepadInput>,
    ui_input_enabled: Cell<bool>,
    keyboard_repeat: Cell<Option<KeyboardRepeat>>,
    /// The device behind the last action, shared with the UI.
    last_device: Rc<Cell<InputDevice>>,
    /// Which glyphs hints show; published to the UI when it changes.
    glyphs: Rc<GlyphTracker>,
    /// Controllers confirm with their back button and back with their
    /// confirm button (Settings → Controller). The keyboard is unchanged.
    swap_confirm: Rc<Cell<bool>>,
    bindings: Cell<crate::domain::controller::GameplayBindings>,
    ds_bindings: Cell<crate::domain::controller::DsBindings>,
    /// Shortcuts: their buttons are held back from the game (see
    /// `Shortcuts::withheld`).
    shortcuts: Cell<crate::domain::controller::Shortcuts>,
}

/// Follows the keyboard or controller used last and tells the UI when its
/// glyph style changes. A mouse or touch screen leaves it as it is.
pub struct GlyphTracker {
    style: Cell<GlyphStyle>,
    sink: Rc<dyn Fn(GlyphStyle)>,
}

impl GlyphTracker {
    fn new(sink: Rc<dyn Fn(GlyphStyle)>) -> Self {
        Self {
            style: Cell::new(GlyphStyle::Keyboard),
            sink,
        }
    }

    fn set(&self, style: GlyphStyle) {
        if self.style.replace(style) != style {
            (self.sink)(style);
        }
    }

    /// Publish the current style, whether or not it changed.
    fn publish(&self, style: GlyphStyle) {
        self.style.set(style);
        (self.sink)(style);
    }
}

impl InputManager {
    pub fn new(
        action_sink: Rc<dyn Fn(UiActionEvent)>,
        status_sink: Rc<dyn Fn(ControllerStatus)>,
        last_device: Rc<Cell<InputDevice>>,
        glyph_sink: Rc<dyn Fn(GlyphStyle)>,
    ) -> Rc<Self> {
        let glyphs = Rc::new(GlyphTracker::new(glyph_sink));
        let swap_confirm = Rc::new(Cell::new(false));
        // Controller actions mark the controller as the device in use, and
        // show its family's glyphs.
        let gamepad_sink: GamepadActionSink = {
            let (sink, device) = (Rc::clone(&action_sink), Rc::clone(&last_device));
            let glyphs = Rc::clone(&glyphs);
            let swap = Rc::clone(&swap_confirm);
            Rc::new(move |event: UiActionEvent, family: ControllerFamily| {
                device.set(InputDevice::Gamepad);
                glyphs.set(GlyphStyle::Controller(family));
                sink(controller_event(event, swap.get()));
            })
        };
        let gamepad_input = match SdlGamepadInput::start(gamepad_sink, Rc::clone(&status_sink)) {
            Ok(input) => Some(input),
            Err(error) => {
                warn!(%error, "SDL3 gamepad input unavailable; keyboard input remains active");
                status_sink(ControllerStatus::default());
                None
            }
        };

        // A connected controller's glyphs show from the start.
        glyphs.publish(
            gamepad_input
                .as_ref()
                .and_then(SdlGamepadInput::first_family)
                .map_or(GlyphStyle::Keyboard, GlyphStyle::Controller),
        );

        Rc::new(Self {
            action_sink,
            gamepad_input,
            ui_input_enabled: Cell::new(true),
            keyboard_repeat: Cell::new(None),
            last_device,
            glyphs,
            swap_confirm,
            bindings: Cell::default(),
            ds_bindings: Cell::default(),
            shortcuts: Cell::default(),
        })
    }

    /// Swap controller A/B and X/Y in menus and gameplay (not the keyboard).
    pub fn set_swap_confirm(&self, swap: bool) {
        self.swap_confirm.set(swap);
    }

    pub fn set_gameplay_bindings(&self, bindings: crate::domain::controller::GameplayBindings) {
        self.bindings.set(bindings);
    }
    pub fn set_ds_bindings(&self, bindings: crate::domain::controller::DsBindings) {
        self.ds_bindings.set(bindings);
    }
    pub fn mapping_snapshot(&self) -> Option<(u16, u64)> {
        self.gamepad_input.as_ref()?.gameplay_snapshot()
    }

    pub fn set_shortcuts(&self, shortcuts: crate::domain::controller::Shortcuts) {
        self.shortcuts.set(shortcuts);
    }
    /// Shortcut buttons held right now (raw controller buttons).
    pub fn shortcut_buttons(&self) -> u16 {
        self.raw_buttons() & self.shortcuts.get().mask()
    }
    fn raw_buttons(&self) -> u16 {
        self.gamepad_input
            .as_ref()
            .map_or(0, SdlGamepadInput::gameplay_buttons)
    }
    /// The controller's buttons for the game: shortcuts' held back.
    fn game_buttons(&self) -> u16 {
        let raw = self.raw_buttons();
        raw & !self.shortcuts.get().withheld(raw)
    }
    pub fn ds_buttons(&self) -> u16 {
        let buttons = self.game_buttons();
        // DS has four face buttons, so it has its own mapping; the global
        // confirm/back swap still applies.
        gameplay_mapping(
            self.ds_bindings.get().apply(buttons),
            self.swap_confirm.get(),
        )
    }

    pub fn three_ds_input(&self) -> (u16, [i16; 4]) {
        let (raw, axes) = self
            .gamepad_input
            .as_ref()
            .and_then(SdlGamepadInput::three_ds_snapshot)
            .unwrap_or_default();
        (
            gameplay_mapping(
                raw & !self.shortcuts.get().withheld(raw),
                self.swap_confirm.get(),
            ),
            axes,
        )
    }

    pub fn gameplay_buttons(&self) -> u16 {
        gameplay_mapping(
            self.bindings.get().apply(self.game_buttons()),
            self.swap_confirm.get(),
        )
    }

    pub fn mark_keyboard_used(&self) {
        self.last_device.set(InputDevice::Keyboard);
        self.glyphs.set(GlyphStyle::Keyboard);
    }

    /// Enable or suspend semantic UI input according to Horizon window ownership.
    ///
    /// SDL continues pumping device/hotplug events while suspended, but it must
    /// not emit navigation actions into a background Horizon window.
    pub fn set_ui_input_enabled(&self, enabled: bool) {
        if self.ui_input_enabled.replace(enabled) == enabled {
            return;
        }

        // Never preserve a held direction across deactivation/re-activation.
        self.keyboard_repeat.set(None);
        if let Some(gamepad_input) = &self.gamepad_input {
            gamepad_input.set_enabled(enabled);
        }
    }

    /// Handle a logical keyboard key forwarded by the Slint/Winit window.
    /// Returns true when the key belongs to Horizon navigation.
    pub fn handle_keyboard(&self, text: &str, repeated: bool) -> bool {
        self.handle_keyboard_at(text, repeated, Instant::now())
    }

    fn handle_keyboard_at(&self, text: &str, repeated: bool, now: Instant) -> bool {
        if !self.ui_input_enabled.get() {
            return false;
        }
        let Some(action) = keyboard::action_for_key(text, repeated) else {
            return false;
        };
        if action.repeatable() {
            if !repeated {
                self.keyboard_repeat.set(Some(KeyboardRepeat {
                    action,
                    pressed_at: now,
                    last_emitted_at: now,
                }));
            } else {
                let Some(mut state) = self
                    .keyboard_repeat
                    .get()
                    .filter(|state| state.action == action)
                else {
                    // Ignore orphaned OS repeat events after a lost key-up.
                    return true;
                };
                if now.duration_since(state.pressed_at) < DIGITAL_INITIAL_REPEAT_DELAY
                    || now.duration_since(state.last_emitted_at) < DIGITAL_REPEAT_INTERVAL
                {
                    return true; // Handled, but no additional navigation step.
                }
                state.last_emitted_at = now;
                self.keyboard_repeat.set(Some(state));
            }
        }
        self.mark_keyboard_used();
        (self.action_sink)(UiActionEvent { action, repeated });
        true
    }

    /// Slint/Winit key-up releases our repeat latch; repeating a key after a
    /// release starts a fresh controller-paced cycle, just as a D-pad does.
    pub fn handle_keyboard_released(&self, text: &str) -> bool {
        let Some(action) = keyboard::action_for_key(text, false) else {
            return false;
        };
        if action.repeatable()
            && self
                .keyboard_repeat
                .get()
                .is_some_and(|state| state.action == action)
        {
            self.keyboard_repeat.set(None);
        }
        true
    }
}

/// Libretro A/B and X/Y follow the same controller face-button preference.
fn gameplay_mapping(buttons: u16, swapped: bool) -> u16 {
    if !swapped {
        return buttons;
    }
    crate::domain::controller::swap_face_buttons(buttons)
}

/// A controller's action as Horizon acts on it: confirm and back exchanged
/// when the user swapped them.
const fn controller_event(event: UiActionEvent, swap_confirm: bool) -> UiActionEvent {
    if swap_confirm {
        UiActionEvent {
            action: event.action.with_confirm_swapped(),
            ..event
        }
    } else {
        event
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use slint::{SharedString, platform::Key};

    use super::*;

    #[test]
    fn held_keyboard_arrows_use_controller_repeat_cap() {
        use std::time::{Duration, Instant};
        let events = Rc::new(RefCell::new(Vec::new()));
        let observed = Rc::clone(&events);
        let manager = InputManager {
            action_sink: Rc::new(move |event| observed.borrow_mut().push(event)),
            gamepad_input: None,
            ui_input_enabled: Cell::new(true),
            keyboard_repeat: Cell::new(None),
            last_device: Rc::default(),
            glyphs: Rc::new(GlyphTracker::new(Rc::new(|_| {}))),
            swap_confirm: Rc::default(),
            bindings: Cell::default(),
            ds_bindings: Cell::default(),
            shortcuts: Cell::default(),
        };
        let right: SharedString = Key::RightArrow.into();
        let t = Instant::now();
        assert!(manager.handle_keyboard_at(right.as_str(), false, t));
        assert!(manager.handle_keyboard_at(right.as_str(), true, t + Duration::from_millis(100)));
        assert!(manager.handle_keyboard_at(right.as_str(), true, t + Duration::from_millis(299)));
        assert_eq!(events.borrow().len(), 1);
        assert!(manager.handle_keyboard_at(right.as_str(), true, t + Duration::from_millis(300)));
        assert!(manager.handle_keyboard_at(right.as_str(), true, t + Duration::from_millis(330)));
        assert_eq!(events.borrow().len(), 2);
        assert!(manager.handle_keyboard_at(right.as_str(), true, t + Duration::from_millis(415)));
        assert_eq!(events.borrow().len(), 3);
        assert!(manager.handle_keyboard_released(right.as_str()));
        assert!(manager.handle_keyboard_at(right.as_str(), true, t + Duration::from_millis(600)));
        assert_eq!(events.borrow().len(), 3);
        assert!(manager.handle_keyboard_at(right.as_str(), false, t + Duration::from_millis(650)));
        assert_eq!(events.borrow().len(), 4);
        manager.set_ui_input_enabled(false);
        manager.set_ui_input_enabled(true);
        assert!(manager.handle_keyboard_at(right.as_str(), true, t + Duration::from_millis(800)));
        assert_eq!(events.borrow().len(), 4);
    }

    #[test]
    fn inactive_window_blocks_semantic_keyboard_actions_and_reactivation_restores_them() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink_events = Rc::clone(&events);
        let action_sink: Rc<dyn Fn(UiActionEvent)> = Rc::new(move |event| {
            sink_events.borrow_mut().push(event);
        });
        let manager = InputManager {
            action_sink,
            gamepad_input: None,
            ui_input_enabled: Cell::new(true),
            keyboard_repeat: Cell::new(None),
            last_device: Rc::default(),
            glyphs: Rc::new(GlyphTracker::new(Rc::new(|_| {}))),
            swap_confirm: Rc::default(),
            bindings: Cell::default(),
            ds_bindings: Cell::default(),
            shortcuts: Cell::default(),
        };
        let right: SharedString = Key::RightArrow.into();

        assert!(manager.handle_keyboard(right.as_str(), false));
        assert_eq!(events.borrow().len(), 1);

        manager.set_ui_input_enabled(false);
        assert!(!manager.handle_keyboard(right.as_str(), false));
        assert_eq!(events.borrow().len(), 1);

        manager.set_ui_input_enabled(true);
        assert!(manager.handle_keyboard(right.as_str(), false));
        assert_eq!(events.borrow().len(), 2);
    }

    #[test]
    fn glyphs_follow_the_last_keyboard_or_controller() {
        let published = Rc::new(RefCell::new(Vec::new()));
        let observed = Rc::clone(&published);
        let manager = InputManager {
            action_sink: Rc::new(|_| {}),
            gamepad_input: None,
            ui_input_enabled: Cell::new(true),
            keyboard_repeat: Cell::new(None),
            last_device: Rc::default(),
            glyphs: Rc::new(GlyphTracker::new(Rc::new(move |style| {
                observed.borrow_mut().push(style)
            }))),
            swap_confirm: Rc::default(),
            bindings: Cell::default(),
            ds_bindings: Cell::default(),
            shortcuts: Cell::default(),
        };
        let pad = GlyphStyle::Controller(ControllerFamily::PlayStation);
        manager.glyphs.set(pad);
        manager.glyphs.set(pad);
        let right: SharedString = Key::RightArrow.into();
        assert!(manager.handle_keyboard(right.as_str(), false));
        assert!(manager.handle_keyboard(right.as_str(), false));
        assert_eq!(*published.borrow(), [pad, GlyphStyle::Keyboard]);
    }

    #[test]
    fn swapping_exchanges_controller_face_pairs_and_preserves_directions() {
        let accept = UiActionEvent::fresh(UiAction::Accept);
        let back = UiActionEvent::fresh(UiAction::Back);
        let up = UiActionEvent::repeated(UiAction::Up);
        assert_eq!(controller_event(accept, false), accept);
        assert_eq!(controller_event(accept, true), back);
        assert_eq!(controller_event(back, true), accept);
        assert_eq!(controller_event(up, true), up);
        assert_eq!(
            controller_event(UiActionEvent::fresh(UiAction::AlternateSecondary), true).action,
            UiAction::Secondary
        );
        assert_eq!(
            controller_event(UiActionEvent::fresh(UiAction::Secondary), true).action,
            UiAction::AlternateSecondary
        );
        assert_eq!(
            UiAction::Secondary.with_confirm_swapped(),
            UiAction::AlternateSecondary
        );

        // Enter still confirms on the keyboard.
        let events = Rc::new(RefCell::new(Vec::new()));
        let observed = Rc::clone(&events);
        let manager = InputManager {
            action_sink: Rc::new(move |event| observed.borrow_mut().push(event)),
            gamepad_input: None,
            ui_input_enabled: Cell::new(true),
            keyboard_repeat: Cell::new(None),
            last_device: Rc::default(),
            glyphs: Rc::new(GlyphTracker::new(Rc::new(|_| {}))),
            swap_confirm: Rc::default(),
            bindings: Cell::default(),
            ds_bindings: Cell::default(),
            shortcuts: Cell::default(),
        };
        manager.set_swap_confirm(true);
        let enter: SharedString = Key::Return.into();
        assert!(manager.handle_keyboard(enter.as_str(), false));
        assert_eq!(events.borrow()[0].action, UiAction::Accept);
    }
}

#[cfg(test)]
mod gameplay_tests {
    use super::gameplay_mapping;
    #[test]
    fn swap_exchanges_both_face_pairs_and_preserves_other_controls() {
        for buttons in 0..=u16::MAX {
            assert_eq!(gameplay_mapping(buttons, false), buttons);
            assert_eq!(
                gameplay_mapping(gameplay_mapping(buttons, true), true),
                buttons
            );
            assert_eq!(gameplay_mapping(buttons, true) & !771, buttons & !771);
        }
        assert_eq!(gameplay_mapping(256, true), 1);
        assert_eq!(gameplay_mapping(1, true), 256);
        assert_eq!(gameplay_mapping(257, true), 257);
        assert_eq!(gameplay_mapping(512, true), 2);
        assert_eq!(gameplay_mapping(2, true), 512);
    }
}

#[cfg(test)]
mod virtual_mapping_tests {
    use super::*;
    use sdl3::{
        gamepad::{Axis, Button},
        joystick::{JoystickType, VirtualJoystickDescription},
    };
    use slint::ComponentHandle;
    use std::cell::RefCell;
    #[test]
    #[ignore = "requires Wayland/SDL virtual gamepad"]
    fn virtual_gamepad_remapping_triggers_and_reconnection() {
        let ui = crate::AppWindow::new().unwrap();
        ui.show().unwrap();
        let sdl = sdl3::init().unwrap();
        let joysticks = sdl.joystick().unwrap();
        let virtual_system = joysticks.clone();
        let attach = move || {
            virtual_system
                .attach_virtual_joystick(
                    VirtualJoystickDescription::new()
                        .name("Horizon mapping test")
                        .joystick_type(JoystickType::Gamepad)
                        .with_buttons([
                            Button::South,
                            Button::East,
                            Button::West,
                            Button::North,
                            Button::Back,
                            Button::Guide,
                            Button::LeftStick,
                            Button::RightStick,
                            Button::Start,
                            Button::LeftShoulder,
                            Button::RightShoulder,
                            Button::DPadUp,
                            Button::DPadDown,
                            Button::DPadLeft,
                            Button::DPadRight,
                        ])
                        .with_axes([
                            Axis::LeftX,
                            Axis::LeftY,
                            Axis::RightX,
                            Axis::RightY,
                            Axis::TriggerLeft,
                            Axis::TriggerRight,
                        ]),
                )
                .unwrap()
        };
        let connection = RefCell::new(Some(attach()));
        let first_id = connection.borrow().as_ref().unwrap().id();
        let mut joystick = joysticks.open(first_id).unwrap();
        let actions = Rc::new(RefCell::new(Vec::new()));
        let seen = actions.clone();
        let input = InputManager::new(
            Rc::new(move |event| seen.borrow_mut().push(event.action)),
            Rc::new(|_| {}),
            Rc::new(Cell::new(InputDevice::Keyboard)),
            Rc::new(|_| {}),
        );
        joystick
            .set_virtual_button(Button::South as u32, true)
            .unwrap();
        let timer = slint::Timer::default();
        let stage = Rc::new(Cell::new(0));
        let progress = stage.clone();
        let ticks = Cell::new(0);
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(150),
            move || {
                ticks.set(ticks.get() + 1);
                assert!(
                    ticks.get() < 40,
                    "virtual pad timed out at {}",
                    progress.get()
                );
                match progress.get() {
                    0 => {
                        if input
                            .mapping_snapshot()
                            .is_none_or(|(_, id)| id != u64::from(first_id.raw()))
                        {
                            return;
                        }
                        assert_ne!(input.gameplay_buttons() & (1 << 8), 0);
                        joystick
                            .set_virtual_button(Button::South as u32, false)
                            .unwrap();
                        input.set_gameplay_bindings(
                            crate::domain::controller::GameplayBindings::default()
                                .assign(0, 9, false),
                        );
                        joystick
                            .set_virtual_button(Button::West as u32, true)
                            .unwrap();
                        progress.set(1);
                    }
                    1 => {
                        assert_eq!(input.gameplay_buttons(), 1 << 8);
                        assert!(actions.borrow().contains(&UiAction::Secondary));
                        input.set_swap_confirm(true);
                        progress.set(2);
                    }
                    2 => {
                        assert_eq!(input.gameplay_buttons(), 1);
                        joystick
                            .set_virtual_button(Button::West as u32, false)
                            .unwrap();
                        input.set_gameplay_bindings(
                            crate::domain::controller::GameplayBindings::default()
                                .assign(0, 9, false)
                                .assign(2, 12, false),
                        );
                        joystick
                            .set_virtual_axis(Axis::TriggerLeft as u32, 32767)
                            .unwrap();
                        joystick
                            .set_virtual_button(Button::Start as u32, true)
                            .unwrap();
                        progress.set(3);
                    }
                    3 => {
                        assert_eq!(
                            input.gameplay_buttons(),
                            (1 << 10) | (1 << 3),
                            "snapshot={:?}, axis={:?}, button={:?}, actions={:?}",
                            input.mapping_snapshot(),
                            joystick.axis(Axis::TriggerLeft as u32),
                            joystick.button(Button::Start as u32),
                            actions.borrow()
                        );
                        input.set_ui_input_enabled(false);
                        assert_eq!(input.gameplay_buttons(), 0);
                        assert!(input.mapping_snapshot().is_none());
                        input.set_ui_input_enabled(true);
                        joystick
                            .set_virtual_axis(Axis::TriggerLeft as u32, 0)
                            .unwrap();
                        joystick
                            .set_virtual_button(Button::Start as u32, false)
                            .unwrap();
                        connection.borrow_mut().take();
                        progress.set(4);
                    }
                    4 => {
                        if input.mapping_snapshot().is_some() {
                            return;
                        }
                        assert_eq!(input.gameplay_buttons(), 0);
                        let next = attach();
                        assert_ne!(next.id(), first_id);
                        joystick = joysticks.open(next.id()).unwrap();
                        *connection.borrow_mut() = Some(next);
                        joystick
                            .set_virtual_button(Button::West as u32, true)
                            .unwrap();
                        progress.set(5);
                    }
                    5 => {
                        if input
                            .mapping_snapshot()
                            .is_none_or(|(_, id)| id == u64::from(first_id.raw()))
                        {
                            return;
                        }
                        assert_eq!(input.gameplay_buttons(), 1);
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
