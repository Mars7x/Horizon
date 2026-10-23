use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::Receiver,
    },
    time::Duration,
};

use slint::{
    Color, ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString,
    Timer, VecModel,
};
use tracing::{debug, warn};

use crate::{
    AppWindow, GameCardData,
    audio::UiSoundCue,
    domain::{GameId, LibraryGame},
    input::{UiAction, UiActionEvent},
    navigation::step_with_edge_wrap,
    presentation::{
        CallbackSlot,
        catalog::{Catalog, CatalogChanges, SharedCatalog},
        wake::drain_notify,
    },
    services::{
        activity::LaunchActivitySink,
        artwork::{ArtworkService, SquareArtwork},
        launch::{GameLaunchMode, GameLaunchService},
        runtime::{RuntimeObservationEvent, RuntimeObservationId},
        settings::ArtworkPreferences,
        steamgriddb::SteamGridDbLookup,
        steamgriddb_artwork::{self, ArtworkLookupJob, ArtworkWorkerEvent},
        worker::{WorkerSender, worker_channel},
    },
};

#[derive(Debug, Default)]
struct HomeState {
    selected_index: i32,
}

impl HomeState {
    fn select(&mut self, requested_index: i32, game_count: usize) -> i32 {
        if game_count == 0 {
            self.selected_index = 0;
            return 0;
        }

        let max_index = game_count.saturating_sub(1) as i32;
        self.selected_index = requested_index.clamp(0, max_index);
        self.selected_index
    }

    fn reset_for_global_home(&mut self, game_count: usize) -> i32 {
        self.select(0, game_count)
    }

    fn move_by(&mut self, delta: i32, game_count: usize, allow_wrap: bool) -> i32 {
        if game_count == 0 {
            self.selected_index = 0;
            return 0;
        }

        let max_index = game_count.saturating_sub(1) as i32;
        self.selected_index =
            step_with_edge_wrap(self.selected_index, 0, max_index, delta, !allow_wrap);

        self.selected_index
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LaunchFeedbackState {
    #[default]
    Idle,
    Launching {
        index: i32,
    },
    // Dispatch is not a confirmed running process. Keep this transient state
    // separate from the per-card runtime lifecycle badge.
    Dispatched,
    Failed,
}

impl LaunchFeedbackState {
    fn handoff_pending(self) -> bool {
        matches!(self, Self::Launching { .. } | Self::Dispatched)
    }

    fn status_text(self) -> &'static str {
        match self {
            Self::Failed => "Launch failed",
            Self::Idle | Self::Launching { .. } | Self::Dispatched => "",
        }
    }
}

// Runtime start/stop events own the Playing label, not window activation,
// title selection, launch success, or the visual press timer. Keeping the
// observation identity also handles concurrent games and overlapping launches.
#[derive(Debug, Default)]
struct PlayingState {
    pending_runtime: BTreeMap<RuntimeObservationId, i32>,
    running_runtime: BTreeMap<RuntimeObservationId, i32>,
}

impl PlayingState {
    fn arm_runtime(&mut self, observation_id: RuntimeObservationId, index: i32) {
        self.pending_runtime.insert(observation_id, index);
    }

    fn runtime_event(&mut self, event: &RuntimeObservationEvent) -> Option<i32> {
        match event {
            RuntimeObservationEvent::Started { observation_id, .. } => {
                let index = self.pending_runtime.remove(observation_id)?;
                self.running_runtime.insert(*observation_id, index);
                Some(index)
            }
            RuntimeObservationEvent::Terminal { observation_id, .. } => self
                .pending_runtime
                .remove(observation_id)
                .or_else(|| self.running_runtime.remove(observation_id)),
        }
    }

    fn is_playing(&self, index: i32) -> bool {
        self.running_runtime
            .values()
            .any(|running| *running == index)
    }
}

/// Owns Home presentation state for the durable source-backed library.
///
/// Slint receives only card presentation data. `LibraryGame` identity stays in
/// Rust so Accept can launch through the generic service/source boundary.
pub struct HomeController {
    cards: Rc<VecModel<GameCardData>>,
    home_cards: Rc<VecModel<GameCardData>>,
    /// Where each Home card was before the row last changed (-1: new to the
    /// row), so the carousel can glide them there (`home-from-indices`).
    home_from: Rc<VecModel<i32>>,
    /// The row as it was last seen on screen. A change made while Home is
    /// hidden (a library reload) plays when Home is next shown.
    seen_order: RefCell<Vec<usize>>,
    home_order: RefCell<Vec<usize>>,
    /// Provider-owned fallbacks, never replaced by downloaded art (a library
    /// reload adds or rereads them).
    source_cards: RefCell<Vec<GameCardData>>,
    artwork_lookup_jobs: RefCell<Vec<ArtworkLookupJob>>,
    artwork_cache_root: Option<PathBuf>,
    artwork_sender: WorkerSender<ArtworkWorkerEvent>,
    artwork_receiver: RefCell<Receiver<ArtworkWorkerEvent>>,
    artwork_generation: Arc<AtomicU64>,
    catalog: SharedCatalog,
    titles: RefCell<Vec<SharedString>>,
    launch_service: Rc<GameLaunchService>,
    launch_activity: Rc<dyn LaunchActivitySink>,
    state: RefCell<HomeState>,
    launch_feedback: RefCell<LaunchFeedbackState>,
    playing: RefCell<PlayingState>,
    // Invalidates an old press-release callback on failure, navigation or a
    // subsequent launch; the visual press is independent of session lifetime.
    press_generation: Rc<Cell<u64>>,
    card_changed: CallbackSlot<dyn Fn(usize, GameCardData)>,
    /// UI sounds for clicks (a controller's are played by navigation).
    sound: CallbackSlot<dyn Fn(UiSoundCue)>,
}

impl HomeController {
    pub fn new(
        ui: &AppWindow,
        catalog: SharedCatalog,
        recent_game_ids: &[GameId],
        artwork_service: &ArtworkService,
        registry: &crate::sources::SourceRegistry,
        launch_service: Rc<GameLaunchService>,
        launch_activity: Rc<dyn LaunchActivitySink>,
    ) -> Rc<Self> {
        // A fresh catalog: every slot is present.
        let library_games = catalog
            .borrow()
            .iter()
            .map(|(_, game)| game.clone())
            .collect::<Vec<_>>();
        let card_data = library_games
            .iter()
            .zip(artwork_service.square_artworks(&library_games))
            .map(|(game, artwork)| game_card(game, artwork, registry))
            .collect::<Vec<_>>();
        let titles = card_data.iter().map(|game| game.title.clone()).collect();
        let artwork_lookup_jobs = library_games
            .iter()
            .enumerate()
            .map(|(index, game)| lookup_job(index, game, registry))
            .collect();
        // Artwork results wake the UI thread; nothing polls for them.
        let weak_ui = ui.as_weak();
        let (artwork_notify, artwork_drain) = drain_notify(move |home: &Self| {
            if let Some(ui) = weak_ui.upgrade() {
                home.collect_steamgriddb_results(&ui);
            }
        });
        let (artwork_sender, artwork_receiver) = worker_channel(artwork_notify);
        let card_data_for_fallback = card_data.clone();
        let cards = Rc::new(VecModel::from(card_data));
        let home_order = ordered_home_indices(&catalog.borrow(), recent_game_ids);
        let home_cards = Rc::new(VecModel::from(
            home_order
                .iter()
                .filter_map(|index| cards.row_data(*index))
                .collect::<Vec<_>>(),
        ));
        ui.set_games(ModelRc::from(Rc::clone(&home_cards)));
        let home_from = Rc::new(VecModel::from(vec![-1; home_cards.row_count()]));
        ui.set_home_from_indices(ModelRc::from(Rc::clone(&home_from)));

        let controller = Rc::new(Self {
            cards,
            home_cards,
            home_from,
            seen_order: RefCell::new(home_order.clone()),
            home_order: RefCell::new(home_order),
            source_cards: RefCell::new(card_data_for_fallback),
            artwork_lookup_jobs: RefCell::new(artwork_lookup_jobs),
            artwork_cache_root: artwork_service.steamgriddb_cache_root(),
            artwork_sender,
            artwork_receiver: RefCell::new(artwork_receiver),
            artwork_generation: Arc::new(AtomicU64::new(0)),
            catalog,
            titles: RefCell::new(titles),
            launch_service,
            launch_activity,
            state: RefCell::new(HomeState::default()),
            launch_feedback: RefCell::new(LaunchFeedbackState::default()),
            playing: RefCell::new(PlayingState::default()),
            press_generation: Rc::new(Cell::new(0)),
            card_changed: RefCell::new(None),
            sound: RefCell::new(None),
        });

        artwork_drain.bind(&controller);
        controller.select_index(ui, 0);

        let ui_weak = ui.as_weak();
        let callback_controller = Rc::clone(&controller);
        ui.on_select_game(move |requested_index| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };

            callback_controller.activate_from_pointer(&ui, requested_index);
        });

        controller
    }

    /// Reapply persisted precedence immediately and schedule only eligible
    /// external lookups off-thread. Changing key/preference invalidates old work.
    pub fn refresh_steamgriddb(&self, preferences: ArtworkPreferences) {
        self.update_steamgriddb(preferences, false);
    }

    /// Manual refresh deliberately re-queries the API even for cached images
    /// and previous misses. Existing cards remain visible until new results.
    pub fn force_refresh_steamgriddb(&self, preferences: ArtworkPreferences) {
        self.update_steamgriddb(preferences, true);
    }

    fn update_steamgriddb(&self, preferences: ArtworkPreferences, force_refresh: bool) {
        if !force_refresh {
            let source_cards = self.source_cards.borrow().clone();
            for (index, card) in source_cards.into_iter().enumerate() {
                self.publish_card(index, card);
            }
        }
        self.start_steamgriddb(preferences, force_refresh);
    }

    /// Look up every game that needs a cover, superseding any earlier pass.
    /// Covers already shown stay until a new one arrives.
    fn start_steamgriddb(&self, preferences: ArtworkPreferences, force_refresh: bool) {
        let generation = self.artwork_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let Some(api_key) = preferences.api_key else {
            return;
        };
        let catalog = self.catalog.borrow();
        let source_cards = self.source_cards.borrow();
        let jobs = self
            .artwork_lookup_jobs
            .borrow()
            .iter()
            .filter(|job| {
                catalog.get(job.index).is_some()
                    && needs_external_artwork(
                        preferences.prefer_steamgriddb,
                        source_cards[job.index].has_artwork,
                    )
            })
            .cloned()
            .collect();
        drop((catalog, source_cards));
        steamgriddb_artwork::start_artwork_worker(
            generation,
            Arc::clone(&self.artwork_generation),
            self.artwork_sender.clone(),
            api_key,
            self.artwork_cache_root.clone(),
            jobs,
            force_refresh,
        );
    }

    /// Drained on the UI thread, where VecModel updates belong, whenever the
    /// worker sends. The original source card is always available.
    fn collect_steamgriddb_results(&self, ui: &AppWindow) {
        loop {
            let Ok(event) = self.artwork_receiver.borrow().try_recv() else {
                break;
            };
            match event {
                ArtworkWorkerEvent::Artwork(result) => {
                    if result.generation != self.artwork_generation.load(Ordering::Acquire) {
                        continue;
                    }
                    let Some(mut card) = self.source_cards.borrow().get(result.index).cloned()
                    else {
                        continue;
                    };
                    card.pixelated_artwork = result.artwork.pixelated();
                    card.artwork = square_artwork_to_slint(result.artwork);
                    card.has_artwork = true;
                    self.publish_card(result.index, card);
                }
                ArtworkWorkerEvent::RefreshProgress {
                    generation,
                    completed,
                    total,
                    finished,
                    error,
                } => {
                    if generation != self.artwork_generation.load(Ordering::Acquire) {
                        continue;
                    }
                    ui.set_settings_refresh_completed(completed.min(i32::MAX as usize) as i32);
                    ui.set_settings_refresh_total(total.min(i32::MAX as usize) as i32);
                    ui.set_settings_refresh_error(error.unwrap_or_default().into());
                    if finished {
                        ui.set_settings_refresh_running(false);
                    }
                }
                ArtworkWorkerEvent::Status {
                    generation,
                    message,
                } => {
                    if generation == self.artwork_generation.load(Ordering::Acquire) {
                        ui.set_settings_artwork_status(message.into());
                    }
                }
            }
        }
    }

    pub fn refresh_recent_games(&self, ui: &AppWindow, ids: &[GameId]) {
        let order = ordered_home_indices(&self.catalog.borrow(), ids);
        if *self.home_order.borrow() == order {
            return;
        }
        self.show_home_order(ui, order);
    }

    /// The shared catalog was reloaded: make cards for new games, reread
    /// changed ones, show the new Home row, and look up covers again (shown
    /// covers stay; cached ones come straight back).
    pub fn catalog_changed(
        &self,
        ui: &AppWindow,
        changes: &CatalogChanges,
        artwork_service: &ArtworkService,
        registry: &crate::sources::SourceRegistry,
        preferences: ArtworkPreferences,
        recent_game_ids: &[GameId],
    ) {
        let reread = changes
            .added
            .iter()
            .chain(&changes.updated)
            .copied()
            .collect::<Vec<_>>();
        let games = {
            let catalog = self.catalog.borrow();
            reread
                .iter()
                .map(|index| catalog[*index].clone())
                .collect::<Vec<_>>()
        };
        let artworks = artwork_service.square_artworks(&games);
        for ((index, game), artwork) in reread.into_iter().zip(&games).zip(artworks) {
            let card = game_card(game, artwork, registry);
            let job = lookup_job(index, game, registry);
            if index < self.cards.row_count() {
                self.source_cards.borrow_mut()[index] = card.clone();
                self.titles.borrow_mut()[index] = card.title.clone();
                self.artwork_lookup_jobs.borrow_mut()[index] = job;
                self.publish_card(index, card);
            } else {
                // Added slots arrive in order, at the end.
                self.source_cards.borrow_mut().push(card.clone());
                self.titles.borrow_mut().push(card.title.clone());
                self.artwork_lookup_jobs.borrow_mut().push(job);
                self.cards.push(card);
            }
        }

        let order = ordered_home_indices(&self.catalog.borrow(), recent_game_ids);
        if *self.home_order.borrow() != order {
            self.show_home_order(ui, order);
        } else {
            self.publish_selection(ui, self.state.borrow().selected_index);
        }
        if !changes.added.is_empty() || !changes.updated.is_empty() {
            self.start_steamgriddb(preferences, false);
        }
    }

    /// Show `order`, keeping the selected game selected when it's still there.
    fn show_home_order(&self, ui: &AppWindow, order: Vec<usize>) {
        let previous = self.state.borrow().selected_index as usize;
        let previous_catalog_index = self.home_order.borrow().get(previous).copied();
        let next_selection = previous_catalog_index
            .and_then(|index| order.iter().position(|item| *item == index))
            .unwrap_or_else(|| {
                if previous == self.home_order.borrow().len() {
                    order.len()
                } else {
                    0
                }
            });
        // The old places go in before the cards, so the first frame of the
        // new row already glides (and a new game fades in at the front).
        self.home_from
            .set_vec(home_from_indices(&self.seen_order.borrow(), &order));
        self.home_cards.set_vec(
            order
                .iter()
                .filter_map(|index| self.cards.row_data(*index))
                .collect::<Vec<_>>(),
        );
        *self.home_order.borrow_mut() = order;
        if ui.get_current_route() == crate::AppRouteView::Home {
            self.play_reorder(ui, false);
        }
        self.state
            .borrow_mut()
            .select(next_selection as i32, self.home_item_count());
        self.publish_selection(ui, self.state.borrow().selected_index);
    }

    /// Home is being shown again: play what changed in the row meanwhile,
    /// once the page has arrived.
    pub fn reveal_row_changes(&self, ui: &AppWindow) {
        if *self.seen_order.borrow() != *self.home_order.borrow() {
            self.play_reorder(ui, true);
        }
    }

    /// Glide the row from how it was last seen to how it is now.
    fn play_reorder(&self, ui: &AppWindow, after_page_transition: bool) {
        let order = self.home_order.borrow().clone();
        self.home_from
            .set_vec(home_from_indices(&self.seen_order.borrow(), &order));
        ui.set_home_previous_count(self.seen_order.borrow().len() as i32);
        ui.set_home_reorder_delayed(after_page_transition);
        ui.set_home_reorder_sequence(ui.get_home_reorder_sequence().wrapping_add(1));
        *self.seen_order.borrow_mut() = order;
    }

    fn home_item_count(&self) -> usize {
        self.home_order.borrow().len() + 1
    }

    pub fn library_tile_selected(&self) -> bool {
        self.state.borrow().selected_index as usize == self.home_order.borrow().len()
    }

    pub fn card_at(&self, index: usize) -> Option<GameCardData> {
        self.cards.row_data(index)
    }

    pub fn set_card_changed(&self, listener: Rc<dyn Fn(usize, GameCardData)>) {
        *self.card_changed.borrow_mut() = Some(listener);
    }

    pub fn set_sound(&self, callback: Rc<dyn Fn(UiSoundCue)>) {
        *self.sound.borrow_mut() = Some(callback);
    }

    pub fn launch_from_library(&self, ui: &AppWindow, index: usize) {
        self.launch_index(ui, index as i32);
    }

    pub fn game_count(&self) -> usize {
        self.home_item_count()
    }

    pub fn selected_index(&self) -> i32 {
        self.state.borrow().selected_index
    }

    /// The selected card's game, if it is a game (not the Library tile).
    pub fn selected_game_id(&self) -> Option<i64> {
        let selected = usize::try_from(self.state.borrow().selected_index).ok()?;
        let index = *self.home_order.borrow().get(selected)?;
        Some(self.catalog.borrow().get(index)?.game().id().get())
    }

    /// Reset Home to its canonical global-Home destination: the first game.
    pub fn reset_for_global_home(&self, ui: &AppWindow) {
        self.clear_launch_feedback(ui);
        let selected_index = self
            .state
            .borrow_mut()
            .reset_for_global_home(self.home_item_count());
        self.publish_selection(ui, selected_index);
    }

    pub fn activate_from_pointer(&self, ui: &AppWindow, index: i32) {
        if ui.get_current_route() != crate::AppRouteView::Home
            || index < 0
            || index as usize >= self.home_order.borrow().len()
        {
            return;
        }
        self.select_index(ui, index);
        self.launch_selected(ui);
    }

    /// Select a game from shell-level spatial focus transfer. Pointer and page
    /// navigation still converge on the same Rust-owned selection state.
    pub fn select_from_shell(&self, ui: &AppWindow, requested_index: i32) {
        self.select_index(ui, requested_index);
    }

    /// Window activation ends only temporary launch handoff feedback, never
    /// source-verified Playing state.
    pub fn handle_application_active_changed(&self, ui: &AppWindow, active: bool) {
        if !active {
            self.clear_launch_feedback(ui);
        }
    }

    /// Consume the same lifecycle events used by Activity, including terminal
    /// errors/loss. The UI does not infer running status from focus or a URI.
    pub fn handle_runtime_observation_event(&self, event: &RuntimeObservationEvent) {
        let changed_index = self.playing.borrow_mut().runtime_event(event);
        if let Some(index) = changed_index {
            self.refresh_playing_card(index);
        }
    }

    fn publish_card(&self, index: usize, mut card: GameCardData) {
        card.is_playing = self.playing.borrow().is_playing(index as i32);
        self.cards.set_row_data(index, card.clone());
        if let Some(home_index) = self
            .home_order
            .borrow()
            .iter()
            .position(|item| *item == index)
        {
            self.home_cards.set_row_data(home_index, card.clone());
        }
        if let Some(listener) = self.card_changed.borrow().as_ref() {
            listener(index, card);
        }
    }

    fn refresh_playing_card(&self, index: i32) {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        if let Some(mut card) = self.cards.row_data(index) {
            card.is_playing = self.playing.borrow().is_playing(index as i32);
            self.cards.set_row_data(index, card.clone());
            if let Some(home_index) = self
                .home_order
                .borrow()
                .iter()
                .position(|item| *item == index)
            {
                self.home_cards.set_row_data(home_index, card.clone());
            }
            if let Some(listener) = self.card_changed.borrow().as_ref() {
                listener(index, card);
            }
        }
    }

    pub fn handle_action(&self, ui: &AppWindow, event: UiActionEvent) {
        if self.launch_feedback.borrow().handoff_pending()
            && matches!(
                event.action,
                UiAction::Left | UiAction::Right | UiAction::Accept
            )
        {
            debug!(
                action = ?event.action,
                "Home carousel input ignored while launch handoff is pending"
            );
            return;
        }

        match event.action {
            UiAction::Left => self.move_selection(ui, -1, !event.repeated),
            UiAction::Right => self.move_selection(ui, 1, !event.repeated),
            UiAction::Accept if !event.repeated => self.launch_selected(ui),
            UiAction::Accept => {}
            UiAction::Up
            | UiAction::Down
            | UiAction::Back
            | UiAction::Menu
            | UiAction::Home
            | UiAction::LeftBumper
            | UiAction::RightBumper
            | UiAction::Secondary
            | UiAction::AlternateSecondary
            | UiAction::StickPress
            | UiAction::RightStickPress
            | UiAction::Forward => {
                debug!(action = ?event.action, repeated = event.repeated, "UI action has no Home-local behavior");
            }
        }
    }

    fn launch_selected(&self, ui: &AppWindow) {
        let index = self.state.borrow().selected_index;
        if let Some(catalog_index) = self.home_order.borrow().get(index as usize).copied() {
            self.launch_index(ui, catalog_index as i32);
        }
    }

    fn launch_index(&self, ui: &AppWindow, index: i32) {
        if self.launch_feedback.borrow().handoff_pending() {
            return;
        }
        let game = usize::try_from(index)
            .ok()
            .and_then(|index| self.catalog.borrow().get(index).cloned());
        let Some(game) = game.as_ref() else {
            debug!("Accept ignored because the source-backed Home library is empty");
            return;
        };

        if !self.launch_service.can_launch(game) {
            ui.invoke_playback_unavailable();
            return;
        }

        let visual_index = self
            .home_order
            .borrow()
            .iter()
            .position(|item| *item == index as usize)
            .map(|position| position as i32)
            .unwrap_or(-1);
        self.set_launch_feedback(
            ui,
            LaunchFeedbackState::Launching {
                index: visual_index,
            },
        );

        if ui.get_current_route() == crate::AppRouteView::Library {
            ui.set_library_launching_index(ui.get_library_selected_index());
        }
        match self.launch_service.launch_game(game) {
            Ok(receipt) => {
                match receipt.mode() {
                    GameLaunchMode::Embedded => {
                        // The player shares this window, so no foreground
                        // handoff will arrive to release the legacy launch latch.
                        // Release the legacy latch, but let the player's first-frame
                        // handoff release the visual press in sync with its zoom.
                        *self.launch_feedback.borrow_mut() = LaunchFeedbackState::Idle;
                        ui.set_launch_feedback_text("".into());
                        return;
                    }
                    GameLaunchMode::External => {
                        self.launch_activity
                            .launch_dispatched(game.game().id(), receipt.source_id().clone());
                        debug!(
                            game_id = game.game().id().get(),
                            title = %game.game().title().as_str(),
                            source = %receipt.source_id(),
                            "external game launch dispatched; waiting for foreground handoff"
                        );
                    }
                    GameLaunchMode::Observed(observation_id) => {
                        self.playing.borrow_mut().arm_runtime(observation_id, index);
                        self.launch_activity.runtime_observation_armed(
                            observation_id,
                            game.game().id(),
                            receipt.source_id().clone(),
                        );
                        debug!(
                            game_id = game.game().id().get(),
                            title = %game.game().title().as_str(),
                            source = %receipt.source_id(),
                            observation_id = observation_id.get(),
                            "external game launch dispatched with source runtime observation"
                        );
                    }
                }
                // Handoff/press feedback stays transient; source-confirmed
                // runtime lifecycles drive per-card Playing.
                self.set_launch_feedback(ui, LaunchFeedbackState::Dispatched);
                self.schedule_press_release(ui);
            }
            Err(error) => {
                ui.set_library_launching_index(-1);
                self.set_launch_feedback(ui, LaunchFeedbackState::Failed);
                warn!(
                    game_id = game.game().id().get(),
                    title = %game.game().title().as_str(),
                    %error,
                    "game launch failed"
                );
            }
        }
    }

    fn set_launch_feedback(&self, ui: &AppWindow, state: LaunchFeedbackState) {
        *self.launch_feedback.borrow_mut() = state;
        match state {
            LaunchFeedbackState::Launching { index } => {
                self.press_generation
                    .set(self.press_generation.get().wrapping_add(1));
                ui.set_launching_game_index(index);
            }
            LaunchFeedbackState::Dispatched => {
                // Keep the short press held until the release timer fires.
            }
            LaunchFeedbackState::Idle | LaunchFeedbackState::Failed => {
                self.press_generation
                    .set(self.press_generation.get().wrapping_add(1));
                ui.set_launching_game_index(-1);
            }
        }
        ui.set_launch_feedback_text(state.status_text().into());
    }

    fn schedule_press_release(&self, ui: &AppWindow) {
        let press_generation = Rc::clone(&self.press_generation);
        let current = press_generation.get();
        let ui_weak = ui.as_weak();
        // Give the press-in frame time to render before releasing it. The card
        // springs back without modifying a game's lifecycle-backed pill.
        Timer::single_shot(Duration::from_millis(125), move || {
            if press_generation.get() != current {
                return;
            }
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_launching_game_index(-1);
            }
        });
    }

    fn clear_launch_feedback(&self, ui: &AppWindow) {
        if *self.launch_feedback.borrow() == LaunchFeedbackState::Idle {
            return;
        }
        if self.launch_feedback.borrow().handoff_pending() {
            self.launch_activity.cancel_pending_launch();
        }
        self.set_launch_feedback(ui, LaunchFeedbackState::Idle);
    }

    fn move_selection(&self, ui: &AppWindow, delta: i32, allow_wrap: bool) {
        self.clear_launch_feedback(ui);
        let selected_index =
            self.state
                .borrow_mut()
                .move_by(delta, self.home_item_count(), allow_wrap);
        self.publish_selection(ui, selected_index);
    }

    fn select_index(&self, ui: &AppWindow, requested_index: i32) {
        if self.launch_feedback.borrow().handoff_pending() {
            return;
        }
        self.clear_launch_feedback(ui);
        let selected_index = self
            .state
            .borrow_mut()
            .select(requested_index, self.home_item_count());
        self.publish_selection(ui, selected_index);
    }

    fn publish_selection(&self, ui: &AppWindow, selected_index: i32) {
        ui.set_selected_index(selected_index);

        let title = usize::try_from(selected_index)
            .ok()
            .and_then(|index| self.home_order.borrow().get(index).copied())
            .and_then(|index| self.titles.borrow().get(index).cloned())
            .unwrap_or_default();
        ui.set_selected_title(if self.library_tile_selected() {
            "Library".into()
        } else {
            title
        });
    }
}

pub(crate) const HOME_RECENT_LIMIT: usize = 15;

/// Play history has priority; use an alphabetical catalogue fallback to keep
/// Home useful before any sessions have been recorded. Neither group changes
/// the original catalogue indices used for artwork, launches, or Playing.
fn ordered_home_indices(catalog: &Catalog, ids: &[GameId]) -> Vec<usize> {
    let mut seen = HashSet::new();
    let mut order = Vec::with_capacity(catalog.len().min(HOME_RECENT_LIMIT));

    // Activity has already sorted these distinct game identities by most
    // recent session, but dedupe defensively before projecting them to Home.
    for id in ids {
        if !seen.insert(*id) {
            continue;
        }
        if let Some(index) = catalog.position(*id)
            && !catalog[index].hidden()
        {
            order.push(index);
            if order.len() == HOME_RECENT_LIMIT {
                return order;
            }
        }
    }

    // Never label an unplayed game as recently played. It occupies only an
    // otherwise empty Home slot, in case-insensitive A–Z order.
    let mut fallback: Vec<usize> = catalog
        .iter()
        .filter_map(|(index, game)| {
            (!game.hidden() && !seen.contains(&game.game().id())).then_some(index)
        })
        .collect();
    fallback.sort_by(|&left, &right| {
        let lhs = catalog[left].game().title().as_str();
        let rhs = catalog[right].game().title().as_str();
        lhs.to_lowercase()
            .cmp(&rhs.to_lowercase())
            .then_with(|| lhs.cmp(rhs))
            .then_with(|| catalog[left].game().id().cmp(&catalog[right].game().id()))
    });

    order.extend(fallback.into_iter().take(HOME_RECENT_LIMIT - order.len()));
    order
}

/// Each game's place in the previous row, or -1 for a game new to it.
fn home_from_indices(previous: &[usize], next: &[usize]) -> Vec<i32> {
    next.iter()
        .map(|index| {
            previous
                .iter()
                .position(|old| old == index)
                .map_or(-1, |position| position as i32)
        })
        .collect()
}

fn lookup_job(
    index: usize,
    game: &LibraryGame,
    registry: &crate::sources::SourceRegistry,
) -> ArtworkLookupJob {
    ArtworkLookupJob {
        index,
        identity: steamgriddb_artwork::cache_identity(game),
        lookup: SteamGridDbLookup::for_game(game, registry),
    }
}

fn needs_external_artwork(prefer_external: bool, has_source_artwork: bool) -> bool {
    prefer_external || !has_source_artwork
}

fn game_card(
    game: &LibraryGame,
    square_artwork: Option<SquareArtwork>,
    registry: &crate::sources::SourceRegistry,
) -> GameCardData {
    let title = super::titles::game_title(game.game().title().as_str());
    let (primary, secondary, highlight) =
        fallback_palette(game.game().id().get(), game.game().title().as_str());
    let pixelated_artwork = square_artwork
        .as_ref()
        .is_some_and(SquareArtwork::pixelated);
    let artwork = square_artwork.map(square_artwork_to_slint);
    let has_artwork = artwork.is_some();

    GameCardData {
        title: title.as_str().into(),
        platform: game
            .sources()
            .iter()
            .filter_map(|reference| registry.get(reference.source_id()))
            .map(|source| source.descriptor().display_name())
            .collect::<Vec<_>>()
            .join(" · ")
            .into(),
        monogram: monogram(&title).into(),
        artwork: artwork.unwrap_or_default(),
        has_artwork,
        pixelated_artwork,
        is_playing: false,
        cover_primary: rgb(primary),
        cover_secondary: rgb(secondary),
        cover_highlight: rgb(highlight),
    }
}

fn square_artwork_to_slint(artwork: SquareArtwork) -> Image {
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        artwork.rgba(),
        artwork.size(),
        artwork.size(),
    );
    Image::from_rgba8(buffer)
}

fn monogram(title: &str) -> String {
    let words = title
        .split_whitespace()
        .filter_map(|word| word.chars().find(|character| character.is_alphanumeric()))
        .take(2)
        .collect::<Vec<_>>();

    let characters = if words.len() >= 2 {
        words
    } else {
        title
            .chars()
            .filter(|character| character.is_alphanumeric())
            .take(2)
            .collect()
    };

    characters
        .into_iter()
        .flat_map(|character| character.to_uppercase())
        .collect::<String>()
}

type Rgb8 = (u8, u8, u8);

fn fallback_palette(game_id: i64, title: &str) -> (Rgb8, Rgb8, Rgb8) {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ game_id as u64;
    for byte in title.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    let channel =
        |shift: u32, base: u8, span: u8| base.saturating_add(((hash >> shift) as u8) % span);
    let primary = (
        channel(0, 32, 112),
        channel(8, 42, 112),
        channel(16, 54, 112),
    );
    let secondary = (
        channel(24, 72, 128),
        channel(32, 78, 128),
        channel(40, 84, 128),
    );
    let highlight = (
        channel(48, 148, 96),
        channel(20, 148, 96),
        channel(36, 148, 96),
    );
    (primary, secondary, highlight)
}

fn rgb((red, green, blue): (u8, u8, u8)) -> Color {
    Color::from_rgb_u8(red, green, blue)
}

#[cfg(test)]
mod tests {
    use super::{
        HOME_RECENT_LIMIT, HomeState, LaunchFeedbackState, PlayingState, home_from_indices,
        monogram, needs_external_artwork,
    };

    #[test]
    fn a_new_game_at_the_front_pushes_the_row_along() {
        // Game 7 arrives first; 3 falls off the end.
        assert_eq!(home_from_indices(&[1, 2, 3], &[7, 1, 2]), vec![-1, 0, 1]);
        assert_eq!(home_from_indices(&[], &[4]), vec![-1]);
    }
    use crate::presentation::catalog::Catalog;

    fn ordered_home_indices(games: &[LibraryGame], ids: &[GameId]) -> Vec<usize> {
        super::ordered_home_indices(&Catalog::new(games.to_vec()), ids)
    }
    use crate::domain::{Game, GameId, GameTitle, LibraryGame};
    use crate::services::runtime::{
        RuntimeObservationEvent, RuntimeObservationId, RuntimeObservationTerminalState,
    };

    fn catalogue(titles: &[&str]) -> Vec<LibraryGame> {
        titles
            .iter()
            .enumerate()
            .map(|(index, title)| {
                LibraryGame::new(
                    Game::new(
                        GameId::new(index as i64 + 1).unwrap(),
                        GameTitle::new(*title).unwrap(),
                    ),
                    vec![],
                )
            })
            .collect()
    }

    #[test]
    fn artwork_precedence_skips_external_when_source_present_by_default() {
        assert!(!needs_external_artwork(false, true));
        assert!(needs_external_artwork(false, false));
        assert!(needs_external_artwork(true, true));
        assert!(needs_external_artwork(true, false));
    }

    #[test]
    fn imported_titles_get_stable_short_monograms() {
        assert_eq!(monogram("ELDEN RING"), "ER");
        assert_eq!(monogram("Portal"), "PO");
        assert_eq!(monogram("NieR:Automata"), "NI");
    }

    #[test]
    fn launch_feedback_distinguishes_dispatch_and_failed_states_without_source_details() {
        let launching = LaunchFeedbackState::Launching { index: 3 };
        assert!(launching.handoff_pending());
        assert_eq!(launching.status_text(), "");

        let dispatched = LaunchFeedbackState::Dispatched;
        assert!(dispatched.handoff_pending());
        assert_eq!(dispatched.status_text(), "");

        assert!(!LaunchFeedbackState::Failed.handoff_pending());
        assert_eq!(LaunchFeedbackState::Failed.status_text(), "Launch failed");
    }

    #[test]
    fn playing_requires_running_event_and_ends_at_runtime_terminal() {
        let mut state = PlayingState::default();
        let id = RuntimeObservationId::new(7).unwrap();
        state.arm_runtime(id, 3);
        assert!(!state.is_playing(3));
        assert_eq!(
            state.runtime_event(&RuntimeObservationEvent::Started {
                observation_id: id,
                started_at: 12,
            }),
            Some(3)
        );
        assert!(state.is_playing(3));
        assert!(!state.is_playing(2));
        assert_eq!(
            state.runtime_event(&RuntimeObservationEvent::Terminal {
                observation_id: id,
                terminal: RuntimeObservationTerminalState::Exited {
                    started_at: 12,
                    ended_at: 30
                },
            }),
            Some(3)
        );
        assert!(!state.is_playing(3));
    }

    #[test]
    fn concurrent_running_observations_do_not_clear_each_other() {
        let mut state = PlayingState::default();
        for value in [1, 2] {
            let id = RuntimeObservationId::new(value).unwrap();
            state.arm_runtime(id, 4);
            state.runtime_event(&RuntimeObservationEvent::Started {
                observation_id: id,
                started_at: 1,
            });
        }
        let first = RuntimeObservationId::new(1).unwrap();
        state.runtime_event(&RuntimeObservationEvent::Terminal {
            observation_id: first,
            terminal: RuntimeObservationTerminalState::Lost,
        });
        assert!(state.is_playing(4));
    }

    #[test]
    fn playing_indicator_ignores_handoff_reset_and_clears_on_lost_lifecycle() {
        let mut badge = PlayingState::default();
        let mut handoff = LaunchFeedbackState::Launching { index: 6 };
        let id = RuntimeObservationId::new(11).unwrap();
        assert!(handoff.handoff_pending());
        badge.arm_runtime(id, 6);
        badge.runtime_event(&RuntimeObservationEvent::Started {
            observation_id: id,
            started_at: 100,
        });
        handoff = LaunchFeedbackState::Idle; // app loses focus
        assert!(!handoff.handoff_pending());
        assert!(badge.is_playing(6));
        badge.runtime_event(&RuntimeObservationEvent::Terminal {
            observation_id: id,
            terminal: RuntimeObservationTerminalState::Lost,
        });
        assert!(!badge.is_playing(6));
    }

    #[test]
    fn home_recent_projection_maps_to_full_catalogue_without_duplicate_entries() {
        let games = (1..=18)
            .map(|number| {
                LibraryGame::new(
                    Game::new(
                        GameId::new(number).unwrap(),
                        GameTitle::new(format!("Game {number}")).unwrap(),
                    ),
                    vec![],
                )
            })
            .collect::<Vec<_>>();
        let ids = (1..=18)
            .rev()
            .map(|number| GameId::new(number).unwrap())
            .collect::<Vec<_>>();
        let order = ordered_home_indices(&games, &ids);
        assert_eq!(order.len(), HOME_RECENT_LIMIT);
        assert_eq!(order[0], 17);
        assert_eq!(order[14], 3);
        assert_eq!(ordered_home_indices(&games, &[]).len(), HOME_RECENT_LIMIT);
    }

    #[test]
    fn home_without_play_history_fills_fifteen_games_alphabetically() {
        let games = catalogue(&[
            "Zelda", "alpha", "Moss", "banana", "Delta", "game 7", "Game 1", "Echo", "stardew",
            "Lunar", "Oxygen", "Quartz", "Terraria", "Portal", "Cobalt", "Yonder", "Ridge",
        ]);
        let order = ordered_home_indices(&games, &[]);
        assert_eq!(order.len(), HOME_RECENT_LIMIT);
        let names = order
            .iter()
            .map(|&index| games[index].game().title().as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "alpha", "banana", "Cobalt", "Delta", "Echo", "Game 1", "game 7", "Lunar", "Moss",
                "Oxygen", "Portal", "Quartz", "Ridge", "stardew", "Terraria",
            ]
        );
    }

    #[test]
    fn played_games_precede_alphabetical_fallback_without_duplicates() {
        let games = catalogue(&["Zelda", "Alpha", "Mario", "stardew", "Terraria"]);
        let ids = [
            GameId::new(5).unwrap(),
            GameId::new(3).unwrap(),
            GameId::new(5).unwrap(),
        ];
        // Two distinct recently played games, then all three unplayed A–Z.
        assert_eq!(ordered_home_indices(&games, &ids), vec![4, 2, 1, 3, 0]);
    }

    #[test]
    fn hidden_games_are_excluded_from_recent_and_alphabetical_home_slots() {
        let mut games = catalogue(&["Hidden", "Visible"]);
        games[0] = games[0].clone().with_hidden(true);
        assert_eq!(
            ordered_home_indices(&games, &[games[0].game().id()]),
            vec![1]
        );
        assert_eq!(ordered_home_indices(&games, &[]), vec![1]);
    }

    #[test]
    fn partial_history_fills_remaining_slots_without_repeating_played_games() {
        let titles = (0..20).map(|n| format!("Game {n:02}")).collect::<Vec<_>>();
        let games = titles
            .iter()
            .enumerate()
            .map(|(index, title)| {
                LibraryGame::new(
                    Game::new(
                        GameId::new(index as i64 + 1).unwrap(),
                        GameTitle::new(title.as_str()).unwrap(),
                    ),
                    vec![],
                )
            })
            .collect::<Vec<_>>();
        let played_ids = (7..=19)
            .rev()
            .map(|n| GameId::new(n).unwrap())
            .collect::<Vec<_>>();
        let order = ordered_home_indices(&games, &played_ids);
        assert_eq!(order.len(), HOME_RECENT_LIMIT);
        assert_eq!(&order[..13], &(6..19).rev().collect::<Vec<_>>()[..]);
        assert_eq!(&order[13..], &[0, 1]);
    }

    #[test]
    fn only_currently_imported_ids_are_used_and_small_libraries_fill_all_slots() {
        let games = catalogue(&["Zulu", "Bravo", "alpha"]);
        let absent = GameId::new(999).unwrap();
        assert_eq!(ordered_home_indices(&games, &[absent]), vec![2, 1, 0]);
        assert_eq!(
            ordered_home_indices(&games, &[GameId::new(1).unwrap()]),
            vec![0, 2, 1]
        );
        assert!(ordered_home_indices(&[], &[]).is_empty());
    }

    #[test]
    fn a_newly_played_game_moves_to_front_and_evicts_the_last_fallback() {
        let titles = (0..18).map(|n| format!("Game {n:02}")).collect::<Vec<_>>();
        let games = titles
            .iter()
            .enumerate()
            .map(|(index, title)| {
                LibraryGame::new(
                    Game::new(
                        GameId::new(index as i64 + 1).unwrap(),
                        GameTitle::new(title.as_str()).unwrap(),
                    ),
                    vec![],
                )
            })
            .collect::<Vec<_>>();
        let before = ordered_home_indices(&games, &[]);
        let after = ordered_home_indices(&games, &[GameId::new(18).unwrap()]);
        assert_eq!(before, (0..15).collect::<Vec<_>>());
        assert_eq!(after[0], 17);
        assert_eq!(&after[1..], &(0..14).collect::<Vec<_>>()[..]);
        assert!(!after.contains(&14));
    }

    #[test]
    fn global_home_reset_selects_first_game() {
        let mut state = HomeState::default();
        assert_eq!(state.select(6, 8), 6);
        assert_eq!(state.reset_for_global_home(8), 0);
    }

    #[test]
    fn navigation_clamps_at_both_ends() {
        let mut state = HomeState::default();
        assert_eq!(state.move_by(-1, 8, false), 0);
        assert_eq!(state.select(7, 8), 7);
        assert_eq!(state.move_by(1, 8, false), 7);
    }

    #[test]
    fn navigation_moves_without_sdl_hardware() {
        let mut state = HomeState::default();
        assert_eq!(state.move_by(1, 8, false), 1);
        assert_eq!(state.move_by(1, 8, false), 2);
        assert_eq!(state.move_by(-1, 8, false), 1);
    }

    #[test]
    fn fresh_boundary_press_wraps_but_repeated_hold_does_not() {
        let mut state = HomeState::default();

        assert_eq!(state.move_by(-1, 8, true), 7);
        assert_eq!(state.move_by(1, 8, true), 0);

        assert_eq!(state.select(6, 8), 6);
        assert_eq!(state.move_by(1, 8, false), 7);
        assert_eq!(state.move_by(1, 8, false), 7);

        assert_eq!(state.select(1, 8), 1);
        assert_eq!(state.move_by(-1, 8, false), 0);
        assert_eq!(state.move_by(-1, 8, false), 0);
    }

    #[test]
    fn empty_library_has_stable_zero_selection() {
        let mut state = HomeState { selected_index: 5 };
        assert_eq!(state.select(5, 0), 0);
        assert_eq!(state.move_by(1, 0, false), 0);
    }
}
