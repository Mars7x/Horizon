//! Application-level navigation state.
//!
//! This module is intentionally UI-framework agnostic. Slint renders the active
//! route, but route history and Back/Home semantics are owned by Rust.

/// Top-level destinations in Horizon's Phase 4 application shell.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppRoute {
    #[default]
    Home,
    Library,
    Activity,
    Settings,
}

/// Pure Rust navigation state for Horizon's top-level shell.
///
/// `navigate_to` behaves like pushing a destination onto a back stack. The
/// global Home action is deliberately different: it resets the stack and makes
/// Home the root so Back cannot immediately return to the page that Home left.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Navigator {
    current: AppRoute,
    back_stack: Vec<AppRoute>,
}

impl Navigator {
    pub fn current(&self) -> AppRoute {
        self.current
    }

    pub fn back_stack_depth(&self) -> usize {
        self.back_stack.len()
    }

    /// Navigate to a new top-level route.
    ///
    /// Returns `true` only when the active route changes. Navigating to the
    /// already-active route is a no-op and does not grow history.
    pub fn navigate_to(&mut self, route: AppRoute) -> bool {
        if route == self.current {
            return false;
        }

        self.back_stack.push(self.current);
        self.current = route;
        true
    }

    /// Return to the previous top-level route when history exists.
    pub fn go_back(&mut self) -> bool {
        let Some(route) = self.back_stack.pop() else {
            return false;
        };

        self.current = route;
        true
    }

    /// Make Home the root route and discard prior top-level history.
    pub fn go_home(&mut self) -> bool {
        let changed = self.current != AppRoute::Home || !self.back_stack.is_empty();
        self.current = AppRoute::Home;
        self.back_stack.clear();
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::{AppRoute, Navigator};

    #[test]
    fn starts_on_home_with_empty_history() {
        let navigator = Navigator::default();
        assert_eq!(navigator.current(), AppRoute::Home);
        assert_eq!(navigator.back_stack_depth(), 0);
    }

    #[test]
    fn navigation_pushes_previous_route_and_back_restores_it() {
        let mut navigator = Navigator::default();

        assert!(navigator.navigate_to(AppRoute::Library));
        assert!(navigator.navigate_to(AppRoute::Activity));
        assert_eq!(navigator.current(), AppRoute::Activity);
        assert_eq!(navigator.back_stack_depth(), 2);

        assert!(navigator.go_back());
        assert_eq!(navigator.current(), AppRoute::Library);
        assert!(navigator.go_back());
        assert_eq!(navigator.current(), AppRoute::Home);
        assert!(!navigator.go_back());
    }

    #[test]
    fn navigating_to_current_route_is_a_no_op() {
        let mut navigator = Navigator::default();

        assert!(!navigator.navigate_to(AppRoute::Home));
        assert_eq!(navigator.back_stack_depth(), 0);

        assert!(navigator.navigate_to(AppRoute::Settings));
        assert!(!navigator.navigate_to(AppRoute::Settings));
        assert_eq!(navigator.back_stack_depth(), 1);
    }

    #[test]
    fn home_action_clears_history_instead_of_becoming_back_target() {
        let mut navigator = Navigator::default();

        navigator.navigate_to(AppRoute::Library);
        navigator.navigate_to(AppRoute::Settings);
        assert!(navigator.go_home());

        assert_eq!(navigator.current(), AppRoute::Home);
        assert_eq!(navigator.back_stack_depth(), 0);
        assert!(!navigator.go_back());
    }

    #[test]
    fn every_phase_four_route_participates_in_history() {
        let mut navigator = Navigator::default();
        for route in [
            AppRoute::Library,
            AppRoute::Activity,
            AppRoute::Settings,
            AppRoute::Home,
        ] {
            assert!(navigator.navigate_to(route));
        }

        assert_eq!(navigator.current(), AppRoute::Home);
        assert_eq!(navigator.back_stack_depth(), 4);
    }
}
