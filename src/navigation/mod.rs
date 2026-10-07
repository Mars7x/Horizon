//! Application-level navigation state.
//!
//! This module is intentionally UI-framework agnostic. Slint renders the active
//! route and shell focus, but route history, utility destinations, and focus
//! movement policy are owned by Rust.

/// Full-shell submenu pages opened from the six top utility icons.
///
/// These are real navigation destinations, not modal overlays. Grouping them
/// under one type keeps the shell model explicit without pretending every
/// utility page is an unrelated root-level section.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UtilityPage {
    Friends,
    Album,
    #[default]
    Activity,
    Web,
    Settings,
    Shop,
}

impl UtilityPage {
    const COUNT: usize = 6;

    const fn index(self) -> usize {
        match self {
            Self::Friends => 0,
            Self::Album => 1,
            Self::Activity => 2,
            Self::Web => 3,
            Self::Settings => 4,
            Self::Shop => 5,
        }
    }
}

/// Navigable destinations in Horizon's Phase 4 application shell.
///
/// Home and Library use the persistent shell chrome. The six utility icons
/// open durable full-shell submenu pages that participate in normal Back/Home
/// history.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppRoute {
    #[default]
    Home,
    Library,
    Utility(UtilityPage),
}

impl AppRoute {
    const COUNT: usize = 2 + UtilityPage::COUNT;

    const fn index(self) -> usize {
        match self {
            Self::Home => 0,
            Self::Library => 1,
            Self::Utility(page) => 2 + page.index(),
        }
    }

    /// Whether this route participates in the persistent Home/Library shell
    /// chrome and can therefore move focus into the top utility row.
    pub const fn uses_shell_chrome(self) -> bool {
        matches!(self, Self::Home | Self::Library)
    }

    pub const fn utility_page(self) -> Option<UtilityPage> {
        match self {
            Self::Utility(page) => Some(page),
            Self::Home | Self::Library => None,
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

/// Stable ordering of the six utility icons in the persistent header.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TopUtility {
    Friends,
    Album,
    #[default]
    Activity,
    Web,
    Settings,
    Shop,
}

impl TopUtility {
    pub const COUNT: i32 = 6;

    pub const fn index(self) -> i32 {
        match self {
            Self::Friends => 0,
            Self::Album => 1,
            Self::Activity => 2,
            Self::Web => 3,
            Self::Settings => 4,
            Self::Shop => 5,
        }
    }

    pub const fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Friends),
            1 => Some(Self::Album),
            2 => Some(Self::Activity),
            3 => Some(Self::Web),
            4 => Some(Self::Settings),
            5 => Some(Self::Shop),
            _ => None,
        }
    }

    pub const fn page(self) -> UtilityPage {
        match self {
            Self::Friends => UtilityPage::Friends,
            Self::Album => UtilityPage::Album,
            Self::Activity => UtilityPage::Activity,
            Self::Web => UtilityPage::Web,
            Self::Settings => UtilityPage::Settings,
            Self::Shop => UtilityPage::Shop,
        }
    }

    pub const fn route(self) -> AppRoute {
        AppRoute::Utility(self.page())
    }
}

impl UtilityPage {
    pub const fn top_utility(self) -> TopUtility {
        match self {
            Self::Friends => TopUtility::Friends,
            Self::Album => TopUtility::Album,
            Self::Activity => TopUtility::Activity,
            Self::Web => TopUtility::Web,
            Self::Settings => TopUtility::Settings,
            Self::Shop => TopUtility::Shop,
        }
    }
}

/// Stable shell-focus state remembered independently for each navigable route.
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
            AppRoute::Home | AppRoute::Library => TopUtility::Activity,
            AppRoute::Utility(page) => page.top_utility(),
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

    /// Return a focus snapshot that is valid for the route that will own it.
    ///
    /// Full-shell utility submenus never expose the persistent top utility row,
    /// so they must never retain or restore `TopUtilities` as their active focus
    /// region. Keeping this normalization with the snapshot type makes the
    /// invariant impossible for callers to accidentally bypass.
    pub const fn normalized_for_route(self, route: AppRoute) -> Self {
        if route.uses_shell_chrome() {
            self
        } else {
            self.as_content()
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
                FocusSnapshot::for_route(AppRoute::Utility(UtilityPage::Friends)),
                FocusSnapshot::for_route(AppRoute::Utility(UtilityPage::Album)),
                FocusSnapshot::for_route(AppRoute::Utility(UtilityPage::Activity)),
                FocusSnapshot::for_route(AppRoute::Utility(UtilityPage::Web)),
                FocusSnapshot::for_route(AppRoute::Utility(UtilityPage::Settings)),
                FocusSnapshot::for_route(AppRoute::Utility(UtilityPage::Shop)),
            ],
        }
    }
}

impl RouteFocusMemory {
    pub fn remember(&mut self, route: AppRoute, snapshot: FocusSnapshot) {
        self.snapshots[route.index()] = snapshot.normalized_for_route(route);
    }

    pub fn recall(&self, route: AppRoute) -> FocusSnapshot {
        self.snapshots[route.index()].normalized_for_route(route)
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

/// Global shell-menu state.
///
/// The menu is modal presentation state rather than a route: opening it must
/// not modify route history or route-local focus memory. Menu and Back can
/// close it; Home also closes it before resetting navigation.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ShellMenuState {
    open: bool,
}

impl ShellMenuState {
    pub const fn is_open(self) -> bool {
        self.open
    }

    pub fn open(&mut self) -> bool {
        if self.open {
            return false;
        }
        self.open = true;
        true
    }

    pub fn close(&mut self) -> bool {
        if !self.open {
            return false;
        }
        self.open = false;
        true
    }

    pub fn toggle(&mut self) -> bool {
        self.open = !self.open;
        self.open
    }
}

/// Pure Rust navigation state for Horizon's routed shell.
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

    /// Navigate to a new route.
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

    /// Return to the previous route when history exists.
    pub fn go_back(&mut self) -> bool {
        let Some(route) = self.back_stack.pop() else {
            return false;
        };

        self.current = route;
        true
    }

    /// Make Home the root route and discard prior route history.
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
        ShellMenuState, TopUtility, UtilityPage, nearest_game_for_x, nearest_utility_for_x,
        utility_center_x,
    };

    #[test]
    fn only_home_and_library_use_persistent_shell_chrome() {
        assert!(AppRoute::Home.uses_shell_chrome());
        assert!(AppRoute::Library.uses_shell_chrome());

        for page in [
            UtilityPage::Friends,
            UtilityPage::Album,
            UtilityPage::Activity,
            UtilityPage::Web,
            UtilityPage::Settings,
            UtilityPage::Shop,
        ] {
            assert!(!AppRoute::Utility(page).uses_shell_chrome());
        }
    }

    #[test]
    fn starts_on_home_with_empty_history() {
        let navigator = Navigator::default();
        assert_eq!(navigator.current(), AppRoute::Home);
        assert_eq!(navigator.back_stack_depth(), 0);
    }

    #[test]
    fn navigation_pushes_previous_route_and_back_restores_it() {
        let mut navigator = Navigator::default();
        let activity = AppRoute::Utility(UtilityPage::Activity);

        assert!(navigator.navigate_to(AppRoute::Library));
        assert!(navigator.navigate_to(activity));
        assert_eq!(navigator.current(), activity);
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
        let settings = AppRoute::Utility(UtilityPage::Settings);

        assert!(!navigator.navigate_to(AppRoute::Home));
        assert_eq!(navigator.back_stack_depth(), 0);

        assert!(navigator.navigate_to(settings));
        assert!(!navigator.navigate_to(settings));
        assert_eq!(navigator.back_stack_depth(), 1);
    }

    #[test]
    fn home_action_clears_history_instead_of_becoming_back_target() {
        let mut navigator = Navigator::default();

        navigator.navigate_to(AppRoute::Library);
        navigator.navigate_to(AppRoute::Utility(UtilityPage::Settings));
        assert!(navigator.go_home());

        assert_eq!(navigator.current(), AppRoute::Home);
        assert_eq!(navigator.back_stack_depth(), 0);
        assert!(!navigator.go_back());
    }

    #[test]
    fn every_shell_route_participates_in_history() {
        let mut navigator = Navigator::default();
        for route in [
            AppRoute::Library,
            AppRoute::Utility(UtilityPage::Friends),
            AppRoute::Utility(UtilityPage::Album),
            AppRoute::Utility(UtilityPage::Activity),
            AppRoute::Utility(UtilityPage::Web),
            AppRoute::Utility(UtilityPage::Settings),
            AppRoute::Utility(UtilityPage::Shop),
            AppRoute::Home,
        ] {
            assert!(navigator.navigate_to(route));
        }

        assert_eq!(navigator.current(), AppRoute::Home);
        assert_eq!(navigator.back_stack_depth(), 8);
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
        assert_eq!(focus.utility(), TopUtility::Shop);
        assert!(!focus.move_utility(1));
    }

    #[test]
    fn every_top_utility_opens_a_durable_submenu_route() {
        for (utility, page) in [
            (TopUtility::Friends, UtilityPage::Friends),
            (TopUtility::Album, UtilityPage::Album),
            (TopUtility::Activity, UtilityPage::Activity),
            (TopUtility::Web, UtilityPage::Web),
            (TopUtility::Settings, UtilityPage::Settings),
            (TopUtility::Shop, UtilityPage::Shop),
        ] {
            assert_eq!(utility.page(), page);
            assert_eq!(utility.route(), AppRoute::Utility(page));
        }
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
    fn shell_menu_toggle_and_close_are_history_independent_state() {
        let mut menu = ShellMenuState::default();

        assert!(!menu.is_open());
        assert!(menu.open());
        assert!(menu.is_open());
        assert!(!menu.open());
        assert!(!menu.toggle());
        assert!(!menu.is_open());
        assert!(menu.toggle());
        assert!(menu.close());
        assert!(!menu.is_open());
        assert!(!menu.close());
    }

    #[test]
    fn route_focus_memory_has_route_appropriate_defaults() {
        let memory = RouteFocusMemory::default();

        assert_eq!(
            memory.recall(AppRoute::Home),
            FocusSnapshot::for_route(AppRoute::Home)
        );

        for (page, utility) in [
            (UtilityPage::Friends, TopUtility::Friends),
            (UtilityPage::Album, TopUtility::Album),
            (UtilityPage::Activity, TopUtility::Activity),
            (UtilityPage::Web, TopUtility::Web),
            (UtilityPage::Settings, TopUtility::Settings),
            (UtilityPage::Shop, TopUtility::Shop),
        ] {
            let snapshot = memory.recall(AppRoute::Utility(page));
            assert_eq!(snapshot.utility(), utility);
            assert_eq!(snapshot.region(), ShellFocusRegion::Content);
        }
    }

    #[test]
    fn route_focus_memory_keeps_routes_independent() {
        let mut memory = RouteFocusMemory::default();
        let mut focus = ShellFocus::default();
        let settings = AppRoute::Utility(UtilityPage::Settings);

        focus.enter_utilities_from_content(TopUtility::Web, 2);
        memory.remember(AppRoute::Home, focus.snapshot());

        focus.restore(FocusSnapshot::for_route(settings));
        focus.enter_utilities();
        memory.remember(settings, focus.snapshot());

        assert_eq!(memory.recall(AppRoute::Home).utility(), TopUtility::Web);
        assert_eq!(
            memory.recall(AppRoute::Home).region(),
            ShellFocusRegion::TopUtilities
        );
        assert_eq!(memory.recall(settings).utility(), TopUtility::Settings);
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

        assert_eq!(
            nearest_utility_for_x(480.0, viewport_width, step),
            TopUtility::Friends
        );
        assert_eq!(
            nearest_utility_for_x(544.0, viewport_width, step),
            TopUtility::Album
        );
        assert_eq!(
            nearest_utility_for_x(608.0, viewport_width, step),
            TopUtility::Activity
        );
        assert_eq!(
            nearest_utility_for_x(672.0, viewport_width, step),
            TopUtility::Web
        );
        assert_eq!(
            nearest_utility_for_x(736.0, viewport_width, step),
            TopUtility::Settings
        );
        assert_eq!(
            nearest_utility_for_x(800.0, viewport_width, step),
            TopUtility::Shop
        );
    }

    #[test]
    fn utility_and_game_center_helpers_support_reverse_spatial_transfer() {
        let activity_x = utility_center_x(TopUtility::Activity, 1280.0, 64.0);
        assert_eq!(activity_x, 608.0);

        assert_eq!(nearest_game_for_x(activity_x, 439.0, 250.0, 8), 1);
        assert_eq!(nearest_game_for_x(768.0, 439.0, 250.0, 8), 1);
    }

    #[test]
    fn full_shell_focus_memory_normalizes_hidden_header_focus() {
        let mut memory = RouteFocusMemory::default();
        let route = AppRoute::Utility(UtilityPage::Web);
        let mut focus = ShellFocus::default();

        assert!(focus.enter_utilities_from_content(TopUtility::Settings, 6));
        assert_eq!(focus.region(), ShellFocusRegion::TopUtilities);

        memory.remember(route, focus.snapshot());
        let restored = memory.recall(route);

        assert_eq!(restored.region(), ShellFocusRegion::Content);
        assert_eq!(restored.utility(), TopUtility::Settings);
    }

    #[test]
    fn rapid_route_churn_unwinds_exactly_in_reverse_order() {
        let routes = [
            AppRoute::Library,
            AppRoute::Utility(UtilityPage::Friends),
            AppRoute::Utility(UtilityPage::Album),
            AppRoute::Utility(UtilityPage::Activity),
            AppRoute::Utility(UtilityPage::Web),
            AppRoute::Utility(UtilityPage::Settings),
            AppRoute::Utility(UtilityPage::Shop),
        ];
        let mut navigator = Navigator::default();
        let mut visited = vec![AppRoute::Home];

        for step in 0..120 {
            let route = routes[step % routes.len()];
            assert!(navigator.navigate_to(route));
            visited.push(route);
        }

        assert_eq!(navigator.back_stack_depth(), 120);
        while visited.len() > 1 {
            visited.pop();
            assert!(navigator.go_back());
            assert_eq!(navigator.current(), *visited.last().unwrap());
        }

        assert_eq!(navigator.current(), AppRoute::Home);
        assert_eq!(navigator.back_stack_depth(), 0);
        assert!(!navigator.go_back());
    }

    #[test]
    fn global_home_remains_a_hard_reset_after_deep_route_churn() {
        let mut navigator = Navigator::default();

        for step in 0..200 {
            let route = if step % 2 == 0 {
                AppRoute::Utility(UtilityPage::Settings)
            } else {
                AppRoute::Library
            };
            assert!(navigator.navigate_to(route));
        }

        assert!(navigator.go_home());
        assert_eq!(navigator.current(), AppRoute::Home);
        assert_eq!(navigator.back_stack_depth(), 0);
        assert!(!navigator.go_back());
    }

    #[test]
    fn spatial_helpers_stay_bounded_for_resize_extremes() {
        for viewport_width in [1.0_f32, 1280.0, 1720.0, 3440.0, 20_000.0] {
            let utility = nearest_utility_for_x(
                viewport_width * 0.9,
                viewport_width,
                64.0,
            );
            assert!((0..TopUtility::COUNT).contains(&utility.index()));
        }

        for x in [-10_000.0_f32, 0.0, 640.0, 10_000.0] {
            let game = nearest_game_for_x(x, 439.0, 250.0, 8);
            assert!((0..8).contains(&game));
        }

        assert_eq!(nearest_utility_for_x(f32::NAN, 1280.0, 64.0), TopUtility::Activity);
        assert_eq!(nearest_game_for_x(f32::INFINITY, 439.0, 250.0, 8), 0);
        assert_eq!(nearest_game_for_x(640.0, 439.0, 0.0, 8), 0);
        assert_eq!(nearest_game_for_x(640.0, 439.0, 250.0, 0), 0);
    }
}
