use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use tracing::warn;

use slint::{Model, ModelRc, VecModel};

use super::home::HomeController;
use crate::{
    AppWindow, LibraryCardData,
    domain::LibraryGame,
    input::{UiAction, UiActionEvent},
    persistence::SqliteLibraryRepository,
    services::activity::{ActivityService, LibrarySortMetrics},
};

// Keep all the viewport thresholds and dimensions in lockstep with
// ui/pages/library.slint. Slint renders in logical pixels, not physical pixels.
const CONTENT_TOP: f32 = 88.0; // Metrics.page-header-height
const CONTENT_BOTTOM: f32 = 64.0; // Metrics.page-bottom-bar-height
const GRID_PAD: f32 = 22.0;
const HORIZONTAL_GUTTER: f32 = 48.0; // gallery viewport x = page-margin - 16px
const CARD_GAP: f32 = 28.0;
const OVERSCAN_ROWS: usize = 2;

#[derive(Debug, Clone, Copy)]
struct GridLayout {
    columns: usize,
    stride: f32,
}

impl GridLayout {
    fn for_width(width: f32) -> Self {
        // Fewer, substantially larger covers on Switch/1080p-class screens;
        // ultrawide may show more, but never by shrinking the actual artwork.
        let (card_size, maximum_columns) = if width < 800.0 {
            (132.0, 4)
        } else if width < 1100.0 {
            (156.0, 4)
        } else if width < 1450.0 {
            (180.0, 5)
        } else if width < 2200.0 {
            (206.0, 6)
        } else if width < 3000.0 {
            (218.0, 8)
        } else {
            (226.0, 10)
        };
        let stride = card_size + CARD_GAP;
        // Slint gallery width: columns * stride - gap + 2 * GRID_PAD;
        // viewport width: width - HORIZONTAL_GUTTER.
        let available = width - HORIZONTAL_GUTTER - 2.0 * GRID_PAD + CARD_GAP;
        let columns = ((available / stride).floor() as usize).clamp(1, maximum_columns);
        Self { columns, stride }
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
enum LibrarySort {
    #[default]
    Alphabetical,
    RecentlyPlayed,
    TimePlayed,
    RecentlyAdded,
}
impl LibrarySort {
    fn next(self) -> Self {
        match self {
            Self::Alphabetical => Self::RecentlyPlayed,
            Self::RecentlyPlayed => Self::TimePlayed,
            Self::TimePlayed => Self::RecentlyAdded,
            Self::RecentlyAdded => Self::Alphabetical,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Alphabetical => "Alphabetical",
            Self::RecentlyPlayed => "Recently played",
            Self::TimePlayed => "Time played",
            Self::RecentlyAdded => "Recently added",
        }
    }
}

fn sorted_catalog_indices(
    catalog: &[LibraryGame],
    source: Option<&str>,
    sort: LibrarySort,
    metrics: &BTreeMap<i64, LibrarySortMetrics>,
) -> Vec<usize> {
    let mut order: Vec<usize> = catalog
        .iter()
        .enumerate()
        .filter(|(_, game)| {
            source.is_none_or(|filter| {
                game.sources()
                    .iter()
                    .any(|entry| entry.source_id().as_str() == filter)
            })
        })
        .map(|(i, _)| i)
        .collect();
    order.sort_by(|a, b| {
        let a_game = catalog[*a].game();
        let b_game = catalog[*b].game();
        let a_metric = metrics.get(&a_game.id().get());
        let b_metric = metrics.get(&b_game.id().get());
        let rank = match sort {
            LibrarySort::Alphabetical => std::cmp::Ordering::Equal,
            LibrarySort::RecentlyPlayed => b_metric
                .and_then(|s| s.last_played_at)
                .cmp(&a_metric.and_then(|s| s.last_played_at)),
            LibrarySort::TimePlayed => b_metric
                .map(|s| s.time_played_seconds())
                .unwrap_or(0)
                .cmp(&a_metric.map(|s| s.time_played_seconds()).unwrap_or(0)),
            LibrarySort::RecentlyAdded => b_metric
                .map(|s| s.added_at)
                .cmp(&a_metric.map(|s| s.added_at)),
        };
        rank.then_with(|| {
            a_game
                .title()
                .as_str()
                .to_lowercase()
                .cmp(&b_game.title().as_str().to_lowercase())
        })
        .then_with(|| a_game.id().get().cmp(&b_game.id().get()))
    });
    order
}

fn source_title(id: &str) -> String {
    let mut letters = id.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + letters.as_str(),
        None => String::new(),
    }
}

fn grid_columns(viewport_width: f32) -> usize {
    GridLayout::for_width(viewport_width).columns
}

fn visible_rows(viewport_height: f32, layout: GridLayout) -> usize {
    let content = viewport_height - CONTENT_TOP - CONTENT_BOTTOM - 2.0 * GRID_PAD;
    ((content + CARD_GAP) / layout.stride).floor().max(1.0) as usize
}

fn total_rows(count: usize, columns: usize) -> usize {
    count.div_ceil(columns.max(1))
}

// Camera-only pointer scrolling. Focus identity and metadata are deliberately
// independent from this viewport movement.
fn wheel_scroll_top(top: usize, count: usize, columns: usize, visible: usize, delta: i32) -> usize {
    let max_top = total_rows(count, columns).saturating_sub(visible);
    (top as i64 + i64::from(delta)).clamp(0, max_top as i64) as usize
}

fn scroll_to_selection(
    selection: usize,
    columns: usize,
    visible: usize,
    previous_top: usize,
    count: usize,
) -> usize {
    let row = selection / columns.max(1);
    let max_top = total_rows(count, columns).saturating_sub(visible);
    let next = if row < previous_top {
        row
    } else if row >= previous_top + visible {
        row + 1 - visible
    } else {
        previous_top
    };
    next.min(max_top)
}

/// Horizontal navigation follows reading order through row boundaries on
/// both fresh presses and held repeat. Neither end wraps to the opposite end.
fn horizontal_step(selected: usize, count: usize, delta: i32) -> usize {
    if count == 0 {
        return 0;
    }
    if delta > 0 {
        (selected + 1).min(count - 1)
    } else if delta < 0 {
        selected.saturating_sub(1)
    } else {
        selected
    }
}

/// Vertical navigation keeps the same column where possible and clamps at
/// collection boundaries; there is no surprising jump to the opposite end.
fn vertical_step(selected: usize, count: usize, columns: usize, down: bool) -> usize {
    if count == 0 {
        return 0;
    }
    if down {
        if selected + columns < count {
            selected + columns
        } else if selected < (total_rows(count, columns) - 1) * columns {
            count - 1
        } else {
            selected
        }
    } else if selected >= columns {
        selected - columns
    } else {
        selected
    }
}

struct State {
    source_index: usize,
    sort: LibrarySort,
    order: Vec<usize>,
    selection: usize,
    scroll_top: usize,
    wheel_scrolled: bool,
    mounted_start: usize,
    mounted_len: usize,
    window_dirty: bool,
    metadata_game_id: Option<i64>,
    metadata_sequence: i32,
    browse_sequence: i32,
    browse_transition_kind: i32, // 0 none, 1 Source, 2 Sort
}
impl Default for State {
    fn default() -> Self {
        Self {
            source_index: 0,
            sort: LibrarySort::Alphabetical,
            order: Vec::new(),
            selection: 0,
            scroll_top: 0,
            wheel_scrolled: false,
            mounted_start: 0,
            mounted_len: 0,
            window_dirty: true,
            metadata_game_id: None,
            metadata_sequence: 0,
            browse_sequence: 0,
            browse_transition_kind: 0,
        }
    }
}

/// Source-neutral Library view of the unchanged Home catalogue. Rust retains
/// absolute game identities while the Slint model only mounts visible rows.
pub struct LibraryController {
    catalog: Vec<LibraryGame>,
    sources: Vec<String>,
    home: Rc<HomeController>,
    activity: Rc<ActivityService<SqliteLibraryRepository>>,
    metrics: RefCell<BTreeMap<i64, LibrarySortMetrics>>,
    rows: Rc<VecModel<LibraryCardData>>,
    state: RefCell<State>,
}

impl LibraryController {
    pub fn new(
        ui: &AppWindow,
        catalog: Vec<LibraryGame>,
        home: Rc<HomeController>,
        activity: Rc<ActivityService<SqliteLibraryRepository>>,
    ) -> Rc<Self> {
        let rows = Rc::new(VecModel::from(Vec::<LibraryCardData>::new()));
        ui.set_library_cards(ModelRc::from(Rc::clone(&rows)));
        let mut sources = catalog
            .iter()
            .flat_map(|game| {
                game.sources()
                    .iter()
                    .map(|source| source.source_id().as_str().to_owned())
            })
            .collect::<Vec<_>>();
        sources.sort();
        sources.dedup();
        let controller = Rc::new(Self {
            catalog,
            sources,
            home,
            activity,
            metrics: RefCell::new(BTreeMap::new()),
            rows,
            state: RefCell::new(State::default()),
        });
        controller.refresh_metrics();
        controller.rebuild(ui, None);
        controller
    }

    pub fn on_enter(&self, ui: &AppWindow) {
        let selected = self.selection_game_id();
        self.refresh_metrics();
        self.rebuild(ui, selected);
    }

    fn refresh_metrics(&self) {
        match self.activity.library_sort_metrics() {
            Ok(rows) => {
                *self.metrics.borrow_mut() = rows
                    .into_iter()
                    .map(|row| (row.game_id.get(), row))
                    .collect();
            }
            Err(error) => {
                warn!(%error, "Library sort statistics unavailable; keeping cached order")
            }
        }
    }
    pub fn on_viewport_changed(&self, ui: &AppWindow) {
        self.publish(ui);
    }

    fn selection_game_id(&self) -> Option<i64> {
        let s = self.state.borrow();
        s.order
            .get(s.selection)
            .map(|index| self.catalog[*index].game().id().get())
    }

    /// Freeze only the already-mounted overscan rows, not the entire library.
    /// The current VecModel is about to be replaced by a new source/sort order;
    /// reusing that model for both views produced the old barely-visible flash.
    fn capture_previous_grid(&self, ui: &AppWindow) {
        let previous = (0..self.rows.row_count())
            .filter_map(|index| self.rows.row_data(index))
            .collect::<Vec<_>>();
        let has_previous = !previous.is_empty();
        let s = self.state.borrow();
        ui.set_library_previous_cards(ModelRc::from(Rc::new(VecModel::from(previous))));
        ui.set_library_previous_grid_columns(
            grid_columns(ui.get_logical_viewport_width_px()) as i32
        );
        ui.set_library_previous_scroll_row(s.scroll_top as i32);
        ui.set_library_previous_selected_index(s.selection as i32);
        ui.set_library_has_previous_grid(has_previous);
    }

    fn rebuild(&self, ui: &AppWindow, preferred_id: Option<i64>) {
        let mut s = self.state.borrow_mut();
        let source = s
            .source_index
            .checked_sub(1)
            .and_then(|index| self.sources.get(index))
            .map(String::as_str);
        let order = sorted_catalog_indices(&self.catalog, source, s.sort, &self.metrics.borrow());
        let prior = s.selection;
        s.selection = preferred_id
            .and_then(|id| {
                order
                    .iter()
                    .position(|i| self.catalog[*i].game().id().get() == id)
            })
            .unwrap_or(prior.min(order.len().saturating_sub(1)));
        s.order = order;
        s.wheel_scrolled = false;
        s.window_dirty = true;
        if s.order.is_empty() {
            s.scroll_top = 0;
        }
        drop(s);
        self.publish(ui);
    }

    fn publish(&self, ui: &AppWindow) {
        let columns = grid_columns(ui.get_logical_viewport_width_px());
        let visible = visible_rows(
            ui.get_logical_viewport_height_px(),
            GridLayout::for_width(ui.get_logical_viewport_width_px()),
        );
        let mut s = self.state.borrow_mut();
        let count = s.order.len();
        // Pointer wheel is camera-only; preserve it until the next keyboard/
        // controller focus movement. Never auto-scroll to the selected game
        // just because the data model or viewport was republished.
        if s.wheel_scrolled {
            s.scroll_top = s
                .scroll_top
                .min(total_rows(count, columns).saturating_sub(visible));
        } else {
            s.scroll_top = scroll_to_selection(s.selection, columns, visible, s.scroll_top, count);
        }
        let start_row = s.scroll_top.saturating_sub(OVERSCAN_ROWS);
        let end_row = (s.scroll_top + visible + OVERSCAN_ROWS).min(total_rows(count, columns));
        let start = start_row * columns;
        let end = (end_row * columns).min(count);
        // Do not throw away the image model on every left/right focus step;
        // rebuild only when the overscan window or filtered order changes.
        let refresh_window = s.window_dirty
            || s.mounted_start != start
            || s.mounted_len != end.saturating_sub(start);
        s.mounted_start = start;
        s.mounted_len = end.saturating_sub(start);
        s.window_dirty = false;
        let selected = s.order.get(s.selection).copied();
        let selected_game_id = selected.map(|original| self.catalog[original].game().id().get());
        if s.metadata_game_id != selected_game_id {
            s.metadata_game_id = selected_game_id;
            s.metadata_sequence = s.metadata_sequence.wrapping_add(1);
        }
        let metadata_sequence = s.metadata_sequence;
        let browse_sequence = s.browse_sequence;
        let browse_transition_kind = s.browse_transition_kind;
        let cards: Vec<LibraryCardData> = if refresh_window {
            s.order[start..end]
                .iter()
                .enumerate()
                .filter_map(|(i, &original)| {
                    self.home.card_at(original).map(|card| LibraryCardData {
                        game: card,
                        source: self.catalog[original]
                            .sources()
                            .iter()
                            .map(|source| source_title(source.source_id().as_str()))
                            .collect::<Vec<_>>()
                            .join(" · ")
                            .into(),
                        absolute_index: (start + i) as i32,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        let (title, source) = selected
            .map(|index| {
                (
                    self.catalog[index].game().title().as_str().to_owned(),
                    self.catalog[index]
                        .sources()
                        .iter()
                        .map(|source| source_title(source.source_id().as_str()))
                        .collect::<Vec<_>>()
                        .join(" · "),
                )
            })
            .unwrap_or_default();
        let filter = s
            .source_index
            .checked_sub(1)
            .and_then(|index| self.sources.get(index))
            .map(|source| source_title(source))
            .unwrap_or_else(|| "All sources".to_owned());
        let sort = s.sort.label();
        let selection = s.selection;
        let top = s.scroll_top;
        drop(s);
        // Signal the new arrangement before mutating the model/scroll offsets.
        // This lets Slint choose an instant camera reposition while the two
        // stationary snapshots crossfade rather than also sliding the grid.
        ui.set_library_browse_transition_kind(browse_transition_kind);
        ui.set_library_browse_sequence(browse_sequence);
        if refresh_window {
            self.rows.set_vec(cards);
        }
        ui.set_library_grid_columns(columns as i32);
        ui.set_library_scroll_row(top as i32);
        ui.set_library_selected_index(selection as i32);
        ui.set_library_selected_title(title.into());
        ui.set_library_selected_source(source.into());
        // Publish after both metadata strings so Slint captures a coherent pair.
        ui.set_library_metadata_sequence(metadata_sequence);
        ui.set_library_filter_text(filter.into());
        ui.set_library_sort_text(sort.into());
        ui.set_library_total_count(count as i32);
        ui.set_library_count_text(
            format!("{} {}", count, if count == 1 { "game" } else { "games" }).into(),
        );
        ui.set_library_collection_count(self.catalog.len() as i32);
    }

    pub fn on_card_updated(&self, original: usize, card: crate::GameCardData) {
        let s = self.state.borrow();
        if let Some(position) = s
            .order
            .iter()
            .skip(s.mounted_start)
            .take(s.mounted_len)
            .position(|i| *i == original)
            && let Some(mut row) = self.rows.row_data(position)
        {
            row.game = card;
            self.rows.set_row_data(position, row);
        }
    }

    pub fn select_on_page(&self, ui: &AppWindow, index: i32) {
        if index < 0 {
            return;
        }
        let index = index as usize;
        let mut s = self.state.borrow_mut();
        if index >= s.order.len() {
            return;
        }
        let same = s.selection == index;
        s.selection = index;
        // Explicit pointer selection reanchors the camera to controller focus.
        // Merely turning the mouse wheel never changes that focus identity.
        s.wheel_scrolled = false;
        let original = if same {
            s.order.get(index).copied()
        } else {
            None
        };
        drop(s);
        self.publish(ui);
        // An already-selected cover behaves as the primary launch action.
        if let Some(original) = original {
            self.home.launch_from_library(ui, original);
        }
    }

    fn move_selection(&self, ui: &AppWindow, target: usize) {
        let mut s = self.state.borrow_mut();
        if s.order.is_empty() || target >= s.order.len() {
            return;
        }
        if target == s.selection && !s.wheel_scrolled {
            return;
        }
        s.selection = target;
        // First keyboard/controller movement after a wheel pan restores the
        // selected cover to view, even if navigation hit a list boundary.
        s.wheel_scrolled = false;
        drop(s);
        self.publish(ui);
    }

    fn move_grid_horizontally(&self, ui: &AppWindow, delta: i32) {
        let target = {
            let s = self.state.borrow();
            horizontal_step(s.selection, s.order.len(), delta)
        };
        self.move_selection(ui, target);
    }

    fn move_vertical(&self, ui: &AppWindow, down: bool) {
        let columns = grid_columns(ui.get_logical_viewport_width_px());
        let s = self.state.borrow();
        let target = vertical_step(s.selection, s.order.len(), columns, down);
        drop(s);
        self.move_selection(ui, target);
    }

    pub fn scroll_by_row(&self, ui: &AppWindow, direction: i32) {
        if direction == 0 {
            return;
        }
        let layout = GridLayout::for_width(ui.get_logical_viewport_width_px());
        let visible = visible_rows(ui.get_logical_viewport_height_px(), layout);
        let mut s = self.state.borrow_mut();
        let next = wheel_scroll_top(
            s.scroll_top,
            s.order.len(),
            layout.columns,
            visible,
            direction,
        );
        if next == s.scroll_top {
            return;
        }
        s.scroll_top = next;
        s.wheel_scrolled = true;
        drop(s);
        self.publish(ui);
    }

    pub fn cycle_filter(&self, ui: &AppWindow) {
        if self.sources.is_empty() {
            return;
        }
        let selected_id = self.selection_game_id();
        self.capture_previous_grid(ui);
        {
            let mut s = self.state.borrow_mut();
            s.source_index = (s.source_index + 1) % (self.sources.len() + 1);
            s.browse_transition_kind = 1;
            s.browse_sequence = s.browse_sequence.wrapping_add(1);
        }
        self.rebuild(ui, selected_id);
    }
    pub fn cycle_sort(&self, ui: &AppWindow) {
        let selected_id = self.selection_game_id();
        self.refresh_metrics();
        self.capture_previous_grid(ui);
        {
            let mut s = self.state.borrow_mut();
            s.sort = s.sort.next();
            s.browse_transition_kind = 2;
            s.browse_sequence = s.browse_sequence.wrapping_add(1);
        }
        self.rebuild(ui, selected_id);
    }

    pub fn handle_action(&self, ui: &AppWindow, event: UiActionEvent) {
        match event.action {
            UiAction::Left => self.move_grid_horizontally(ui, -1),
            UiAction::Right => self.move_grid_horizontally(ui, 1),
            UiAction::Down => self.move_vertical(ui, true),
            UiAction::Up => self.move_vertical(ui, false),
            UiAction::LeftBumper if !event.repeated => self.cycle_filter(ui),
            UiAction::RightBumper if !event.repeated => self.cycle_sort(ui),
            UiAction::Accept if !event.repeated => {
                let original = {
                    let s = self.state.borrow();
                    s.order.get(s.selection).copied()
                };
                if let Some(original) = original {
                    self.home.launch_from_library(ui, original);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CARD_GAP, GRID_PAD, GridLayout, LibrarySort, OVERSCAN_ROWS, grid_columns, horizontal_step,
        scroll_to_selection, sorted_catalog_indices, source_title, total_rows, vertical_step,
        visible_rows, wheel_scroll_top,
    };
    use crate::domain::{
        ExternalGameId, Game, GameId, GameTitle, LibraryGame, SourceGameRef, SourceId,
    };

    fn game(id: i64, title: &str, source: &str) -> LibraryGame {
        LibraryGame::new(
            Game::new(GameId::new(id).unwrap(), GameTitle::new(title).unwrap()),
            vec![SourceGameRef::new(
                SourceId::new(source).unwrap(),
                ExternalGameId::new(id.to_string()).unwrap(),
            )],
        )
    }

    #[test]
    fn mouse_wheel_scroll_is_clamped_independently_of_selection() {
        assert_eq!(wheel_scroll_top(0, 24, 4, 3, 1), 1);
        assert_eq!(wheel_scroll_top(3, 24, 4, 3, -1), 2);
        assert_eq!(wheel_scroll_top(3, 24, 4, 3, 2), 3); // bottom
        assert_eq!(wheel_scroll_top(0, 2, 4, 3, 1), 0); // all visible
        assert_eq!(wheel_scroll_top(0, 24, 4, 3, -1), 0);
    }

    #[test]
    fn stable_title_order_and_source_membership() {
        let catalog = vec![
            game(9, "zelda", "steam"),
            game(3, "Alpha", "heroic"),
            game(2, "alpha", "steam"),
        ];
        let empty = std::collections::BTreeMap::new();
        assert_eq!(
            sorted_catalog_indices(&catalog, None, LibrarySort::Alphabetical, &empty),
            vec![2, 1, 0]
        );
        assert_eq!(
            sorted_catalog_indices(&catalog, Some("steam"), LibrarySort::Alphabetical, &empty),
            vec![2, 0]
        );
        assert_eq!(
            sorted_catalog_indices(&catalog, Some("heroic"), LibrarySort::Alphabetical, &empty),
            vec![1]
        );
    }

    #[test]
    fn all_sort_modes_and_missing_playtime_are_deterministic() {
        use crate::services::activity::LibrarySortMetrics;
        let games = vec![
            game(1, "Alpha", "steam"),
            game(2, "Beta", "steam"),
            game(3, "Gamma", "heroic"),
        ];
        let stats = [
            LibrarySortMetrics {
                game_id: GameId::new(1).unwrap(),
                added_at: 10,
                last_played_at: Some(50),
                observed_seconds: 300,
                reported_seconds: Some(100),
            },
            LibrarySortMetrics {
                game_id: GameId::new(2).unwrap(),
                added_at: 30,
                last_played_at: Some(90),
                observed_seconds: 200,
                reported_seconds: Some(2000),
            },
            LibrarySortMetrics {
                game_id: GameId::new(3).unwrap(),
                added_at: 20,
                last_played_at: None,
                observed_seconds: 900,
                reported_seconds: None,
            },
        ]
        .into_iter()
        .map(|row| (row.game_id.get(), row))
        .collect();
        assert_eq!(
            sorted_catalog_indices(&games, None, LibrarySort::Alphabetical, &stats),
            vec![0, 1, 2]
        );
        assert_eq!(
            sorted_catalog_indices(&games, None, LibrarySort::RecentlyPlayed, &stats),
            vec![1, 0, 2]
        );
        assert_eq!(
            sorted_catalog_indices(&games, None, LibrarySort::TimePlayed, &stats),
            vec![1, 2, 0]
        );
        assert_eq!(
            sorted_catalog_indices(&games, None, LibrarySort::RecentlyAdded, &stats),
            vec![1, 2, 0]
        );
        assert_eq!(
            sorted_catalog_indices(&games, Some("steam"), LibrarySort::TimePlayed, &stats),
            vec![1, 0]
        );
        assert_eq!(stats.get(&1).unwrap().time_played_seconds(), 100); // NOT 100 + 300
        assert_eq!(LibrarySort::RecentlyAdded.next(), LibrarySort::Alphabetical);
    }

    #[test]
    fn responsive_grid_and_bounded_window() {
        assert_eq!(grid_columns(760.0), 4);
        assert_eq!(grid_columns(930.0), 4);
        assert_eq!(grid_columns(1280.0), 5); // 720p window / Switch-like layout
        assert_eq!(grid_columns(1920.0), 6); // 1080p
        assert_eq!(grid_columns(2560.0), 8);
        assert_eq!(grid_columns(3440.0), 10);
        assert_eq!(visible_rows(720.0, GridLayout::for_width(1280.0)), 2);
        assert_eq!(visible_rows(800.0, GridLayout::for_width(1920.0)), 2);
        assert_eq!(visible_rows(1080.0, GridLayout::for_width(1920.0)), 3);
        assert_eq!(visible_rows(560.0, GridLayout::for_width(1280.0)), 1);
        assert_eq!(total_rows(36, 5), 8);
        assert_eq!(scroll_to_selection(22, 5, 2, 0, 36), 3);
        assert_eq!(OVERSCAN_ROWS, 2);
        // Selected ring and glow remain within the viewport at each width.
        for width in [540.0, 760.0, 930.0, 1280.0, 1920.0, 2560.0, 3440.0] {
            let layout = GridLayout::for_width(width);
            let gallery = layout.columns as f32 * layout.stride - CARD_GAP + 2.0 * GRID_PAD;
            assert!(
                gallery <= width - 48.0,
                "gallery exceeds viewport at {width}"
            );
            assert!(layout.stride - CARD_GAP >= 132.0);
        }
        assert_eq!(source_title("heroic"), "Heroic");
    }

    #[test]
    fn horizontal_navigation_is_contiguous_and_clamps_at_collection_edges() {
        // Repeated Right crosses the row boundary, exactly like a fresh press.
        assert_eq!(horizontal_step(6, 36, 1), 7);
        assert_eq!(horizontal_step(13, 36, 1), 14);
        assert_eq!(horizontal_step(7, 36, -1), 6);
        assert_eq!(horizontal_step(0, 36, -1), 0);
        assert_eq!(horizontal_step(35, 36, 1), 35);
        assert_eq!(horizontal_step(0, 0, 1), 0);
    }

    #[test]
    fn vertical_navigation_preserves_columns_and_clamps_at_edges() {
        assert_eq!(vertical_step(29, 36, 7, true), 35);
        assert_eq!(vertical_step(35, 36, 7, true), 35);
        assert_eq!(vertical_step(1, 36, 7, false), 1);
        assert_eq!(vertical_step(14, 36, 7, false), 7);
    }
}
