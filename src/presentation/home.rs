use std::{
    cell::RefCell, rc::Rc, sync::{Arc, atomic::{AtomicU64, Ordering}, mpsc::{self, Receiver, Sender}},
    path::PathBuf,
};

use slint::{
    Color, ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString,
    VecModel,
};
use tracing::{debug, warn};

use crate::{
    AppWindow, GameCardData,
    domain::LibraryGame,
    input::{UiAction, UiActionEvent},
    services::{
        activity::LaunchActivitySink,
        artwork::{ArtworkService, SquareArtwork},
        launch::{GameLaunchMode, GameLaunchService},
        settings::ArtworkPreferences,
        steamgriddb::SteamGridDbLookup,
        steamgriddb_artwork::{self, ArtworkLookupJob, ArtworkWorkerEvent},
        session::ManagedSessionId,
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
        if allow_wrap && delta < 0 && self.selected_index == 0 {
            self.selected_index = max_index;
        } else if allow_wrap && delta > 0 && self.selected_index == max_index {
            self.selected_index = 0;
        } else {
            self.selected_index = self
                .selected_index
                .saturating_add(delta)
                .clamp(0, max_index);
        }

        self.selected_index
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LaunchFeedbackState {
    #[default]
    Idle,
    Launching {
        index: i32,
        managed_session: Option<ManagedSessionId>,
    },
    Failed,
}

impl LaunchFeedbackState {
    fn is_launching(self) -> bool {
        matches!(self, Self::Launching { .. })
    }

    fn launching_index(self) -> i32 {
        match self {
            Self::Launching { index, .. } => index,
            Self::Idle | Self::Failed => -1,
        }
    }

    fn status_text(self) -> &'static str {
        match self {
            Self::Idle => "",
            Self::Launching { .. } => "Launching…",
            Self::Failed => "Launch failed",
        }
    }
}

/// Owns Home presentation state for the durable source-backed library.
///
/// Slint receives only card presentation data. `LibraryGame` identity stays in
/// Rust so Accept can launch through the generic service/source boundary.
pub struct HomeController {
    cards: Rc<VecModel<GameCardData>>,
    /// Immutable provider-owned fallbacks, never replaced by downloaded art.
    source_cards: Vec<GameCardData>,
    artwork_lookup_jobs: Vec<ArtworkLookupJob>,
    artwork_cache_root: Option<PathBuf>,
    artwork_sender: Sender<ArtworkWorkerEvent>,
    artwork_receiver: RefCell<Receiver<ArtworkWorkerEvent>>,
    artwork_generation: Arc<AtomicU64>,
    library_games: Vec<LibraryGame>,
    titles: Vec<SharedString>,
    launch_service: Rc<GameLaunchService>,
    launch_activity: Rc<dyn LaunchActivitySink>,
    state: RefCell<HomeState>,
    launch_feedback: RefCell<LaunchFeedbackState>,
}

impl HomeController {
    pub fn new(
        ui: &AppWindow,
        library_games: Vec<LibraryGame>,
        artwork_service: &ArtworkService,
        registry: &crate::sources::SourceRegistry,
        launch_service: Rc<GameLaunchService>,
        launch_activity: Rc<dyn LaunchActivitySink>,
    ) -> Rc<Self> {
        let card_data = library_games
            .iter()
            .map(|game| game_card(game, artwork_service))
            .collect::<Vec<_>>();
        let titles = card_data.iter().map(|game| game.title.clone()).collect();
        let artwork_lookup_jobs = library_games.iter().enumerate().map(|(index, game)| {
            ArtworkLookupJob {
                index,
                identity: steamgriddb_artwork::cache_identity(game),
                lookup: SteamGridDbLookup::for_game(game, registry),
            }
        }).collect();
        let (artwork_sender, artwork_receiver) = mpsc::channel();
        let card_data_for_fallback = card_data.clone();
        let cards = Rc::new(VecModel::from(card_data));
        ui.set_games(ModelRc::from(Rc::clone(&cards)));

        let controller = Rc::new(Self {
            cards,
            source_cards: card_data_for_fallback,
            artwork_lookup_jobs,
            artwork_cache_root: artwork_service.steamgriddb_cache_root(),
            artwork_sender,
            artwork_receiver: RefCell::new(artwork_receiver),
            artwork_generation: Arc::new(AtomicU64::new(0)),
            library_games,
            titles,
            launch_service,
            launch_activity,
            state: RefCell::new(HomeState::default()),
            launch_feedback: RefCell::new(LaunchFeedbackState::default()),
        });

        controller.select_index(ui, 0);

        let ui_weak = ui.as_weak();
        let callback_controller = Rc::clone(&controller);
        ui.on_select_game(move |requested_index| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };

            callback_controller.select_index(&ui, requested_index);
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
        let generation = self.artwork_generation.fetch_add(1, Ordering::AcqRel) + 1;
        if !force_refresh {
            for (index, card) in self.source_cards.iter().enumerate() {
                self.cards.set_row_data(index, card.clone());
            }
        }
        let Some(api_key) = preferences.api_key else { return; };
        let jobs = self.artwork_lookup_jobs.iter().filter(|job| {
            needs_external_artwork(preferences.prefer_steamgriddb, self.source_cards[job.index].has_artwork)
        }).cloned().collect();
        steamgriddb_artwork::start_artwork_worker(
            generation, Arc::clone(&self.artwork_generation), self.artwork_sender.clone(),
            api_key, self.artwork_cache_root.clone(), jobs, force_refresh,
        );
    }

    /// Drained by the existing Slint timer on the presentation thread, where
    /// VecModel updates belong. The original source card is always available.
    pub fn collect_steamgriddb_results(&self, ui: &AppWindow) {
        for _ in 0..32 {
            let Ok(event) = self.artwork_receiver.borrow().try_recv() else { break; };
            match event {
                ArtworkWorkerEvent::Artwork(result) => {
                    if result.generation != self.artwork_generation.load(Ordering::Acquire) { continue; }
                    let Some(fallback) = self.source_cards.get(result.index) else { continue; };
                    let mut card = fallback.clone();
                    card.pixelated_artwork = result.artwork.pixelated();
                    card.artwork = square_artwork_to_slint(result.artwork);
                    card.has_artwork = true;
                    self.cards.set_row_data(result.index, card);
                }
                ArtworkWorkerEvent::RefreshProgress { generation, completed, total,
                    finished, error } => {
                    if generation != self.artwork_generation.load(Ordering::Acquire) { continue; }
                    ui.set_settings_refresh_completed(completed.min(i32::MAX as usize) as i32);
                    ui.set_settings_refresh_total(total.min(i32::MAX as usize) as i32);
                    ui.set_settings_refresh_error(error.unwrap_or_default().into());
                    if finished { ui.set_settings_refresh_running(false); }
                }
                ArtworkWorkerEvent::Status { generation, message } => {
                    if generation == self.artwork_generation.load(Ordering::Acquire) {
                        ui.set_settings_artwork_status(message.into());
                    }
                }
            }
        }
    }

    pub fn game_count(&self) -> usize {
        self.cards.row_count()
    }

    pub fn selected_index(&self) -> i32 {
        self.state.borrow().selected_index
    }

    /// Reset Home to its canonical global-Home destination: the first game.
    pub fn reset_for_global_home(&self, ui: &AppWindow) {
        self.clear_launch_feedback(ui);
        let selected_index = self
            .state
            .borrow_mut()
            .reset_for_global_home(self.titles.len());
        self.publish_selection(ui, selected_index);
    }

    /// Select a game from shell-level spatial focus transfer. Pointer and page
    /// navigation still converge on the same Rust-owned selection state.
    pub fn select_from_shell(&self, ui: &AppWindow, requested_index: i32) {
        self.select_index(ui, requested_index);
    }

    /// Clear transient launch feedback when Horizon yields foreground ownership
    /// to the launched game (or another application).
    pub fn handle_application_active_changed(&self, ui: &AppWindow, active: bool) {
        if !active {
            self.clear_launch_feedback(ui);
        }
    }

    /// Resolve launch feedback for a managed session that terminates before or
    /// after the normal foreground handoff. The managed session itself remains
    /// owned by the launch/activity services; Home only owns transient feedback.
    pub fn handle_managed_session_completion(
        &self,
        ui: &AppWindow,
        session_id: ManagedSessionId,
        failed: bool,
    ) {
        let matches_session = matches!(
            *self.launch_feedback.borrow(),
            LaunchFeedbackState::Launching {
                managed_session: Some(current),
                ..
            } if current == session_id
        );
        if !matches_session {
            return;
        }

        self.set_launch_feedback(
            ui,
            if failed {
                LaunchFeedbackState::Failed
            } else {
                LaunchFeedbackState::Idle
            },
        );
    }

    pub fn handle_action(&self, ui: &AppWindow, event: UiActionEvent) {
        if self.launch_feedback.borrow().is_launching()
            && matches!(event.action, UiAction::Left | UiAction::Right | UiAction::Accept)
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
            UiAction::Accept => {},
            UiAction::Up
            | UiAction::Down
            | UiAction::Back
            | UiAction::Menu
            | UiAction::Home
            | UiAction::LeftBumper
            | UiAction::RightBumper => {
                debug!(action = ?event.action, repeated = event.repeated, "UI action has no Home-local behavior");
            }
        }
    }

    fn launch_selected(&self, ui: &AppWindow) {
        let index = self.state.borrow().selected_index;
        let Some(game) = usize::try_from(index)
            .ok()
            .and_then(|index| self.library_games.get(index))
        else {
            debug!("Accept ignored because the source-backed Home library is empty");
            return;
        };

        self.set_launch_feedback(
            ui,
            LaunchFeedbackState::Launching {
                index,
                managed_session: None,
            },
        );

        match self.launch_service.launch_game(game) {
            Ok(receipt) => {
                match receipt.mode() {
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
                    GameLaunchMode::Managed(session_id) => {
                        self.set_launch_feedback(
                            ui,
                            LaunchFeedbackState::Launching {
                                index,
                                managed_session: Some(session_id),
                            },
                        );
                        self.launch_activity.managed_session_started(
                            session_id,
                            game.game().id(),
                            receipt.source_id().clone(),
                        );
                        debug!(
                            game_id = game.game().id().get(),
                            title = %game.game().title().as_str(),
                            source = %receipt.source_id(),
                            session_id = session_id.get(),
                            "managed game session started"
                        );
                    }
                }
            }
            Err(error) => {
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
        ui.set_launching_game_index(state.launching_index());
        ui.set_launch_feedback_text(state.status_text().into());
    }

    fn clear_launch_feedback(&self, ui: &AppWindow) {
        if *self.launch_feedback.borrow() == LaunchFeedbackState::Idle {
            return;
        }
        if self.launch_feedback.borrow().is_launching() {
            self.launch_activity.cancel_pending_launch();
        }
        self.set_launch_feedback(ui, LaunchFeedbackState::Idle);
    }

    fn move_selection(&self, ui: &AppWindow, delta: i32, allow_wrap: bool) {
        self.clear_launch_feedback(ui);
        let selected_index = self
            .state
            .borrow_mut()
            .move_by(delta, self.titles.len(), allow_wrap);
        self.publish_selection(ui, selected_index);
    }

    fn select_index(&self, ui: &AppWindow, requested_index: i32) {
        if self.launch_feedback.borrow().is_launching() {
            return;
        }
        self.clear_launch_feedback(ui);
        let selected_index = self
            .state
            .borrow_mut()
            .select(requested_index, self.titles.len());
        self.publish_selection(ui, selected_index);
    }

    fn publish_selection(&self, ui: &AppWindow, selected_index: i32) {
        ui.set_selected_index(selected_index);

        let title = usize::try_from(selected_index)
            .ok()
            .and_then(|index| self.titles.get(index))
            .cloned()
            .unwrap_or_default();
        ui.set_selected_title(title.clone());
    }
}

fn needs_external_artwork(prefer_external: bool, has_source_artwork: bool) -> bool {
    prefer_external || !has_source_artwork
}

fn game_card(game: &LibraryGame, artwork_service: &ArtworkService) -> GameCardData {
    let title = game.game().title().as_str();
    let (primary, secondary, highlight) = fallback_palette(game.game().id().get(), title);
    let square_artwork = artwork_service.square_artwork(game);
    let pixelated_artwork = square_artwork
        .as_ref()
        .is_some_and(SquareArtwork::pixelated);
    let artwork = square_artwork.map(square_artwork_to_slint);
    let has_artwork = artwork.is_some();

    GameCardData {
        title: title.into(),
        platform: "PC".into(),
        monogram: monogram(title).into(),
        artwork: artwork.unwrap_or_default(),
        has_artwork,
        pixelated_artwork,
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

fn fallback_palette(game_id: i64, title: &str) -> ((u8, u8, u8), (u8, u8, u8), (u8, u8, u8)) {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ game_id as u64;
    for byte in title.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    let channel = |shift: u32, base: u8, span: u8| {
        base.saturating_add(((hash >> shift) as u8) % span)
    };
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
    use super::{HomeState, LaunchFeedbackState, monogram, needs_external_artwork};

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
    fn launch_feedback_distinguishes_pending_and_failed_states_without_source_details() {
        let launching = LaunchFeedbackState::Launching {
            index: 3,
            managed_session: None,
        };
        assert!(launching.is_launching());
        assert_eq!(launching.launching_index(), 3);
        assert_eq!(launching.status_text(), "Launching…");

        assert!(!LaunchFeedbackState::Failed.is_launching());
        assert_eq!(LaunchFeedbackState::Failed.launching_index(), -1);
        assert_eq!(LaunchFeedbackState::Failed.status_text(), "Launch failed");
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
