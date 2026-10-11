//! The Steam friends panel on the Profile page.
//!
//! The list comes from the Steam account's Web API key on a worker thread
//! (`SteamWebApiClient::friends`), refreshed when the page opens if it is
//! more than a minute old. Avatars follow, a few at a time, and are kept by
//! URL so a refresh downloads only new ones. The list is read-only: nothing
//! in it takes focus; Up/Down and the wheel scroll it (`ScrollCamera`).

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::mpsc::Receiver,
    time::{Duration, Instant, SystemTime},
};

use image::RgbaImage;
use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use tracing::warn;

use crate::{
    AppWindow, FriendData,
    presentation::wake::drain_notify,
    services::{
        friends::{self, Friend, FriendsError},
        image as decoding,
        steam_account::{SteamAccountService, SteamWebApiClient},
        worker::{WorkerSender, spawn, worker_channel},
    },
};

/// A list younger than this isn't fetched again on opening the page.
const FRESH_FOR: Duration = Duration::from_secs(60);
/// Avatars downloaded at once.
const AVATAR_WORKERS: usize = 4;
/// Side avatars are kept at: Steam's medium avatar, sharp at 2x in a row.
const AVATAR_SIDE: u32 = 64;

/// What the panel shows (`profile-friends-state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// No Steam account set up.
    NoAccount = 0,
    /// The first fetch is under way.
    Loading = 1,
    Ready = 2,
    /// Steam won't share the friend list.
    Private = 3,
    /// The fetch failed and there is no earlier list.
    Failed = 4,
}

enum FriendsEvent {
    Listed {
        generation: u64,
        result: Result<Vec<Friend>, FriendsError>,
    },
    Avatar {
        url: String,
        picture: RgbaImage,
    },
}

pub struct FriendsController {
    ui: slint::Weak<AppWindow>,
    steam_account: Rc<RefCell<SteamAccountService>>,
    sender: WorkerSender<FriendsEvent>,
    receiver: RefCell<Receiver<FriendsEvent>>,
    friends: RefCell<Vec<Friend>>,
    rows: Rc<VecModel<FriendData>>,
    avatars: RefCell<HashMap<String, Image>>,
    state: Cell<State>,
    /// A fetch is under way.
    fetching: Cell<bool>,
    /// When the shown list was fetched.
    fetched_at: Cell<Option<Instant>>,
    /// Bumped by an account change, so an older fetch's answer is ignored.
    generation: Cell<u64>,
    /// The row at the top of the list's view.
    top_row: Cell<i32>,
}

impl FriendsController {
    pub fn new(ui: &AppWindow, steam_account: Rc<RefCell<SteamAccountService>>) -> Rc<Self> {
        let (notify, drain) = drain_notify(|controller: &FriendsController| controller.drain());
        let (sender, receiver) = worker_channel(notify);
        let rows = Rc::new(VecModel::<FriendData>::default());
        ui.set_profile_friends(ModelRc::from(Rc::clone(&rows)));
        let connected = steam_account.borrow().connected();
        let controller = Rc::new(Self {
            ui: ui.as_weak(),
            steam_account,
            sender,
            receiver: RefCell::new(receiver),
            friends: RefCell::new(Vec::new()),
            rows,
            avatars: RefCell::new(HashMap::new()),
            state: Cell::new(if connected {
                State::Loading
            } else {
                State::NoAccount
            }),
            fetching: Cell::new(false),
            fetched_at: Cell::new(None),
            generation: Cell::new(0),
            top_row: Cell::new(0),
        });
        drain.bind(&controller);
        controller.publish(ui);
        controller
    }

    /// The Profile page opened: the list from its top, refreshed if stale.
    pub fn on_enter(&self, ui: &AppWindow) {
        self.top_row.set(0);
        let stale = self
            .fetched_at
            .get()
            .is_none_or(|at| at.elapsed() >= FRESH_FOR);
        if stale {
            self.fetch();
        }
        // "Last online" lines age while the page is closed.
        self.rebuild_rows();
        self.publish(ui);
    }

    /// The Steam account was saved or removed: start over with it.
    pub fn account_changed(&self, ui: &AppWindow) {
        self.generation.set(self.generation.get() + 1);
        self.fetching.set(false);
        self.fetched_at.set(None);
        self.friends.borrow_mut().clear();
        self.top_row.set(0);
        self.state.set(State::NoAccount);
        self.fetch();
        self.rebuild_rows();
        self.publish(ui);
    }

    /// Move the list by whole rows (Up/Down, or the wheel's report of where
    /// its camera now rests), within the list.
    pub fn scroll(&self, ui: &AppWindow, rows: i32) {
        let visible = ui.get_profile_friends_visible_rows().max(1);
        let last = (self.friends.borrow().len() as i32 - visible).max(0);
        let top = (self.top_row.get() + rows).clamp(0, last);
        if top != self.top_row.get() {
            self.top_row.set(top);
            ui.set_profile_friends_row(top);
        }
    }

    fn fetch(&self) {
        if self.fetching.get() {
            return;
        }
        let Some(credentials) = self.steam_account.borrow().credentials() else {
            self.state.set(State::NoAccount);
            return;
        };
        if self.friends.borrow().is_empty() {
            self.state.set(State::Loading);
        }
        self.fetching.set(true);
        let generation = self.generation.get();
        let sender = self.sender.clone();
        let started = spawn("friends", move || {
            let result = SteamWebApiClient::new(credentials)
                .map_err(|error| FriendsError::Network(error.without_url().to_string()))
                .and_then(|client| client.friends());
            let _ = sender.send(FriendsEvent::Listed { generation, result });
        });
        if !started {
            self.fetching.set(false);
        }
    }

    /// Download the avatars not already kept.
    fn fetch_avatars(&self) {
        let Some(credentials) = self.steam_account.borrow().credentials() else {
            return;
        };
        let mut wanted: Vec<String> = {
            let kept = self.avatars.borrow();
            self.friends
                .borrow()
                .iter()
                .filter_map(|friend| friend.avatar_url.clone())
                .filter(|url| !kept.contains_key(url))
                .collect()
        };
        wanted.sort_unstable();
        wanted.dedup();
        if wanted.is_empty() {
            return;
        }
        let sender = self.sender.clone();
        spawn("friend-avatars", move || {
            let Ok(client) = SteamWebApiClient::new(credentials) else {
                return;
            };
            let per_worker = wanted.len().div_ceil(AVATAR_WORKERS);
            std::thread::scope(|scope| {
                for urls in wanted.chunks(per_worker) {
                    let (client, sender) = (&client, sender.clone());
                    scope.spawn(move || {
                        for url in urls {
                            let picture = client.friend_avatar(url).and_then(|bytes| {
                                decoding::decode(&bytes, None)
                                    .ok()
                                    .map(|image| decoding::fit(image, (AVATAR_SIDE, AVATAR_SIDE)))
                            });
                            if let Some(picture) = picture
                                && sender
                                    .send(FriendsEvent::Avatar {
                                        url: url.clone(),
                                        picture,
                                    })
                                    .is_err()
                            {
                                return;
                            }
                        }
                    });
                }
            });
        });
    }

    fn drain(&self) {
        let events: Vec<_> = self.receiver.borrow().try_iter().collect();
        if events.is_empty() {
            return;
        }
        let mut listed = false;
        for event in events {
            match event {
                FriendsEvent::Listed { generation, result } => {
                    if generation != self.generation.get() {
                        continue;
                    }
                    self.fetching.set(false);
                    match result {
                        Ok(mut list) => {
                            friends::sort(&mut list);
                            *self.friends.borrow_mut() = list;
                            self.fetched_at.set(Some(Instant::now()));
                            self.state.set(State::Ready);
                            listed = true;
                        }
                        Err(FriendsError::Private) => {
                            self.friends.borrow_mut().clear();
                            self.fetched_at.set(Some(Instant::now()));
                            self.state.set(State::Private);
                            listed = true;
                        }
                        Err(error) => {
                            warn!(%error, "Steam friends were not fetched");
                            // An earlier list stays; without one, say so.
                            if self.friends.borrow().is_empty() {
                                self.state.set(State::Failed);
                                listed = true;
                            }
                        }
                    }
                }
                FriendsEvent::Avatar { url, picture } => {
                    let image = slint_image(picture);
                    for index in 0..self.rows.row_count() {
                        let wants = self
                            .friends
                            .borrow()
                            .get(index)
                            .is_some_and(|friend| friend.avatar_url.as_deref() == Some(&url));
                        if wants && let Some(mut row) = self.rows.row_data(index) {
                            row.avatar = image.clone();
                            row.has_avatar = true;
                            self.rows.set_row_data(index, row);
                        }
                    }
                    self.avatars.borrow_mut().insert(url, image);
                }
            }
        }
        if listed {
            self.rebuild_rows();
            self.fetch_avatars();
        }
        if let Some(ui) = self.ui.upgrade() {
            if listed {
                // Keep the view within the new list.
                self.scroll(&ui, 0);
            }
            self.publish(&ui);
        }
    }

    fn rebuild_rows(&self) {
        let now = SystemTime::now();
        let avatars = self.avatars.borrow();
        let rows: Vec<FriendData> = self
            .friends
            .borrow()
            .iter()
            .map(|friend| {
                let avatar = friend
                    .avatar_url
                    .as_ref()
                    .and_then(|url| avatars.get(url))
                    .cloned();
                FriendData {
                    name: friend.name.as_str().into(),
                    status: friend.presence.describe(now).into(),
                    kind: match friend.presence {
                        friends::Presence::Playing(_) => 0,
                        friends::Presence::Online => 1,
                        friends::Presence::Busy | friends::Presence::Away => 2,
                        friends::Presence::Offline { .. } => 3,
                    },
                    has_avatar: avatar.is_some(),
                    avatar: avatar.unwrap_or_default(),
                    initial: crate::services::profile::monogram(&friend.name).into(),
                }
            })
            .collect();
        self.rows.set_vec(rows);
    }

    fn publish(&self, ui: &AppWindow) {
        let friends = self.friends.borrow();
        let state = self.state.get();
        ui.set_profile_friends_state(state as i32);
        let online = friends.iter().filter(|f| f.presence.is_online()).count();
        ui.set_profile_friends_summary(
            match state {
                State::Ready if friends.is_empty() => String::new(),
                State::Ready => format!(
                    "{online} online · {} {}",
                    friends.len(),
                    if friends.len() == 1 {
                        "friend"
                    } else {
                        "friends"
                    }
                ),
                _ => String::new(),
            }
            .into(),
        );
        ui.set_profile_friends_count(friends.len() as i32);
        ui.set_profile_friends_row(self.top_row.get());
    }
}

fn slint_image(picture: RgbaImage) -> Image {
    let (width, height) = picture.dimensions();
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        picture.as_raw(),
        width,
        height,
    ))
}
