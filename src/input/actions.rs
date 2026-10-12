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
    /// Pressing either stick (L3 or R3). The on-screen keyboard uses it as
    /// Shift.
    StickPress,
    /// A mouse's forward button: reopen the page Back last left.
    Forward,
}

/// What the user last used, so the UI can offer what suits it: the
/// on-screen keyboard appears for a controller or a touch screen, not for a
/// physical keyboard or a mouse.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum InputDevice {
    #[default]
    Keyboard,
    Gamepad,
    /// A mouse or touchpad. Under gamescope a touch screen can arrive as
    /// this too, so nothing may depend on telling the two apart.
    Pointer,
    Touch,
}

/// A controller's button family, which decides its glyphs (and, for
/// Nintendo's, which face button confirms).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ControllerFamily {
    /// Xbox, and any controller not recognised as another family.
    #[default]
    Xbox,
    PlayStation,
    Nintendo,
    SteamDeck,
    /// The AYN Odin 3's own controls: Nintendo-style labels (A on the
    /// right) with L1/R1 shoulders.
    Odin,
}

/// The button glyphs hints show: those of the keyboard or controller used
/// last. A mouse or touch screen keeps whichever was showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlyphStyle {
    #[default]
    Keyboard,
    Controller(ControllerFamily),
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
    /// The action with confirm and back exchanged, for controllers when the
    /// "Swap A and B" setting is on.
    pub const fn with_confirm_swapped(self) -> Self {
        match self {
            Self::Accept => Self::Back,
            Self::Back => Self::Accept,
            other => other,
        }
    }

    /// Actions interpreted by the application shell before page-local input.
    ///
    /// Accept remains contextual: the active focus region/page decides what it
    /// activates. Back, Forward, Menu, and Home are always shell-level actions.
    pub const fn is_global(self) -> bool {
        matches!(self, Self::Back | Self::Forward | Self::Menu | Self::Home)
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
            "L3" | "R3" => Self::StickPress,
            "+" => Self::Menu,
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
        assert!(UiAction::Forward.is_global());
        assert!(!UiAction::Accept.is_global());
        assert!(!UiAction::Left.is_global());
    }
}
