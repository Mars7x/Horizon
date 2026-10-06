use std::rc::Rc;

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
    _gamepad_input: Option<SdlGamepadInput>,
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
            _gamepad_input: gamepad_input,
        })
    }

    /// Handle a logical keyboard key forwarded by the Slint/Winit window.
    /// Returns true when the key belongs to Horizon navigation.
    pub fn handle_keyboard(&self, text: &str, repeated: bool) -> bool {
        let Some(action) = keyboard::action_for_key(text, repeated) else {
            return false;
        };

        (self.action_sink)(UiActionEvent { action, repeated });
        true
    }
}
