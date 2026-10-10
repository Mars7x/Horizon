use std::{cell::RefCell, rc::Rc, time::Duration};

use slint::{ComponentHandle, Model};
use tracing::debug;

use crate::{
    AppRouteView, AppWindow,
    audio::UiSoundCue,
    input::{UiAction, UiActionEvent},
    navigation::{
        AppRoute, Navigator, RouteFocusMemory, ShellFocus, ShellFocusRegion,
        TopUtility, UtilityPage, nearest_game_for_x, nearest_utility_for_x, utility_center_x,
    },
};

use super::{CallbackSlot, achievements::AchievementsController, activity::ActivityDetailsActions, home::HomeController, library::LibraryController, settings::SettingsController};

/// Bridges pure Rust navigation/focus state to Slint presentation state.
///
/// Device adapters remain screen-agnostic. They emit `UiActionEvent`; this
/// controller decides whether an action is global navigation, shell focus, or
/// page-local behavior.
pub struct NavigationController {
    navigator: RefCell<Navigator>,
    focus: RefCell<ShellFocus>,
    focus_memory: RefCell<RouteFocusMemory>,
    home: Rc<HomeController>,
    library: Rc<LibraryController>,
    settings: Rc<SettingsController>,
    action_sound: CallbackSlot<dyn Fn(UiSoundCue)>,
    activity_on_enter: CallbackSlot<dyn Fn(&AppWindow)>,
    activity_details: CallbackSlot<dyn ActivityDetailsActions>,
    achievements: RefCell<Option<Rc<AchievementsController>>>,
}

impl NavigationController {
    pub fn new(ui: &AppWindow, home: Rc<HomeController>, library: Rc<LibraryController>, settings: Rc<SettingsController>) -> Rc<Self> {
        let controller = Rc::new(Self {
            navigator: RefCell::new(Navigator::default()),
            focus: RefCell::new(ShellFocus::default()),
            focus_memory: RefCell::new(RouteFocusMemory::default()),
            home,
            library,
            settings,
            action_sound: RefCell::new(None),
            activity_on_enter: RefCell::new(None),
            activity_details: RefCell::new(None),
            achievements: RefCell::new(None),
        });
        controller.publish_route(ui);
        controller.publish_focus(ui);
        controller.bind_ui_callbacks(ui);
        controller
    }

    pub fn set_action_sound(&self, callback: Rc<dyn Fn(UiSoundCue)>) {
        self.settings.set_action_sound(Rc::clone(&callback));
        *self.action_sound.borrow_mut() = Some(callback);
    }

    /// Refresh Activity's visible data before publishing the new page route.
    pub fn set_activity_on_enter(&self, callback: Rc<dyn Fn(&AppWindow)>) {
        *self.activity_on_enter.borrow_mut() = Some(callback);
    }

    pub fn set_achievements(&self, controller: Rc<AchievementsController>, ui: &AppWindow) {
        *self.achievements.borrow_mut() = Some(Rc::clone(&controller));
        let weak = ui.as_weak();
        ui.on_achievements_select(move |index| {
            if let Some(ui) = weak.upgrade() {
                controller.choose_game(&ui, index);
            }
        });
        let controller = self.achievements.borrow().as_ref().cloned().expect("Achievements controller is installed");
        let weak = ui.as_weak();
        ui.on_achievements_back(move || {
            if let Some(ui) = weak.upgrade() { controller.exit_entries(&ui); }
        });
        let controller = self.achievements.borrow().as_ref().cloned().expect("Achievements controller is installed");
        let weak = ui.as_weak();
        ui.on_achievements_scroll_entries(move |delta| {
            if let Some(ui) = weak.upgrade() { controller.move_entries(&ui, delta); }
        });
        let controller = self.achievements.borrow().as_ref().cloned().expect("Achievements controller is installed");
        let weak = ui.as_weak();
        ui.on_achievements_cycle_source(move |direction| {
            if let Some(ui) = weak.upgrade() {
                controller.cycle_source(&ui, direction);
            }
        });
    }

    pub fn set_activity_details(&self, actions: Rc<dyn ActivityDetailsActions>) {
        *self.activity_details.borrow_mut() = Some(actions);
    }

    /// Entering Activity through either a direct route or the Back stack is a
    /// new visit. Nested detail Back is not: that path preserves the cover.
    fn prepare_activity_visit(&self, ui: &AppWindow) {
        // Populate the read-only Milestones preview even if the user has never
        // opened the dedicated Achievements utility. Fetching stays off-thread.
        if let Some(achievements) = self.achievements.borrow().as_ref() {
            achievements.on_enter(ui);
        }
        // Leaving Activity through Home/Menu can retain its nested details UI.
        // A new visit must show the overview, not that previous game's history.
        if ui.get_activity_details_visible()
            && let Some(details) = self.activity_details.borrow().as_ref() {
            details.close(ui);
        }
        ui.set_activity_pressed_index(-1);
        ui.set_activity_selected_index(0);
        if let Some(refresh) = self.activity_on_enter.borrow().as_ref() {
            refresh(ui);
        }
        // A reordered most-played list may restore the former first GameId
        // during refresh. Force the NEW first cover for every new visit.
        ui.set_activity_selected_index(0);
    }

    fn cue(&self, cue: UiSoundCue) {
        if let Some(callback) = self.action_sound.borrow().as_ref() { callback(cue); }
    }

    pub fn current_route(&self) -> AppRoute {
        self.navigator.borrow().current()
    }

    /// Entry point for shell/header callbacks. Route policy, history, and
    /// route-local shell-focus restoration remain Rust-owned.
    pub fn navigate_to(&self, ui: &AppWindow, route: AppRoute) {
        let from = self.current_route();
        if route == from {
            return;
        }

        self.remember_focus_for_route(from);
        if self.navigator.borrow_mut().navigate_to(route) {
            debug!(?from, to = ?route, "route changed");
            if from == AppRoute::Utility(UtilityPage::Settings) { self.settings.on_leave(ui); }
            if route == AppRoute::Utility(UtilityPage::Settings) { self.settings.on_enter(ui); }
            if route == AppRoute::Library { self.library.on_enter(ui); }
            if route == AppRoute::Utility(UtilityPage::Achievements)
                && let Some(controller) = self.achievements.borrow().as_ref() { controller.start_new_visit(ui); }
            if route == AppRoute::Utility(UtilityPage::Activity) {
                self.prepare_activity_visit(ui);
            }
            self.publish_route_change(ui, from, route);
            self.restore_focus_for_route(ui, route);
        }
    }

    pub fn handle_action(&self, ui: &AppWindow, event: UiActionEvent) {
        if event.repeated && event.action.is_global() {
            debug!(action = ?event.action, "repeated global action ignored by navigation layer");
            return;
        }
        if self.current_route() == AppRoute::Utility(UtilityPage::Achievements)
            && event.action == UiAction::Back
            && self.achievements.borrow().as_ref().is_some_and(|c| c.exit_entries(ui)) {
            self.cue(UiSoundCue::Back);
            return;
        }
        if self.current_route() == AppRoute::Utility(UtilityPage::Settings)
            && self.settings.handle_action(ui, event)
        {
            return;
        }
        match event.action {
            UiAction::Back => self.handle_back(ui),
            UiAction::Home => self.handle_home(ui),
            UiAction::Menu => self.handle_menu(ui),
            UiAction::Accept if !event.repeated
                && self.current_route() == AppRoute::Home
                && self.focus.borrow().region() != ShellFocusRegion::TopUtilities
                && self.home.library_tile_selected() => {
                self.open_library(ui);
            }
            _ if self.current_route().uses_shell_chrome()
                && self.focus.borrow().region() == ShellFocusRegion::TopUtilities => {
                self.handle_utility_action(ui, event)
            }
            UiAction::Up if self.current_route().uses_shell_chrome() => {
                let changed = if self.current_route() == AppRoute::Home {
                    let selected_index = self.home.selected_index();
                    let anchored_utility = self
                        .focus
                        .borrow_mut()
                        .anchored_utility_for_content(selected_index);

                    let utility = anchored_utility.unwrap_or_else(|| {
                        let game_center_x = ui.get_home_first_game_center_x_px()
                            + selected_index as f32 * ui.get_home_game_stride_px();
                        nearest_utility_for_x(
                            game_center_x,
                            ui.get_logical_viewport_width_px(),
                            ui.get_utility_center_step_px(),
                        )
                    });

                    self.focus
                        .borrow_mut()
                        .enter_utilities_from_content(utility, selected_index)
                } else {
                    self.focus.borrow_mut().enter_utilities()
                };

                if changed {
                    debug!(
                        utility = ?self.focus.borrow().utility(),
                        "focus entered top utilities by reciprocal anchor or shortest center distance"
                    );
                    self.publish_focus(ui);
                }
            }
            _ => self.dispatch_to_active_page(ui, event),
        }
    }

    fn bind_ui_callbacks(self: &Rc<Self>, ui: &AppWindow) {
        let ui_weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_activate_utility(move |index| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let Some(utility) = TopUtility::from_index(index) else {
                return;
            };

            controller.focus.borrow_mut().select_utility(utility);
            controller.publish_focus(&ui);
            controller.activate_utility(&ui, utility);
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_open_library(move || {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Home { controller.open_library(&ui); }
        });

        // Pointer activation is routed through the shell too, so it restores
        // content focus and cannot operate a retained/outgoing Library page.
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_library_select(move |index| {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Library {
                controller.focus.borrow_mut().leave_utilities();
                controller.publish_focus(&ui);
                controller.library.select_on_page(&ui, index);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_library_cycle_filter(move || {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Library {
                controller.focus.borrow_mut().leave_utilities();
                controller.publish_focus(&ui);
                controller.cue(UiSoundCue::Ok);
                controller.library.cycle_filter(&ui);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_library_cycle_sort(move || {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Library {
                controller.focus.borrow_mut().leave_utilities();
                controller.publish_focus(&ui);
                controller.cue(UiSoundCue::Ok);
                controller.library.cycle_sort(&ui);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_library_scroll_rows(move |direction| {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Library {
                controller.library.scroll_by_row(&ui, direction);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_library_back(move || {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Library {
                controller.handle_back(&ui);
            }
        });
        // Activity has exactly one controller focus region (the cover strip).
        // Charts and the Milestones placeholder never capture navigation.
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_activity_choose(move |index| {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Utility(UtilityPage::Activity) {
                let len = ui.get_activity_covers().row_count() as i32;
                if index >= 0 && index < len && !ui.get_activity_details_visible() {
                    ui.set_activity_selected_index(index);
                    controller.begin_activity_details_open(&ui, index as usize);
                }
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_activity_detail_back(move || {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Utility(UtilityPage::Activity)
                && ui.get_activity_details_visible() {
                controller.handle_back(&ui);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_activity_detail_layout_changed(move || {
            if let Some(ui) = weak.upgrade()
                && let Some(details) = controller.activity_details.borrow().as_ref() {
                details.refresh_visible_rows(&ui);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_activity_detail_scroll(move |direction| {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Utility(UtilityPage::Activity)
                && ui.get_activity_details_visible()
                && let Some(details) = controller.activity_details.borrow().as_ref() {
                details.scroll_rows(&ui, direction);
            }
        });
        // Each move closure takes ownership of its own Weak/AppWindow and Rc.
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_library_layout_changed(move || {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Library {
                controller.library.on_viewport_changed(&ui);
            }
        });
    }

    fn move_activity_selection(&self, ui: &AppWindow, delta: i32) {
        let len = ui.get_activity_covers().row_count();
        if len == 0 { return; }
        let last = len.saturating_sub(1).min(i32::MAX as usize) as i32;
        let next = ui.get_activity_selected_index().saturating_add(delta).clamp(0, last);
        if next != ui.get_activity_selected_index() {
            ui.set_activity_pressed_index(-1);
        }
        ui.set_activity_selected_index(next);
    }

    fn open_library(&self, ui: &AppWindow) {
        if self.current_route() == AppRoute::Library { return; }
        self.cue(UiSoundCue::Ok);
        self.navigate_to(ui, AppRoute::Library);
    }

    fn handle_back(&self, ui: &AppWindow) {
        if self.current_route() == AppRoute::Utility(UtilityPage::Activity)
            && ui.get_activity_details_visible() {
            if let Some(details) = self.activity_details.borrow().as_ref() {
                details.close(ui);
                self.cue(UiSoundCue::Back);
            }
            return;
        }
        let from = self.current_route();
        self.remember_focus_for_route(from);
        if self.navigator.borrow_mut().go_back() {
            let to = self.current_route();
            debug!(?from, ?to, "Back restored previous route and focus");
            if from == AppRoute::Utility(UtilityPage::Settings) { self.settings.on_leave(ui); }
            if to == AppRoute::Utility(UtilityPage::Settings) { self.settings.on_enter(ui); }
            if to == AppRoute::Utility(UtilityPage::Achievements)
                && let Some(controller) = self.achievements.borrow().as_ref() { controller.start_new_visit(ui); }
            if to == AppRoute::Utility(UtilityPage::Activity) {
                self.prepare_activity_visit(ui);
            }
            // Queue feedback before potentially expensive route redraws.
            self.cue(UiSoundCue::Back);
            self.publish_route_change(ui, from, to);
            self.restore_focus_for_route(ui, to);
        } else {
            debug!(route = ?from, "Back ignored at navigation root");
        }
    }

    fn handle_home(&self, ui: &AppWindow) {

        let from = self.current_route();
        self.remember_focus_for_route(from);

        let home_snapshot = self
            .focus_memory
            .borrow()
            .recall(AppRoute::Home)
            .as_content();
        self.focus.borrow_mut().restore(home_snapshot);
        self.focus_memory
            .borrow_mut()
            .remember(AppRoute::Home, home_snapshot);
        self.home.reset_for_global_home(ui);
        self.publish_focus(ui);

        if self.navigator.borrow_mut().go_home() {
            debug!(
                ?from,
                to = ?AppRoute::Home,
                "Home reset top-level navigation, content focus, and first-game selection"
            );
            if from == AppRoute::Utility(UtilityPage::Settings) { self.settings.on_leave(ui); }
            self.publish_route_change(ui, from, AppRoute::Home);
        }
    }

    // Menu/Start provides the same direct destination as the Home Library tile.
    fn handle_menu(&self, ui: &AppWindow) {
        self.open_library(ui);
    }

    fn handle_utility_action(&self, ui: &AppWindow, event: UiActionEvent) {
        match event.action {
            UiAction::Left | UiAction::Right => {
                let delta = if event.action == UiAction::Left { -1 } else { 1 };
                if self.focus.borrow_mut().move_utility(delta, event.repeated) {
                    debug!(utility = ?self.focus.borrow().utility(), "top utility focus moved");
                    self.publish_focus(ui);
                }
            }
            UiAction::Down => {
                let changed = if self.current_route() == AppRoute::Home {
                    let (utility, exact_return_index) = {
                        let focus = self.focus.borrow();
                        (focus.utility(), focus.content_anchor_index())
                    };

                    let game_index = exact_return_index.unwrap_or_else(|| {
                        let utility_x = utility_center_x(
                            utility,
                            ui.get_logical_viewport_width_px(),
                            ui.get_utility_center_step_px(),
                        );
                        nearest_game_for_x(
                            utility_x,
                            ui.get_home_first_game_center_x_px(),
                            ui.get_home_game_stride_px(),
                            self.home.game_count(),
                        )
                    });

                    self.home.select_from_shell(ui, game_index);
                    self.focus
                        .borrow_mut()
                        .leave_utilities_to_content(game_index)
                } else {
                    self.focus.borrow_mut().leave_utilities()
                };

                if changed {
                    debug!("focus returned to page content with reciprocal anchor or spatial transfer");
                    self.publish_focus(ui);
                }
            }
            UiAction::Accept if !event.repeated => {
                let utility = self.focus.borrow().utility();
                self.activate_utility(ui, utility);
            }
            UiAction::Up
            | UiAction::Accept
            | UiAction::LeftBumper
            | UiAction::RightBumper => {
                debug!(
                    action = ?event.action,
                    repeated = event.repeated,
                    "action has no top-utility behavior"
                );
            }
            UiAction::Back | UiAction::Menu | UiAction::Home => {
                unreachable!("global actions handled first")
            }
        }
    }

    fn activate_utility(&self, ui: &AppWindow, utility: TopUtility) {
        let route = utility.route();
        let from = self.current_route();
        if route == from { return; }
        // The destination is a known valid utility route. Dispatch OK before
        // synchronous on_enter() and route publishing, not after them.
        self.cue(UiSoundCue::Ok);
        self.navigate_to(ui, route);
        if self.current_route() != from {
            debug!(?from, to = ?route, ?utility, "utility submenu opened");
        }
    }

    /// Share the same Home-like press-in for controller Accept and pointer
    /// activation. Wait just long enough for the selected cover to visibly
    /// depress before the Settings-style details crossfade begins.
    fn begin_activity_details_open(&self, ui: &AppWindow, index: usize) {
        if self.current_route() != AppRoute::Utility(UtilityPage::Activity)
            || ui.get_activity_details_visible()
            || ui.get_activity_pressed_index() >= 0
            || index >= ui.get_activity_covers().row_count()
        {
            return;
        }

        // Reduced Motion skips both the press timing and the visual transition.
        if ui.get_activity_reduced_motion() {
            if let Some(details) = self.activity_details.borrow().as_ref()
                && details.open(ui, index) {
                self.cue(UiSoundCue::Ok);
            }
            return;
        }

        ui.set_activity_pressed_index(index as i32);
        let weak_ui = ui.as_weak();
        let details = self.activity_details.borrow().as_ref().cloned();
        let cue = self.action_sound.borrow().as_ref().cloned();
        slint::Timer::single_shot(Duration::from_millis(125), move || {
            let Some(ui) = weak_ui.upgrade() else { return };
            if ui.get_activity_pressed_index() != index as i32 { return; }
            ui.set_activity_pressed_index(-1);
            // An earlier Back/Home/route change cancels the pending press.
            if ui.get_current_route() != AppRouteView::Activity
                || ui.get_activity_details_visible()
                || ui.get_activity_selected_index() != index as i32
            {
                return;
            }
            if let Some(details) = details.as_ref()
                && details.open(&ui, index)
                && let Some(cue) = cue.as_ref()
            {
                cue(UiSoundCue::Ok);
            }
        });
    }

    fn dispatch_to_active_page(&self, ui: &AppWindow, event: UiActionEvent) {
        match self.current_route() {
            AppRoute::Home => self.home.handle_action(ui, event),
            AppRoute::Library => self.library.handle_action(ui, event),
            AppRoute::Utility(UtilityPage::Achievements) => {
                if let Some(controller) = self.achievements.borrow().as_ref() {
                    if ui.get_achievements_viewing_entries() {
                        match event.action {
                            UiAction::Up => controller.move_entries(ui, -1),
                            UiAction::Down => controller.move_entries(ui, 1),
                            UiAction::Left => { controller.exit_entries(ui); },
                            _ => {}
                        }
                    } else {
                        match event.action {
                            UiAction::Up => controller.move_game(ui, -1),
                            UiAction::Down => controller.move_game(ui, 1),
                            UiAction::Right | UiAction::Accept if !event.repeated => controller.enter_entries(ui),
                            UiAction::LeftBumper if !event.repeated => controller.cycle_source(ui, -1),
                            UiAction::RightBumper if !event.repeated => controller.cycle_source(ui, 1),
                            _ => {}
                        }
                    }
                }
            }
            AppRoute::Utility(UtilityPage::Activity) => {
                if ui.get_activity_details_visible() {
                    if let Some(details) = self.activity_details.borrow().as_ref() {
                        match event.action {
                            UiAction::Up => details.scroll_rows(ui, -1),
                            UiAction::Down => details.scroll_rows(ui, 1),
                            _ => {}
                        }
                    }
                } else {
                    match event.action {
                        UiAction::Left => self.move_activity_selection(ui, -1),
                        UiAction::Right => self.move_activity_selection(ui, 1),
                        UiAction::Accept if !event.repeated => {
                            self.begin_activity_details_open(ui, ui.get_activity_selected_index().max(0) as usize);
                        }
                        _ => {}
                    }
                }
            }
            route => {
                debug!(
                    ?route,
                    action = ?event.action,
                    repeated = event.repeated,
                    "page-local action deferred until its controller is introduced"
                );
            }
        }
    }

    fn remember_focus_for_route(&self, route: AppRoute) {
        self.focus_memory
            .borrow_mut()
            .remember(route, self.focus.borrow().snapshot());
    }

    fn restore_focus_for_route(&self, ui: &AppWindow, route: AppRoute) {
        let snapshot = self.focus_memory.borrow().recall(route);
        self.focus.borrow_mut().restore(snapshot);
        self.publish_focus(ui);
    }

    fn publish_route(&self, ui: &AppWindow) {
        let route = self.current_route();
        ui.set_transition_from_route(route_view(route));
        ui.set_current_route(route_view(route));
    }

    /// Publish exactly one outgoing route for the visual transition layer.
    /// Older outgoing pages are dropped immediately when rapid navigation
    /// retargets the shell, preventing several half-faded pages from stacking.
    fn publish_route_change(&self, ui: &AppWindow, from: AppRoute, to: AppRoute) {
        if from == AppRoute::Utility(UtilityPage::Activity) && from != to
            && let Some(details) = self.activity_details.borrow().as_ref() {
            details.close(ui);
        }
        ui.set_transition_from_route(route_view(from));
        ui.set_current_route(route_view(to));
    }

    fn publish_focus(&self, ui: &AppWindow) {
        let focus = *self.focus.borrow();
        ui.set_top_utilities_focused(focus.region() == ShellFocusRegion::TopUtilities);
        ui.set_focused_utility_index(focus.utility().index());
    }

}

fn route_view(route: AppRoute) -> AppRouteView {
    match route {
        AppRoute::Home => AppRouteView::Home,
        AppRoute::Library => AppRouteView::Library,
        AppRoute::Utility(UtilityPage::Friends) => AppRouteView::Friends,
        AppRoute::Utility(UtilityPage::Album) => AppRouteView::Album,
        AppRoute::Utility(UtilityPage::Activity) => AppRouteView::Activity,
        AppRoute::Utility(UtilityPage::Achievements) => AppRouteView::Achievements,
        AppRoute::Utility(UtilityPage::Web) => AppRouteView::Web,
        AppRoute::Utility(UtilityPage::Settings) => AppRouteView::Settings,
        AppRoute::Utility(UtilityPage::Shop) => AppRouteView::Shop,
    }
}

#[cfg(test)]
mod tests {
    use super::route_view;
    use crate::{
        AppRouteView,
        navigation::{AppRoute, UtilityPage},
    };

    #[test]
    fn every_rust_route_has_an_explicit_slint_view_mapping() {
        assert!(matches!(route_view(AppRoute::Home), AppRouteView::Home));
        assert!(matches!(
            route_view(AppRoute::Library),
            AppRouteView::Library
        ));

        assert!(route_view(AppRoute::Utility(UtilityPage::Friends)) == AppRouteView::Friends);
        assert!(route_view(AppRoute::Utility(UtilityPage::Album)) == AppRouteView::Album);
        assert!(route_view(AppRoute::Utility(UtilityPage::Activity)) == AppRouteView::Activity);
        assert!(route_view(AppRoute::Utility(UtilityPage::Achievements)) == AppRouteView::Achievements);
        assert!(route_view(AppRoute::Utility(UtilityPage::Web)) == AppRouteView::Web);
        assert!(route_view(AppRoute::Utility(UtilityPage::Settings)) == AppRouteView::Settings);
        assert!(route_view(AppRoute::Utility(UtilityPage::Shop)) == AppRouteView::Shop);
    }
}
