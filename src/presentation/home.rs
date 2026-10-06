use std::{cell::RefCell, rc::Rc};

use slint::{Color, ComponentHandle, Model, ModelRc, SharedString, VecModel};
use tracing::debug;

use crate::{AppWindow, GameCardData, input::{UiAction, UiActionEvent}};

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

/// Owns the Phase 3 home-screen presentation state.
///
/// Pointer, keyboard, and controller input all enter through this controller.
/// Slint renders the resulting state but does not decide navigation behavior.
pub struct HomeController {
    games: Rc<VecModel<GameCardData>>,
    titles: Vec<SharedString>,
    state: RefCell<HomeState>,
}

impl HomeController {
    pub fn new(ui: &AppWindow) -> Rc<Self> {
        let demo_games = demo_games();
        let titles = demo_games.iter().map(|game| game.title.clone()).collect();
        let games = Rc::new(VecModel::from(demo_games));
        ui.set_games(ModelRc::from(Rc::clone(&games)));

        let controller = Rc::new(Self {
            games,
            titles,
            state: RefCell::new(HomeState::default()),
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

    pub fn game_count(&self) -> usize {
        self.games.row_count()
    }

    pub fn handle_action(&self, ui: &AppWindow, event: UiActionEvent) {
        match event.action {
            UiAction::Left => self.move_selection(ui, -1, !event.repeated),
            UiAction::Right => self.move_selection(ui, 1, !event.repeated),
            UiAction::Accept => {
                if let Some(title) = self.selected_title() {
                    debug!(game = %title, "accept pressed; launching is deferred to a later phase");
                }
            }
            UiAction::Up
            | UiAction::Down
            | UiAction::Back
            | UiAction::Menu
            | UiAction::Home
            | UiAction::LeftBumper
            | UiAction::RightBumper => {
                debug!(action = ?event.action, repeated = event.repeated, "UI action has no Phase 3 home behavior");
            }
        }
    }

    fn move_selection(&self, ui: &AppWindow, delta: i32, allow_wrap: bool) {
        let selected_index = self
            .state
            .borrow_mut()
            .move_by(delta, self.titles.len(), allow_wrap);
        self.publish_selection(ui, selected_index);
    }

    fn select_index(&self, ui: &AppWindow, requested_index: i32) {
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
        ui.set_selected_title(title);
    }

    fn selected_title(&self) -> Option<&SharedString> {
        let index = self.state.borrow().selected_index;
        usize::try_from(index)
            .ok()
            .and_then(|index| self.titles.get(index))
    }
}

fn demo_games() -> Vec<GameCardData> {
    vec![
        game(
            "Solar Drift",
            "PC",
            "SD",
            (243, 94, 69),
            (244, 173, 70),
            (255, 220, 117),
        ),
        game(
            "Verdant Echo",
            "HANDHELD",
            "VE",
            (35, 119, 92),
            (72, 170, 112),
            (183, 225, 126),
        ),
        game(
            "Neon Circuit",
            "PC",
            "NC",
            (64, 63, 177),
            (165, 68, 181),
            (83, 207, 240),
        ),
        game(
            "Deep Atlas",
            "HANDHELD",
            "DA",
            (20, 84, 126),
            (35, 139, 154),
            (94, 217, 196),
        ),
        game(
            "Starfall",
            "PC",
            "SF",
            (49, 55, 92),
            (102, 82, 164),
            (238, 116, 173),
        ),
        game(
            "Midnight Rally",
            "PC",
            "MR",
            (35, 42, 52),
            (65, 87, 112),
            (237, 111, 67),
        ),
        game(
            "Wild Current",
            "HANDHELD",
            "WC",
            (31, 109, 152),
            (36, 165, 187),
            (102, 223, 202),
        ),
        game(
            "Arcade Zero",
            "PC",
            "AZ",
            (143, 41, 80),
            (222, 69, 79),
            (255, 184, 81),
        ),
    ]
}

fn game(
    title: &str,
    platform: &str,
    monogram: &str,
    primary: (u8, u8, u8),
    secondary: (u8, u8, u8),
    highlight: (u8, u8, u8),
) -> GameCardData {
    GameCardData {
        title: title.into(),
        platform: platform.into(),
        monogram: monogram.into(),
        cover_primary: rgb(primary),
        cover_secondary: rgb(secondary),
        cover_highlight: rgb(highlight),
    }
}

fn rgb((red, green, blue): (u8, u8, u8)) -> Color {
    Color::from_rgb_u8(red, green, blue)
}

#[cfg(test)]
mod tests {
    use super::{HomeState, demo_games};

    #[test]
    fn phase_three_has_enough_demo_games_to_exercise_controller_navigation() {
        assert!(demo_games().len() >= 8);
    }

    #[test]
    fn demo_game_titles_are_unique() {
        let games = demo_games();
        let mut titles: Vec<_> = games.iter().map(|game| game.title.to_string()).collect();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), games.len());
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
