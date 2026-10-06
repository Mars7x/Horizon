use slint::{SharedString, platform::Key};

use super::actions::UiAction;

/// Convert a logical key delivered by Slint/Winit into a device-independent
/// Horizon action. Slint forwards key text only; it does not decide what a
/// key means to application navigation.
pub fn action_for_key(text: &str, repeated: bool) -> Option<UiAction> {
    let action = if key_matches(text, Key::UpArrow) {
        UiAction::Up
    } else if key_matches(text, Key::DownArrow) {
        UiAction::Down
    } else if key_matches(text, Key::LeftArrow) {
        UiAction::Left
    } else if key_matches(text, Key::RightArrow) {
        UiAction::Right
    } else if key_matches(text, Key::Return) || key_matches(text, Key::Space) {
        UiAction::Accept
    } else if key_matches(text, Key::Escape) || key_matches(text, Key::Back) {
        UiAction::Back
    } else if key_matches(text, Key::Menu) {
        UiAction::Menu
    } else if key_matches(text, Key::Home) {
        UiAction::Home
    } else {
        return None;
    };

    if repeated && !action.repeatable() {
        return None;
    }

    Some(action)
}

fn key_matches(text: &str, key: Key) -> bool {
    let encoded: SharedString = key.into();
    text == encoded.as_str()
}

#[cfg(test)]
mod tests {
    use slint::{SharedString, platform::Key};

    use super::action_for_key;
    use crate::input::actions::UiAction;

    fn encoded(key: Key) -> SharedString {
        key.into()
    }

    #[test]
    fn arrows_map_to_navigation() {
        assert_eq!(
            action_for_key(encoded(Key::LeftArrow).as_str(), false),
            Some(UiAction::Left)
        );
        assert_eq!(
            action_for_key(encoded(Key::RightArrow).as_str(), false),
            Some(UiAction::Right)
        );
    }

    #[test]
    fn held_accept_is_not_repeated() {
        assert_eq!(
            action_for_key(encoded(Key::Return).as_str(), true),
            None
        );
    }

    #[test]
    fn held_direction_is_repeated() {
        assert_eq!(
            action_for_key(encoded(Key::RightArrow).as_str(), true),
            Some(UiAction::Right)
        );
    }
}
