//! The Shop: Flathub, browsed with a controller, and the apps it installed
//! managed from it.
//!
//! A grid of apps, a category at a time (LB/RB) or what a search finds (the
//! field above the grid); Installed lists what's on this machine, updates
//! first. Pages of apps load as the selection nears the end. Only the apps
//! near the camera are mounted, and only their icons are downloaded, the
//! ones on screen first; a bounded number of icons is kept. A opens an app's page (description, screenshots, and
//! Install, or Update and Uninstall). Installing, updating and
//! uninstalling run one at a time through the host's Flatpak
//! (`platform::flatpak_host`, ADR 0181); without it, Install hands the app
//! to the system's software store. Every request runs on a worker thread.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
        mpsc::Receiver,
    },
    time::{Duration, Instant},
};

use image::RgbaImage;
use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use tracing::warn;

use crate::{
    AppWindow, ShopAppData, ShopShotData,
    audio::UiSoundCue,
    input::{UiAction, UiActionEvent},
    navigation::step_with_edge_wrap,
    platform::{
        flatpak_host::{self, HostFlatpakError, InstalledApp},
        launcher::open_uri,
    },
    presentation::{
        CallbackSlot,
        mounted::{shift_window, window_shift},
        wake::drain_notify,
    },
    services::{
        flathub::{self, AppDetails, AppSummary, Category, FlathubClient, FlathubError, Page},
        image as decoding,
        worker::{WorkerSender, spawn, worker_channel},
    },
};

/// A category's apps shown again without asking Flathub, while this fresh.
const FRESH_FOR: Duration = Duration::from_secs(30 * 60);
/// Typing pauses this long before the search runs.
const SEARCH_DELAY: Duration = Duration::from_millis(350);
/// Icons downloaded at once.
const ICON_WORKERS: usize = 4;
/// Icons are kept at this size: sharp on an app's page.
const ICON_SIDE: u32 = 128;
/// Screenshots are kept at most this large.
const SHOT_SIZE: (u32, u32) = (1280, 800);
/// Screenshots shown on an app's page.
const MAX_SHOTS: usize = 8;
/// Rows left below the selection when the next page starts loading.
const LOAD_AHEAD_ROWS: usize = 3;
/// Rows mounted above and below the ones in view.
const OVERSCAN_ROWS: usize = 3;
/// Icons kept in memory (about 64 KB each), most recently shown first.
const MAX_ICONS: usize = 240;

/// Installed apps' summaries fetched at once.
const INSTALLED_WORKERS: usize = 4;

/// Something done to an installed (or to-be) app, one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Job {
    Install,
    Update,
    Uninstall,
}

impl Job {
    fn busy_label(self) -> &'static str {
        match self {
            Self::Install => "Installing",
            Self::Update => "Updating",
            Self::Uninstall => "Uninstalling",
        }
    }

    fn failure(self) -> &'static str {
        match self {
            Self::Install => "Couldn't install",
            Self::Update => "Couldn't update",
            Self::Uninstall => "Couldn't uninstall",
        }
    }
}

/// The buttons on an app's page, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageAction {
    Install,
    Update,
    Uninstall,
}

impl PageAction {
    fn label(self) -> &'static str {
        match self {
            Self::Install => "Install",
            Self::Update => "Update",
            Self::Uninstall => "Uninstall",
        }
    }
}

/// Not installed: Install. Installed: Update when one is waiting, and
/// Uninstall (never for Horizon itself).
fn page_actions(installed: bool, update: bool, own: bool) -> Vec<PageAction> {
    if !installed {
        return vec![PageAction::Install];
    }
    let mut actions = Vec::new();
    if update {
        actions.push(PageAction::Update);
    }
    if !own {
        actions.push(PageAction::Uninstall);
    }
    actions
}

/// Horizon's own app ID, when it runs as a Flatpak.
fn own_app_id() -> Option<String> {
    std::env::var("FLATPAK_ID").ok()
}

/// What a Shop input did, for the navigation layer's sounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShopFeedback {
    /// Not the Shop's (Back to leave the page, Home, Menu).
    Ignored,
    /// Handled quietly (a move is heard through the focus snapshot).
    Handled,
    Ok,
    Back,
}

/// Where a direction takes the grid's selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GridMove {
    Stay,
    To(usize),
    /// Up from the first row: the search field.
    Search,
}

/// Left/Right move within a row (the shared edge rule), Up/Down a row. Down
/// onto a shorter last row lands on its last app; Up from the first row
/// reaches the search field.
fn grid_step(
    selected: usize,
    len: usize,
    columns: usize,
    action: UiAction,
    repeated: bool,
) -> GridMove {
    if len == 0 {
        return if action == UiAction::Up {
            GridMove::Search
        } else {
            GridMove::Stay
        };
    }
    let columns = columns.max(1);
    let selected = selected.min(len - 1);
    let row_first = selected - selected % columns;
    let row_last = (row_first + columns - 1).min(len - 1);
    let to = match action {
        UiAction::Left | UiAction::Right => {
            let delta = if action == UiAction::Left { -1 } else { 1 };
            step_with_edge_wrap(
                selected as i32,
                row_first as i32,
                row_last as i32,
                delta,
                repeated,
            ) as usize
        }
        UiAction::Up if row_first == 0 => return GridMove::Search,
        UiAction::Up => selected - columns,
        UiAction::Down if selected + columns < len => selected + columns,
        UiAction::Down if row_last + 1 < len => len - 1,
        _ => selected,
    };
    if to == selected {
        GridMove::Stay
    } else {
        GridMove::To(to)
    }
}

enum ShopEvent {
    Listed {
        generation: u64,
        page: usize,
        result: Result<Page, FlathubError>,
    },
    Icon {
        url: String,
        picture: RgbaImage,
    },
    Details {
        app_id: String,
        result: Result<AppDetails, FlathubError>,
    },
    Shot {
        app_id: String,
        index: usize,
        picture: RgbaImage,
    },
    /// The host's installed apps.
    Installed(Result<Vec<InstalledApp>, String>),
    /// The installed apps with an update waiting.
    Updates(HashSet<String>),
    Progress {
        app_id: String,
        percent: u8,
    },
    JobDone {
        app_id: String,
        job: Job,
        result: Result<(), String>,
    },
}

/// A category's listing kept for the next visit.
struct Cached {
    apps: Vec<AppSummary>,
    total: usize,
    pages: usize,
    at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListState {
    Loading = 0,
    Ready = 1,
    Failed = 2,
}

struct Details {
    app: AppSummary,
    state: ListState,
    shot_count: usize,
    shot: usize,
    description_row: i32,
}

pub struct ShopController {
    ui: slint::Weak<AppWindow>,
    sender: WorkerSender<ShopEvent>,
    receiver: RefCell<Receiver<ShopEvent>>,
    category: Cell<usize>,
    /// The search being shown ("" for the category).
    query: RefCell<String>,
    apps: RefCell<Vec<AppSummary>>,
    total: Cell<usize>,
    pages: Cell<usize>,
    loading: Cell<bool>,
    state: Cell<ListState>,
    /// Bumped whenever the listing changes, so an older answer is ignored.
    generation: Cell<u64>,
    cache: RefCell<HashMap<usize, Cached>>,
    selected: Cell<usize>,
    first_row: Cell<i32>,
    search_focused: Cell<bool>,
    /// A finished search is loading: focus its results when they arrive.
    focus_results: Cell<bool>,
    /// The mounted apps: `apps[mounted_start..]`, by ID.
    rows: Rc<VecModel<ShopAppData>>,
    mounted_start: Cell<usize>,
    mounted: RefCell<Vec<String>>,
    icons: RefCell<IconCache>,
    icon_queue: Arc<Mutex<IconQueue>>,
    details: RefCell<Option<Details>>,
    shots: Rc<VecModel<ShopShotData>>,
    /// Bumped when an app's page opens or closes: its screenshots stop
    /// downloading.
    shots_generation: Arc<AtomicU64>,
    /// The page buttons last published.
    shown_actions: RefCell<Vec<PageAction>>,
    /// Installed apps' Flathub summaries, read once.
    summaries: Arc<Mutex<HashMap<String, AppSummary>>>,
    search_timer: slint::Timer,
    /// The host's installed apps, by ID (`None` until first read).
    installed: RefCell<Option<HashMap<String, InstalledApp>>>,
    updates: RefCell<HashSet<String>>,
    /// The job running: its app, what, and how far (when flatpak says).
    running: RefCell<Option<(String, Job, Option<u8>)>>,
    queue: RefCell<VecDeque<(String, Job)>>,
    /// The focused button on an app's page.
    action_focus: Cell<usize>,
    /// Uninstall's question is open, with Cancel (0) or Uninstall (1) focused.
    confirm: Cell<Option<i32>>,
    /// UI sounds for clicks and swipes (a controller's are played by
    /// navigation).
    sound: CallbackSlot<dyn Fn(UiSoundCue)>,
    /// Told when an install, update or uninstall finishes, so the library
    /// can pick up (or drop) Flatpak games.
    apps_changed: CallbackSlot<dyn Fn()>,
}

impl ShopController {
    pub fn new(ui: &AppWindow) -> Rc<Self> {
        let (notify, drain) = drain_notify(|controller: &ShopController| controller.drain());
        let (sender, receiver) = worker_channel(notify);
        let rows = Rc::new(VecModel::<ShopAppData>::default());
        ui.set_shop_apps(ModelRc::from(Rc::clone(&rows)));
        let shots = Rc::new(VecModel::<ShopShotData>::default());
        ui.set_shop_shots(ModelRc::from(Rc::clone(&shots)));
        let controller = Rc::new(Self {
            ui: ui.as_weak(),
            sender,
            receiver: RefCell::new(receiver),
            category: Cell::new(0),
            query: RefCell::new(String::new()),
            apps: RefCell::new(Vec::new()),
            total: Cell::new(0),
            pages: Cell::new(0),
            loading: Cell::new(false),
            state: Cell::new(ListState::Loading),
            generation: Cell::new(0),
            cache: RefCell::new(HashMap::new()),
            selected: Cell::new(0),
            first_row: Cell::new(0),
            search_focused: Cell::new(false),
            focus_results: Cell::new(false),
            rows,
            mounted_start: Cell::new(0),
            mounted: RefCell::new(Vec::new()),
            icons: RefCell::new(IconCache::default()),
            icon_queue: Arc::new(Mutex::new(IconQueue::default())),
            details: RefCell::new(None),
            shots,
            shots_generation: Arc::new(AtomicU64::new(0)),
            shown_actions: RefCell::new(Vec::new()),
            summaries: Arc::new(Mutex::new(HashMap::new())),
            search_timer: slint::Timer::default(),
            installed: RefCell::new(None),
            updates: RefCell::new(HashSet::new()),
            running: RefCell::new(None),
            queue: RefCell::new(VecDeque::new()),
            action_focus: Cell::new(0),
            confirm: Cell::new(None),
            sound: RefCell::new(None),
            apps_changed: RefCell::new(None),
        });
        drain.bind(&controller);
        controller.bind_callbacks(ui);
        controller.publish(ui);
        controller
    }

    /// Clicks play what the same choice plays from a controller.
    pub fn set_sound(&self, callback: Rc<dyn Fn(UiSoundCue)>) {
        *self.sound.borrow_mut() = Some(callback);
    }

    pub fn set_apps_changed(&self, callback: Rc<dyn Fn()>) {
        *self.apps_changed.borrow_mut() = Some(callback);
    }

    fn cue(&self, cue: UiSoundCue) {
        if let Some(callback) = self.sound.borrow().as_ref() {
            callback(cue);
        }
    }

    fn bind_callbacks(self: &Rc<Self>, ui: &AppWindow) {
        let on = |shop: &Rc<Self>| Rc::downgrade(shop);
        let weak = on(self);
        ui.on_shop_choose(move |index| {
            if let Some(shop) = weak.upgrade()
                && let Some(ui) = shop.ui.upgrade()
            {
                // A click on a card ends typing, leaving the search as it is.
                shop.search_timer.stop();
                ui.set_shop_searching(false);
                shop.search_focused.set(false);
                shop.select(&ui, index.max(0) as usize);
                if shop.open_details(&ui) {
                    shop.cue(UiSoundCue::Ok);
                }
            }
        });
        let weak = on(self);
        ui.on_shop_cycle_category(move |direction| {
            if let Some(shop) = weak.upgrade()
                && let Some(ui) = shop.ui.upgrade()
            {
                shop.cycle_category(&ui, direction);
                shop.cue(UiSoundCue::Ok);
            }
        });
        let weak = on(self);
        ui.on_shop_scroll_rows(move |rows| {
            if let Some(shop) = weak.upgrade()
                && let Some(ui) = shop.ui.upgrade()
            {
                shop.first_row.set((shop.first_row.get() + rows).max(0));
                ui.set_shop_first_row(shop.first_row.get());
                shop.publish_rows(&ui, false);
                let bottom = (shop.first_row.get() + ui.get_shop_visible_rows()) as usize;
                shop.load_more_near(&ui, bottom * shop.columns(&ui));
            }
        });
        let weak = on(self);
        ui.on_shop_search_open(move || {
            if let Some(shop) = weak.upgrade()
                && let Some(ui) = shop.ui.upgrade()
                && shop.details.borrow().is_none()
                && !ui.get_shop_searching()
            {
                shop.start_search(&ui);
                shop.cue(UiSoundCue::Ok);
            }
        });
        let weak = on(self);
        ui.on_shop_search_edited(move |text| {
            if let Some(shop) = weak.upgrade() {
                shop.search_edited(text.to_string());
            }
        });
        let weak = on(self);
        ui.on_shop_search_finish(move || {
            if let Some(shop) = weak.upgrade()
                && let Some(ui) = shop.ui.upgrade()
            {
                shop.finish_search(&ui);
            }
        });
        let weak = on(self);
        ui.on_shop_search_clear(move || {
            if let Some(shop) = weak.upgrade()
                && let Some(ui) = shop.ui.upgrade()
            {
                shop.clear_search(&ui);
                shop.cue(UiSoundCue::Back);
            }
        });
        let weak = on(self);
        ui.on_shop_activate_action(move |index| {
            if let Some(shop) = weak.upgrade()
                && let Some(ui) = shop.ui.upgrade()
            {
                shop.action_focus.set(index.max(0) as usize);
                if shop.activate_action(&ui) {
                    shop.cue(UiSoundCue::Ok);
                }
            }
        });
        let weak = on(self);
        ui.on_shop_confirm(move |choice| {
            if let Some(shop) = weak.upgrade()
                && let Some(ui) = shop.ui.upgrade()
                && shop.confirm.get().is_some()
            {
                shop.choose_confirm(&ui, choice);
                shop.cue(if choice == 1 {
                    UiSoundCue::Ok
                } else {
                    UiSoundCue::Back
                });
            }
        });
        let weak = on(self);
        ui.on_shop_close_details(move || {
            if let Some(shop) = weak.upgrade()
                && let Some(ui) = shop.ui.upgrade()
                && shop.details.borrow().is_some()
            {
                shop.close_details(&ui);
                shop.cue(UiSoundCue::Back);
            }
        });
        let weak = on(self);
        ui.on_shop_step_shot(move |delta| {
            if let Some(shop) = weak.upgrade()
                && let Some(ui) = shop.ui.upgrade()
            {
                let before = ui.get_shop_shot();
                shop.step_shot(&ui, delta, false);
                if ui.get_shop_shot() != before {
                    shop.cue(UiSoundCue::Navigation);
                }
            }
        });
        let weak = on(self);
        ui.on_shop_scroll_description(move |rows| {
            if let Some(shop) = weak.upgrade()
                && let Some(ui) = shop.ui.upgrade()
            {
                shop.scroll_description(&ui, rows);
            }
        });
    }

    /// Every visit starts on Games, at the top, with no search. A listing
    /// fetched recently shows at once.
    pub fn start_new_visit(&self, ui: &AppWindow) {
        self.search_timer.stop();
        *self.details.borrow_mut() = None;
        self.shots.set_vec(Vec::new());
        ui.set_shop_details_open(false);
        ui.set_shop_searching(false);
        ui.set_shop_query("".into());
        self.query.borrow_mut().clear();
        self.search_focused.set(false);
        self.confirm.set(None);
        self.category.set(0);
        self.show_listing(ui);
        self.refresh_installed(true);
        ui.set_shop_visit(ui.get_shop_visit().wrapping_add(1));
        self.publish(ui);
    }

    /// Leaving the page ends a search in progress (and its keyboard).
    pub fn on_leave(&self, ui: &AppWindow) {
        self.search_timer.stop();
        ui.set_shop_searching(false);
    }

    pub fn handle_action(&self, ui: &AppWindow, event: UiActionEvent) -> ShopFeedback {
        let action = event.action;
        if let Some(focus) = self.confirm.get() {
            return match action {
                UiAction::Left | UiAction::Right | UiAction::Up | UiAction::Down => {
                    let next = i32::from(matches!(action, UiAction::Right | UiAction::Down));
                    if next != focus {
                        self.confirm.set(Some(next));
                        self.publish_details(ui);
                    }
                    ShopFeedback::Handled
                }
                UiAction::Accept if !event.repeated => {
                    self.choose_confirm(ui, focus);
                    if focus == 1 {
                        ShopFeedback::Ok
                    } else {
                        ShopFeedback::Back
                    }
                }
                UiAction::Back => {
                    self.choose_confirm(ui, 0);
                    ShopFeedback::Back
                }
                UiAction::Home | UiAction::Menu | UiAction::Forward => {
                    self.confirm.set(None);
                    self.publish_details(ui);
                    ShopFeedback::Ignored
                }
                _ => ShopFeedback::Handled,
            };
        }
        if self.details.borrow().is_some() {
            return match action {
                UiAction::Back => {
                    self.close_details(ui);
                    ShopFeedback::Back
                }
                UiAction::Accept if !event.repeated => {
                    if self.activate_action(ui) {
                        ShopFeedback::Ok
                    } else {
                        ShopFeedback::Handled
                    }
                }
                UiAction::Left | UiAction::Right => {
                    let count = self.current_actions().len() as i32;
                    if count > 0 && self.current_busy().is_none() {
                        let next = step_with_edge_wrap(
                            self.action_focus.get() as i32,
                            0,
                            count - 1,
                            if action == UiAction::Left { -1 } else { 1 },
                            event.repeated,
                        );
                        self.action_focus.set(next as usize);
                        self.publish_details(ui);
                    }
                    ShopFeedback::Handled
                }
                UiAction::LeftBumper | UiAction::RightBumper => {
                    self.step_shot(
                        ui,
                        if action == UiAction::LeftBumper {
                            -1
                        } else {
                            1
                        },
                        event.repeated,
                    );
                    ShopFeedback::Handled
                }
                UiAction::Up | UiAction::Down => {
                    self.scroll_description(ui, if action == UiAction::Up { -1 } else { 1 });
                    ShopFeedback::Handled
                }
                UiAction::Home | UiAction::Menu | UiAction::Forward => ShopFeedback::Ignored,
                _ => ShopFeedback::Handled,
            };
        }
        if ui.get_shop_searching() {
            // The keyboard (if open) has already taken its input.
            return match action {
                UiAction::Back | UiAction::Down => {
                    self.finish_search(ui);
                    if action == UiAction::Back {
                        ShopFeedback::Back
                    } else {
                        ShopFeedback::Handled
                    }
                }
                UiAction::Accept if !event.repeated => {
                    ui.invoke_keyboard_show(2);
                    ShopFeedback::Ok
                }
                UiAction::Home | UiAction::Menu | UiAction::Forward => ShopFeedback::Ignored,
                _ => ShopFeedback::Handled,
            };
        }
        match action {
            UiAction::LeftBumper | UiAction::RightBumper if !event.repeated => {
                self.cycle_category(
                    ui,
                    if action == UiAction::LeftBumper {
                        -1
                    } else {
                        1
                    },
                );
                ShopFeedback::Ok
            }
            UiAction::Accept if !event.repeated => {
                if self.on_search() {
                    self.start_search(ui);
                    ShopFeedback::Ok
                } else if self.open_details(ui) {
                    ShopFeedback::Ok
                } else {
                    ShopFeedback::Handled
                }
            }
            UiAction::Secondary if !event.repeated => {
                if self.on_search() && !ui.get_shop_query().is_empty() {
                    self.clear_search(ui);
                    ShopFeedback::Back
                } else {
                    ShopFeedback::Handled
                }
            }
            UiAction::Up | UiAction::Down | UiAction::Left | UiAction::Right => {
                self.move_selection(ui, action, event.repeated);
                ShopFeedback::Handled
            }
            _ => ShopFeedback::Ignored,
        }
    }

    /// The Installed category is on screen (not a search).
    fn showing_installed(&self) -> bool {
        Category::ALL[self.category.get()] == Category::Installed && self.query.borrow().is_empty()
    }

    /// The search field has focus: chosen, or there is nothing else.
    fn on_search(&self) -> bool {
        self.search_focused.get() || self.apps.borrow().is_empty()
    }

    fn columns(&self, ui: &AppWindow) -> usize {
        ui.get_shop_columns().max(1) as usize
    }

    fn move_selection(&self, ui: &AppWindow, action: UiAction, repeated: bool) {
        let len = self.apps.borrow().len();
        if self.search_focused.get() {
            if action == UiAction::Down && len > 0 {
                self.search_focused.set(false);
                // Back onto the first row, where focus left it.
                let columns = self.columns(ui);
                self.selected
                    .set(self.selected.get().min(columns - 1).min(len - 1));
                self.publish(ui);
            }
            return;
        }
        match grid_step(self.selected.get(), len, self.columns(ui), action, repeated) {
            GridMove::Stay => {}
            GridMove::To(index) => self.select(ui, index),
            GridMove::Search if !repeated => {
                self.search_focused.set(true);
                self.publish(ui);
            }
            GridMove::Search => {}
        }
    }

    fn select(&self, ui: &AppWindow, index: usize) {
        let len = self.apps.borrow().len();
        if len == 0 {
            return;
        }
        self.selected.set(index.min(len - 1));
        self.load_more_near(ui, self.selected.get());
        self.publish(ui);
    }

    /// Load the next page once `index` is within a few rows of the end.
    fn load_more_near(&self, ui: &AppWindow, index: usize) {
        let len = self.apps.borrow().len();
        let ahead = LOAD_AHEAD_ROWS * self.columns(ui);
        let more =
            len < self.total.get() && self.pages.get() * flathub::PAGE_SIZE < self.total.get();
        if more && index + ahead >= len && self.state.get() != ListState::Failed {
            self.fetch(self.pages.get() + 1);
        }
    }

    fn cycle_category(&self, ui: &AppWindow, direction: i32) {
        if self.details.borrow().is_some() {
            return;
        }
        let count = Category::ALL.len() as i32;
        // From a search, the category it left steps on.
        let next = (self.category.get() as i32 + direction.signum()).rem_euclid(count);
        self.category.set(next as usize);
        self.search_timer.stop();
        self.query.borrow_mut().clear();
        ui.set_shop_query("".into());
        self.search_focused.set(false);
        self.show_listing(ui);
        self.publish(ui);
    }

    /// Show the current category (or search) from the start: from the cache
    /// when fresh, otherwise fetched.
    fn show_listing(&self, ui: &AppWindow) {
        self.generation.set(self.generation.get() + 1);
        self.focus_results.set(false);
        self.loading.set(false);
        self.selected.set(0);
        self.first_row.set(0);
        ui.set_shop_first_row(0);
        let installed_view = Category::ALL[self.category.get()] == Category::Installed;
        let cached = if self.query.borrow().is_empty() && !installed_view {
            self.cache
                .borrow()
                .get(&self.category.get())
                .filter(|cached| cached.at.elapsed() < FRESH_FOR)
                .map(|cached| (cached.apps.clone(), cached.total, cached.pages))
        } else {
            None
        };
        match cached {
            Some((apps, total, pages)) => {
                *self.apps.borrow_mut() = apps;
                self.total.set(total);
                self.pages.set(pages);
                self.state.set(ListState::Ready);
                self.rebuild_rows(ui, 0);
            }
            None => {
                self.apps.borrow_mut().clear();
                self.total.set(0);
                self.pages.set(0);
                self.state.set(ListState::Loading);
                self.rebuild_rows(ui, 0);
                self.fetch(1);
            }
        }
    }

    /// Fetch the listing again, keeping the selection where it can.
    fn reload_listing(&self) {
        self.generation.set(self.generation.get() + 1);
        self.loading.set(false);
        self.fetch(1);
    }

    fn fetch(&self, page: usize) {
        if self.loading.get() {
            return;
        }
        self.loading.set(true);
        let generation = self.generation.get();
        let category = Category::ALL[self.category.get()];
        let query = self.query.borrow().clone();
        let sender = self.sender.clone();
        let summaries = Arc::clone(&self.summaries);
        let started = spawn("shop-list", move || {
            let result = if category == Category::Installed && query.is_empty() {
                installed_page(&summaries)
            } else {
                FlathubClient::shared().and_then(|client| client.list(category, &query, page))
            };
            let _ = sender.send(ShopEvent::Listed {
                generation,
                page,
                result,
            });
        });
        if !started {
            self.loading.set(false);
        }
    }

    fn start_search(&self, ui: &AppWindow) {
        if self.details.borrow().is_some() {
            return;
        }
        self.search_focused.set(true);
        ui.set_shop_searching(true);
        self.publish(ui);
    }

    fn search_edited(self: &Rc<Self>, text: String) {
        let weak = Rc::downgrade(self);
        self.search_timer
            .start(slint::TimerMode::SingleShot, SEARCH_DELAY, move || {
                if let Some(shop) = weak.upgrade()
                    && let Some(ui) = shop.ui.upgrade()
                {
                    shop.apply_query(&ui, &text);
                }
            });
    }

    fn apply_query(&self, ui: &AppWindow, text: &str) {
        let text = text.trim();
        if *self.query.borrow() == text {
            return;
        }
        *self.query.borrow_mut() = text.to_owned();
        self.show_listing(ui);
        self.publish(ui);
    }

    /// Done typing: the search runs now, and focus goes to what it found.
    fn finish_search(&self, ui: &AppWindow) {
        self.search_timer.stop();
        ui.set_shop_searching(false);
        let text = ui.get_shop_query().to_string();
        self.apply_query(ui, &text);
        if self.apps.borrow().is_empty() {
            // Still loading: focus goes to the results when they arrive.
            self.focus_results.set(self.loading.get());
        } else {
            self.search_focused.set(false);
            self.selected.set(0);
        }
        self.publish(ui);
    }

    fn clear_search(&self, ui: &AppWindow) {
        self.search_timer.stop();
        ui.set_shop_query("".into());
        self.apply_query(ui, "");
    }

    fn open_details(&self, ui: &AppWindow) -> bool {
        let Some(app) = self.apps.borrow().get(self.selected.get()).cloned() else {
            return false;
        };
        let icon = app
            .icon_url
            .as_ref()
            .and_then(|url| self.icons.borrow_mut().get(url));
        ui.set_shop_detail_name(app.name.as_str().into());
        ui.set_shop_detail_developer(app.developer.as_str().into());
        ui.set_shop_detail_summary(app.summary.as_str().into());
        ui.set_shop_detail_verified(app.verified);
        ui.set_shop_detail_has_icon(icon.is_some());
        ui.set_shop_detail_icon(icon.unwrap_or_default());
        ui.set_shop_detail_description("".into());
        ui.set_shop_feedback("".into());
        self.shots.set_vec(Vec::new());
        let app_id = app.app_id.clone();
        self.action_focus.set(0);
        self.confirm.set(None);
        *self.details.borrow_mut() = Some(Details {
            app,
            state: ListState::Loading,
            shot_count: 0,
            shot: 0,
            description_row: 0,
        });
        ui.set_shop_details_open(true);
        self.shots_generation.fetch_add(1, Ordering::Relaxed);
        self.publish_details(ui);
        let sender = self.sender.clone();
        spawn("shop-details", move || {
            let result = FlathubClient::shared().and_then(|client| client.details(&app_id));
            let _ = sender.send(ShopEvent::Details { app_id, result });
        });
        true
    }

    fn close_details(&self, ui: &AppWindow) {
        if self.details.borrow_mut().take().is_none() {
            return;
        }
        // The screenshots go with the page, and stop downloading.
        self.shots_generation.fetch_add(1, Ordering::Relaxed);
        self.shots.set_vec(Vec::new());
        ui.set_shop_details_open(false);
        self.publish(ui);
    }

    fn step_shot(&self, ui: &AppWindow, delta: i32, repeated: bool) {
        if let Some(details) = self.details.borrow_mut().as_mut()
            && details.shot_count > 1
        {
            details.shot = step_with_edge_wrap(
                details.shot as i32,
                0,
                details.shot_count as i32 - 1,
                delta,
                repeated,
            ) as usize;
        }
        self.publish_details(ui);
    }

    fn scroll_description(&self, ui: &AppWindow, rows: i32) {
        let steps = ui.get_shop_description_steps().max(0);
        if let Some(details) = self.details.borrow_mut().as_mut() {
            details.description_row = (details.description_row + rows).clamp(0, steps);
        }
        self.publish_details(ui);
    }

    fn current_app_id(&self) -> Option<String> {
        self.details
            .borrow()
            .as_ref()
            .map(|details| details.app.app_id.clone())
    }

    fn is_installed(&self, app_id: &str) -> bool {
        self.installed
            .borrow()
            .as_ref()
            .is_some_and(|installed| installed.contains_key(app_id))
    }

    /// The open page's buttons.
    fn current_actions(&self) -> Vec<PageAction> {
        let Some(app_id) = self.current_app_id() else {
            return Vec::new();
        };
        page_actions(
            self.is_installed(&app_id),
            self.updates.borrow().contains(&app_id),
            own_app_id().as_deref() == Some(app_id.as_str()),
        )
    }

    /// What's happening to `app_id`, if anything: the job (or "Waiting"
    /// in the queue) and how far it is (when flatpak says).
    fn busy(&self, app_id: &str) -> Option<(&'static str, Option<u8>)> {
        if let Some((running, job, progress)) = self.running.borrow().as_ref()
            && running == app_id
        {
            return Some((job.busy_label(), *progress));
        }
        self.queue
            .borrow()
            .iter()
            .any(|(queued, _)| queued == app_id)
            .then_some(("Waiting", None))
    }

    fn current_busy(&self) -> Option<(&'static str, Option<u8>)> {
        self.current_app_id().and_then(|app_id| self.busy(&app_id))
    }

    /// A (or a click) on the page's focused button.
    fn activate_action(&self, ui: &AppWindow) -> bool {
        let Some(app_id) = self.current_app_id() else {
            return false;
        };
        if self.busy(&app_id).is_some() {
            return false;
        }
        let actions = self.current_actions();
        let Some(action) =
            actions.get(self.action_focus.get().min(actions.len().saturating_sub(1)))
        else {
            return false;
        };
        ui.set_shop_feedback("".into());
        match action {
            PageAction::Install => self.enqueue(ui, app_id, Job::Install),
            PageAction::Update => self.enqueue(ui, app_id, Job::Update),
            PageAction::Uninstall => return self.ask_uninstall(ui),
        }
        self.publish(ui);
        true
    }

    /// The Uninstall button: ask first.
    fn ask_uninstall(&self, ui: &AppWindow) -> bool {
        let Some(app_id) = self.current_app_id() else {
            return false;
        };
        if self.busy(&app_id).is_some() || !self.current_actions().contains(&PageAction::Uninstall)
        {
            return false;
        }
        self.confirm.set(Some(0));
        self.publish_details(ui);
        true
    }

    fn choose_confirm(&self, ui: &AppWindow, choice: i32) {
        if self.confirm.take().is_none() {
            return;
        }
        if choice == 1
            && let Some(app_id) = self.current_app_id()
        {
            ui.set_shop_feedback("".into());
            self.enqueue(ui, app_id, Job::Uninstall);
        }
        self.publish(ui);
    }

    fn enqueue(&self, ui: &AppWindow, app_id: String, job: Job) {
        if self.busy(&app_id).is_some() {
            return;
        }
        self.queue.borrow_mut().push_back((app_id, job));
        self.start_next();
        self.refresh_row_states();
        self.publish_details(ui);
    }

    /// Start the next job when none is running.
    fn start_next(&self) {
        if self.running.borrow().is_some() {
            return;
        }
        let Some((app_id, job)) = self.queue.borrow_mut().pop_front() else {
            return;
        };
        let installation = self
            .installed
            .borrow()
            .as_ref()
            .and_then(|installed| installed.get(&app_id))
            .map(|app| app.installation);
        *self.running.borrow_mut() = Some((app_id.clone(), job, None));
        let sender = self.sender.clone();
        let started = spawn("shop-job", move || {
            let mut last = None;
            let mut progress = |percent: u8| {
                if last != Some(percent) {
                    last = Some(percent);
                    let _ = sender.send(ShopEvent::Progress {
                        app_id: app_id.clone(),
                        percent,
                    });
                }
            };
            let result = match (job, installation) {
                (Job::Install, _) => match flatpak_host::install(&app_id, &mut progress) {
                    // No host Flatpak to talk to: the software store installs.
                    Err(HostFlatpakError::Spawn(error)) => {
                        warn!(%error, "host Flatpak unavailable; handing Install to the store");
                        hand_to_store(&app_id)
                    }
                    other => other.map_err(|error| error.to_string()),
                },
                (Job::Update, Some(installation)) => {
                    flatpak_host::update(&app_id, installation, &mut progress)
                        .map_err(|error| error.to_string())
                }
                (Job::Uninstall, Some(installation)) => {
                    flatpak_host::uninstall(&app_id, installation)
                        .map_err(|error| error.to_string())
                }
                (_, None) => Err("not installed".to_owned()),
            };
            let _ = sender.send(ShopEvent::JobDone {
                app_id,
                job,
                result,
            });
        });
        if !started {
            *self.running.borrow_mut() = None;
        }
    }

    /// Read what's installed (and, with `updates`, what can be updated).
    fn refresh_installed(&self, updates: bool) {
        let sender = self.sender.clone();
        spawn("shop-installed", move || {
            let installed = flatpak_host::installed().map_err(|error| error.to_string());
            let readable = installed.is_ok();
            let _ = sender.send(ShopEvent::Installed(installed));
            if updates && readable {
                match flatpak_host::updates() {
                    Ok(found) => {
                        let _ = sender.send(ShopEvent::Updates(found));
                    }
                    Err(error) => warn!(%error, "Flatpak updates were not checked"),
                }
            }
        });
    }

    /// Put each mounted card's Installed/Update/progress state in the grid.
    fn refresh_row_states(&self) {
        let start = self.mounted_start.get();
        for (index, app) in self
            .apps
            .borrow()
            .iter()
            .skip(start)
            .take(self.rows.row_count())
            .enumerate()
        {
            if let Some(mut row) = self.rows.row_data(index) {
                let (status, busy, progress) = self.row_state(&app.app_id);
                if row.status != status || row.busy != busy || row.progress != progress {
                    row.status = status;
                    row.busy = busy;
                    row.progress = progress;
                    self.rows.set_row_data(index, row);
                }
            }
        }
    }

    /// A card's state line, whether a job is on it, and its progress (-1
    /// when flatpak doesn't say).
    fn row_state(&self, app_id: &str) -> (slint::SharedString, bool, f32) {
        if let Some((label, progress)) = self.busy(app_id) {
            return (
                label.into(),
                true,
                progress.map_or(-1.0, |p| f32::from(p) / 100.0),
            );
        }
        let status = if self.updates.borrow().contains(app_id) && self.is_installed(app_id) {
            "Update"
        } else if self.is_installed(app_id) {
            "Installed"
        } else {
            ""
        };
        (status.into(), false, -1.0)
    }

    /// Download the icons of `urls` not kept yet, in order, instead of any
    /// still waiting: a few workers take them one at a time.
    fn want_icons(&self, urls: Vec<String>) {
        let needed = {
            let icons = self.icons.borrow();
            let mut queue = self
                .icon_queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let queue = &mut *queue;
            queue.wanted = urls
                .into_iter()
                .filter(|url| {
                    !icons.contains(url)
                        && !queue.in_flight.contains(url)
                        && !queue.failed.contains(url)
                })
                .collect();
            let needed = ICON_WORKERS
                .min(queue.wanted.len())
                .saturating_sub(queue.workers);
            queue.workers += needed;
            needed
        };
        for _ in 0..needed {
            let (queue, sender) = (Arc::clone(&self.icon_queue), self.sender.clone());
            if !spawn("shop-icons", move || icon_worker(&queue, &sender)) {
                let mut queue = self
                    .icon_queue
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                queue.workers = queue.workers.saturating_sub(1);
            }
        }
    }

    fn fetch_shots(&self, app_id: String, urls: Vec<String>) {
        let sender = self.sender.clone();
        let (current, generation) = (
            Arc::clone(&self.shots_generation),
            self.shots_generation.load(Ordering::Relaxed),
        );
        spawn("shop-screenshots", move || {
            let Ok(client) = FlathubClient::shared() else {
                return;
            };
            // The first one first: it's the one on screen. Leaving the page
            // stops the rest.
            for (index, url) in urls.iter().enumerate() {
                if current.load(Ordering::Relaxed) != generation {
                    return;
                }
                let picture = client.media(url).and_then(|bytes| {
                    decoding::decode(&bytes, None)
                        .ok()
                        .map(|image| decoding::fit(image, SHOT_SIZE))
                });
                if let Some(picture) = picture
                    && sender
                        .send(ShopEvent::Shot {
                            app_id: app_id.clone(),
                            index,
                            picture,
                        })
                        .is_err()
                {
                    return;
                }
            }
        });
    }

    fn drain(&self) {
        let events: Vec<_> = self.receiver.borrow().try_iter().collect();
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        for event in events {
            match event {
                ShopEvent::Listed {
                    generation,
                    page,
                    result,
                } => self.listed(&ui, generation, page, result),
                ShopEvent::Icon { url, picture } => {
                    let image = slint_image(&picture);
                    let start = self.mounted_start.get();
                    for (index, app) in self
                        .apps
                        .borrow()
                        .iter()
                        .skip(start)
                        .take(self.rows.row_count())
                        .enumerate()
                    {
                        if app.icon_url.as_deref() == Some(url.as_str())
                            && let Some(mut row) = self.rows.row_data(index)
                        {
                            row.icon = image.clone();
                            row.has_icon = true;
                            self.rows.set_row_data(index, row);
                        }
                    }
                    let for_details = self.details.borrow().as_ref().is_some_and(|details| {
                        details.app.icon_url.as_deref() == Some(url.as_str())
                    });
                    if for_details {
                        ui.set_shop_detail_icon(image.clone());
                        ui.set_shop_detail_has_icon(true);
                    }
                    self.icons.borrow_mut().insert(url, image);
                }
                ShopEvent::Details { app_id, result } => {
                    let mut details = self.details.borrow_mut();
                    let Some(details) = details.as_mut().filter(|d| d.app.app_id == app_id) else {
                        continue;
                    };
                    match result {
                        Ok(found) => {
                            details.state = ListState::Ready;
                            let urls: Vec<String> =
                                found.screenshots.into_iter().take(MAX_SHOTS).collect();
                            details.shot_count = urls.len();
                            ui.set_shop_detail_description(found.description.into());
                            self.shots.set_vec(
                                urls.iter()
                                    .map(|_| ShopShotData {
                                        image: Image::default(),
                                        loaded: false,
                                    })
                                    .collect::<Vec<_>>(),
                            );
                            self.fetch_shots(app_id, urls);
                        }
                        Err(error) => {
                            warn!(%error, "Flathub app page was not fetched");
                            details.state = ListState::Failed;
                        }
                    }
                }
                ShopEvent::Shot {
                    app_id,
                    index,
                    picture,
                } => {
                    let current = self
                        .details
                        .borrow()
                        .as_ref()
                        .is_some_and(|details| details.app.app_id == app_id);
                    if current && index < self.shots.row_count() {
                        self.shots.set_row_data(
                            index,
                            ShopShotData {
                                image: slint_image(&picture),
                                loaded: true,
                            },
                        );
                    }
                }
                ShopEvent::Installed(Ok(list)) => {
                    let ids_before: Option<HashSet<String>> = self
                        .installed
                        .borrow()
                        .as_ref()
                        .map(|installed| installed.keys().cloned().collect());
                    let ids: HashSet<String> = list.iter().map(|app| app.app_id.clone()).collect();
                    *self.installed.borrow_mut() = Some(
                        list.into_iter()
                            .map(|app| (app.app_id.clone(), app))
                            .collect(),
                    );
                    // Installed changed under the Installed list: list it again.
                    if self.showing_installed() && ids_before.is_some_and(|before| before != ids) {
                        self.reload_listing();
                    }
                    self.refresh_row_states();
                }
                ShopEvent::Installed(Err(error)) => {
                    warn!(%error, "installed Flatpaks were not read");
                }
                ShopEvent::Updates(found) => {
                    *self.updates.borrow_mut() = found;
                    self.refresh_row_states();
                }
                ShopEvent::Progress { app_id, percent } => {
                    if let Some((running, _, progress)) = self.running.borrow_mut().as_mut()
                        && *running == app_id
                    {
                        *progress = Some(percent);
                    }
                    self.refresh_row_states();
                }
                ShopEvent::JobDone {
                    app_id,
                    job,
                    result,
                } => {
                    *self.running.borrow_mut() = None;
                    match result {
                        Err(error) => {
                            warn!(%error, ?job, "Flatpak job failed");
                            if self.current_app_id().as_deref() == Some(app_id.as_str()) {
                                ui.set_shop_feedback(job.failure().into());
                            }
                        }
                        Ok(()) => {
                            if let Some(callback) = self.apps_changed.borrow().as_ref() {
                                callback();
                            }
                        }
                    }
                    // Uninstalled apps' buttons start over at Install.
                    self.action_focus.set(0);
                    self.refresh_installed(job != Job::Uninstall);
                    self.start_next();
                    self.refresh_row_states();
                }
            }
        }
        self.publish(&ui);
    }

    fn listed(
        &self,
        ui: &AppWindow,
        generation: u64,
        page: usize,
        result: Result<Page, FlathubError>,
    ) {
        if generation != self.generation.get() {
            return;
        }
        self.loading.set(false);
        match result {
            Ok(found) => {
                let shown = self.apps.borrow().len();
                {
                    let mut apps = self.apps.borrow_mut();
                    if page <= 1 {
                        apps.clear();
                    }
                    let known: HashSet<String> =
                        apps.iter().map(|app| app.app_id.clone()).collect();
                    let arrived = found.apps.len();
                    apps.extend(
                        found
                            .apps
                            .into_iter()
                            .filter(|app| !known.contains(&app.app_id)),
                    );
                    // An empty page ends the listing, whatever the total says.
                    self.total.set(if arrived == 0 {
                        apps.len()
                    } else {
                        found.total
                    });
                }
                self.pages.set(page);
                self.state.set(ListState::Ready);
                if self.showing_installed() {
                    // Updates first, then by name.
                    let updates = self.updates.borrow();
                    self.apps.borrow_mut().sort_by_cached_key(|app| {
                        (!updates.contains(&app.app_id), app.name.to_lowercase())
                    });
                } else if self.query.borrow().is_empty() {
                    self.cache.borrow_mut().insert(
                        self.category.get(),
                        Cached {
                            apps: self.apps.borrow().clone(),
                            total: self.total.get(),
                            pages: page,
                            at: Instant::now(),
                        },
                    );
                }
                self.rebuild_rows(ui, if page <= 1 { 0 } else { shown });
                if self.focus_results.take() && !self.apps.borrow().is_empty() {
                    self.search_focused.set(false);
                    self.selected.set(0);
                }
            }
            Err(error) => {
                warn!(%error, "Flathub listing was not fetched");
                self.focus_results.set(false);
                // What's shown stays; with nothing, say so.
                if self.apps.borrow().is_empty() {
                    self.state.set(ListState::Failed);
                }
            }
        }
    }

    /// The listing changed. Apps from `kept` on are new: they fade in, the
    /// ones before stay put. Nothing kept: the grid starts over.
    fn rebuild_rows(&self, ui: &AppWindow, kept: usize) {
        ui.set_shop_catalog_base(kept as i32);
        ui.set_shop_catalog_revision(ui.get_shop_catalog_revision().wrapping_add(1));
        self.publish_rows(ui, kept == 0);
    }

    /// Mount the apps near the camera: the rows in view and a few either
    /// side. A window that only scrolled keeps the cards that stay; `rebuild`
    /// (a new listing) replaces them all.
    fn publish_rows(&self, ui: &AppWindow, rebuild: bool) {
        let apps = self.apps.borrow();
        let (start, end, in_view) = mounted_range(
            apps.len(),
            self.columns(ui),
            self.first_row.get().max(0) as usize,
            ui.get_shop_visible_rows().max(1) as usize,
        );
        ui.set_shop_app_count(apps.len() as i32);
        let ids: Vec<String> = apps[start..end]
            .iter()
            .map(|app| app.app_id.clone())
            .collect();
        let old_start = self.mounted_start.get();
        if !rebuild && ids == *self.mounted.borrow() && start == old_start {
            return;
        }
        let rows: Vec<ShopAppData> = apps[start..end]
            .iter()
            .enumerate()
            .map(|(offset, app)| self.row(start + offset, app))
            .collect();
        if !rebuild && window_shift(&self.mounted.borrow(), old_start, &ids, start) {
            shift_window(&self.rows, old_start, start, rows);
        } else {
            self.rows.set_vec(rows);
        }
        self.mounted_start.set(start);
        *self.mounted.borrow_mut() = ids;
        // Icons: the rows in view first, then those below, then above.
        let urls = (in_view.start..end)
            .chain(start..in_view.start)
            .filter_map(|index| apps[index].icon_url.clone())
            .collect();
        drop(apps);
        self.want_icons(urls);
    }

    fn row(&self, index: usize, app: &AppSummary) -> ShopAppData {
        let icon = app
            .icon_url
            .as_ref()
            .and_then(|url| self.icons.borrow_mut().get(url));
        let (status, busy, progress) = self.row_state(&app.app_id);
        ShopAppData {
            index: index as i32,
            name: app.name.as_str().into(),
            developer: app.developer.as_str().into(),
            summary: app.summary.as_str().into(),
            verified: app.verified,
            has_icon: icon.is_some(),
            icon: icon.unwrap_or_default(),
            status,
            busy,
            progress,
        }
    }

    fn publish(&self, ui: &AppWindow) {
        // The grid's shape may have changed (a resize).
        self.publish_rows(ui, false);
        let len = self.apps.borrow().len();
        if len > 0 && self.selected.get() >= len {
            self.selected.set(len - 1);
        }
        ui.set_shop_selected(self.selected.get() as i32);
        ui.set_shop_search_focused(self.on_search());
        ui.set_shop_state(self.state.get() as i32);
        ui.set_shop_category(Category::ALL[self.category.get()].label().into());
        ui.set_shop_count(match self.state.get() {
            ListState::Ready => count_text(self.total.get()),
            _ => "".into(),
        });
        self.publish_details(ui);
    }

    fn publish_details(&self, ui: &AppWindow) {
        if let Some(details) = self.details.borrow().as_ref() {
            ui.set_shop_detail_state(details.state as i32);
            ui.set_shop_shot(details.shot as i32);
            ui.set_shop_description_row(details.description_row);
        }
        let actions = self.current_actions();
        let focus = self.action_focus.get().min(actions.len().saturating_sub(1));
        self.action_focus.set(focus);
        let busy = self.current_busy();
        // A new model rebuilds the buttons: only when they change.
        if *self.shown_actions.borrow() != actions {
            ui.set_shop_actions(ModelRc::new(VecModel::from(
                actions
                    .iter()
                    .map(|action| slint::SharedString::from(action.label()))
                    .collect::<Vec<_>>(),
            )));
            *self.shown_actions.borrow_mut() = actions.clone();
        }
        ui.set_shop_action_focus(focus as i32);
        ui.set_shop_action_label(
            actions
                .get(focus)
                .map_or("", |action| action.label())
                .into(),
        );
        ui.set_shop_busy_label(busy.map_or("", |(label, _)| label).into());
        ui.set_shop_progress(
            busy.and_then(|(_, progress)| progress)
                .map_or(-1.0, |p| f32::from(p) / 100.0),
        );
        ui.set_shop_confirm_open(self.confirm.get().is_some());
        ui.set_shop_confirm_focus(self.confirm.get().unwrap_or(0));
    }
}

/// Without the host's Flatpak, Install opens the software store on the app,
/// or Flathub's page for it.
fn hand_to_store(app_id: &str) -> Result<(), String> {
    flathub::store_uri(app_id)
        .map_err(|error| error.to_string())
        .and_then(|uri| open_uri(&uri).map_err(|error| error.to_string()))
        .or_else(|error| {
            warn!(%error, "no software store opened; trying Flathub's page");
            flathub::web_uri(app_id)
                .map_err(|error| error.to_string())
                .and_then(|uri| open_uri(&uri).map_err(|error| error.to_string()))
        })
}

/// The installed apps as the grid lists them: Flathub's own summaries
/// (icon, developer), else what Flatpak knows (the name). A summary read
/// once is kept in `summaries`.
fn installed_page(summaries: &Mutex<HashMap<String, AppSummary>>) -> Result<Page, FlathubError> {
    let installed =
        flatpak_host::installed().map_err(|error| FlathubError::Network(error.to_string()))?;
    let client = FlathubClient::shared()?;
    let per_worker = installed.len().div_ceil(INSTALLED_WORKERS).max(1);
    let apps: Vec<AppSummary> = std::thread::scope(|scope| {
        let workers: Vec<_> = installed
            .chunks(per_worker)
            .map(|chunk| {
                let client = &client;
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|app| {
                            let kept = summaries
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .get(&app.app_id)
                                .cloned();
                            let found = kept.or_else(|| {
                                let found = (app.origin == "flathub")
                                    .then(|| client.app(&app.app_id).ok())
                                    .flatten()?;
                                summaries
                                    .lock()
                                    .unwrap_or_else(PoisonError::into_inner)
                                    .insert(app.app_id.clone(), found.clone());
                                Some(found)
                            });
                            found.unwrap_or_else(|| AppSummary {
                                app_id: app.app_id.clone(),
                                name: app.name.clone(),
                                summary: String::new(),
                                developer: String::new(),
                                icon_url: None,
                                verified: false,
                            })
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap_or_default())
            .collect()
    });
    Ok(Page {
        total: apps.len(),
        apps,
    })
}

/// The apps to mount, `start..end`, for a grid of `len` apps in `columns`
/// with `visible` rows in view from `first_row`; and the ones in view.
fn mounted_range(
    len: usize,
    columns: usize,
    first_row: usize,
    visible: usize,
) -> (usize, usize, std::ops::Range<usize>) {
    let columns = columns.max(1);
    let rows = len.div_ceil(columns);
    let first_row = first_row.min(rows);
    let at = |row: usize| (row * columns).min(len);
    let start = at(first_row.saturating_sub(OVERSCAN_ROWS));
    let end = at((first_row + visible + OVERSCAN_ROWS).min(rows));
    (
        start,
        end,
        at(first_row)..at((first_row + visible).min(rows)),
    )
}

/// Downloaded icons, at most `MAX_ICONS`: the one shown least recently
/// goes first.
#[derive(Default)]
struct IconCache {
    icons: HashMap<String, (Image, u64)>,
    clock: u64,
}

impl IconCache {
    fn contains(&self, url: &str) -> bool {
        self.icons.contains_key(url)
    }

    fn get(&mut self, url: &str) -> Option<Image> {
        self.clock += 1;
        let (image, used) = self.icons.get_mut(url)?;
        *used = self.clock;
        Some(image.clone())
    }

    fn insert(&mut self, url: String, image: Image) {
        self.clock += 1;
        self.icons.insert(url, (image, self.clock));
        while self.icons.len() > MAX_ICONS {
            let Some(oldest) = self
                .icons
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(url, _)| url.clone())
            else {
                break;
            };
            self.icons.remove(&oldest);
        }
    }
}

/// Icons waiting to download, downloading, and that couldn't be; and how
/// many workers are taking them.
#[derive(Default)]
struct IconQueue {
    wanted: VecDeque<String>,
    in_flight: HashSet<String>,
    failed: HashSet<String>,
    workers: usize,
}

/// Take icons from the queue until it is empty.
fn icon_worker(queue: &Mutex<IconQueue>, sender: &WorkerSender<ShopEvent>) {
    let client = FlathubClient::shared().ok();
    loop {
        let url = {
            let mut queue = queue.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(url) = queue.wanted.pop_front() else {
                queue.workers = queue.workers.saturating_sub(1);
                return;
            };
            queue.in_flight.insert(url.clone());
            url
        };
        let picture = client.as_ref().and_then(|client| {
            let bytes = client.media(&url)?;
            decoding::decode(&bytes, None)
                .ok()
                .map(|image| decoding::fit(image, (ICON_SIDE, ICON_SIDE)))
        });
        let failed = picture.is_none();
        let gone = picture.is_some_and(|picture| {
            sender
                .send(ShopEvent::Icon {
                    url: url.clone(),
                    picture,
                })
                .is_err()
        });
        let mut queue = queue.lock().unwrap_or_else(PoisonError::into_inner);
        queue.in_flight.remove(&url);
        if gone {
            // The Shop is gone.
            queue.wanted.clear();
            queue.workers = queue.workers.saturating_sub(1);
            return;
        }
        if failed {
            queue.failed.insert(url);
        }
    }
}

fn count_text(total: usize) -> slint::SharedString {
    let digits = total.to_string();
    // Thousands grouped: "2,890".
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    format!("{grouped} {}", if total == 1 { "app" } else { "apps" }).into()
}

fn slint_image(picture: &RgbaImage) -> Image {
    let (width, height) = picture.dimensions();
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        picture.as_raw(),
        width,
        height,
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        GridMove, IconCache, MAX_ICONS, OVERSCAN_ROWS, PageAction, count_text, grid_step,
        mounted_range, page_actions,
    };
    use crate::input::UiAction::{Down, Left, Right, Up};

    #[test]
    fn the_grid_moves_by_rows_and_stops_at_its_edges() {
        // 3 columns, 7 apps: rows [0 1 2] [3 4 5] [6].
        assert_eq!(grid_step(1, 7, 3, Down, false), GridMove::To(4));
        assert_eq!(grid_step(4, 7, 3, Down, false), GridMove::To(6));
        assert_eq!(grid_step(5, 7, 3, Down, false), GridMove::To(6));
        assert_eq!(grid_step(6, 7, 3, Down, false), GridMove::Stay);
        assert_eq!(grid_step(4, 7, 3, Up, false), GridMove::To(1));
        // Up from the first row is the search field.
        assert_eq!(grid_step(2, 7, 3, Up, false), GridMove::Search);
        assert_eq!(grid_step(0, 0, 3, Up, false), GridMove::Search);
        // Left/Right stay in the row: held stops, a fresh press wraps.
        assert_eq!(grid_step(5, 7, 3, Right, true), GridMove::Stay);
        assert_eq!(grid_step(5, 7, 3, Right, false), GridMove::To(3));
        assert_eq!(grid_step(3, 7, 3, Left, false), GridMove::To(5));
        assert_eq!(grid_step(6, 7, 3, Right, false), GridMove::Stay);
    }

    #[test]
    fn an_apps_page_offers_what_fits_its_state() {
        use PageAction::{Install, Uninstall, Update};
        assert_eq!(page_actions(false, false, false), vec![Install]);
        assert_eq!(page_actions(true, false, false), vec![Uninstall]);
        assert_eq!(page_actions(true, true, false), vec![Update, Uninstall]);
        // Horizon never offers to remove itself.
        assert_eq!(page_actions(true, false, true), vec![]);
    }

    #[test]
    fn counts_are_grouped() {
        assert_eq!(count_text(1).as_str(), "1 app");
        assert_eq!(count_text(604).as_str(), "604 apps");
        assert_eq!(count_text(2890).as_str(), "2,890 apps");
        assert_eq!(count_text(1_234_567).as_str(), "1,234,567 apps");
    }

    #[test]
    fn only_the_rows_near_the_camera_are_mounted() {
        // 2,890 apps in 3 columns, 5 rows in view.
        let (start, end, view) = mounted_range(2890, 3, 0, 5);
        assert_eq!((start, end, view), (0, (5 + OVERSCAN_ROWS) * 3, 0..15));
        let (start, end, view) = mounted_range(2890, 3, 100, 5);
        assert_eq!(start, (100 - OVERSCAN_ROWS) * 3);
        assert_eq!(end, (105 + OVERSCAN_ROWS) * 3);
        assert_eq!(view, 300..315);
        // The last row is short, and nothing goes past the end.
        let (_, end, view) = mounted_range(2890, 3, 962, 5);
        assert_eq!((end, view.end), (2890, 2890));
        assert_eq!(mounted_range(0, 3, 4, 5), (0, 0, 0..0));
    }

    #[test]
    fn the_icon_shown_least_recently_goes_first() {
        let mut icons = IconCache::default();
        for at in 0..MAX_ICONS {
            icons.insert(format!("{at}"), slint::Image::default());
        }
        // The first one is shown again: the second is now the oldest.
        assert!(icons.get("0").is_some());
        icons.insert("new".into(), slint::Image::default());
        assert!(icons.contains("0") && icons.contains("new"));
        assert!(!icons.contains("1"));
        assert_eq!(icons.icons.len(), MAX_ICONS);
    }
}
