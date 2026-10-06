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
}
