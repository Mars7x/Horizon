use std::{cell::RefCell, rc::Rc, time::Duration};

use slint::{ComponentHandle, Timer, TimerMode};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::{
    AppWindow,
    error::AppError,
    input::{ControllerStatus, InputManager, UiActionEvent},
    persistence::SqliteLibraryRepository,
    platform::{
        data_paths,
        launcher::PortalLaunchExecutor,
        session_helper::DbusManagedSessionExecutor,
        slint_backend,
    },
    presentation::{
        activity::ActivityController, appearance::AppearanceController, clock::ClockController,
        home::HomeController, navigation::NavigationController,
    },
    services::{
        activity::{ActivityService, ActivitySessionTransition, LaunchActivitySink},
        artwork::ArtworkService,
        import::{SourceImportOutcome, SourceImportService},
        launch::GameLaunchService,
        library::LibraryService,
        runtime::{RuntimeObservationExecutor, SourceRuntimeObservationExecutor},
        session::{ManagedSessionExecutor, ManagedSessionTerminalState},
    },
    sources::production_source_registry,
};

const APP_ID: &str = "io.github.Mars7x.Horizon";

pub fn run() -> Result<(), AppError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("horizon=info")),
        )
        .init();

    info!("starting Horizon phase 9.5.34");

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

    let registry = production_source_registry()?;

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

    // Discovery owns the repository exclusively during startup. After the
    // import pass is complete, share that same repository with the runtime
    // Activity service so library and play-history writes stay in one database
    // connection without leaking SQLite into presentation code.
    let runtime_repository = Rc::new(RefCell::new(library.into_repository()));
    let activity_service = Rc::new(ActivityService::new(Rc::clone(&runtime_repository)));
    let interrupted_sessions = activity_service.recover_interrupted_sessions()?;
    if interrupted_sessions > 0 {
        warn!(
            interrupted_sessions,
            "recovered unfinished activity sessions through their last durable checkpoint"
        );
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

    let activity_overview = activity_service.overview(
        ActivityController::recent_session_limit(),
        ActivityController::top_game_limit(),
    )?;
    ActivityController::publish(&ui, &activity_overview);
    info!(
        observed_seconds = activity_overview.observed_playtime().get(),
        sessions = activity_overview.completed_sessions(),
        "persisted Activity overview initialized"
    );

    let registry = Rc::new(registry);
    let artwork_service = ArtworkService::new(Rc::clone(&registry));
    let mut launch_service = GameLaunchService::new(
        Rc::clone(&registry),
        Rc::new(PortalLaunchExecutor),
    );
    match DbusManagedSessionExecutor::new() {
        Ok(executor) => {
            let available = executor.probe();
            info!(
                available,
                "optional managed-session host helper probe completed"
            );
            let executor: Rc<dyn ManagedSessionExecutor> = Rc::new(executor);
            launch_service = launch_service.with_managed_executor(executor);
        }
        Err(error) => {
            warn!(
                %error,
                "managed-session D-Bus client could not initialize; normal launch remains available"
            );
        }
    }
    // Source-runtime observation must work in the ordinary Flatpak without the
    // optional host helper. Provider adapters expose only provider-owned state
    // that is already readable through Horizon's narrow filesystem grants.
    // Steam uses its local gameprocess_log.txt rather than /proc.
    let runtime_observer: Rc<dyn RuntimeObservationExecutor> = Rc::new(
        SourceRuntimeObservationExecutor::new(Rc::clone(&registry)),
    );
    launch_service = launch_service.with_runtime_observer(runtime_observer);
    info!("in-process source-runtime observation initialized");
    let launch_service = Rc::new(launch_service);
    let launch_activity: Rc<dyn LaunchActivitySink> = activity_service.clone();
    let home = HomeController::new(
        &ui,
        library_games,
        &artwork_service,
        Rc::clone(&launch_service),
        launch_activity,
    );
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
    let activation_activity = Rc::clone(&activity_service);
    let activation_ui = ui.as_weak();
    ui.on_application_active_changed(move |active| {
        activation_input.set_ui_input_enabled(active);

        // Activity observes the same OS activation boundary that owns
        // controller input. Handle the activity transition before Home clears
        // its launch feedback so a pending launch can become a real observed
        // foreground session on deactivation.
        let activity_transition =
            match activation_activity.handle_application_active_changed(active) {
                Ok(transition) => transition,
                Err(error) => {
                    warn!(%error, active, "activity session transition could not be persisted");
                    ActivitySessionTransition::None
                }
            };

        if let Some(ui) = activation_ui.upgrade() {
            activation_home.handle_application_active_changed(&ui, active);

            if matches!(activity_transition, ActivitySessionTransition::Completed(_)) {
                match activation_activity.overview(
                    ActivityController::recent_session_limit(),
                    ActivityController::top_game_limit(),
                ) {
                    Ok(overview) => ActivityController::publish(&ui, &overview),
                    Err(error) => warn!(%error, "Activity overview could not be refreshed"),
                }
            }
        }
    });

    // External URI launchers (Steam/Heroic) can bounce activation back
    // to Horizon briefly during startup. ActivityService treats that as a
    // candidate return; confirm it only after the candidate remains stable.
    let foreground_return_timer = Timer::default();
    let foreground_activity = Rc::clone(&activity_service);
    let foreground_ui = ui.as_weak();
    foreground_return_timer.start(
        TimerMode::Repeated,
        Duration::from_millis(250),
        move || match foreground_activity.poll_foreground_return() {
            Ok(ActivitySessionTransition::Completed(_)) => {
                let Some(ui) = foreground_ui.upgrade() else {
                    return;
                };
                match foreground_activity.overview(
                    ActivityController::recent_session_limit(),
                    ActivityController::top_game_limit(),
                ) {
                    Ok(overview) => ActivityController::publish(&ui, &overview),
                    Err(error) => warn!(%error, "Activity overview could not be refreshed"),
                }
            }
            Ok(_) => {}
            Err(error) => {
                warn!(%error, "foreground activity return could not be confirmed");
            }
        },
    );

    // Source-owned runtime observation is independent of Horizon window focus.
    // Steam derives exact AppID-scoped running-list transitions from its local
    // gameprocess_log.txt, so this works in the normal Flatpak without a helper.
    let runtime_observation_timer = Timer::default();
    let runtime_launch_service = Rc::clone(&launch_service);
    let runtime_activity = Rc::clone(&activity_service);
    let runtime_ui = ui.as_weak();
    runtime_observation_timer.start(TimerMode::Repeated, Duration::from_millis(500), move || {
        let events = runtime_launch_service.poll_runtime_observations();
        if events.is_empty() {
            return;
        }

        let mut refresh_activity = false;
        for event in events {
            match runtime_activity.handle_runtime_observation_event(&event) {
                Ok(transition) => refresh_activity |= transition.changed(),
                Err(error) => {
                    warn!(%error, ?event, "source-runtime activity transition could not be persisted");
                }
            }
        }

        if refresh_activity
            && let Some(ui) = runtime_ui.upgrade()
        {
            match runtime_activity.overview(
                ActivityController::recent_session_limit(),
                ActivityController::top_game_limit(),
            ) {
                Ok(overview) => ActivityController::publish(&ui, &overview),
                Err(error) => warn!(%error, "Activity overview could not be refreshed"),
            }
        }
    });

    // Keep open sessions visibly ticking in Activity without committing a
    // synthetic end timestamp. The repository exposes open rows separately;
    // completed totals remain durable history until a real terminal event.
    let live_activity_timer = Timer::default();
    let live_activity = Rc::clone(&activity_service);
    let live_activity_ui = ui.as_weak();
    live_activity_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
        if !live_activity.has_live_session() {
            return;
        }

        let Some(ui) = live_activity_ui.upgrade() else {
            return;
        };
        match live_activity.overview(
            ActivityController::recent_session_limit(),
            ActivityController::top_game_limit(),
        ) {
            Ok(overview) => ActivityController::publish(&ui, &overview),
            Err(error) => warn!(%error, "live Activity overview could not be refreshed"),
        }
    });

    // Persist a conservative observation checkpoint for every live session.
    // A hard process/system loss can then recover confirmed playtime through
    // the last committed checkpoint instead of discarding the entire session.
    // The terminal lifecycle signal still owns the exact end of clean sessions.
    let activity_checkpoint_timer = Timer::default();
    let checkpoint_activity = Rc::clone(&activity_service);
    activity_checkpoint_timer.start(TimerMode::Repeated, Duration::from_secs(5), move || {
        if !checkpoint_activity.has_live_session() {
            return;
        }
        if let Err(error) = checkpoint_activity.checkpoint_live_sessions() {
            warn!(%error, "live Activity checkpoint could not be persisted");
        }
    });

    let managed_session_timer = Timer::default();
    let managed_launch_service = Rc::clone(&launch_service);
    let managed_activity = Rc::clone(&activity_service);
    let managed_home = Rc::clone(&home);
    let managed_ui = ui.as_weak();
    managed_session_timer.start(TimerMode::Repeated, Duration::from_millis(500), move || {
        let completions = managed_launch_service.poll_managed_sessions();
        if completions.is_empty() {
            return;
        }

        let Some(ui) = managed_ui.upgrade() else {
            return;
        };
        let mut refresh_activity = false;

        for completion in completions {
            let failed = !matches!(
                completion.terminal(),
                ManagedSessionTerminalState::Exited { .. }
            );
            managed_home.handle_managed_session_completion(
                &ui,
                completion.session_id(),
                failed,
            );

            match managed_activity.handle_managed_session_terminal(
                completion.session_id(),
                completion.terminal(),
            ) {
                Ok(transition) => {
                    refresh_activity |= transition.changed();
                }
                Err(error) => {
                    warn!(
                        session_id = completion.session_id().get(),
                        %error,
                        "managed activity session transition could not be persisted"
                    );
                }
            }
        }

        if refresh_activity {
            match managed_activity.overview(
                ActivityController::recent_session_limit(),
                ActivityController::top_game_limit(),
            ) {
                Ok(overview) => ActivityController::publish(&ui, &overview),
                Err(error) => warn!(%error, "Activity overview could not be refreshed"),
            }
        }
    });

    let keyboard_input = Rc::clone(&input);
    ui.on_raw_key_input(move |text, repeated| {
        keyboard_input.handle_keyboard(text.as_str(), repeated)
    });

    // Keep services/timers/input managers alive for the full UI event loop.
    let _runtime_repository = runtime_repository;
    let _activity_service = activity_service;
    let _source_registry = registry;
    let _clock = clock;
    let _navigation = navigation;
    let _input = input;
    let _foreground_return_timer = foreground_return_timer;
    let _runtime_observation_timer = runtime_observation_timer;
    let _live_activity_timer = live_activity_timer;
    let _activity_checkpoint_timer = activity_checkpoint_timer;
    let _managed_session_timer = managed_session_timer;

    ui.run()?;

    Ok(())
}
