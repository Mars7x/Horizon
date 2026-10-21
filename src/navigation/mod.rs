//! Application-level navigation state.
//!
//! This module is intentionally UI-framework agnostic. Slint renders the active
//! route and shell focus, but route history, utility destinations, and focus
//! movement policy are owned by Rust.

mod edge_step;
pub mod grid;
pub mod keyboard;
pub use edge_step::step_with_edge_wrap;

/// Full-shell submenu pages opened from the four top utility icons.
///
/// These are real navigation destinations, not modal overlays. Grouping them
/// under one type keeps the shell model explicit without pretending every
/// utility page is an unrelated root-level section.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UtilityPage {
    Album,
    #[default]
    Activity,
    Achievements,
    Settings,
}

impl UtilityPage {
    const COUNT: usize = 4;

    const fn index(self) -> usize {
        match self {
            Self::Album => 0,
            Self::Activity => 1,
            Self::Achievements => 2,
            Self::Settings => 3,
        }
    }
}

/// Navigable destinations in Horizon's application shell.
///
/// Only Home uses persistent top/bottom shell chrome. Library and all four
/// utility destinations are durable full-shell routes with Back/Home history.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppRoute {
    #[default]
    Home,
    Library,
    Utility(UtilityPage),
    /// The user's profile (name, picture and Steam friends), opened from the
    /// avatar at the top left of Home.
    Profile,
}

impl AppRoute {
    const COUNT: usize = 3 + UtilityPage::COUNT;

    const fn index(self) -> usize {
        match self {
            Self::Home => 0,
            Self::Library => 1,
            Self::Utility(page) => 2 + page.index(),
            Self::Profile => 2 + UtilityPage::COUNT,
        }
    }

    /// Only Home exposes top utilities; every other route owns full-screen focus.
    pub const fn uses_shell_chrome(self) -> bool {
        matches!(self, Self::Home)
    }

    pub const fn utility_page(self) -> Option<UtilityPage> {
        match self {
            Self::Utility(page) => Some(page),
            Self::Home | Self::Library | Self::Profile => None,
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

/// Stable ordering of the four utility icons in the persistent header.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TopUtility {
    Album,
    #[default]
    Activity,
    Achievements,
    Settings,
}

impl TopUtility {
    pub const COUNT: i32 = 4;

    pub const fn index(self) -> i32 {
        match self {
            Self::Album => 0,
            Self::Activity => 1,
            Self::Achievements => 2,
            Self::Settings => 3,
        }
    }

    pub const fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Album),
            1 => Some(Self::Activity),
            2 => Some(Self::Achievements),
            3 => Some(Self::Settings),
            _ => None,
        }
    }

    pub const fn page(self) -> UtilityPage {
        match self {
            Self::Album => UtilityPage::Album,
            Self::Activity => UtilityPage::Activity,
            Self::Achievements => UtilityPage::Achievements,
            Self::Settings => UtilityPage::Settings,
        }
    }

    pub const fn route(self) -> AppRoute {
        AppRoute::Utility(self.page())
    }
}

impl UtilityPage {
    pub const fn top_utility(self) -> TopUtility {
        match self {
            Self::Album => TopUtility::Album,
            Self::Activity => TopUtility::Activity,
            Self::Achievements => TopUtility::Achievements,
            Self::Settings => TopUtility::Settings,
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
    /// The avatar, left of the utilities, has focus (in the top row).
    profile: bool,
}

impl FocusSnapshot {
    pub const fn for_route(route: AppRoute) -> Self {
        let utility = match route {
            AppRoute::Home | AppRoute::Library | AppRoute::Profile => TopUtility::Activity,
            AppRoute::Utility(page) => page.top_utility(),
        };

        Self {
            region: ShellFocusRegion::Content,
            utility,
            profile: false,
        }
    }

    pub const fn region(self) -> ShellFocusRegion {
        self.region
    }

    pub const fn utility(self) -> TopUtility {
        self.utility
    }

    pub const fn profile(self) -> bool {
        self.profile
    }

    pub const fn as_content(self) -> Self {
        Self {
            region: ShellFocusRegion::Content,
            utility: self.utility,
            profile: false,
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
                FocusSnapshot::for_route(AppRoute::Utility(UtilityPage::Album)),
                FocusSnapshot::for_route(AppRoute::Utility(UtilityPage::Activity)),
                FocusSnapshot::for_route(AppRoute::Utility(UtilityPage::Achievements)),
                FocusSnapshot::for_route(AppRoute::Utility(UtilityPage::Settings)),
                FocusSnapshot::for_route(AppRoute::Profile),
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
    /// In the top row, the avatar (left of the first utility) has focus.
    profile: bool,
}

impl ShellFocus {
    pub const fn region(self) -> ShellFocusRegion {
        self.region
    }

    pub const fn utility(self) -> TopUtility {
        self.utility
    }

    /// The avatar has focus: the top row is focused, on its leftmost item.
    pub const fn profile(self) -> bool {
        self.profile && matches!(self.region, ShellFocusRegion::TopUtilities)
    }

    pub const fn snapshot(self) -> FocusSnapshot {
        FocusSnapshot {
            region: self.region,
            utility: self.utility,
            profile: self.profile(),
        }
    }

    /// Restore durable route-local focus and clear all temporary transfer state.
    pub fn restore(&mut self, snapshot: FocusSnapshot) {
        self.region = snapshot.region();
        self.utility = snapshot.utility();
        self.profile = snapshot.profile();
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
        self.profile = false;
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
        let changed = self.region != ShellFocusRegion::TopUtilities
            || self.utility != utility
            || self.profile;
        self.region = ShellFocusRegion::TopUtilities;
        self.profile = false;
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
        // From the avatar, Up returns to the first utility.
        self.content_return_utility = Some(if self.profile {
            TopUtility::Album
        } else {
            self.utility
        });
        self.profile = false;
        self.content_return_game_index = Some(content_index);
        true
    }

    pub fn leave_utilities(&mut self) -> bool {
        if self.region == ShellFocusRegion::Content {
            return false;
        }
        self.region = ShellFocusRegion::Content;
        self.profile = false;
        self.content_anchor_index = None;
        self.utility_moved_since_entry = false;
        self.content_return_utility = None;
        self.content_return_game_index = None;
        true
    }

    pub fn select_utility(&mut self, utility: TopUtility) -> bool {
        let changed = self.utility != utility
            || self.region != ShellFocusRegion::TopUtilities
            || self.profile;
        self.utility = utility;
        self.region = ShellFocusRegion::TopUtilities;
        self.profile = false;
        self.content_anchor_index = None;
        self.utility_moved_since_entry = false;
        self.content_return_utility = None;
        self.content_return_game_index = None;
        changed
    }

    /// Move horizontally inside the utility strip. The row never wraps:
    /// the avatar is its left end and Settings its right end. A real move
    /// clears the return anchor.
    pub fn move_utility(&mut self, delta: i32) -> bool {
        if self.region != ShellFocusRegion::TopUtilities || delta == 0 {
            return false;
        }
        if self.profile {
            if delta < 0 {
                return false;
            }
            self.profile = false;
            self.utility = TopUtility::Album;
            self.utility_moved_since_entry = true;
            return true;
        }
        if delta < 0 && self.utility.index() == 0 {
            self.profile = true;
            self.utility_moved_since_entry = true;
            return true;
        }

        let index = (self.utility.index() + delta).clamp(0, TopUtility::COUNT - 1);
        let Some(next) = TopUtility::from_index(index) else {
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
pub fn nearest_utility_for_x(x: f32, viewport_width: f32, utility_center_step: f32) -> TopUtility {
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
pub fn utility_center_x(utility: TopUtility, viewport_width: f32, utility_center_step: f32) -> f32 {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsPage {
    #[default]
    Root,
    Appearance,
    Controller,
    GameBindings,
    DsBindings,
    Cores,
    Mgba,
    MelonDs,
    // Third-Party lists one "Open" row per service; each service has its
    // own sub-page so SteamGridDB and Steam work the same way.
    ThirdParty,
    SteamGridDb,
    SteamAccount,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum PageLocation {
    #[default]
    Root,
    Settings(SettingsPage),
    Activity(crate::domain::GameId),
    Album(std::path::PathBuf),
}
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Location {
    route: AppRoute,
    page: PageLocation,
}

/// Pure Rust navigation state for Horizon's routed shell.
///
/// `navigate_to` behaves like pushing a destination onto a back stack. The
/// global Home action is deliberately different: it resets the stack and makes
/// Home the root so Back cannot immediately return to the page that Home left.
/// Like a browser, Back remembers the page it left so Forward (a mouse's
/// forward button) can reopen it; opening any page or going Home forgets it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Navigator {
    current: Location,
    back_stack: Vec<Location>,
    forward_stack: Vec<Location>,
}

impl Navigator {
    pub fn current(&self) -> AppRoute {
        self.current.route
    }
    pub fn page(&self) -> PageLocation {
        self.current.page.clone()
    }
    pub fn record_page(&mut self, page: PageLocation) {
        if page == self.current.page {
            return;
        }
        if page == PageLocation::Root
            && self.current.page != PageLocation::Root
            && self
                .back_stack
                .last()
                .is_some_and(|location| location.route == self.current.route)
        {
            self.go_back();
            return;
        }
        // Gallery siblings replace the viewer address, rather than stacking slides.
        if !matches!(
            (&self.current.page, &page),
            (PageLocation::Album(_), PageLocation::Album(_))
        ) {
            self.back_stack.push(self.current.clone());
        }
        self.current.page = page;
        self.forward_stack.clear();
    }

    pub fn back_stack_depth(&self) -> usize {
        self.back_stack.len()
    }

    /// Navigate to a new route.
    ///
    /// Returns `true` only when the active route changes. Navigating to the
    /// already-active route is a no-op and does not grow history.
    pub fn navigate_to(&mut self, route: AppRoute) -> bool {
        if route == self.current.route {
            return false;
        }

        self.back_stack.push(self.current.clone());
        self.forward_stack.clear();
        self.current = Location {
            route,
            page: PageLocation::Root,
        };
        true
    }

    /// Return to the previous route when history exists.
    pub fn go_back(&mut self) -> bool {
        let Some(route) = self.back_stack.pop() else {
            return false;
        };

        self.forward_stack.push(self.current.clone());
        self.current = route;
        true
    }

    /// Reopen the route Back last left, when there is one.
    pub fn go_forward(&mut self) -> bool {
        let Some(route) = self.forward_stack.pop() else {
            return false;
        };

        self.back_stack.push(self.current.clone());
        self.current = route;
        true
    }

    /// Make Home the root route and discard prior route history.
    pub fn go_home(&mut self) -> bool {
        let changed = self.current.route != AppRoute::Home
            || self.current.page != PageLocation::Root
            || !self.back_stack.is_empty();
        self.current = Location::default();
        self.back_stack.clear();
        self.forward_stack.clear();
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AppRoute, FocusSnapshot, Navigator, RouteFocusMemory, ShellFocus, ShellFocusRegion,
        TopUtility, UtilityPage, nearest_game_for_x, nearest_utility_for_x, utility_center_x,
    };

    #[test]
    fn nested_pages_share_multi_step_back_forward_and_branch_history() {
        use super::{PageLocation as Page, SettingsPage as Settings};
        let mut nav = Navigator::default();
        nav.navigate_to(AppRoute::Utility(UtilityPage::Settings));
        nav.record_page(Page::Settings(Settings::Cores));
        nav.record_page(Page::Settings(Settings::Mgba));
        nav.navigate_to(AppRoute::Utility(UtilityPage::Album));
        nav.record_page(Page::Album("first.png".into()));
        nav.record_page(Page::Album("second.png".into()));
        let expected = [
            Page::Root,
            Page::Settings(Settings::Mgba),
            Page::Settings(Settings::Cores),
            Page::Root,
            Page::Root,
        ];
        for page in expected {
            assert!(nav.go_back());
            assert_eq!(nav.page(), page);
        }
        assert_eq!(nav.current(), AppRoute::Home);
        for _ in 0..5 {
            assert!(nav.go_forward());
        }
        assert_eq!(nav.page(), Page::Album("second.png".into()));
        nav.record_page(Page::Root); // closing a viewer records a Back, not a new branch
        assert!(nav.go_forward());
        assert_eq!(nav.page(), Page::Album("second.png".into()));
        assert!(nav.go_back());
        nav.navigate_to(AppRoute::Utility(UtilityPage::Activity));
        nav.record_page(Page::Activity(crate::domain::GameId::new(7).unwrap()));
        assert!(!nav.go_forward());
        assert!(nav.go_back());
        assert_eq!(nav.page(), Page::Root);
        assert!(nav.go_forward());
        assert!(matches!(nav.page(), Page::Activity(_)));
    }

    #[test]
    fn only_home_uses_persistent_shell_chrome() {
        assert!(AppRoute::Home.uses_shell_chrome());
        assert!(!AppRoute::Library.uses_shell_chrome());
        let library_focus = FocusSnapshot::for_route(AppRoute::Library);
        assert_eq!(
            library_focus
                .normalized_for_route(AppRoute::Library)
                .region(),
            ShellFocusRegion::Content
        );

        for page in [
            UtilityPage::Album,
            UtilityPage::Activity,
            UtilityPage::Achievements,
            UtilityPage::Settings,
        ] {
            assert!(!AppRoute::Utility(page).uses_shell_chrome());
        }
    }

    #[test]
    fn forward_reopens_what_back_left_until_a_new_page_opens() {
        let mut navigator = Navigator::default();
        let album = AppRoute::Utility(UtilityPage::Album);
        let settings = AppRoute::Utility(UtilityPage::Settings);
        assert!(!navigator.go_forward());
        navigator.navigate_to(album);
        navigator.navigate_to(settings);
        assert!(navigator.go_back());
        assert!(navigator.go_back());
        assert_eq!(navigator.current(), AppRoute::Home);
        assert!(navigator.go_forward());
        assert_eq!(navigator.current(), album);
        assert!(navigator.go_forward());
        assert_eq!(navigator.current(), settings);
        assert!(!navigator.go_forward());

        // Back, then opening another page, forgets the way forward.
        assert!(navigator.go_back());
        navigator.navigate_to(AppRoute::Library);
        assert!(!navigator.go_forward());
        assert!(navigator.go_back());
        assert_eq!(navigator.current(), album);

        // So does Home.
        assert!(navigator.go_back());
        navigator.navigate_to(album);
        assert!(navigator.go_back());
        navigator.go_home();
        assert!(!navigator.go_forward());
    }

    #[test]
    fn the_avatar_sits_left_of_the_first_utility() {
        let mut focus = ShellFocus::default();
        assert!(focus.enter_utilities_from_content(TopUtility::Activity, 3));
        assert!(focus.move_utility(-1));
        assert_eq!(focus.utility(), TopUtility::Album);
        assert!(!focus.profile());
        // Left from Album reaches the avatar, fresh or held, and stops there.
        assert!(focus.move_utility(-1));
        assert!(focus.profile());
        assert!(!focus.move_utility(-1));
        assert!(focus.profile());
        // Right goes back to Album.
        assert!(focus.move_utility(1));
        assert!(!focus.profile());
        assert_eq!(focus.utility(), TopUtility::Album);
        // Remembered with the route, forgotten on leaving the row.
        focus.move_utility(-1);
        let snapshot = focus.snapshot();
        assert!(snapshot.profile());
        let mut restored = ShellFocus::default();
        restored.restore(snapshot);
        assert!(restored.profile());
        assert!(focus.leave_utilities_to_content(0));
        assert!(!focus.profile());
        assert_eq!(
            focus.anchored_utility_for_content(0),
            Some(TopUtility::Album)
        );
        // Other routes never keep header focus.
        assert!(!snapshot.normalized_for_route(AppRoute::Profile).profile());
    }

    #[test]
    fn the_profile_page_is_a_full_shell_route_with_history() {
        assert!(!AppRoute::Profile.uses_shell_chrome());
        assert_eq!(AppRoute::Profile.utility_page(), None);
        let mut navigator = Navigator::default();
        assert!(navigator.navigate_to(AppRoute::Profile));
        assert!(navigator.go_back());
        assert_eq!(navigator.current(), AppRoute::Home);
        let mut memory = RouteFocusMemory::default();
        memory.remember(
            AppRoute::Profile,
            FocusSnapshot::for_route(AppRoute::Profile),
        );
        assert_eq!(
            memory.recall(AppRoute::Profile).region(),
            ShellFocusRegion::Content
        );
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
            AppRoute::Utility(UtilityPage::Album),
            AppRoute::Utility(UtilityPage::Activity),
            AppRoute::Utility(UtilityPage::Achievements),
            AppRoute::Utility(UtilityPage::Settings),
            AppRoute::Home,
        ] {
            assert!(navigator.navigate_to(route));
        }

        assert_eq!(navigator.current(), AppRoute::Home);
        assert_eq!(navigator.back_stack_depth(), 6);
    }

    #[test]
    fn shell_focus_enters_header_at_center_and_restores_last_utility() {
        let mut focus = ShellFocus::default();
        assert_eq!(focus.region(), ShellFocusRegion::Content);
        assert_eq!(focus.utility(), TopUtility::Activity);

        assert!(focus.enter_utilities());
        assert!(focus.move_utility(1));
        assert_eq!(focus.utility(), TopUtility::Achievements);
        assert!(focus.leave_utilities());
        assert!(focus.enter_utilities());
        assert_eq!(focus.utility(), TopUtility::Achievements);
    }

    #[test]
    fn utility_strip_never_wraps() {
        let mut focus = ShellFocus::default();
        focus.enter_utilities();

        assert!(focus.move_utility(-2));
        assert_eq!(focus.utility(), TopUtility::Album);
        // The row's left end is the avatar, not a wrap to Settings.
        assert!(focus.move_utility(-1));
        assert!(focus.profile());
        assert!(!focus.move_utility(-1));
        assert!(focus.move_utility(1));
        // The right end stops at Settings, held or fresh.
        for _ in 0..5 {
            focus.move_utility(1);
        }
        assert_eq!(focus.utility(), TopUtility::Settings);
        assert!(!focus.move_utility(1));
        assert!(!focus.move_utility(1));
        assert_eq!(focus.utility(), TopUtility::Settings);
    }

    #[test]
    fn every_top_utility_opens_a_durable_submenu_route() {
        for (utility, page) in [
            (TopUtility::Album, UtilityPage::Album),
            (TopUtility::Activity, UtilityPage::Activity),
            (TopUtility::Achievements, UtilityPage::Achievements),
            (TopUtility::Settings, UtilityPage::Settings),
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

        assert!(focus.enter_utilities_from_content(TopUtility::Settings, 4));
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

        for (page, utility) in [
            (UtilityPage::Album, TopUtility::Album),
            (UtilityPage::Activity, TopUtility::Activity),
            (UtilityPage::Achievements, TopUtility::Achievements),
            (UtilityPage::Settings, TopUtility::Settings),
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

        focus.enter_utilities_from_content(TopUtility::Achievements, 2);
        memory.remember(AppRoute::Home, focus.snapshot());

        focus.restore(FocusSnapshot::for_route(settings));
        focus.enter_utilities();
        memory.remember(settings, focus.snapshot());

        assert_eq!(
            memory.recall(AppRoute::Home).utility(),
            TopUtility::Achievements
        );
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

        for (x, utility) in [
            (544.0, TopUtility::Album),
            (608.0, TopUtility::Activity),
            (672.0, TopUtility::Achievements),
            (736.0, TopUtility::Settings),
        ] {
            assert_eq!(nearest_utility_for_x(x, viewport_width, step), utility);
        }
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
        let route = AppRoute::Utility(UtilityPage::Album);
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
            AppRoute::Utility(UtilityPage::Album),
            AppRoute::Utility(UtilityPage::Activity),
            AppRoute::Utility(UtilityPage::Achievements),
            AppRoute::Utility(UtilityPage::Settings),
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
            let utility = nearest_utility_for_x(viewport_width * 0.9, viewport_width, 64.0);
            assert!((0..TopUtility::COUNT).contains(&utility.index()));
        }

        for x in [-10_000.0_f32, 0.0, 640.0, 10_000.0] {
            let game = nearest_game_for_x(x, 439.0, 250.0, 8);
            assert!((0..8).contains(&game));
        }

        assert_eq!(
            nearest_utility_for_x(f32::NAN, 1280.0, 64.0),
            TopUtility::Activity
        );
        assert_eq!(nearest_game_for_x(f32::INFINITY, 439.0, 250.0, 8), 0);
        assert_eq!(nearest_game_for_x(640.0, 439.0, 0.0, 8), 0);
        assert_eq!(nearest_game_for_x(640.0, 439.0, 250.0, 0), 0);
    }
}

pub mod onboarding;
