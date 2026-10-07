use std::rc::Rc;

use chrono::{Local, TimeZone, Utc};
use slint::{ModelRc, VecModel};

use crate::{
    ActivityGameData, ActivitySessionData, AppWindow,
    domain::{PlaytimeSeconds, SessionTrackingMethod},
    services::activity::ActivityOverview,
};

const RECENT_SESSION_LIMIT: usize = 6;
const TOP_GAME_LIMIT: usize = 4;

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
        ui.set_activity_total_playtime(format_duration(overview.observed_playtime()).into());
        ui.set_activity_session_count(overview.completed_sessions().to_string().into());
        ui.set_activity_game_count(overview.played_games().to_string().into());

        let recent = overview
            .recent_sessions()
            .iter()
            .map(|entry| ActivitySessionData {
                title: entry.title().as_str().into(),
                duration: entry
                    .session()
                    .duration()
                    .map(format_duration)
                    .unwrap_or_else(|| "Unknown".to_owned())
                    .into(),
                when: format_timestamp(entry.session().started_at()).into(),
                method: tracking_method_label(entry.session().tracking_method()).into(),
            })
            .collect::<Vec<_>>();
        ui.set_activity_recent_sessions(ModelRc::from(Rc::new(VecModel::from(recent))));

        let top_games = overview
            .top_games()
            .iter()
            .map(|entry| ActivityGameData {
                title: entry.title().as_str().into(),
                playtime: format_duration(entry.observed_playtime()).into(),
                sessions: format_session_count(entry.completed_sessions()).into(),
            })
            .collect::<Vec<_>>();
        ui.set_activity_top_games(ModelRc::from(Rc::new(VecModel::from(top_games))));
    }
}

fn tracking_method_label(method: SessionTrackingMethod) -> &'static str {
    match method {
        SessionTrackingMethod::ForegroundHandoff => "Observed foreground",
        SessionTrackingMethod::ManagedSession => "Managed session",
        SessionTrackingMethod::SourceRuntime => "Source runtime",
    }
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

fn format_timestamp(timestamp: i64) -> String {
    let Some(utc) = Utc.timestamp_opt(timestamp, 0).single() else {
        return "Unknown time".to_owned();
    };
    let local = utc.with_timezone(&Local);
    if local.date_naive() == Local::now().date_naive() {
        "Today".to_owned()
    } else {
        local.format("%b %-d").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_format_is_compact_for_activity_cards() {
        assert_eq!(format_duration(PlaytimeSeconds::new(0).expect("duration")), "0 min");
        assert_eq!(format_duration(PlaytimeSeconds::new(30).expect("duration")), "<1 min");
        assert_eq!(format_duration(PlaytimeSeconds::new(3_600).expect("duration")), "1h");
        assert_eq!(format_duration(PlaytimeSeconds::new(5_100).expect("duration")), "1h 25m");
    }

    #[test]
    fn tracking_method_labels_cover_every_activity_method() {
        assert_eq!(
            tracking_method_label(SessionTrackingMethod::ForegroundHandoff),
            "Observed foreground"
        );
        assert_eq!(
            tracking_method_label(SessionTrackingMethod::ManagedSession),
            "Managed session"
        );
        assert_eq!(
            tracking_method_label(SessionTrackingMethod::SourceRuntime),
            "Source runtime"
        );
    }

    #[test]
    fn session_count_uses_singular_only_for_one() {
        assert_eq!(format_session_count(1), "1 session");
        assert_eq!(format_session_count(2), "2 sessions");
    }
}
