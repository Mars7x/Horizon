use std::{cell::RefCell, rc::Rc};

use slint::ComponentHandle;
use tracing::debug;

use crate::{
    AppRouteView, AppWindow, UtilityOverlayView,
    input::{UiAction, UiActionEvent},
    navigation::{
        AppRoute, Navigator, RouteFocusMemory, ShellFocus, ShellFocusRegion, TopUtility,
        UtilityDestination, UtilityOverlay, nearest_game_for_x, nearest_utility_for_x,
        utility_center_x,
    },
};

use super::home::HomeController;

/// Bridges pure Rust navigation/focus state to Slint presentation state.
///
/// Device adapters remain screen-agnostic. They emit `UiActionEvent`; this
/// controller decides whether an action is global navigation, shell focus, or
/// page-local behavior.
pub struct NavigationController {
    navigator: RefCell<Navigator>,
    focus: RefCell<ShellFocus>,
    focus_memory: RefCell<RouteFocusMemory>,
    overlay: RefCell<Option<UtilityOverlay>>,
    home: Rc<HomeController>,
}

impl NavigationController {
    pub fn new(ui: &AppWindow, home: Rc<HomeController>) -> Rc<Self> {
        let controller = Rc::new(Self {
            navigator: RefCell::new(Navigator::default()),
            focus: RefCell::new(ShellFocus::default()),
            focus_memory: RefCell::new(RouteFocusMemory::default()),
            overlay: RefCell::new(None),
            home,
        });
        controller.publish_route(ui);
        controller.publish_focus(ui);
        controller.publish_overlay(ui);
        controller.bind_ui_callbacks(ui);
        controller
    }

    pub fn current_route(&self) -> AppRoute {
        self.navigator.borrow().current()
    }

    /// Entry point for shell/header callbacks. Route policy, history, and
    /// route-local shell-focus restoration remain Rust-owned.
    pub fn navigate_to(&self, ui: &AppWindow, route: AppRoute) {
        self.close_overlay(ui);
        let from = self.current_route();
        if route == from {
            return;
        }

        self.remember_focus_for_route(from);
        if self.navigator.borrow_mut().navigate_to(route) {
            debug!(?from, to = ?route, "top-level route changed");
            self.publish_route(ui);
            self.restore_focus_for_route(ui, route);
        }
    }

    pub fn handle_action(&self, ui: &AppWindow, event: UiActionEvent) {
        match event.action {
            UiAction::Back if event.repeated => {
                debug!("repeated Back ignored by navigation layer");
            }
            UiAction::Home if event.repeated => {
                debug!("repeated Home ignored by navigation layer");
            }
            UiAction::Back => self.handle_back(ui),
            UiAction::Home => self.handle_home(ui),
            _ if self.overlay.borrow().is_some() => {
                debug!(
                    action = ?event.action,
                    "utility overlay is modal; action ignored until Back/Home"
                );
            }
            _ if self.focus.borrow().region() == ShellFocusRegion::TopUtilities => {
                self.handle_utility_action(ui, event)
            }
            UiAction::Up => {
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
    }

    fn handle_back(&self, ui: &AppWindow) {
        if self.close_overlay(ui) {
            debug!("Back closed utility overlay");
            return;
        }

        let from = self.current_route();
        self.remember_focus_for_route(from);
        if self.navigator.borrow_mut().go_back() {
            let to = self.current_route();
            debug!(?from, ?to, "Back restored previous top-level route and focus");
            self.publish_route(ui);
            self.restore_focus_for_route(ui, to);
        } else {
            debug!(route = ?from, "Back ignored at navigation root");
        }
    }

    fn handle_home(&self, ui: &AppWindow) {
        self.close_overlay(ui);

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
        self.publish_focus(ui);

        if self.navigator.borrow_mut().go_home() {
            debug!(
                ?from,
                to = ?AppRoute::Home,
                "Home reset top-level navigation and content focus"
            );
            self.publish_route(ui);
        }
    }

    fn handle_utility_action(&self, ui: &AppWindow, event: UiActionEvent) {
        match event.action {
            UiAction::Left | UiAction::Right => {
                let delta = if event.action == UiAction::Left { -1 } else { 1 };
                if self.focus.borrow_mut().move_utility(delta) {
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
            | UiAction::Menu
            | UiAction::LeftBumper
            | UiAction::RightBumper => {
                debug!(
                    action = ?event.action,
                    repeated = event.repeated,
                    "action has no top-utility behavior"
                );
            }
            UiAction::Back | UiAction::Home => unreachable!("global actions handled first"),
        }
    }

    fn activate_utility(&self, ui: &AppWindow, utility: TopUtility) {
        match utility.destination() {
            UtilityDestination::Route(route) => {
                let from = self.current_route();
                self.navigate_to(ui, route);
                if self.current_route() != from {
                    debug!(?from, to = ?route, ?utility, "utility opened top-level route");
                }
            }
            UtilityDestination::Overlay(overlay) => {
                *self.overlay.borrow_mut() = Some(overlay);
                debug!(?overlay, ?utility, "utility overlay opened");
                self.publish_overlay(ui);
            }
        }
    }

    fn close_overlay(&self, ui: &AppWindow) -> bool {
        if self.overlay.borrow().is_none() {
            return false;
        }
        *self.overlay.borrow_mut() = None;
        self.publish_overlay(ui);
        true
    }

    fn dispatch_to_active_page(&self, ui: &AppWindow, event: UiActionEvent) {
        match self.current_route() {
            AppRoute::Home => self.home.handle_action(ui, event),
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
        let snapshot = self.focus.borrow().snapshot();
        self.focus_memory.borrow_mut().remember(route, snapshot);
    }

    fn restore_focus_for_route(&self, ui: &AppWindow, route: AppRoute) {
        let snapshot = self.focus_memory.borrow().recall(route);
        self.focus.borrow_mut().restore(snapshot);
        self.publish_focus(ui);
    }

    fn publish_route(&self, ui: &AppWindow) {
        ui.set_current_route(route_view(self.current_route()));
    }

    fn publish_focus(&self, ui: &AppWindow) {
        let focus = *self.focus.borrow();
        ui.set_top_utilities_focused(focus.region() == ShellFocusRegion::TopUtilities);
        ui.set_focused_utility_index(focus.utility().index());
    }

    fn publish_overlay(&self, ui: &AppWindow) {
        let overlay = match *self.overlay.borrow() {
            None => UtilityOverlayView::Closed,
            Some(UtilityOverlay::Friends) => UtilityOverlayView::Friends,
            Some(UtilityOverlay::Album) => UtilityOverlayView::Album,
            Some(UtilityOverlay::Web) => UtilityOverlayView::Web,
        };
        ui.set_utility_overlay(overlay);
    }
}

fn route_view(route: AppRoute) -> AppRouteView {
    match route {
        AppRoute::Home => AppRouteView::Home,
        AppRoute::Library => AppRouteView::Library,
        AppRoute::Activity => AppRouteView::Activity,
        AppRoute::Settings => AppRouteView::Settings,
    }
}

#[cfg(test)]
mod tests {
    use super::route_view;
    use crate::{AppRouteView, navigation::AppRoute};

    #[test]
    fn every_rust_route_has_an_explicit_slint_view_mapping() {
        assert!(matches!(route_view(AppRoute::Home), AppRouteView::Home));
        assert!(matches!(
            route_view(AppRoute::Library),
            AppRouteView::Library
        ));
        assert!(matches!(
            route_view(AppRoute::Activity),
            AppRouteView::Activity
        ));
        assert!(matches!(
            route_view(AppRoute::Settings),
            AppRouteView::Settings
        ));
    }
}
