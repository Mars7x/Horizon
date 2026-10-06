//! Application-level navigation state.
//!
//! This module is intentionally UI-framework agnostic. Slint renders the active
//! route and shell focus, but route history, utility destinations, and focus
//! movement policy are owned by Rust.

/// Top-level destinations in Horizon's Phase 4 application shell.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppRoute {
    #[default]
    Home,
    Library,
    Activity,
    Settings,
}

/// The two screen-level focus regions currently available in the shell.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ShellFocusRegion {
    #[default]
    Content,
    TopUtilities,
}

/// Stable ordering of the five utility icons in the persistent header.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TopUtility {
    Friends,
    Album,
    #[default]
    Activity,
    Web,
    Settings,
}

impl TopUtility {
    pub const COUNT: i32 = 5;

    pub const fn index(self) -> i32 {
        match self {
            Self::Friends => 0,
            Self::Album => 1,
            Self::Activity => 2,
            Self::Web => 3,
            Self::Settings => 4,
        }
    }

    pub const fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Friends),
            1 => Some(Self::Album),
            2 => Some(Self::Activity),
            3 => Some(Self::Web),
            4 => Some(Self::Settings),
            _ => None,
        }
    }

    pub const fn destination(self) -> UtilityDestination {
        match self {
            Self::Friends => UtilityDestination::Overlay(UtilityOverlay::Friends),
            Self::Album => UtilityDestination::Overlay(UtilityOverlay::Album),
            Self::Activity => UtilityDestination::Route(AppRoute::Activity),
            Self::Web => UtilityDestination::Overlay(UtilityOverlay::Web),
            Self::Settings => UtilityDestination::Route(AppRoute::Settings),
        }
    }

    /// Return the utility whose horizontal order most closely matches a game.
    ///
    /// This preserves spatial intent when focus moves vertically between the
    /// Home carousel and the five fixed utility positions. Endpoints pair with
    /// endpoints and intermediate positions are distributed proportionally.
    pub fn for_game_index(game_index: i32, game_count: usize) -> Self {
        let index = spatial_pair_index(game_index, game_count, Self::COUNT as usize);
        Self::from_index(index).unwrap_or_default()
    }

    /// Return the Home game whose horizontal order most closely matches this
    /// utility. This is the inverse-direction companion to `for_game_index`.
    pub fn paired_game_index(self, game_count: usize) -> i32 {
        spatial_pair_index(self.index(), Self::COUNT as usize, game_count)
    }
}

/// Transient utility surfaces are deliberately separate from top-level routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UtilityOverlay {
    Friends,
    Album,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UtilityDestination {
    Route(AppRoute),
    Overlay(UtilityOverlay),
}

/// Pure Rust focus state for the persistent shell.
///
/// The last selected utility is preserved when returning to page content so an
/// Up press later re-enters the same header item instead of jumping around.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ShellFocus {
    region: ShellFocusRegion,
    utility: TopUtility,
}

impl ShellFocus {
    pub const fn region(self) -> ShellFocusRegion {
        self.region
    }

    pub const fn utility(self) -> TopUtility {
        self.utility
    }

    pub fn enter_utilities(&mut self) -> bool {
        if self.region == ShellFocusRegion::TopUtilities {
            return false;
        }
        self.region = ShellFocusRegion::TopUtilities;
        true
    }

    pub fn leave_utilities(&mut self) -> bool {
        if self.region == ShellFocusRegion::Content {
            return false;
        }
        self.region = ShellFocusRegion::Content;
        true
    }

    pub fn select_utility(&mut self, utility: TopUtility) -> bool {
        let changed = self.utility != utility || self.region != ShellFocusRegion::TopUtilities;
        self.utility = utility;
        self.region = ShellFocusRegion::TopUtilities;
        changed
    }

    /// Move horizontally inside the utility strip. Header navigation clamps at
    /// the ends; unlike the game carousel, it never wraps.
    pub fn move_utility(&mut self, delta: i32) -> bool {
        if self.region != ShellFocusRegion::TopUtilities || delta == 0 {
            return false;
        }

        let next = self
            .utility
            .index()
            .saturating_add(delta)
            .clamp(0, TopUtility::COUNT - 1);
        let Some(next) = TopUtility::from_index(next) else {
            return false;
        };
        if next == self.utility {
            return false;
        }
        self.utility = next;
        true
    }
}


/// Map one ordered horizontal position into another ordered collection.
///
/// Integer rounding keeps this deterministic and framework-independent. With
/// five utilities and eight games the mapping is 0→0, 1→2, 2→4, 3→5, 4→7.
fn spatial_pair_index(source_index: i32, source_count: usize, target_count: usize) -> i32 {
    if target_count == 0 {
        return 0;
    }
    if source_count <= 1 || target_count == 1 {
        return 0;
    }

    let source_max = source_count.saturating_sub(1) as i64;
    let target_max = target_count.saturating_sub(1) as i64;
    let source = i64::from(source_index).clamp(0, source_max);
    let numerator = source * target_max;
    ((numerator + source_max / 2) / source_max) as i32
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
    use super::{
        AppRoute, Navigator, ShellFocus, ShellFocusRegion, TopUtility, UtilityDestination,
        UtilityOverlay,
    };

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

    #[test]
    fn shell_focus_enters_header_at_center_and_restores_last_utility() {
        let mut focus = ShellFocus::default();
        assert_eq!(focus.region(), ShellFocusRegion::Content);
        assert_eq!(focus.utility(), TopUtility::Activity);

        assert!(focus.enter_utilities());
        assert!(focus.move_utility(1));
        assert_eq!(focus.utility(), TopUtility::Web);
        assert!(focus.leave_utilities());
        assert!(focus.enter_utilities());
        assert_eq!(focus.utility(), TopUtility::Web);
    }

    #[test]
    fn utility_strip_clamps_instead_of_wrapping() {
        let mut focus = ShellFocus::default();
        focus.enter_utilities();

        assert!(focus.move_utility(-2));
        assert_eq!(focus.utility(), TopUtility::Friends);
        assert!(!focus.move_utility(-1));

        assert!(focus.move_utility(99));
        assert_eq!(focus.utility(), TopUtility::Settings);
        assert!(!focus.move_utility(1));
    }

    #[test]
    fn utility_destinations_keep_transient_tools_out_of_route_history() {
        assert_eq!(
            TopUtility::Friends.destination(),
            UtilityDestination::Overlay(UtilityOverlay::Friends)
        );
        assert_eq!(
            TopUtility::Album.destination(),
            UtilityDestination::Overlay(UtilityOverlay::Album)
        );
        assert_eq!(
            TopUtility::Activity.destination(),
            UtilityDestination::Route(AppRoute::Activity)
        );
        assert_eq!(
            TopUtility::Web.destination(),
            UtilityDestination::Overlay(UtilityOverlay::Web)
        );
        assert_eq!(
            TopUtility::Settings.destination(),
            UtilityDestination::Route(AppRoute::Settings)
        );
    }

    #[test]
    fn utilities_pair_spatially_with_home_games() {
        assert_eq!(TopUtility::Friends.paired_game_index(8), 0);
        assert_eq!(TopUtility::Album.paired_game_index(8), 2);
        assert_eq!(TopUtility::Activity.paired_game_index(8), 4);
        assert_eq!(TopUtility::Web.paired_game_index(8), 5);
        assert_eq!(TopUtility::Settings.paired_game_index(8), 7);
    }

    #[test]
    fn games_pair_spatially_with_nearest_utility_order() {
        assert_eq!(TopUtility::for_game_index(0, 8), TopUtility::Friends);
        assert_eq!(TopUtility::for_game_index(2, 8), TopUtility::Album);
        assert_eq!(TopUtility::for_game_index(4, 8), TopUtility::Activity);
        assert_eq!(TopUtility::for_game_index(5, 8), TopUtility::Web);
        assert_eq!(TopUtility::for_game_index(7, 8), TopUtility::Settings);
    }
}
