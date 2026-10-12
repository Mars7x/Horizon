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
        // Controller actions mark the controller as the device in use, and
        // show its family's glyphs.
        let gamepad_sink: GamepadActionSink = {
            let (sink, device) = (Rc::clone(&action_sink), Rc::clone(&last_device));
            let glyphs = Rc::clone(&glyphs);
            Rc::new(move |event, family: ControllerFamily| {
                device.set(InputDevice::Gamepad);
                glyphs.set(GlyphStyle::Controller(family));
                sink(event);
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
        })
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
        self.last_device.set(InputDevice::Keyboard);
        self.glyphs.set(GlyphStyle::Keyboard);
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
        };
        let pad = GlyphStyle::Controller(ControllerFamily::PlayStation);
        manager.glyphs.set(pad);
        manager.glyphs.set(pad);
        let right: SharedString = Key::RightArrow.into();
        assert!(manager.handle_keyboard(right.as_str(), false));
        assert!(manager.handle_keyboard(right.as_str(), false));
        assert_eq!(*published.borrow(), [pad, GlyphStyle::Keyboard]);
    }
}
