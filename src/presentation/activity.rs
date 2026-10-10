use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use chrono::{Local, TimeZone};
use slint::{Model, ModelRc, VecModel};
use tracing::warn;

use crate::{
    ActivityCoverData, ActivityDayData, ActivityHistoryRowData, AppWindow, GameCardData,
    domain::{GameId, LibraryGame, PlaySessionState, PlaytimeSeconds},
    presentation::home::HomeController,
    services::activity::{ActivityOverview, ActivityRepository, ActivityService},
};

const RECENT_SESSION_LIMIT: usize = 6;
const TOP_GAME_LIMIT: usize = 12;

/// Pure presentation adapter for persisted Activity data.
///
/// Session lifecycle and SQL remain in services/persistence. This layer only
/// formats stable domain/application values for Slint.
pub struct ActivityController;

impl ActivityController {
    pub const fn recent_session_limit() -> usize {
        RECENT_SESSION_LIMIT
    }

    pub const fn top_game_limit() -> usize {
        TOP_GAME_LIMIT
    }

    pub fn publish(ui: &AppWindow, overview: &ActivityOverview) {
        let week_seconds: i64 = overview
            .week_days()
            .iter()
            .map(|(_, seconds)| *seconds)
            .fold(0_i64, i64::saturating_add);
        ui.set_activity_week_total(format_seconds(week_seconds).into());
        ui.set_activity_month_total(format_seconds(overview.month_seconds()).into());
        let maximum = overview
            .week_days()
            .iter()
            .map(|(_, value)| *value)
            .max()
            .unwrap_or(0)
            .max(1) as f32;
        let days = overview
            .week_days()
            .iter()
            .map(|(label, value)| ActivityDayData {
                label: label.as_str().into(),
                duration: format_seconds(*value).into(),
                fraction: (*value as f32 / maximum).clamp(0.0, 1.0),
            })
            .collect::<Vec<_>>();
        ui.set_activity_days(ModelRc::from(Rc::new(VecModel::from(days))));
    }
}

/// Keeps the Activity cover model mounted between route changes and refreshes.
/// Refreshing on entry changes the data *before* the route becomes visible;
/// asynchronous Home artwork is then forwarded to the existing rows directly.
pub struct ActivityShowcaseController {
    catalog: Vec<LibraryGame>,
    home: Rc<HomeController>,
    covers: Rc<VecModel<ActivityCoverData>>,
    displayed_ids: RefCell<Vec<GameId>>,
}

impl ActivityShowcaseController {
    pub fn new(ui: &AppWindow, catalog: Vec<LibraryGame>, home: Rc<HomeController>) -> Rc<Self> {
        let covers = Rc::new(VecModel::from(Vec::<ActivityCoverData>::new()));
        ui.set_activity_covers(ModelRc::from(Rc::clone(&covers)));
        Rc::new(Self {
            catalog,
            home,
            covers,
            displayed_ids: RefCell::new(Vec::new()),
        })
    }

    pub fn refresh(&self, ui: &AppWindow, overview: &ActivityOverview) {
        let mut new_ids = Vec::new();
        let mut new_covers = Vec::new();
        for summary in overview.top_games() {
            let Some(index) = self
                .catalog
                .iter()
                .position(|game| game.game().id() == summary.game_id())
            else {
                continue;
            };
            let Some(game) = self.home.card_at(index) else {
                continue;
            };
            new_ids.push(summary.game_id());
            new_covers.push(ActivityCoverData {
                game,
                observed_time: format_duration(summary.observed_playtime()).into(),
            });
        }

        let previous_ids = self.displayed_ids.borrow();
        if *previous_ids == new_ids {
            drop(previous_ids);
            // The same games in the same order need no model replacement,
            // hence no flash/reset on the existing three-second sync.
            for (index, updated) in new_covers.into_iter().enumerate() {
                if let Some(existing) = self.covers.row_data(index)
                    && existing.observed_time != updated.observed_time
                {
                    self.covers.set_row_data(index, updated);
                }
            }
            return;
        }

        let selected =
            retained_selected_index(&previous_ids, ui.get_activity_selected_index(), &new_ids);
        drop(previous_ids);
        self.covers.set_vec(new_covers);
        *self.displayed_ids.borrow_mut() = new_ids;
        ui.set_activity_selected_index(selected as i32);
    }

    pub fn game_at(&self, index: usize) -> Option<(GameId, GameCardData, usize)> {
        let game_id = *self.displayed_ids.borrow().get(index)?;
        let catalog_index = self.catalog_index_for(game_id)?;
        Some((game_id, self.covers.row_data(index)?.game, catalog_index))
    }

    pub fn source_labels_for(&self, game_id: GameId) -> String {
        self.catalog
            .iter()
            .find(|game| game.game().id() == game_id)
            .map(|game| {
                game.sources()
                    .iter()
                    .map(|source| source_display_name(source.source_id().as_str()))
                    .collect::<Vec<_>>()
                    .join(" · ")
            })
            .unwrap_or_default()
    }

    pub fn catalog_index_for(&self, game_id: GameId) -> Option<usize> {
        self.catalog
            .iter()
            .position(|game| game.game().id() == game_id)
    }

    /// A Home artwork lookup finishing must not wait for Activity's timer.
    /// Update the one matching cover without remounting the carousel model.
    pub fn on_card_updated(&self, catalog_index: usize, card: GameCardData) {
        let Some(game) = self.catalog.get(catalog_index) else {
            return;
        };
        let position = self
            .displayed_ids
            .borrow()
            .iter()
            .position(|id| id == &game.game().id());
        let Some(position) = position else {
            return;
        };
        if let Some(mut cover) = self.covers.row_data(position) {
            cover.game = card;
            self.covers.set_row_data(position, cover);
        }
    }
}

/// Restore selection by durable GameId, not the previous visual position.
fn retained_selected_index(previous: &[GameId], selected: i32, next: &[GameId]) -> usize {
    previous
        .get(selected.max(0) as usize)
        .and_then(|id| next.iter().position(|candidate| candidate == id))
        .unwrap_or(0)
        .min(next.len().saturating_sub(1))
}

/// Controller boundary for the nested Activity game details view. Uses only the
/// generic Activity repository and stable GameId. It cannot launch games.
pub trait ActivityDetailsActions {
    fn open(&self, ui: &AppWindow, cover_index: usize) -> bool;
    fn close(&self, ui: &AppWindow);
    fn scroll_rows(&self, ui: &AppWindow, delta: i32);
    fn refresh_visible_rows(&self, ui: &AppWindow);
    fn update_artwork(&self, ui: &AppWindow, catalog_index: usize, card: GameCardData);
}

pub struct ActivityDetailsController<R: ActivityRepository> {
    activity: Rc<ActivityService<R>>,
    showcase: Rc<ActivityShowcaseController>,
    rows: RefCell<Vec<ActivityHistoryRowData>>,
    active_game: Cell<Option<GameId>>,
}

impl<R: ActivityRepository> ActivityDetailsController<R> {
    pub fn new(
        activity: Rc<ActivityService<R>>,
        showcase: Rc<ActivityShowcaseController>,
    ) -> Rc<Self> {
        Rc::new(Self {
            activity,
            showcase,
            rows: RefCell::new(Vec::new()),
            active_game: Cell::new(None),
        })
    }

    fn publish_window(&self, ui: &AppWindow, requested_first: usize) {
        let rows = self.rows.borrow();
        let visible = ui.get_activity_detail_visible_rows().max(1) as usize;
        let max_first = rows.len().saturating_sub(visible);
        let first = requested_first.min(max_first);
        let slice = rows
            .iter()
            .skip(first)
            .take(visible)
            .cloned()
            .collect::<Vec<_>>();
        ui.set_activity_detail_first_row(first.min(i32::MAX as usize) as i32);
        ui.set_activity_detail_rows(ModelRc::from(Rc::new(VecModel::from(slice))));
    }
}

impl<R: ActivityRepository + 'static> ActivityDetailsActions for ActivityDetailsController<R> {
    fn open(&self, ui: &AppWindow, index: usize) -> bool {
        let Some((game_id, card, _catalog_index)) = self.showcase.game_at(index) else {
            return false;
        };
        let history = match self.activity.game_activity_history(game_id) {
            Ok(history) => history,
            Err(error) => {
                warn!(game_id = game_id.get(), %error, "could not open per-game Activity history");
                return false;
            }
        };
        let mut rows = Vec::with_capacity(history.sessions.len());
        let mut total_seconds = 0_i64;
        let mut longest = 0_i64;
        let mut ended_count = 0_i64;
        for session in &history.sessions {
            let duration = session.duration();
            if let Some(duration) = duration {
                total_seconds = total_seconds.saturating_add(duration.get());
                longest = longest.max(duration.get());
                ended_count += 1;
            }
            let date = format_full_local_date(session.started_at());
            let started = format_local_clock(session.started_at());
            let times = session
                .ended_at()
                .map(|end| format!("{} – {}", started, format_local_clock(end)))
                .unwrap_or(started);
            let status = match session.state() {
                PlaySessionState::Completed => source_display_name(session.source_id().as_str()),
                PlaySessionState::Interrupted => "Recovered".to_owned(),
                PlaySessionState::Open => "Playing now".to_owned(),
            };
            rows.push(ActivityHistoryRowData {
                date: date.into(),
                times: times.into(),
                duration: duration
                    .map(format_duration)
                    .unwrap_or_else(|| match session.state() {
                        PlaySessionState::Open => "In progress".to_owned(),
                        _ => "Unknown duration".to_owned(),
                    })
                    .into(),
                status: status.into(),
            });
        }
        let last_played = history
            .sessions
            .first()
            .map(|s| format_full_local_date(s.started_at()))
            .unwrap_or_else(|| "Never recorded".to_owned());
        let provider = history.reported.first(); // largest; never sum providers
        let provider_label = provider
            .map(|(id, _)| format!("{} lifetime", source_display_name(id.as_str())))
            .unwrap_or_else(|| "Source lifetime".to_owned());
        let provider_time = provider
            .map(|(_, time)| format_duration(*time))
            .unwrap_or_else(|| "Not available".to_owned());
        ui.set_activity_detail_game(card);
        ui.set_activity_detail_source(self.showcase.source_labels_for(game_id).into());
        ui.set_activity_detail_last_played(last_played.into());
        ui.set_activity_detail_observed(format_seconds(total_seconds).into());
        ui.set_activity_detail_provider_label(provider_label.into());
        ui.set_activity_detail_provider_time(provider_time.into());
        ui.set_activity_detail_sessions_summary(format_session_count(rows.len()).into());
        ui.set_activity_detail_session_count(rows.len() as i32);
        ui.set_activity_detail_longest(if ended_count > 0 {
            format_seconds(longest).into()
        } else {
            "—".into()
        });
        ui.set_activity_detail_average(if ended_count > 0 {
            format_seconds(total_seconds / ended_count).into()
        } else {
            "—".into()
        });
        *self.rows.borrow_mut() = rows;
        self.active_game.set(Some(game_id));
        ui.set_activity_detail_first_row(0);
        self.publish_window(ui, 0);
        ui.set_activity_details_visible(true);
        true
    }

    fn close(&self, ui: &AppWindow) {
        // Keep the outgoing details content mounted for the Settings-style
        // reverse crossfade. The next open replaces this cached snapshot.
        // Clearing the row model immediately would flash an empty history
        // while the old page is still animating out.
        ui.set_activity_details_visible(false);
        self.active_game.set(None);
    }

    fn scroll_rows(&self, ui: &AppWindow, delta: i32) {
        let first = ui.get_activity_detail_first_row().max(0) as usize;
        let visible = ui.get_activity_detail_visible_rows().max(1) as usize;
        let target = scroll_history_window(first, delta, self.rows.borrow().len(), visible);
        if target != first {
            self.publish_window(ui, target);
        }
    }

    fn refresh_visible_rows(&self, ui: &AppWindow) {
        if ui.get_activity_details_visible() {
            self.publish_window(ui, ui.get_activity_detail_first_row().max(0) as usize);
        }
    }

    fn update_artwork(&self, ui: &AppWindow, index: usize, card: GameCardData) {
        if !ui.get_activity_details_visible() {
            return;
        }
        if let Some(game_id) = self.active_game.get()
            && self.showcase.catalog_index_for(game_id) == Some(index)
        {
            ui.set_activity_detail_game(card);
        }
    }
}

/// Scroll the read-only history viewport, with no selected session or focus.
/// A short history never scrolls. Clamping also handles a changed viewport.
fn scroll_history_window(first: usize, delta: i32, count: usize, visible: usize) -> usize {
    let max_first = count.saturating_sub(visible.max(1));
    let next = if delta < 0 {
        first.saturating_sub(delta.unsigned_abs() as usize)
    } else {
        first.saturating_add(delta as usize)
    };
    next.min(max_first)
}

fn format_full_local_date(timestamp: i64) -> String {
    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|time| time.format("%b %-d, %Y").to_string())
        .unwrap_or_else(|| "Unknown date".to_owned())
}

fn format_local_clock(timestamp: i64) -> String {
    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|time| time.format("%-I:%M %p").to_string())
        .unwrap_or_else(|| "Unknown time".to_owned())
}

fn format_seconds(seconds: i64) -> String {
    PlaytimeSeconds::new(seconds.max(0))
        .map(format_duration)
        .unwrap_or_else(|_| "0 min".to_owned())
}

fn source_display_name(source: &str) -> String {
    let mut chars = source.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

fn format_duration(duration: PlaytimeSeconds) -> String {
    let seconds = duration.get();
    if seconds == 0 {
        return "0 min".to_owned();
    }
    if seconds < 60 {
        return "<1 min".to_owned();
    }

    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes} min");
    }

    let hours = minutes / 60;
    let remaining_minutes = minutes % 60;
    if remaining_minutes == 0 {
        format!("{hours}h")
    } else {
        format!("{hours}h {remaining_minutes}m")
    }
}

fn format_session_count(count: usize) -> String {
    if count == 1 {
        "1 session".to_owned()
    } else {
        format!("{count} sessions")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_selection_tracks_game_identity_through_reordering() {
        let a = GameId::new(1).expect("game id");
        let b = GameId::new(2).expect("game id");
        let c = GameId::new(3).expect("game id");
        assert_eq!(retained_selected_index(&[a, b, c], 1, &[c, a, b]), 2);
        assert_eq!(retained_selected_index(&[a, b], 1, &[c]), 0);
        assert_eq!(retained_selected_index(&[], 0, &[]), 0);
    }

    #[test]
    fn duration_format_is_compact_for_activity_cards() {
        assert_eq!(
            format_duration(PlaytimeSeconds::new(0).expect("duration")),
            "0 min"
        );
        assert_eq!(
            format_duration(PlaytimeSeconds::new(30).expect("duration")),
            "<1 min"
        );
        assert_eq!(
            format_duration(PlaytimeSeconds::new(3_600).expect("duration")),
            "1h"
        );
        assert_eq!(
            format_duration(PlaytimeSeconds::new(5_100).expect("duration")),
            "1h 25m"
        );
    }

    #[test]
    fn read_only_history_scroll_clamps_and_never_requires_focus() {
        assert_eq!(scroll_history_window(0, 1, 2, 6), 0); // no overflow
        assert_eq!(scroll_history_window(0, 1, 14, 5), 1);
        assert_eq!(scroll_history_window(1, -1, 14, 5), 0);
        assert_eq!(scroll_history_window(0, -1, 14, 5), 0);
        assert_eq!(scroll_history_window(8, 4, 14, 5), 9); // bottom
        assert_eq!(scroll_history_window(9, 1, 14, 5), 9);
        assert_eq!(scroll_history_window(9, 0, 14, 12), 2); // resized
    }

    #[test]
    fn session_count_uses_singular_only_for_one() {
        assert_eq!(format_session_count(1), "1 session");
        assert_eq!(format_session_count(2), "2 sessions");
    }
}
