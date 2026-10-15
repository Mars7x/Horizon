//! First-run focus and step policy, independent of Slint and persistence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum SetupStep {
    #[default]
    Welcome = 0,
    Profile = 1,
    Library = 2,
    Ready = 3,
}

impl SetupStep {
    pub fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Welcome),
            1 => Some(Self::Profile),
            2 => Some(Self::Library),
            3 => Some(Self::Ready),
            _ => None,
        }
    }
    pub fn back(self) -> Self {
        match self {
            Self::Welcome | Self::Profile => Self::Welcome,
            Self::Library => Self::Profile,
            Self::Ready => Self::Library,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// Profile: 0 name, 1..4 placeholder avatars, 5 Back, 6 Continue.
/// Library: 0 add games, 1 later, 2 Back. Ready: 0 finish, 1 Back.
pub fn move_focus(step: SetupStep, focus: i32, avatar: u8, direction: Direction) -> i32 {
    use Direction::*;
    match step {
        SetupStep::Welcome => 0,
        SetupStep::Profile => match (focus, direction) {
            (0, Down) => i32::from(avatar.min(3)) + 1,
            (1..=4, Up) => 0,
            (1..=4, Down) => 6,
            (1..=4, Left) => (focus - 1).max(1),
            (1..=4, Right) => (focus + 1).min(4),
            (5 | 6, Up) => i32::from(avatar.min(3)) + 1,
            (5 | 6, Left) => 5,
            (5 | 6, Right) => 6,
            _ => focus,
        },
        SetupStep::Library => match direction {
            Left => (focus + 1).min(2),
            Right => (focus - 1).max(0),
            Up | Down => focus,
        },
        SetupStep::Ready => match direction {
            Left => 1,
            Right => 0,
            Up | Down => focus,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bottom_action_rows_follow_visual_order_and_stop_at_edges() {
        use Direction::*;
        // Back is leftmost; the primary action is rightmost and starts focused.
        for (step, visual_order) in [
            (SetupStep::Library, vec![2, 1, 0]),
            (SetupStep::Ready, vec![1, 0]),
        ] {
            for (position, &focus) in visual_order.iter().enumerate() {
                assert_eq!(
                    move_focus(step, focus, 0, Left),
                    visual_order[position.saturating_sub(1)]
                );
                assert_eq!(
                    move_focus(step, focus, 0, Right),
                    visual_order[(position + 1).min(visual_order.len() - 1)]
                );
                assert_eq!(move_focus(step, focus, 0, Up), focus);
                assert_eq!(move_focus(step, focus, 0, Down), focus);
            }
        }
    }
    #[test]
    fn profile_reaches_every_control_and_restores_selected_avatar_row() {
        use Direction::*;
        assert_eq!(move_focus(SetupStep::Profile, 0, 2, Down), 3);
        assert_eq!(move_focus(SetupStep::Profile, 3, 2, Left), 2);
        assert_eq!(move_focus(SetupStep::Profile, 1, 2, Left), 1);
        assert_eq!(move_focus(SetupStep::Profile, 4, 2, Right), 4);
        assert_eq!(move_focus(SetupStep::Profile, 2, 2, Down), 6);
        assert_eq!(move_focus(SetupStep::Profile, 6, 2, Left), 5);
        assert_eq!(move_focus(SetupStep::Profile, 5, 2, Up), 3);
        assert_eq!(move_focus(SetupStep::Profile, 3, 2, Up), 0);
    }
    #[test]
    fn welcome_cannot_be_backed_out_and_unknown_steps_are_rejected() {
        assert_eq!(SetupStep::Welcome.back(), SetupStep::Welcome);
        assert_eq!(SetupStep::Ready.back(), SetupStep::Library);
        assert!(SetupStep::from_index(4).is_none());
        for direction in [
            Direction::Up,
            Direction::Down,
            Direction::Left,
            Direction::Right,
        ] {
            assert_eq!(move_focus(SetupStep::Welcome, 0, 0, direction), 0);
        }
    }
}
