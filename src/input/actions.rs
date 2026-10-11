/// Device-independent actions understood by Horizon's navigation layer.
///
/// Raw SDL buttons and keyboard key codes must be converted into one of these
/// values before they reach presentation/application state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    Up,
    Down,
    Left,
    Right,
    Accept,
    Back,
    Menu,
    Home,
    LeftBumper,
    RightBumper,
    /// The X (west) face button, or Delete on a keyboard. Pages give it a
    /// meaning; the Album uses it to delete.
    Secondary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiActionEvent {
    pub action: UiAction,
    pub repeated: bool,
}

impl UiActionEvent {
    pub const fn fresh(action: UiAction) -> Self {
        Self {
            action,
            repeated: false,
        }
    }

    pub const fn repeated(action: UiAction) -> Self {
        Self {
            action,
            repeated: true,
        }
    }
}

impl UiAction {
    /// Actions interpreted by the application shell before page-local input.
    ///
    /// Accept remains contextual: the active focus region/page decides what it
    /// activates. Back, Menu, and Home are always shell-level actions.
    pub const fn is_global(self) -> bool {
        matches!(self, Self::Back | Self::Menu | Self::Home)
    }

    /// The button a clicked on-screen hint stands for (`HintBar` glyph names).
    /// Clicking a hint is that controller press, nothing more.
    pub fn from_hint(button: &str) -> Option<Self> {
        Some(match button {
            "A" => Self::Accept,
            "B" => Self::Back,
            "LB" => Self::LeftBumper,
            "RB" => Self::RightBumper,
            "X" => Self::Secondary,
            _ => return None,
        })
    }

    /// Only directional navigation is repeated when a keyboard key is held.
    /// Actions that activate or leave a screen require a fresh press.
    pub const fn repeatable(self) -> bool {
        matches!(self, Self::Up | Self::Down | Self::Left | Self::Right)
    }
}

#[cfg(test)]
mod tests {
    use super::{UiAction, UiActionEvent};

    #[test]
    fn action_events_preserve_repeat_state() {
        assert!(!UiActionEvent::fresh(UiAction::Right).repeated);
        assert!(UiActionEvent::repeated(UiAction::Right).repeated);
    }

    #[test]
    fn only_directional_actions_repeat() {
        assert!(UiAction::Left.repeatable());
        assert!(UiAction::Right.repeatable());
        assert!(UiAction::Up.repeatable());
        assert!(UiAction::Down.repeatable());
        assert!(!UiAction::Accept.repeatable());
        assert!(!UiAction::Back.repeatable());
        assert!(!UiAction::Menu.repeatable());
        assert!(!UiAction::Home.repeatable());
    }

    #[test]
    fn hints_map_to_the_buttons_they_show() {
        assert_eq!(UiAction::from_hint("A"), Some(UiAction::Accept));
        assert_eq!(UiAction::from_hint("B"), Some(UiAction::Back));
        assert_eq!(UiAction::from_hint("LB"), Some(UiAction::LeftBumper));
        assert_eq!(UiAction::from_hint("RB"), Some(UiAction::RightBumper));
        assert_eq!(UiAction::from_hint("X"), Some(UiAction::Secondary));
        assert_eq!(UiAction::from_hint("Y"), None);
    }

    #[test]
    fn global_actions_are_explicit() {
        assert!(UiAction::Back.is_global());
        assert!(UiAction::Menu.is_global());
        assert!(UiAction::Home.is_global());
        assert!(!UiAction::Accept.is_global());
        assert!(!UiAction::Left.is_global());
    }
}
