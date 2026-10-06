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

impl AppRoute {
    const COUNT: usize = 4;

    const fn index(self) -> usize {
        match self {
            Self::Home => 0,
            Self::Library => 1,
            Self::Activity => 2,
            Self::Settings => 3,
        }
    }
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

/// Stable shell-focus state remembered independently for each top-level route.
///
/// Only durable route-local state is captured here. Temporary reciprocal
/// Home↔utility transfer anchors are intentionally excluded so they cannot leak
/// across route changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusSnapshot {
    region: ShellFocusRegion,
    utility: TopUtility,
}

impl FocusSnapshot {
    pub const fn for_route(route: AppRoute) -> Self {
        let utility = match route {
            AppRoute::Settings => TopUtility::Settings,
            AppRoute::Home | AppRoute::Library | AppRoute::Activity => TopUtility::Activity,
        };

        Self {
            region: ShellFocusRegion::Content,
            utility,
        }
    }

    pub const fn region(self) -> ShellFocusRegion {
        self.region
    }

    pub const fn utility(self) -> TopUtility {
        self.utility
    }

    pub const fn as_content(self) -> Self {
        Self {
            region: ShellFocusRegion::Content,
            utility: self.utility,
        }
    }
}

/// Last durable shell-focus state for every routed page.
///
/// Route history answers where Back/Home go. This memory independently answers
/// what owned focus the last time each route was active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteFocusMemory {
    snapshots: [FocusSnapshot; AppRoute::COUNT],
}

impl Default for RouteFocusMemory {
    fn default() -> Self {
        Self {
            snapshots: [
                FocusSnapshot::for_route(AppRoute::Home),
                FocusSnapshot::for_route(AppRoute::Library),
                FocusSnapshot::for_route(AppRoute::Activity),
                FocusSnapshot::for_route(AppRoute::Settings),
            ],
        }
    }
}

impl RouteFocusMemory {
    pub fn remember(&mut self, route: AppRoute, snapshot: FocusSnapshot) {
        self.snapshots[route.index()] = snapshot;
    }

    pub fn recall(&self, route: AppRoute) -> FocusSnapshot {
        self.snapshots[route.index()]
    }
}

/// Pure Rust focus state for the persistent shell.
///
/// Vertical Home↔header transfer keeps a temporary reciprocal anchor. If the
/// user reverses direction without moving horizontally in the destination region,
/// focus returns to the exact element it came from. Once horizontal selection
/// actually changes, the next vertical transfer falls back to live rendered
/// center-distance geometry.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ShellFocus {
    region: ShellFocusRegion,
    utility: TopUtility,
    content_anchor_index: Option<i32>,
    utility_moved_since_entry: bool,
    content_return_utility: Option<TopUtility>,
    content_return_game_index: Option<i32>,
}

impl ShellFocus {
    pub const fn region(self) -> ShellFocusRegion {
        self.region
    }

    pub const fn utility(self) -> TopUtility {
        self.utility
    }

    pub const fn snapshot(self) -> FocusSnapshot {
        FocusSnapshot {
            region: self.region,
            utility: self.utility,
        }
    }

    /// Restore durable route-local focus and clear all temporary transfer state.
    pub fn restore(&mut self, snapshot: FocusSnapshot) {
        self.region = snapshot.region();
        self.utility = snapshot.utility();
        self.content_anchor_index = None;
        self.utility_moved_since_entry = false;
        self.content_return_utility = None;
        self.content_return_game_index = None;
    }

    pub fn content_anchor_index(self) -> Option<i32> {
        if self.region == ShellFocusRegion::TopUtilities && !self.utility_moved_since_entry {
            self.content_anchor_index
        } else {
            None
        }
    }

    pub fn enter_utilities(&mut self) -> bool {
        if self.region == ShellFocusRegion::TopUtilities {
            return false;
        }
        self.region = ShellFocusRegion::TopUtilities;
        self.content_anchor_index = None;
        self.utility_moved_since_entry = false;
        self.content_return_utility = None;
        self.content_return_game_index = None;
        true
    }

    pub fn enter_utilities_from_content(
        &mut self,
        utility: TopUtility,
        content_anchor_index: i32,
    ) -> bool {
        let changed = self.region != ShellFocusRegion::TopUtilities || self.utility != utility;
        self.region = ShellFocusRegion::TopUtilities;
        self.utility = utility;
        self.content_anchor_index = Some(content_anchor_index);
        self.utility_moved_since_entry = false;
        self.content_return_utility = None;
        self.content_return_game_index = None;
        changed
    }

    /// Return the utility paired with the currently selected Home game when the
    /// previous vertical transfer came from the utility row. The anchor is
    /// invalidated as soon as a different game is selected.
    pub fn anchored_utility_for_content(&mut self, content_index: i32) -> Option<TopUtility> {
        if self.region != ShellFocusRegion::Content {
            return None;
        }

        if self.content_return_game_index == Some(content_index) {
            self.content_return_utility
        } else {
            self.content_return_utility = None;
            self.content_return_game_index = None;
            None
        }
    }

    /// Leave the utility row for a concrete Home game and remember that exact
    /// utility↔game pair. An immediate Up from the same game returns to the
    /// originating utility even if carousel motion changes rendered geometry.
    pub fn leave_utilities_to_content(&mut self, content_index: i32) -> bool {
        if self.region == ShellFocusRegion::Content {
            return false;
        }

        self.region = ShellFocusRegion::Content;
        self.content_anchor_index = None;
        self.utility_moved_since_entry = false;
        self.content_return_utility = Some(self.utility);
        self.content_return_game_index = Some(content_index);
        true
    }

    pub fn leave_utilities(&mut self) -> bool {
        if self.region == ShellFocusRegion::Content {
            return false;
        }
        self.region = ShellFocusRegion::Content;
        self.content_anchor_index = None;
        self.utility_moved_since_entry = false;
        self.content_return_utility = None;
        self.content_return_game_index = None;
        true
    }

    pub fn select_utility(&mut self, utility: TopUtility) -> bool {
        let changed = self.utility != utility || self.region != ShellFocusRegion::TopUtilities;
        self.utility = utility;
        self.region = ShellFocusRegion::TopUtilities;
        self.content_anchor_index = None;
        self.utility_moved_since_entry = false;
        self.content_return_utility = None;
        self.content_return_game_index = None;
        changed
    }

    /// Move horizontally inside the utility strip. Header navigation clamps at
    /// the ends; unlike the game carousel, it never wraps. Any actual utility
    /// move invalidates the exact return anchor from the originating game.
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
        self.utility_moved_since_entry = true;
        true
    }
}

/// Return the utility whose rendered center is horizontally closest to `x`.
///
/// All utility cells share the same vertical center, so minimizing horizontal
/// center distance is equivalent to choosing the shortest center-to-center
/// transfer path.
pub fn nearest_utility_for_x(
    x: f32,
    viewport_width: f32,
    utility_center_step: f32,
) -> TopUtility {
    if !x.is_finite()
        || !viewport_width.is_finite()
        || !utility_center_step.is_finite()
        || utility_center_step <= 0.0
    {
        return TopUtility::default();
    }

    let middle_index = (TopUtility::COUNT - 1) as f32 / 2.0;
    let raw_index = middle_index + (x - viewport_width / 2.0) / utility_center_step;
    let index = raw_index.round().clamp(0.0, (TopUtility::COUNT - 1) as f32) as i32;

    TopUtility::from_index(index).unwrap_or_default()
}

/// Return the rendered horizontal center of a utility icon.
pub fn utility_center_x(
    utility: TopUtility,
    viewport_width: f32,
    utility_center_step: f32,
) -> f32 {
    let middle_index = (TopUtility::COUNT - 1) as f32 / 2.0;
    viewport_width / 2.0 + (utility.index() as f32 - middle_index) * utility_center_step
}

/// Return the Home game whose rendered center is closest to `x`.
pub fn nearest_game_for_x(
    x: f32,
    first_game_center_x: f32,
    game_stride: f32,
    game_count: usize,
) -> i32 {
    if game_count == 0
        || !x.is_finite()
        || !first_game_center_x.is_finite()
        || !game_stride.is_finite()
        || game_stride <= 0.0
    {
        return 0;
    }

    let raw_index = (x - first_game_center_x) / game_stride;
    raw_index
        .round()
        .clamp(0.0, game_count.saturating_sub(1) as f32) as i32
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
        AppRoute, FocusSnapshot, Navigator, RouteFocusMemory, ShellFocus, ShellFocusRegion,
        TopUtility, UtilityDestination, UtilityOverlay, nearest_game_for_x, nearest_utility_for_x,
        utility_center_x,
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
    fn untouched_vertical_transfer_restores_exact_origin_game() {
        let mut focus = ShellFocus::default();

        assert!(focus.enter_utilities_from_content(TopUtility::Album, 3));
        assert_eq!(focus.content_anchor_index(), Some(3));

        assert!(focus.leave_utilities());
        assert_eq!(focus.content_anchor_index(), None);
    }

    #[test]
    fn moving_to_another_utility_invalidates_exact_origin_anchor() {
        let mut focus = ShellFocus::default();

        assert!(focus.enter_utilities_from_content(TopUtility::Album, 3));
        assert!(focus.move_utility(1));
        assert_eq!(focus.utility(), TopUtility::Activity);
        assert_eq!(focus.content_anchor_index(), None);
    }

    #[test]
    fn utility_to_game_round_trip_restores_exact_origin_utility() {
        let mut focus = ShellFocus::default();

        assert!(focus.enter_utilities_from_content(TopUtility::Album, 3));
        assert!(focus.move_utility(1));
        assert_eq!(focus.utility(), TopUtility::Activity);

        assert!(focus.leave_utilities_to_content(5));
        assert_eq!(
            focus.anchored_utility_for_content(5),
            Some(TopUtility::Activity)
        );

        assert!(focus.enter_utilities_from_content(TopUtility::Activity, 5));
        assert_eq!(focus.content_anchor_index(), Some(5));
    }

    #[test]
    fn changing_game_invalidates_utility_return_anchor() {
        let mut focus = ShellFocus::default();

        assert!(focus.enter_utilities_from_content(TopUtility::Web, 4));
        assert!(focus.leave_utilities_to_content(4));

        assert_eq!(focus.anchored_utility_for_content(5), None);
        assert_eq!(focus.anchored_utility_for_content(4), None);
    }

    #[test]
    fn route_focus_memory_has_route_appropriate_defaults() {
        let memory = RouteFocusMemory::default();

        assert_eq!(
            memory.recall(AppRoute::Home),
            FocusSnapshot::for_route(AppRoute::Home)
        );
        assert_eq!(
            memory.recall(AppRoute::Activity).utility(),
            TopUtility::Activity
        );
        assert_eq!(
            memory.recall(AppRoute::Settings).utility(),
            TopUtility::Settings
        );
        assert_eq!(
            memory.recall(AppRoute::Settings).region(),
            ShellFocusRegion::Content
        );
    }

    #[test]
    fn route_focus_memory_keeps_routes_independent() {
        let mut memory = RouteFocusMemory::default();
        let mut focus = ShellFocus::default();

        focus.enter_utilities_from_content(TopUtility::Web, 2);
        memory.remember(AppRoute::Home, focus.snapshot());

        focus.restore(FocusSnapshot::for_route(AppRoute::Settings));
        focus.enter_utilities();
        memory.remember(AppRoute::Settings, focus.snapshot());

        assert_eq!(memory.recall(AppRoute::Home).utility(), TopUtility::Web);
        assert_eq!(
            memory.recall(AppRoute::Home).region(),
            ShellFocusRegion::TopUtilities
        );
        assert_eq!(
            memory.recall(AppRoute::Settings).utility(),
            TopUtility::Settings
        );
    }

    #[test]
    fn restoring_route_focus_drops_temporary_transfer_anchors() {
        let mut focus = ShellFocus::default();
        focus.enter_utilities_from_content(TopUtility::Album, 3);
        let snapshot = focus.snapshot();

        focus.restore(snapshot);

        assert_eq!(focus.region(), ShellFocusRegion::TopUtilities);
        assert_eq!(focus.utility(), TopUtility::Album);
        assert_eq!(focus.content_anchor_index(), None);
    }

    #[test]
    fn nearest_utility_uses_rendered_center_distance() {
        let viewport_width = 1280.0;
        let step = 64.0;

        assert_eq!(nearest_utility_for_x(512.0, viewport_width, step), TopUtility::Friends);
        assert_eq!(nearest_utility_for_x(575.0, viewport_width, step), TopUtility::Album);
        assert_eq!(nearest_utility_for_x(640.0, viewport_width, step), TopUtility::Activity);
        assert_eq!(nearest_utility_for_x(704.0, viewport_width, step), TopUtility::Web);
        assert_eq!(nearest_utility_for_x(768.0, viewport_width, step), TopUtility::Settings);
    }

    #[test]
    fn utility_and_game_center_helpers_support_reverse_spatial_transfer() {
        let activity_x = utility_center_x(TopUtility::Activity, 1280.0, 64.0);
        assert_eq!(activity_x, 640.0);

        assert_eq!(nearest_game_for_x(activity_x, 439.0, 250.0, 8), 1);
        assert_eq!(nearest_game_for_x(768.0, 439.0, 250.0, 8), 1);
    }
}
