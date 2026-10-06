use std::{cell::RefCell, rc::Rc};

use tracing::debug;

use crate::{
    AppRouteView, AppWindow,
    input::{UiAction, UiActionEvent},
    navigation::{AppRoute, Navigator},
};

use super::home::HomeController;

/// Bridges pure Rust navigation state to Slint presentation state.
///
/// Device adapters remain screen-agnostic. They emit `UiActionEvent`; this
/// controller decides whether an action is global navigation or belongs to the
/// active page controller.
pub struct NavigationController {
    navigator: RefCell<Navigator>,
    home: Rc<HomeController>,
}

impl NavigationController {
    pub fn new(ui: &AppWindow, home: Rc<HomeController>) -> Rc<Self> {
        let controller = Rc::new(Self {
            navigator: RefCell::new(Navigator::default()),
            home,
        });
        controller.publish_route(ui);
        controller
    }

    pub fn current_route(&self) -> AppRoute {
        self.navigator.borrow().current()
    }

    /// Entry point for future page/header callbacks. Phase 4.1 establishes the
    /// route model and publication boundary; Phase 4.2 will render each route.
    pub fn navigate_to(&self, ui: &AppWindow, route: AppRoute) {
        let from = self.current_route();
        if self.navigator.borrow_mut().navigate_to(route) {
            debug!(?from, to = ?route, "top-level route changed");
            self.publish_route(ui);
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
            UiAction::Back => {
                let from = self.current_route();
                if self.navigator.borrow_mut().go_back() {
                    let to = self.current_route();
                    debug!(?from, ?to, "Back restored previous top-level route");
                    self.publish_route(ui);
                } else {
                    debug!(route = ?from, "Back ignored at navigation root");
                }
            }
            UiAction::Home => {
                let from = self.current_route();
                if self.navigator.borrow_mut().go_home() {
                    debug!(?from, to = ?AppRoute::Home, "Home reset top-level navigation");
                    self.publish_route(ui);
                }
            }
            _ => self.dispatch_to_active_page(ui, event),
        }
    }

    fn dispatch_to_active_page(&self, ui: &AppWindow, event: UiActionEvent) {
        match self.current_route() {
            AppRoute::Home => self.home.handle_action(ui, event),
            route => {
                debug!(
                    ?route,
                    action = ?event.action,
                    repeated = event.repeated,
                    "page action deferred until its Phase 4 view exists"
                );
            }
        }
    }

    fn publish_route(&self, ui: &AppWindow) {
        ui.set_current_route(route_view(self.current_route()));
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
