//! Controller-first, source-neutral read-only achievement presentation.
//! Steam is the first adapter; provider identity is never hardcoded in Slint.
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    rc::Rc,
    sync::mpsc::Sender,
    sync::mpsc::{self, Receiver},
};

use chrono::{Local, TimeZone};
use slint::{Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

use crate::{
    AchievementEntryData, AchievementGameData, AppWindow,
    domain::LibraryGame,
    services::achievements::{
        AchievementImportEvent, SteamAchievement, SteamGame, SteamGameAchievements,
        hydrate_game_badges, import_steam_achievements, purge_achievement_cache, steam_games,
    },
    services::steam_account::SteamAccountService,
};

pub struct AchievementsController {
    account: Rc<RefCell<SteamAccountService>>,
    cache_identity: RefCell<Option<String>>,
    steam_games: Vec<SteamGame>,
    receiver: RefCell<Receiver<AchievementImportEvent>>,
    sender: RefCell<Sender<AchievementImportEvent>>,
    loaded: RefCell<Vec<SteamGameAchievements>>,
    selected: Cell<usize>,
    source_index: Cell<usize>,
    entry_scroll: Cell<usize>,
    viewing_entries: Cell<bool>,
    started: Cell<bool>,
    finished: Cell<bool>,
    unavailable: Cell<usize>,
    using_saved_data: Cell<bool>,
    requested_badges: RefCell<BTreeSet<u32>>,
}

impl AchievementsController {
    pub fn new(
        ui: &AppWindow,
        library: &[LibraryGame],
        account: Rc<RefCell<SteamAccountService>>,
    ) -> Rc<Self> {
        let (sender, receiver) = mpsc::channel();
        let identity = account
            .borrow()
            .credentials()
            .map(|c| c.steam_id64().to_owned());
        let controller = Rc::new(Self {
            account,
            cache_identity: RefCell::new(identity),
            steam_games: steam_games(library),
            sender: RefCell::new(sender),
            receiver: RefCell::new(receiver),
            loaded: RefCell::new(Vec::new()),
            selected: Cell::new(0),
            source_index: Cell::new(0),
            entry_scroll: Cell::new(0),
            viewing_entries: Cell::new(false),
            started: Cell::new(false),
            finished: Cell::new(false),
            unavailable: Cell::new(0),
            using_saved_data: Cell::new(false),
            requested_badges: RefCell::new(BTreeSet::new()),
        });
        controller.publish(ui);
        controller
    }

    /// Every independent Achievements visit begins at All sources, first game
    /// and viewport origin. Nested Back from game details does not call this.
    pub fn start_new_visit(&self, ui: &AppWindow) {
        self.selected.set(0);
        self.source_index.set(0);
        self.entry_scroll.set(0);
        self.viewing_entries.set(false);
        ui.set_achievements_viewing_entries(false);
        ui.set_achievements_selected_index(0);
        ui.set_achievements_first_entry(0);
        ui.set_achievements_visit_revision(ui.get_achievements_visit_revision().wrapping_add(1));
        self.on_enter(ui);
        self.publish(ui);
    }

    pub fn on_enter(&self, ui: &AppWindow) {
        self.viewing_entries.set(false);
        self.entry_scroll.set(0);
        ui.set_achievements_viewing_entries(false);
        if self.started.get() {
            self.publish(ui);
            return;
        }
        let Some(credentials) = self.account.borrow().credentials() else {
            self.publish(ui);
            return;
        };
        if self.steam_games.is_empty() {
            ui.set_achievements_status("No imported Steam games are available.".into());
            return;
        }
        self.started.set(true);
        ui.set_achievements_loading(true);
        ui.set_achievements_status(
            format!(
                "Reading Steam achievements… 0 / {} games",
                self.steam_games.len()
            )
            .into(),
        );
        let jobs = self.steam_games.clone();
        let sender = self.sender.borrow().clone();
        std::thread::spawn(move || import_steam_achievements(credentials, jobs, sender));
    }

    /// Discard results from the previous account. Its background worker may
    /// finish later, but its old channel is detached and cannot populate UI.
    pub fn account_changed(&self, ui: &AppWindow) {
        let next = self
            .account
            .borrow()
            .credentials()
            .map(|c| c.steam_id64().to_owned());
        let previous = self.cache_identity.replace(next.clone());
        // A replaced API key (even for the same SteamID) invalidates cached
        // data, as does switching accounts or disconnecting.
        if let Some(old) = previous {
            purge_achievement_cache(&old);
        }
        let (sender, receiver) = mpsc::channel();
        *self.sender.borrow_mut() = sender;
        *self.receiver.borrow_mut() = receiver;
        self.loaded.borrow_mut().clear();
        self.requested_badges.borrow_mut().clear();
        self.started.set(false);
        self.finished.set(false);
        self.unavailable.set(0);
        self.using_saved_data.set(false);
        self.selected.set(0);
        self.source_index.set(0);
        self.entry_scroll.set(0);
        self.viewing_entries.set(false);
        ui.set_achievements_loading(false);
        ui.set_activity_recent_achievements(ModelRc::from(Rc::new(VecModel::from(Vec::<
            AchievementEntryData,
        >::new(
        )))));
        self.publish(ui);
        if matches!(
            ui.get_current_route(),
            crate::AppRouteView::Activity | crate::AppRouteView::Achievements
        ) {
            self.on_enter(ui);
        }
    }

    /// Drains only completed network events on Slint's thread; requests never
    /// block animation, mouse, or controller input.
    pub fn poll(&self, ui: &AppWindow) {
        let mut changed = false;
        let mut catalog_changed = false;
        while let Ok(event) = self.receiver.borrow().try_recv() {
            changed = true;
            if matches!(
                &event,
                AchievementImportEvent::Cached(_)
                    | AchievementImportEvent::Imported(_)
                    | AchievementImportEvent::Refreshed(_)
            ) {
                catalog_changed = true;
            }
            match event {
                AchievementImportEvent::Cached(mut results) => {
                    self.using_saved_data.set(true);
                    results.sort_by_key(|a| a.game.title.to_lowercase());
                    *self.loaded.borrow_mut() = results;
                }
                AchievementImportEvent::Refreshed(valid_ids) => {
                    // Preserve an explicitly saved snapshot during a complete
                    // connectivity failure; a partial successful refresh can
                    // safely discard records Steam no longer makes accessible.
                    if !valid_ids.is_empty() {
                        self.loaded
                            .borrow_mut()
                            .retain(|game| valid_ids.contains(&game.game.app_id));
                        self.using_saved_data.set(false);
                    }
                }
                AchievementImportEvent::Imported(result) => {
                    let mut loaded = self.loaded.borrow_mut();
                    if let Some(existing) = loaded
                        .iter_mut()
                        .find(|g| g.game.app_id == result.game.app_id)
                    {
                        *existing = result;
                    } else {
                        loaded.push(result);
                    }
                    loaded.sort_by_key(|a| a.game.title.to_lowercase());
                }
                AchievementImportEvent::Badge {
                    app_id,
                    api_name,
                    rgba,
                } => {
                    for game in self
                        .loaded
                        .borrow_mut()
                        .iter_mut()
                        .filter(|g| g.game.app_id == app_id)
                    {
                        if let Some(badge) = game
                            .achievements
                            .iter_mut()
                            .find(|a| a.api_name == api_name)
                        {
                            badge.badge_rgba = Some(rgba.clone());
                        }
                    }
                }
                AchievementImportEvent::Unavailable => {
                    self.unavailable.set(self.unavailable.get() + 1)
                }
                AchievementImportEvent::Finished => self.finished.set(true),
            }
        }
        if changed {
            self.publish(ui);
            if catalog_changed {
                ui.set_achievements_catalog_revision(
                    ui.get_achievements_catalog_revision().wrapping_add(1),
                );
            }
        }
    }

    fn publish(&self, ui: &AppWindow) {
        let loaded = self.loaded.borrow();
        // All sources and Steam currently contain the same verified catalog.
        // The filter is represented in the presentation state so a future
        // provider adapter can populate its own provider ID and game rows.
        let rows = loaded
            .iter()
            .map(|entry| {
                let unlocked = entry.achievements.iter().filter(|a| a.unlocked).count();
                let total = entry.achievements.len();
                AchievementGameData {
                    title: entry.game.title.clone().into(),
                    provider_id: "steam".into(),
                    provider: "Steam".into(),
                    progress: format!("{unlocked} / {total} achievements").into(),
                    compact_progress: format!("{unlocked} / {total}").into(),
                    unlocked: unlocked as i32,
                    total: total as i32,
                    fraction: if total == 0 {
                        0.0
                    } else {
                        unlocked as f32 / total as f32
                    },
                }
            })
            .collect::<Vec<_>>();
        // Importer order is stable, so incoming badge events do not reset game
        // selection or scroll. Each row also retains its actual provider.
        let selected = self.selected.get().min(rows.len().saturating_sub(1));
        self.selected.set(selected);
        ui.set_achievements_selected_index(selected as i32);
        ui.set_achievements_saved_data(self.using_saved_data.get());
        ui.set_achievements_game_count(format!("{} games", rows.len()).into());
        ui.set_achievement_games(ModelRc::from(Rc::new(VecModel::from(rows))));
        ui.set_achievements_source_label(
            if self.source_index.get() == 0 {
                "All sources"
            } else {
                "Steam"
            }
            .into(),
        );
        ui.set_achievements_has_source_choice(true);
        self.publish_selected_entries(ui, &loaded);
        let mut recent = loaded
            .iter()
            .flat_map(|game| {
                game.achievements.iter().filter_map(move |a| {
                    if a.unlocked {
                        a.unlocked_at.map(|at| (at, game.game.title.as_str(), a))
                    } else {
                        None
                    }
                })
            })
            .collect::<Vec<_>>();
        recent.sort_unstable_by_key(|row| std::cmp::Reverse(row.0));
        let preview = recent
            .iter()
            .take(3)
            .map(|(timestamp, game, achievement)| as_row(achievement, game, Some(*timestamp)))
            .collect::<Vec<_>>();
        ui.set_activity_recent_achievements(ModelRc::from(Rc::new(VecModel::from(preview))));
        let scanned = loaded.len() + self.unavailable.get();
        let full = self.steam_games.len();
        if self.finished.get() {
            ui.set_achievements_loading(false);
            ui.set_achievements_status("No accessible achievements from connected sources.".into());
            ui.set_activity_achievement_status(if recent.is_empty() {
                "No recently unlocked achievements available".into()
            } else {
                "Recently unlocked achievements".into()
            });
        } else if self.started.get() {
            ui.set_achievements_status(
                format!("Reading Steam achievements… {scanned} / {full} games").into(),
            );
            ui.set_activity_achievement_status(if recent.is_empty() {
                "Checking achievements…".into()
            } else {
                "Recently unlocked achievements".into()
            });
        } else {
            ui.set_achievements_loading(false);
            ui.set_achievements_status(
                if self.account.borrow().connected() {
                    "Open Achievements or Activity to load Steam achievements."
                } else {
                    "Configure SteamID64 and Web API key in Settings > Third-Party > Steam Account."
                }
                .into(),
            );
            ui.set_activity_achievement_status(
                if self.account.borrow().connected() {
                    "Open Activity to load recent achievements."
                } else {
                    "Connect a supported account in Settings > Third-Party."
                }
                .into(),
            );
        }
    }

    fn publish_selected_entries(&self, ui: &AppWindow, loaded: &[SteamGameAchievements]) {
        let selected = loaded.get(self.selected.get());
        let rows = selected
            .map(|game| {
                let mut achievements = game.achievements.iter().collect::<Vec<_>>();
                achievements.sort_by(|a, b| {
                    b.unlocked
                        .cmp(&a.unlocked)
                        .then_with(|| b.unlocked_at.cmp(&a.unlocked_at))
                        .then_with(|| a.name.cmp(&b.name))
                });
                achievements
                    .iter()
                    .map(|a| as_row(a, &game.game.title, a.unlocked_at))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let visible = ui.get_achievements_visible_entries().max(1) as usize;
        let scroll = self
            .entry_scroll
            .get()
            .min(rows.len().saturating_sub(visible));
        self.entry_scroll.set(scroll);
        ui.set_achievements_first_entry(scroll as i32);
        ui.set_achievement_entries(ModelRc::from(Rc::new(VecModel::from(rows))));
        ui.set_achievements_viewing_entries(self.viewing_entries.get());
        ui.set_achievements_selected_game_title(
            selected
                .map(|game| game.game.title.as_str())
                .unwrap_or("Select a game")
                .into(),
        );
        ui.set_achievements_selected_game_provider(selected.map(|_| "Steam").unwrap_or("").into());
        let (progress, fraction) = selected
            .map(|game| {
                let unlocked = game.achievements.iter().filter(|a| a.unlocked).count();
                let total = game.achievements.len();
                (
                    format!("{unlocked} / {total} unlocked"),
                    if total == 0 {
                        0.0
                    } else {
                        unlocked as f32 / total as f32
                    },
                )
            })
            .unwrap_or_default();
        ui.set_achievements_selected_game_progress(progress.into());
        ui.set_achievements_selected_game_fraction(fraction);
    }

    pub fn cycle_source(&self, ui: &AppWindow, delta: i32) {
        // Real selection state (not decorative): all currently accessible
        // providers, then the supported Steam provider. Adding new adapters
        // extends this registry without changing the page layout or controls.
        let next = (self.source_index.get() as i32 + delta).rem_euclid(2);
        self.source_index.set(next as usize);
        self.selected.set(0);
        self.entry_scroll.set(0);
        self.viewing_entries.set(false);
        self.publish(ui);
    }

    fn select_game(&self, ui: &AppWindow, index: i32) {
        if index < 0 || index as usize >= self.loaded.borrow().len() {
            return;
        }
        self.selected.set(index as usize);
        self.entry_scroll.set(0);
        self.publish_selected_entries(ui, &self.loaded.borrow());
        ui.set_achievements_selected_index(index);
    }

    /// Pointer activation opens details; controller movement only changes focus.
    /// Returns true when the game's achievements opened.
    pub fn choose_game(&self, ui: &AppWindow, index: i32) -> bool {
        self.select_game(ui, index);
        self.enter_entries(ui)
    }

    pub fn move_game(&self, ui: &AppWindow, delta: i32) {
        let len = self.loaded.borrow().len();
        if len == 0 {
            return;
        }
        let current = self.selected.get() as i32;
        self.select_game(ui, (current + delta).clamp(0, len as i32 - 1));
    }

    /// Returns true when the game's achievements opened (false when the
    /// game has no achievement data to show).
    pub fn enter_entries(&self, ui: &AppWindow) -> bool {
        if ui.get_achievement_entries().row_count() == 0 {
            return false;
        }
        self.viewing_entries.set(true);
        ui.set_achievements_viewing_entries(true);
        // First load data and show the page; never wait for badge HTTP. Only
        // the opened game's missing badges are requested on the background
        // worker, rather than downloading whole libraries on every refresh.
        let selected = self.loaded.borrow().get(self.selected.get()).cloned();
        if let Some(game) = selected {
            let app_id = game.game.app_id;
            if game.achievements.iter().any(|a| a.badge_rgba.is_none())
                && self.requested_badges.borrow_mut().insert(app_id)
            {
                if let Some(credentials) = self.account.borrow().credentials() {
                    let sender = self.sender.borrow().clone();
                    std::thread::spawn(move || hydrate_game_badges(credentials, game, sender));
                } else {
                    self.requested_badges.borrow_mut().remove(&app_id);
                }
            }
        }
        true
    }

    pub fn exit_entries(&self, ui: &AppWindow) -> bool {
        if !self.viewing_entries.replace(false) {
            return false;
        }
        ui.set_achievements_viewing_entries(false);
        true
    }

    pub fn move_entries(&self, ui: &AppWindow, delta: i32) {
        let count = ui.get_achievement_entries().row_count() as i32;
        let visible = ui.get_achievements_visible_entries().max(1);
        let end = (count - visible).max(0);
        let next = (self.entry_scroll.get() as i32 + delta).clamp(0, end);
        self.entry_scroll.set(next as usize);
        ui.set_achievements_first_entry(next);
    }
}

fn as_row(
    achievement: &SteamAchievement,
    game: &str,
    timestamp: Option<u64>,
) -> AchievementEntryData {
    let when = timestamp
        .and_then(|at| i64::try_from(at).ok())
        .and_then(|at| Local.timestamp_opt(at, 0).single())
        .map(|date| format!("Unlocked {}", date.format("%b %-d, %Y")))
        .unwrap_or_else(|| {
            if achievement.unlocked {
                "Unlocked".to_owned()
            } else {
                "Locked".to_owned()
            }
        });
    let badge = achievement
        .badge_rgba
        .as_ref()
        .filter(|rgba| rgba.len() == 56 * 56 * 4)
        .map(|rgba| {
            Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                rgba, 56, 56,
            ))
        });
    AchievementEntryData {
        title: achievement.name.as_str().into(),
        game: game.into(),
        provider: "Steam".into(),
        has_badge: badge.is_some(),
        badge: badge.unwrap_or_default(),
        description: achievement.description.as_str().into(),
        when: when.into(),
        unlocked: achievement.unlocked,
    }
}
