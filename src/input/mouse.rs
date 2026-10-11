//! A mouse's side buttons, as the controller presses they stand for, and
//! what a scroll event's device and gesture phase are.

use slint::winit_030::winit::event::{MouseButton, MouseScrollDelta, TouchPhase};

use super::UiAction;

/// The back button is the Back (B) press; the forward button reopens the
/// page Back last left. Other buttons stay with the pointer.
pub fn action_for_button(button: MouseButton) -> Option<UiAction> {
    match button {
        MouseButton::Back => Some(UiAction::Back),
        MouseButton::Forward => Some(UiAction::Forward),
        _ => None,
    }
}

/// What Slint's scroll events don't carry (`Wheel` in `wheel.slint`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollInput {
    /// 1 a wheel (whole lines), 2 a touchpad (pixels).
    pub source: i32,
    /// Fingers are on the touchpad: until the gesture ends.
    pub touching: bool,
}

pub fn scroll_input(delta: MouseScrollDelta, phase: TouchPhase) -> ScrollInput {
    match delta {
        MouseScrollDelta::LineDelta(..) => ScrollInput {
            source: 1,
            touching: false,
        },
        MouseScrollDelta::PixelDelta(_) => ScrollInput {
            source: 2,
            touching: !matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MouseButton, MouseScrollDelta, ScrollInput, TouchPhase, UiAction, action_for_button,
        scroll_input,
    };
    use slint::winit_030::winit::dpi::PhysicalPosition;

    #[test]
    fn scroll_input_tells_wheels_from_touchpads_and_lifted_fingers() {
        let pixels = MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -3.0));
        assert_eq!(
            scroll_input(MouseScrollDelta::LineDelta(0.0, -1.0), TouchPhase::Moved),
            ScrollInput {
                source: 1,
                touching: false
            }
        );
        assert_eq!(
            scroll_input(pixels, TouchPhase::Started),
            ScrollInput {
                source: 2,
                touching: true
            }
        );
        assert_eq!(
            scroll_input(pixels, TouchPhase::Moved),
            ScrollInput {
                source: 2,
                touching: true
            }
        );
        assert_eq!(
            scroll_input(pixels, TouchPhase::Ended),
            ScrollInput {
                source: 2,
                touching: false
            }
        );
    }

    #[test]
    fn side_buttons_are_back_and_forward_only() {
        assert_eq!(action_for_button(MouseButton::Back), Some(UiAction::Back));
        assert_eq!(
            action_for_button(MouseButton::Forward),
            Some(UiAction::Forward)
        );
        assert_eq!(action_for_button(MouseButton::Left), None);
        assert_eq!(action_for_button(MouseButton::Right), None);
        assert_eq!(action_for_button(MouseButton::Middle), None);
    }
}
