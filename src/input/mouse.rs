//! A mouse's side buttons, as the controller presses they stand for.

use slint::winit_030::winit::event::MouseButton;

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

#[cfg(test)]
mod tests {
    use super::{MouseButton, UiAction, action_for_button};

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
