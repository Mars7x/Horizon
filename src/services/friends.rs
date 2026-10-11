//! The Steam friends shown on the Profile page: who they are and what they're
//! doing, read through the Steam account's Web API key
//! ([`SteamWebApiClient::friends`](crate::services::steam_account::SteamWebApiClient::friends)).
//!
//! Read-only: Horizon never changes anything on Steam. A friend list Steam
//! won't share (a private profile) is reported as such, never as no friends.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

/// What a friend is doing, as Steam reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presence {
    /// In a game: its name.
    Playing(String),
    Online,
    Busy,
    /// Away or snoozing.
    Away,
    /// Offline, with when Steam last saw them when it says.
    Offline {
        last_online: Option<SystemTime>,
    },
}

impl Presence {
    /// Sorting rank: in a game first, offline last.
    fn rank(&self) -> u8 {
        match self {
            Self::Playing(_) => 0,
            Self::Online => 1,
            Self::Busy => 2,
            Self::Away => 3,
            Self::Offline { .. } => 4,
        }
    }

    pub fn is_online(&self) -> bool {
        !matches!(self, Self::Offline { .. })
    }

    /// The line under the friend's name.
    pub fn describe(&self, now: SystemTime) -> String {
        match self {
            Self::Playing(game) => format!("Playing {game}"),
            Self::Online => "Online".into(),
            Self::Busy => "Busy".into(),
            Self::Away => "Away".into(),
            Self::Offline {
                last_online: Some(seen),
            } => match now.duration_since(*seen) {
                Ok(ago) => format!("Last online {}", ago_text(ago)),
                Err(_) => "Offline".into(),
            },
            Self::Offline { last_online: None } => "Offline".into(),
        }
    }
}

fn ago_text(ago: Duration) -> String {
    let minutes = ago.as_secs() / 60;
    let plural = |n: u64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match minutes {
        0 => "just now".into(),
        1..60 => plural(minutes, "minute"),
        60..1440 => plural(minutes / 60, "hour"),
        1440..43200 => plural(minutes / 1440, "day"),
        43200..525600 => plural(minutes / 43200, "month"),
        _ => plural(minutes / 525600, "year"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Friend {
    pub steam_id: String,
    pub name: String,
    pub presence: Presence,
    /// Steam's 64 px avatar, on one of its avatar hosts.
    pub avatar_url: Option<String>,
}

/// Why the friends couldn't be listed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FriendsError {
    #[error("the Steam friend list is private")]
    Private,
    #[error("Steam didn't answer: {0}")]
    Network(String),
    #[error("Steam's answer couldn't be read")]
    Unreadable,
}

/// Most friends asked about in one GetPlayerSummaries request.
pub const SUMMARIES_PER_REQUEST: usize = 100;

/// The SteamID64s in a GetFriendList answer (friends only, well-formed IDs).
pub fn friend_ids(body: &str) -> Result<Vec<String>, FriendsError> {
    #[derive(Deserialize)]
    struct Answer {
        friendslist: Option<List>,
    }
    #[derive(Deserialize)]
    struct List {
        #[serde(default)]
        friends: Vec<Entry>,
    }
    #[derive(Deserialize)]
    struct Entry {
        steamid: String,
        relationship: Option<String>,
    }
    let answer = serde_json::from_str::<Answer>(body).map_err(|_| FriendsError::Unreadable)?;
    Ok(answer
        .friendslist
        .map(|list| list.friends)
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.relationship.as_deref().is_none_or(|r| r == "friend"))
        .map(|entry| entry.steamid)
        .filter(|id| id.len() == 17 && id.bytes().all(|b| b.is_ascii_digit()))
        .collect())
}

/// The friends in a GetPlayerSummaries answer.
pub fn summaries(body: &str) -> Result<Vec<Friend>, FriendsError> {
    #[derive(Deserialize)]
    struct Answer {
        response: Players,
    }
    #[derive(Deserialize)]
    struct Players {
        #[serde(default)]
        players: Vec<Player>,
    }
    #[derive(Deserialize)]
    struct Player {
        steamid: String,
        #[serde(default)]
        personaname: String,
        #[serde(default)]
        personastate: u8,
        gameextrainfo: Option<String>,
        lastlogoff: Option<u64>,
        avatarmedium: Option<String>,
    }
    let answer = serde_json::from_str::<Answer>(body).map_err(|_| FriendsError::Unreadable)?;
    Ok(answer
        .response
        .players
        .into_iter()
        .map(|player| {
            let game = player
                .gameextrainfo
                .map(|game| game.trim().to_owned())
                .filter(|game| !game.is_empty());
            let presence = match (game, player.personastate) {
                (Some(game), _) => Presence::Playing(game),
                (None, 0) => Presence::Offline {
                    last_online: player
                        .lastlogoff
                        .filter(|&seconds| seconds > 0)
                        .map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds)),
                },
                (None, 2) => Presence::Busy,
                (None, 3 | 4) => Presence::Away,
                // Online, looking to trade, looking to play.
                (None, _) => Presence::Online,
            };
            let name = player.personaname.trim().to_owned();
            Friend {
                name: if name.is_empty() {
                    player.steamid.clone()
                } else {
                    name
                },
                steam_id: player.steamid,
                presence,
                avatar_url: player.avatarmedium.filter(|url| !url.is_empty()),
            }
        })
        .collect())
}

/// In a game first, then online, busy, away and offline; by name within each.
pub fn sort(friends: &mut [Friend]) {
    friends.sort_by_cached_key(|friend| (friend.presence.rank(), friend.name.to_lowercase()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friend_ids_keep_only_well_formed_friends() {
        let body = r#"{"friendslist":{"friends":[
            {"steamid":"76561198000000001","relationship":"friend","friend_since":1},
            {"steamid":"76561198000000002","relationship":"ignored"},
            {"steamid":"not-an-id","relationship":"friend"},
            {"steamid":"76561198000000003"}]}}"#;
        assert_eq!(
            friend_ids(body).unwrap(),
            ["76561198000000001", "76561198000000003"]
        );
        // Steam answers `{}` for an account with no friends.
        assert_eq!(friend_ids("{}").unwrap(), Vec::<String>::new());
        assert_eq!(friend_ids("not json"), Err(FriendsError::Unreadable));
    }

    #[test]
    fn summaries_read_presence_and_sorting_puts_games_first() {
        let body = r#"{"response":{"players":[
            {"steamid":"1","personaname":"zed","personastate":0,"lastlogoff":1000},
            {"steamid":"2","personaname":"Amy","personastate":1,"gameextrainfo":"Hades II",
             "avatarmedium":"https://avatars.steamstatic.com/a_medium.jpg"},
            {"steamid":"3","personaname":"bo","personastate":3},
            {"steamid":"4","personaname":"Cy","personastate":6},
            {"steamid":"5","personaname":"  ","personastate":2},
            {"steamid":"6","personaname":"al","personastate":0}]}}"#;
        let mut friends = summaries(body).unwrap();
        sort(&mut friends);
        let names: Vec<_> = friends.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Amy", "Cy", "5", "bo", "al", "zed"]);
        assert_eq!(friends[0].presence, Presence::Playing("Hades II".into()));
        assert_eq!(
            friends[0].avatar_url.as_deref(),
            Some("https://avatars.steamstatic.com/a_medium.jpg")
        );
        assert_eq!(
            friends[5].presence,
            Presence::Offline {
                last_online: Some(UNIX_EPOCH + Duration::from_secs(1000))
            }
        );
        assert_eq!(friends[4].presence, Presence::Offline { last_online: None });
        assert!(friends[3].presence.is_online());
        assert!(!friends[4].presence.is_online());
    }

    #[test]
    fn presence_reads_as_a_short_line() {
        let now = UNIX_EPOCH + Duration::from_secs(10_000_000);
        let seen = |seconds: u64| Presence::Offline {
            last_online: Some(now - Duration::from_secs(seconds)),
        };
        assert_eq!(
            Presence::Playing("Celeste".into()).describe(now),
            "Playing Celeste"
        );
        assert_eq!(Presence::Away.describe(now), "Away");
        assert_eq!(seen(30).describe(now), "Last online just now");
        assert_eq!(seen(60).describe(now), "Last online 1 minute ago");
        assert_eq!(seen(3 * 3600).describe(now), "Last online 3 hours ago");
        assert_eq!(seen(2 * 86400).describe(now), "Last online 2 days ago");
        assert_eq!(seen(400 * 86400).describe(now), "Last online 1 year ago");
        assert_eq!(
            Presence::Offline { last_online: None }.describe(now),
            "Offline"
        );
    }
}
