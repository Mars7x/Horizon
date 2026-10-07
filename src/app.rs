use std::rc::Rc;

use slint::ComponentHandle;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::{
    AppWindow,
    error::AppError,
    input::{ControllerStatus, InputManager, UiActionEvent},
    persistence::SqliteLibraryRepository,
    platform::{data_paths, launcher::PortalLaunchExecutor, slint_backend},
    presentation::{
        appearance::AppearanceController, clock::ClockController, home::HomeController,
        navigation::NavigationController,
    },
    services::{
        import::{SourceImportOutcome, SourceImportService},
        launch::GameLaunchService,
        library::LibraryService,
    },
    sources::{SourceRegistry, steam::SteamSource},
};

const APP_ID: &str = "io.github.Mars7x.Horizon";

pub fn run() -> Result<(), AppError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("horizon=info")),
        )
        .init();

    info!("starting Horizon phase 7.0.5");

    let database_path = data_paths::library_database_path()?;
    let repository = SqliteLibraryRepository::open(&database_path)?;
    let schema_version = repository.schema_version()?;
    let mut library = LibraryService::new(repository);
    info!(
        path = %database_path.display(),
        schema_version,
        games = library.game_count()?,
        "persistent library initialized"
    );

    let mut registry = SourceRegistry::new();
    registry.register(SteamSource::new()?)?;

    let import_summary = SourceImportService::import_all(&registry, &mut library)?;
    for report in import_summary.reports() {
        match report.outcome() {
            SourceImportOutcome::Imported { discovered } => {
                info!(
                    source = %report.source_id(),
                    discovered,
                    "source discovery imported"
                );
            }
            SourceImportOutcome::Unavailable(reason) => {
                info!(
                    source = %report.source_id(),
                    ?reason,
                    "optional game source is unavailable"
                );
            }
            SourceImportOutcome::Failed(error) => {
                warn!(
                    source = %report.source_id(),
                    %error,
                    "game source discovery failed; continuing with persisted library"
                );
            }
        }
    }

    let library_games = library.games()?;
    info!(games = library_games.len(), "source-backed library ready");
    if library_games.is_empty() {
        warn!("no source-backed games are currently available; Home will show its empty state");
    }

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

    let registry = Rc::new(registry);
    let launch_service = Rc::new(GameLaunchService::new(
        Rc::clone(&registry),
        Rc::new(PortalLaunchExecutor),
    ));
    let home = HomeController::new(&ui, library_games, launch_service);
    info!(games = home.game_count(), "source-backed Home library initialized");

    let navigation = NavigationController::new(&ui, Rc::clone(&home));
    info!(
        route = ?navigation.current_route(),
        "Phase 4.7 hardened navigation shell initialized"
    );

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

    // SDL gamepad events are process-global, unlike keyboard events delivered
    // by the focused Slint window. Bind semantic controller ownership to the
    // window activation signal so a foreground game cannot also navigate the
    // background Horizon shell.
    let activation_input = Rc::clone(&input);
    let activation_home = Rc::clone(&home);
    let activation_ui = ui.as_weak();
    ui.on_application_active_changed(move |active| {
        activation_input.set_ui_input_enabled(active);
        if let Some(ui) = activation_ui.upgrade() {
            activation_home.handle_application_active_changed(&ui, active);
        }
    });

    let keyboard_input = Rc::clone(&input);
    ui.on_raw_key_input(move |text, repeated| {
        keyboard_input.handle_keyboard(text.as_str(), repeated)
    });

    // Keep services/timers/input managers alive for the full UI event loop.
    let _library = library;
    let _source_registry = registry;
    let _clock = clock;
    let _navigation = navigation;
    let _input = input;

    ui.run()?;

    Ok(())
}
