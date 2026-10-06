use std::rc::Rc;

use slint::ComponentHandle;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::{
    AppWindow,
    error::AppError,
    input::{ControllerStatus, InputManager, UiActionEvent},
    platform::slint_backend,
    presentation::{
        appearance::AppearanceController, clock::ClockController, home::HomeController,
        navigation::NavigationController,
    },
};

const APP_ID: &str = "io.github.Mars7x.Horizon";

pub fn run() -> Result<(), AppError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("horizon=info")),
        )
        .init();

    info!("starting Horizon phase 4.2");

    // Backend selection must happen before set_xdg_app_id(), AppWindow::new(),
    // or any other Slint operation that needs the platform.
    slint_backend::initialize()?;
    slint::set_xdg_app_id(APP_ID)?;

    let ui = AppWindow::new()?;
    let appearance = AppearanceController::new(&ui);
    let _appearance_monitor = match appearance.start_portal_monitor(&ui) {
        Ok(monitor) => Some(monitor),
        Err(error) => {
            warn!(%error, "appearance portal monitor did not start; using fallback appearance");
            None
        }
    };

    let clock = ClockController::new(&ui);
    let _clock_format_monitor = match clock.start_portal_monitor(&ui) {
        Ok(monitor) => Some(monitor),
        Err(error) => {
            warn!(%error, "clock-format portal monitor did not start; using 12-hour fallback");
            None
        }
    };

    let home = HomeController::new(&ui);
    info!(games = home.game_count(), "demo home library initialized");

    let navigation = NavigationController::new(&ui, Rc::clone(&home));
    info!(route = ?navigation.current_route(), "Phase 4.2 navigation shell initialized");

    let ui_weak = ui.as_weak();
    let action_navigation = Rc::clone(&navigation);
    let action_sink: Rc<dyn Fn(UiActionEvent)> = Rc::new(move |event| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        action_navigation.handle_action(&ui, event);
    });

    let status_ui = ui.as_weak();
    let status_sink: Rc<dyn Fn(ControllerStatus)> = Rc::new(move |status| {
        let Some(ui) = status_ui.upgrade() else {
            return;
        };
        ui.set_connected_controller_count(status.connected_gamepads as i32);
    });

    let input = InputManager::new(action_sink, status_sink);
    let keyboard_input = Rc::clone(&input);
    ui.on_raw_key_input(move |text, repeated| {
        keyboard_input.handle_keyboard(text.as_str(), repeated)
    });

    // Keep timers/input managers alive for the full UI event loop.
    let _clock = clock;
    let _navigation = navigation;
    let _input = input;

    ui.run()?;

    Ok(())
}
