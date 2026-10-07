use std::{cell::Cell, rc::Rc};

use tracing::warn;

use super::{
    actions::{UiAction, UiActionEvent},
    keyboard,
    sdl::{ControllerStatus, SdlGamepadInput},
};

/// Coordinates Horizon input adapters and publishes only semantic actions.
///
/// The manager deliberately has no knowledge of screens, game selection, or
/// other presentation state. Those concerns live above the input layer.
pub struct InputManager {
    action_sink: Rc<dyn Fn(UiActionEvent)>,
    gamepad_input: Option<SdlGamepadInput>,
    ui_input_enabled: Cell<bool>,
}

impl InputManager {
    pub fn new(
        action_sink: Rc<dyn Fn(UiActionEvent)>,
        status_sink: Rc<dyn Fn(ControllerStatus)>,
    ) -> Rc<Self> {
        let gamepad_input = match SdlGamepadInput::start(
            Rc::clone(&action_sink),
            Rc::clone(&status_sink),
        ) {
            Ok(input) => Some(input),
            Err(error) => {
                warn!(%error, "SDL3 gamepad input unavailable; keyboard input remains active");
                status_sink(ControllerStatus::default());
                None
            }
        };

        Rc::new(Self {
            action_sink,
            gamepad_input,
            ui_input_enabled: Cell::new(true),
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

        if let Some(gamepad_input) = &self.gamepad_input {
            gamepad_input.set_enabled(enabled);
        }
    }

    /// Handle a logical keyboard key forwarded by the Slint/Winit window.
    /// Returns true when the key belongs to Horizon navigation.
    pub fn handle_keyboard(&self, text: &str, repeated: bool) -> bool {
        if !self.ui_input_enabled.get() {
            return false;
        }

        let Some(action) = keyboard::action_for_key(text, repeated) else {
            return false;
        };

        (self.action_sink)(UiActionEvent { action, repeated });
        true
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use slint::{SharedString, platform::Key};

    use super::*;

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
}
