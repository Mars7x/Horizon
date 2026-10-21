//! Album presentation: the capture grid and the full-screen viewer.
//!
//! Everything slow happens elsewhere: scans on a thread per visit, decoding
//! on `MediaLoader` workers. Results come back
//! through one worker channel that wakes the UI thread once per burst, so an idle
//! Album costs nothing (no polling timer). Only rows near the camera are
//! mounted, only their thumbnails are requested, and memory is bounded by
//! small caches of decoded images.
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::Receiver,
    },
    time::Duration,
};

use chrono::{Local, TimeZone};
use image::RgbaImage;
use slint::{
    ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, Timer, VecModel,
};
use tracing::warn;

use crate::{
    AlbumTileData, AppRouteView, AppWindow, HintData,
    domain::{
        LibraryGame, SourceGameRef,
        album::{Capture, CaptureOrigin},
    },
    input::{UiAction, UiActionEvent},
    navigation::grid::{
        horizontal_step, scroll_to_selection, total_rows, vertical_step, wheel_scroll_top,
    },
    presentation::{
        mounted::{shift_window, window_shift},
        wake::drain_notify,
    },
    services::album::{
        AnimationPlayback, HorizonAlbum, MediaLoader, MediaResult, ThumbnailCache, play_animation,
        scan_all,
    },
    services::worker::{WorkerSender, spawn, worker_channel},
    sources::CaptureSource,
};

/// Rows mounted above and below the visible ones, so a camera step never
/// shows an empty row.
// Three: the partly visible row at an edge takes one, and a wheel spun fast
// leaves the gliding camera up to a row behind its destination
// (`ScrollCamera`), which is what these rows are mounted around.
const OVERSCAN_ROWS: usize = 3;
/// Decoded thumbnails kept in memory (at most about 2 MiB each).
const THUMBNAIL_MEMORY: usize = 96;
/// Decoded full-screen photos kept: the open one and its neighbours.
const DISPLAY_MEMORY: usize = 4;
const MEDIA_WORKERS: usize = 2;
/// Longer than the route transition, so the leaving page keeps its tiles.
const RELEASE_AFTER_LEAVING: Duration = Duration::from_secs(1);

enum AlbumEvent {
    Scanned(Vec<Capture>),
    Media(MediaResult),
}

/// One "Game" choice: the games that have captures, most recent first.
/// `None` collects captures of no game (Horizon's own screens).
#[derive(Clone, Debug, PartialEq)]
struct GameChoice {
    game: Option<SourceGameRef>,
    title: String,
}

#[derive(Default)]
struct State {
    captures: Vec<Capture>,
    scanned: bool,
    games: Vec<GameChoice>,
    /// 0 is "All games"; otherwise `games[game_index - 1]`.
    game_index: usize,
    /// Indices into `captures` that pass the filters, newest first.
    order: Vec<usize>,
    selection: usize,
    scroll_top: usize,
    wheel_scrolled: bool,
    mounted: Vec<PathBuf>,
    mounted_start: usize,
    viewing: bool,
    /// The grid's multi-select for deleting, and what is ticked.
    selecting: bool,
    checked: HashSet<PathBuf>,
    dialog: Option<DeleteDialog>,
    /// A one-off message in the bottom bar (a failed delete), until the next
    /// move.
    notice: Option<String>,
    slide_key: i32,
    visit_revision: i32,
    /// A Game change waiting for its first publish: the grid refresh
    /// glides tiles that stay and fades the rest.
    glide: Option<Glide>,
}

/// The grid as it was before a Game change.
struct Glide {
    /// The mounted captures, in `previous_tiles` order.
    previous: Vec<PathBuf>,
    /// Each of those captures' place in the old order.
    from: HashMap<PathBuf, i32>,
}

impl State {
    fn current(&self) -> Option<&Capture> {
        self.order
            .get(self.selection)
            .and_then(|index| self.captures.get(*index))
    }

    fn rebuild_order(&mut self) {
        let game = self
            .game_index
            .checked_sub(1)
            .and_then(|index| self.games.get(index))
            .map(|choice| choice.game.clone());
        self.order = self
            .captures
            .iter()
            .enumerate()
            .filter(|(_, capture)| game.as_ref().is_none_or(|g| capture.game() == g.as_ref()))
            .map(|(index, _)| index)
            .collect();
        self.selection = self.selection.min(self.order.len().saturating_sub(1));
    }
}

/// "Delete …?" confirmation. `focus` 0 is Cancel (the default), 1 Delete.
struct DeleteDialog {
    targets: Vec<PathBuf>,
    focus: i32,
}

/// What a press did, for the shell's sound cue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlbumFeedback {
    None,
    Ok,
    Back,
}

/// A small least-recently-used map of decoded images.
struct ImageMemory<K> {
    limit: usize,
    tick: u64,
    entries: HashMap<K, (Image, u64)>,
}

impl<K: std::hash::Hash + Eq + Clone> ImageMemory<K> {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            tick: 0,
            entries: HashMap::new(),
        }
    }

    fn get(&mut self, key: &K) -> Option<Image> {
        self.tick += 1;
        let tick = self.tick;
        self.entries.get_mut(key).map(|(image, used)| {
            *used = tick;
            image.clone()
        })
    }

    fn contains(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    fn insert(&mut self, key: K, image: Image) {
        self.tick += 1;
        self.entries.insert(key, (image, self.tick));
        while self.entries.len() > self.limit {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

pub struct AlbumController {
    horizon: Option<HorizonAlbum>,
    sources: Vec<Arc<dyn CaptureSource>>,
    cache: ThumbnailCache,
    loader: MediaLoader,
    titles: RefCell<HashMap<SourceGameRef, String>>,
    /// The hints last published, so the bar only changes when they do.
    hints: RefCell<Vec<(&'static str, String)>>,
    sender: WorkerSender<AlbumEvent>,
    receiver: Receiver<AlbumEvent>,
    scan_running: Cell<bool>,
    state: RefCell<State>,
    location_changed: crate::presentation::CallbackSlot<dyn Fn(crate::navigation::PageLocation)>,
    published_location: RefCell<crate::navigation::PageLocation>,
    tiles: Rc<VecModel<AlbumTileData>>,
    /// The grid as it was before a Game change, fading out.
    previous_tiles: Rc<VecModel<AlbumTileData>>,
    thumbnails: RefCell<ImageMemory<PathBuf>>,
    failed: RefCell<HashSet<PathBuf>>,
    display: RefCell<ImageMemory<(PathBuf, (u32, u32))>>,
    animation: RefCell<Option<AnimationPlayback>>,
    /// Reject frames from a previously viewed animated image.
    animation_generation: Arc<AtomicU64>,
    ui: slint::Weak<AppWindow>,
    this: std::rc::Weak<Self>,
}

impl AlbumController {
    pub fn new(
        ui: &AppWindow,
        library: &[LibraryGame],
        // Horizon's own capture folder; `None` shows source captures only.
        horizon_dir: Option<PathBuf>,
        sources: Vec<Arc<dyn CaptureSource>>,
        cache_dir: Option<PathBuf>,
    ) -> Rc<Self> {
        // Scans and decoded images land in one channel,
        // drained on the UI thread once per burst.
        let (notify, drain) = drain_notify(Self::drain);
        let (sender, receiver) = worker_channel(notify);
        let cache = ThumbnailCache::new(cache_dir);
        let media_sender = sender.clone();
        let loader = MediaLoader::start(
            MEDIA_WORKERS,
            cache.clone(),
            Arc::new(move |result| {
                let _ = media_sender.send(AlbumEvent::Media(result));
            }),
        );
        let titles = library
            .iter()
            .flat_map(|game| {
                game.sources().iter().map(|source| {
                    (
                        source.clone(),
                        super::titles::game_title(game.game().title().as_str()),
                    )
                })
            })
            .collect();
        let tiles = Rc::new(VecModel::from(Vec::<AlbumTileData>::new()));
        ui.set_album_tiles(ModelRc::from(Rc::clone(&tiles)));
        let previous_tiles = Rc::new(VecModel::from(Vec::<AlbumTileData>::new()));
        ui.set_album_previous_tiles(ModelRc::from(Rc::clone(&previous_tiles)));
        let controller = Rc::new_cyclic(|this| Self {
            horizon: horizon_dir.map(HorizonAlbum::new),
            sources,
            cache,
            loader,
            titles: RefCell::new(titles),
            hints: RefCell::new(Vec::new()),
            sender,
            receiver,
            scan_running: Cell::new(false),
            state: RefCell::new(State::default()),
            location_changed: RefCell::new(None),
            published_location: RefCell::new(crate::navigation::PageLocation::Root),
            tiles,
            previous_tiles,
            thumbnails: RefCell::new(ImageMemory::new(THUMBNAIL_MEMORY)),
            failed: RefCell::new(HashSet::new()),
            display: RefCell::new(ImageMemory::new(DISPLAY_MEMORY)),
            animation: RefCell::new(None),
            animation_generation: Arc::new(AtomicU64::new(0)),
            ui: ui.as_weak(),
            this: this.clone(),
        });
        drain.bind(&controller);
        controller.publish(ui);
        controller
    }

    /// Every Album visit is fresh: the newest capture, scrolled to the top.
    /// The Game filter is a view choice and is kept. The folder
    /// is rescanned so new captures appear.
    /// Name captures of games a library reload found. Names of games that
    /// left the library stay: their captures are still theirs.
    pub fn add_titles<'a>(&self, games: impl IntoIterator<Item = &'a LibraryGame>) {
        let mut titles = self.titles.borrow_mut();
        for game in games {
            for source in game.sources() {
                titles.insert(
                    source.clone(),
                    super::titles::game_title(game.game().title().as_str()),
                );
            }
        }
    }

    pub fn start_new_visit(&self, ui: &AppWindow) {
        self.stop_animation();
        {
            let mut s = self.state.borrow_mut();
            s.selection = 0;
            s.scroll_top = 0;
            s.wheel_scrolled = false;
            s.viewing = false;
            s.visit_revision = s.visit_revision.wrapping_add(1);
        }
        self.scan();
        self.publish(ui);
    }

    /// The route is leaving: nothing keeps playing or decoding behind it.
    pub fn on_leave(&self, _ui: &AppWindow) {
        self.stop_animation();
        self.loader.want(Vec::new(), Vec::new());
        self.display.borrow_mut().clear();
        // Once the page has zoomed away, give back the decoded thumbnails.
        // The disk cache makes the next visit quick to refill.
        let this = self.this.clone();
        Timer::single_shot(RELEASE_AFTER_LEAVING, move || {
            if let Some(controller) = this.upgrade()
                && let Some(ui) = controller.ui.upgrade()
                && ui.get_current_route() != AppRouteView::Album
            {
                controller.release_memory();
            }
        });
    }

    fn release_memory(&self) {
        self.thumbnails.borrow_mut().clear();
        self.tiles.set_vec(Vec::new());
        self.previous_tiles.set_vec(Vec::new());
        let mut s = self.state.borrow_mut();
        s.mounted.clear();
        s.mounted_start = 0;
    }

    /// Horizon lost the foreground (a game started): pause, never play on.
    pub fn handle_application_active_changed(&self, _ui: &AppWindow, active: bool) {
        if let Some(animation) = self.animation.borrow().as_ref() {
            animation.set_playing(active);
        }
    }

    fn scan(&self) {
        if self.scan_running.replace(true) {
            return;
        }
        let horizon = self.horizon.clone();
        let sources = self.sources.clone();
        let cache = self.cache.clone();
        let sender = self.sender.clone();
        if !spawn("album-scan", move || {
            let captures = scan_all(horizon.as_ref(), &sources);
            cache.prune(&captures);
            let _ = sender.send(AlbumEvent::Scanned(captures));
        }) {
            self.scan_running.set(false);
        }
    }

    fn drain(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let mut republish = false;
        let mut thumbnails = Vec::new();
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                AlbumEvent::Scanned(captures) => {
                    self.scan_running.set(false);
                    republish |= self.apply_scan(captures);
                }
                AlbumEvent::Media(MediaResult::Thumbnail { path, result }) => match result {
                    Ok(thumbnail) => {
                        let image = rgba_image(thumbnail.image);
                        self.thumbnails.borrow_mut().insert(path.clone(), image);
                        thumbnails.push(path);
                    }
                    Err(error) => {
                        warn!(path = %path.display(), %error, "Album thumbnail failed");
                        self.failed.borrow_mut().insert(path.clone());
                        thumbnails.push(path);
                    }
                },
                AlbumEvent::Media(MediaResult::Display { path, max, result }) => match result {
                    Ok(image) => {
                        let image = rgba_image(image);
                        self.display
                            .borrow_mut()
                            .insert((path.clone(), max), image.clone());
                        // The open photo sharpens in place, without a slide.
                        let s = self.state.borrow();
                        if s.viewing
                            && s.current().is_some_and(|c| c.path() == path)
                            && max == display_size(&ui)
                        {
                            drop(s);
                            if !self
                                .animation
                                .borrow()
                                .as_ref()
                                .is_some_and(|playback| playback.is_animated())
                            {
                                self.show(&ui, Some(image));
                            }
                        }
                    }
                    Err(error) => {
                        warn!(path = %path.display(), %error, "Album photo failed");
                        let s = self.state.borrow();
                        if s.viewing && s.current().is_some_and(|c| c.path() == path) {
                            ui.set_album_viewer_problem("Can't open this image".into());
                        }
                    }
                },
            }
        }
        if republish {
            self.publish(&ui);
        }
        if !thumbnails.is_empty() {
            self.refresh_tiles(&thumbnails);
            // The open capture's thumbnail fills an empty viewer; it never
            // replaces a sharper photo or an animation frame already on screen.
            let s = self.state.borrow();
            if s.viewing
                && let Some(current) = s.current()
                && thumbnails.iter().any(|path| path == current.path())
            {
                let image = self.thumbnails.borrow_mut().get(&current.path().to_owned());
                drop(s);
                set_landing(&ui, image.clone());
                if !ui.get_album_viewer_has_image() {
                    self.show(&ui, image);
                }
            }
        }
    }

    /// Returns true when the visible list changed.
    fn apply_scan(&self, captures: Vec<Capture>) -> bool {
        let mut s = self.state.borrow_mut();
        if s.scanned && s.captures == captures {
            return false;
        }
        let selected = s.current().map(|c| c.path().to_owned());
        // Changed files get another chance to decode.
        self.failed.borrow_mut().clear();
        let filter = s
            .game_index
            .checked_sub(1)
            .and_then(|index| s.games.get(index))
            .map(|choice| choice.game.clone());
        s.games = game_choices(&captures, &self.titles.borrow());
        s.game_index = filter
            .and_then(|game| s.games.iter().position(|choice| choice.game == game))
            .map_or(0, |index| index + 1);
        s.captures = captures;
        s.scanned = true;
        s.rebuild_order();
        if let Some(path) = selected
            && let Some(position) = s
                .order
                .iter()
                .position(|index| s.captures[*index].path() == path)
        {
            s.selection = position;
        }
        if s.viewing && s.current().is_none() {
            s.viewing = false;
        }
        true
    }

    // ----- grid -------------------------------------------------------

    pub fn handle_action(&self, ui: &AppWindow, event: UiActionEvent) -> AlbumFeedback {
        if self.state.borrow().dialog.is_some() {
            return self.handle_dialog_action(ui, event);
        }
        if self.state.borrow().viewing {
            return self.handle_viewer_action(ui, event);
        }
        let selecting = self.state.borrow().selecting;
        match event.action {
            UiAction::Left => self.move_horizontally(ui, -1),
            UiAction::Right => self.move_horizontally(ui, 1),
            UiAction::Up => self.move_vertically(ui, false),
            UiAction::Down => self.move_vertically(ui, true),
            UiAction::LeftBumper if !event.repeated && !selecting => self.cycle_game(ui),
            UiAction::Accept if !event.repeated && selecting => return self.toggle_checked(ui),
            UiAction::Accept if !event.repeated => {
                return feedback(self.open_viewer(ui));
            }
            UiAction::Secondary if !event.repeated && selecting => {
                return self.confirm_delete_checked(ui);
            }
            UiAction::Secondary if !event.repeated => return self.start_selecting(ui),
            _ => {}
        }
        AlbumFeedback::None
    }

    /// B, in order: close the dialog, leave multi-select, close the viewer.
    /// False when the Album has nothing open (route Back applies).
    pub fn set_location_changed(&self, callback: Rc<dyn Fn(crate::navigation::PageLocation)>) {
        *self.location_changed.borrow_mut() = Some(callback);
    }
    pub fn has_transient(&self) -> bool {
        let state = self.state.borrow();
        state.dialog.is_some() || state.selecting
    }
    pub fn restore_viewer(&self, ui: &AppWindow, path: &Path) -> bool {
        let mut state = self.state.borrow_mut();
        let index = state
            .order
            .iter()
            .position(|slot| state.captures[*slot].path() == path);
        let Some(index) = index else {
            return false;
        };
        state.selection = index;
        state.viewing = false;
        drop(state);
        self.open_viewer(ui)
    }

    pub fn back(&self, ui: &AppWindow) -> bool {
        if self.state.borrow().dialog.is_some() {
            self.choose_in_dialog(ui, 0);
            return true;
        }
        if self.state.borrow().selecting {
            self.stop_selecting(ui);
            return true;
        }
        self.close_viewer(ui)
    }

    // ----- deleting ---------------------------------------------------

    fn start_selecting(&self, ui: &AppWindow) -> AlbumFeedback {
        {
            let mut s = self.state.borrow_mut();
            let any = s.order.iter().any(|index| s.captures[*index].deletable());
            if !any {
                return AlbumFeedback::None;
            }
            s.selecting = true;
            s.checked.clear();
            s.notice = None;
        }
        self.remount(ui);
        AlbumFeedback::Ok
    }

    fn stop_selecting(&self, ui: &AppWindow) {
        {
            let mut s = self.state.borrow_mut();
            s.selecting = false;
            s.checked.clear();
        }
        self.remount(ui);
    }

    fn toggle_checked(&self, ui: &AppWindow) -> AlbumFeedback {
        {
            let mut s = self.state.borrow_mut();
            let Some(capture) = s.current().filter(|c| c.deletable()) else {
                return AlbumFeedback::None;
            };
            let path = capture.path().to_owned();
            if !s.checked.remove(&path) {
                s.checked.insert(path);
            }
        }
        self.remount(ui);
        AlbumFeedback::Ok
    }

    fn confirm_delete_checked(&self, ui: &AppWindow) -> AlbumFeedback {
        let targets: Vec<PathBuf> = {
            let s = self.state.borrow();
            // In Album order, so the dialog and the result read naturally.
            s.order
                .iter()
                .map(|index| s.captures[*index].path())
                .filter(|path| s.checked.contains(*path))
                .map(Path::to_owned)
                .collect()
        };
        self.open_dialog(ui, targets)
    }

    fn open_dialog(&self, ui: &AppWindow, targets: Vec<PathBuf>) -> AlbumFeedback {
        if targets.is_empty() {
            return AlbumFeedback::None;
        }
        let title = if targets.len() == 1 {
            "Delete this screenshot?".to_owned()
        } else {
            format!("Delete {} screenshots?", targets.len())
        };
        let text = String::new();
        ui.set_album_dialog_title(title.into());
        ui.set_album_dialog_text(text.into());
        ui.set_album_dialog_focus(0);
        ui.set_album_dialog_open(true);
        self.state.borrow_mut().dialog = Some(DeleteDialog { targets, focus: 0 });
        AlbumFeedback::Ok
    }

    fn handle_dialog_action(&self, ui: &AppWindow, event: UiActionEvent) -> AlbumFeedback {
        let focus = match event.action {
            UiAction::Left | UiAction::Up => 0,
            UiAction::Right | UiAction::Down => 1,
            UiAction::Accept if !event.repeated => {
                let focus = self.state.borrow().dialog.as_ref().map_or(0, |d| d.focus);
                return self.choose_in_dialog(ui, focus);
            }
            _ => return AlbumFeedback::None,
        };
        if let Some(dialog) = self.state.borrow_mut().dialog.as_mut() {
            dialog.focus = focus;
        }
        ui.set_album_dialog_focus(focus);
        AlbumFeedback::None
    }

    /// 0 Cancel, 1 Delete (also the dialog's buttons when clicked).
    pub fn choose_in_dialog(&self, ui: &AppWindow, choice: i32) -> AlbumFeedback {
        let Some(dialog) = self.state.borrow_mut().dialog.take() else {
            return AlbumFeedback::None;
        };
        ui.set_album_dialog_open(false);
        if choice != 1 {
            return AlbumFeedback::Back;
        }
        self.delete(ui, &dialog.targets);
        AlbumFeedback::Ok
    }

    fn delete(&self, ui: &AppWindow, targets: &[PathBuf]) {
        let viewing = self.state.borrow().viewing;
        if viewing {
            self.stop_animation();
        }
        let mut deleted = HashSet::new();
        let mut failures = 0;
        for path in targets {
            let origin = self
                .state
                .borrow()
                .captures
                .iter()
                .find(|c| c.path() == path && c.deletable())
                .map(|c| c.origin().clone());
            // Each capture is deleted by its owner: Horizon's album, or the
            // source that listed it (which checks the path again).
            let result = match origin {
                Some(CaptureOrigin::Horizon) => {
                    self.horizon.as_ref().map(|album| album.delete(path))
                }
                Some(CaptureOrigin::Source(id)) => self
                    .sources
                    .iter()
                    .find(|source| *source.source_id() == id)
                    .map(|source| source.delete(path)),
                None => None,
            }
            .unwrap_or_else(|| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "not a capture the Album can delete",
                ))
            });
            match result {
                Ok(()) => {
                    deleted.insert(path.clone());
                }
                Err(error) => {
                    warn!(path = %path.display(), %error, "Album capture could not be deleted");
                    failures += 1;
                }
            }
        }
        {
            let mut s = self.state.borrow_mut();
            s.captures.retain(|c| !deleted.contains(c.path()));
            let filter = s
                .game_index
                .checked_sub(1)
                .and_then(|index| s.games.get(index))
                .map(|choice| choice.game.clone());
            s.games = game_choices(&s.captures, &self.titles.borrow());
            s.game_index = filter
                .and_then(|game| s.games.iter().position(|choice| choice.game == game))
                .map_or(0, |index| index + 1);
            // The capture after the deleted one takes its place.
            s.rebuild_order();
            s.checked.clear();
            s.selecting = false;
            s.wheel_scrolled = false;
            if s.viewing && s.current().is_none() {
                s.viewing = false;
            }
            s.notice = (failures > 0).then(|| {
                format!(
                    "Couldn't delete {failures} {}",
                    if failures == 1 { "capture" } else { "captures" }
                )
            });
        }
        for path in &deleted {
            self.failed.borrow_mut().remove(path);
        }
        if viewing && self.state.borrow().viewing {
            ui.set_album_viewer_previous_has_image(false);
            self.present_current(ui);
        }
        self.remount(ui);
    }

    /// Republish with every mounted tile rebuilt (ticks, removed captures).
    fn remount(&self, ui: &AppWindow) {
        self.state.borrow_mut().mounted.clear();
        self.publish(ui);
    }

    fn move_horizontally(&self, ui: &AppWindow, delta: i32) {
        let target = {
            let s = self.state.borrow();
            horizontal_step(s.selection, s.order.len(), delta)
        };
        self.move_selection(ui, target);
    }

    fn move_vertically(&self, ui: &AppWindow, down: bool) {
        let columns = columns(ui);
        let target = {
            let s = self.state.borrow();
            vertical_step(s.selection, s.order.len(), columns, down)
        };
        self.move_selection(ui, target);
    }

    fn move_selection(&self, ui: &AppWindow, target: usize) {
        {
            let mut s = self.state.borrow_mut();
            if target >= s.order.len() || (target == s.selection && !s.wheel_scrolled) {
                return;
            }
            s.selection = target;
            s.wheel_scrolled = false;
            s.notice = None;
        }
        self.publish(ui);
    }

    /// A click opens the tile straight away (selecting it, so Back returns
    /// to it). While selecting, it ticks the tile instead.
    pub fn choose(&self, ui: &AppWindow, index: i32) -> bool {
        let Ok(index) = usize::try_from(index) else {
            return false;
        };
        let selecting = {
            let mut s = self.state.borrow_mut();
            if s.viewing || s.dialog.is_some() || index >= s.order.len() {
                return false;
            }
            s.selection = index;
            s.wheel_scrolled = false;
            s.notice = None;
            s.selecting
        };
        if selecting {
            return self.toggle_checked(ui) == AlbumFeedback::Ok;
        }
        self.open_viewer(ui)
    }

    pub fn scroll_rows(&self, ui: &AppWindow, delta: i32) {
        let (columns, visible) = (columns(ui), visible_rows(ui));
        {
            let mut s = self.state.borrow_mut();
            if s.viewing {
                return;
            }
            let top = wheel_scroll_top(s.scroll_top, s.order.len(), columns, visible, delta);
            if top == s.scroll_top {
                return;
            }
            s.scroll_top = top;
            s.wheel_scrolled = true;
        }
        self.publish(ui);
    }

    /// Freeze the grid as shown and start the shared grid refresh: tiles in
    /// both grids glide to their new places, the rest fade out or in.
    fn freeze_grid(&self, ui: &AppWindow) {
        let mut s = self.state.borrow_mut();
        self.previous_tiles
            .set_vec(self.tiles.iter().collect::<Vec<_>>());
        let from = s
            .mounted
            .iter()
            .enumerate()
            .map(|(row, path)| (path.clone(), (s.mounted_start + row) as i32))
            .collect();
        // The next publish remounts even an unchanged window, so every tile
        // learns where it came from.
        let previous = std::mem::take(&mut s.mounted);
        s.glide = Some(Glide { previous, from });
        ui.set_album_previous_scroll_row(s.scroll_top as i32);
        ui.set_album_previous_total_count(s.order.len() as i32);
        ui.set_album_previous_selected_index(s.selection as i32);
        ui.set_album_refresh_sequence(ui.get_album_refresh_sequence().wrapping_add(1));
    }

    pub fn cycle_game(&self, ui: &AppWindow) {
        {
            let s = self.state.borrow();
            if s.viewing || s.captures.is_empty() {
                return;
            }
        }
        self.freeze_grid(ui);
        {
            let mut s = self.state.borrow_mut();
            s.game_index = (s.game_index + 1) % (s.games.len() + 1);
            s.selection = 0;
            s.scroll_top = 0;
            s.wheel_scrolled = false;
            s.rebuild_order();
        }
        self.publish(ui);
    }

    pub fn on_layout_changed(&self, ui: &AppWindow) {
        self.publish(ui);
    }

    // ----- viewer -----------------------------------------------------

    fn handle_viewer_action(&self, ui: &AppWindow, event: UiActionEvent) -> AlbumFeedback {
        // Any button brings the fullscreen overlay back.
        wake_overlay(ui);
        match event.action {
            UiAction::Left => self.step(ui, -1),
            UiAction::Right => self.step(ui, 1),
            UiAction::Secondary if !event.repeated => {
                let target = self
                    .state
                    .borrow()
                    .current()
                    .filter(|c| c.deletable())
                    .map(|c| c.path().to_owned());
                return target.map_or(AlbumFeedback::None, |path| self.open_dialog(ui, vec![path]));
            }
            _ => {}
        }
        AlbumFeedback::None
    }

    pub fn viewing(&self) -> bool {
        self.state.borrow().viewing
    }

    pub fn open_viewer(&self, ui: &AppWindow) -> bool {
        {
            let mut s = self.state.borrow_mut();
            if s.viewing || s.selecting || s.dialog.is_some() || s.current().is_none() {
                return false;
            }
            s.viewing = true;
            s.notice = None;
        }
        ui.set_album_viewer_previous_has_image(false);
        self.present_current(ui);
        self.publish(ui);
        wake_overlay(ui);
        true
    }

    /// Back from the viewer to the grid, on the capture that was open.
    pub fn close_viewer(&self, ui: &AppWindow) -> bool {
        {
            let mut s = self.state.borrow_mut();
            if !s.viewing {
                return false;
            }
            s.viewing = false;
            s.wheel_scrolled = false;
        }
        self.stop_animation();
        self.display.borrow_mut().clear();
        self.publish(ui);
        true
    }

    /// Next (+1) or previous (-1) capture, sliding in from that side.
    /// Stops at either end.
    pub fn step(&self, ui: &AppWindow, delta: i32) {
        {
            let mut s = self.state.borrow_mut();
            if !s.viewing || s.dialog.is_some() {
                return;
            }
            let target = horizontal_step(s.selection, s.order.len(), delta);
            if target == s.selection {
                return;
            }
            s.selection = target;
            s.slide_key = s.slide_key.wrapping_add(1);
        }
        // The currently displayed image slides out.
        let leaving = ui.get_album_viewer_has_image();
        ui.set_album_viewer_previous_has_image(leaving);
        if leaving {
            ui.set_album_viewer_previous_image(ui.get_album_viewer_image());
        }
        ui.set_album_slide_direction(delta.signum());
        self.present_current(ui);
        // Publish the key last: the slide starts with both images in place.
        self.publish(ui);
    }

    /// Show the current capture: the sharpest image already decoded, then
    /// ask for better and start any image animation.
    fn present_current(&self, ui: &AppWindow) {
        self.stop_animation();
        ui.set_album_viewer_problem("".into());
        let Some(capture) = self.state.borrow().current().cloned() else {
            return;
        };
        let path = capture.path().to_owned();
        let thumbnail = self.thumbnails.borrow_mut().get(&path);
        set_landing(ui, thumbnail.clone());
        let image = self
            .display
            .borrow_mut()
            .get(&(path.clone(), display_size(ui)))
            .or(thumbnail);
        self.show(ui, image);
        if self.failed.borrow().contains(&path) {
            ui.set_album_viewer_problem("Can't open this image".into());
            return;
        }
        if !ui.global::<crate::Motion>().get_reduced_motion() {
            self.start_animation(ui, &path);
        }
    }

    fn start_animation(&self, ui: &AppWindow, path: &std::path::Path) {
        let generation = self.animation_generation.load(Ordering::SeqCst);
        let current = Arc::clone(&self.animation_generation);
        let pending = Arc::new(AtomicBool::new(false));
        let weak = self.ui.clone();
        match play_animation(
            path,
            display_size(ui),
            Box::new(move |frame| {
                let is_frame = frame.is_ok();
                if current.load(Ordering::SeqCst) != generation
                    || (is_frame && pending.swap(true, Ordering::SeqCst))
                {
                    return;
                }
                let frame = frame.map(|frame| {
                    SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                        frame.as_raw(),
                        frame.width(),
                        frame.height(),
                    )
                });
                let (pending, current) = (Arc::clone(&pending), Arc::clone(&current));
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    if is_frame {
                        pending.store(false, Ordering::SeqCst);
                    }
                    if current.load(Ordering::SeqCst) != generation {
                        return;
                    }
                    match frame {
                        Ok(buffer)
                            if !ui.global::<crate::Motion>().get_reduced_motion()
                                && !ui.global::<crate::Motion>().get_loops_paused() =>
                        {
                            ui.set_album_viewer_image(Image::from_rgba8(buffer));
                            ui.set_album_viewer_has_image(true);
                        }
                        Ok(_) => {}
                        Err(error) => {
                            warn!(%error, "Album JXL animation failed");
                            ui.set_album_viewer_problem("Can't play this animated image".into());
                        }
                    }
                });
            }),
        ) {
            Ok(playback) => *self.animation.borrow_mut() = Some(playback),
            Err(error) => {
                warn!(%error, "Album JXL worker could not start");
                ui.set_album_viewer_problem("Can't play this animated image".into());
            }
        }
    }

    fn show(&self, ui: &AppWindow, image: Option<Image>) {
        ui.set_album_viewer_has_image(image.is_some());
        if let Some(image) = &image {
            ui.set_album_viewer_image(image.clone());
        }
    }

    fn stop_animation(&self) {
        self.animation_generation.fetch_add(1, Ordering::SeqCst);
        *self.animation.borrow_mut() = None;
    }

    // ----- publishing -------------------------------------------------

    fn publish(&self, ui: &AppWindow) {
        self.publish_page(ui);
        self.publish_hints(ui);
        let location = {
            let state = self.state.borrow();
            if state.viewing {
                state
                    .current()
                    .map(|capture| {
                        crate::navigation::PageLocation::Album(capture.path().to_owned())
                    })
                    .unwrap_or_default()
            } else {
                crate::navigation::PageLocation::Root
            }
        };
        if self.published_location.replace(location.clone()) != location
            && let Some(callback) = self.location_changed.borrow().as_ref()
        {
            callback(location);
        }
    }

    /// The hint bar: X, B, A from left to right (A rightmost), each only
    /// when it does something here.
    fn publish_hints(&self, ui: &AppWindow) {
        let hints: Vec<(&'static str, String)> = {
            let s = self.state.borrow();
            let mut hints = Vec::new();
            if s.viewing {
                if let Some(capture) = s.current() {
                    if capture.deletable() {
                        hints.push(("X", "Delete".to_owned()));
                    }
                    hints.push(("B", "Back".to_owned()));
                } else {
                    hints.push(("B", "Back".to_owned()));
                }
            } else if s.selecting {
                if !s.checked.is_empty() {
                    hints.push(("X", "Delete".to_owned()));
                }
                hints.push(("B", "Done".to_owned()));
                hints.push(("A", "Select".to_owned()));
            } else if s.order.is_empty() {
                hints.push(("B", "Back".to_owned()));
            } else {
                if s.order.iter().any(|index| s.captures[*index].deletable()) {
                    hints.push(("X", "Delete".to_owned()));
                }
                hints.push(("B", "Back".to_owned()));
                hints.push(("A", "View".to_owned()));
            }
            hints
        };
        if *self.hints.borrow() == hints {
            return;
        }
        let model: Vec<HintData> = hints
            .iter()
            .map(|(button, label)| HintData {
                button: (*button).into(),
                button2: "".into(),
                label: label.as_str().into(),
            })
            .collect();
        ui.set_album_hints(ModelRc::new(VecModel::from(model)));
        *self.hints.borrow_mut() = hints;
    }

    fn publish_page(&self, ui: &AppWindow) {
        let columns = columns(ui);
        let visible = visible_rows(ui);
        let mut s = self.state.borrow_mut();
        let count = s.order.len();
        if s.wheel_scrolled {
            s.scroll_top = s
                .scroll_top
                .min(total_rows(count, columns).saturating_sub(visible));
        } else {
            s.scroll_top = scroll_to_selection(s.selection, columns, visible, s.scroll_top, count);
        }
        let start_row = s.scroll_top.saturating_sub(OVERSCAN_ROWS);
        let end_row = (s.scroll_top + visible + OVERSCAN_ROWS).min(total_rows(count, columns));
        let start = (start_row * columns).min(count);
        let end = (end_row * columns).min(count);
        let mounted: Vec<PathBuf> = s.order[start..end]
            .iter()
            .map(|index| s.captures[*index].path().to_owned())
            .collect();
        if mounted != s.mounted || start != s.mounted_start {
            let glide = s.glide.take();
            // A grid refresh rebuilds every tile (its glide needs that).
            let refresh = glide.is_some();
            let tiles: Vec<AlbumTileData> = s.order[start..end]
                .iter()
                .enumerate()
                .map(|(offset, index)| {
                    let capture = &s.captures[*index];
                    let mut tile =
                        self.tile(capture, start + offset, s.checked.contains(capture.path()));
                    tile.from_index = glide
                        .as_ref()
                        .and_then(|glide| glide.from.get(capture.path()))
                        .copied()
                        .unwrap_or(-1);
                    tile
                })
                .collect();
            if let Some(glide) = glide {
                // Old tiles still mounted glide instead of fading.
                for (row, path) in glide.previous.iter().enumerate() {
                    if let Some(mut tile) = self.previous_tiles.row_data(row) {
                        tile.stays = mounted.contains(path);
                        self.previous_tiles.set_row_data(row, tile);
                    }
                }
            }
            let shifted = !refresh && window_shift(&s.mounted, s.mounted_start, &mounted, start);
            if shifted {
                shift_window(&self.tiles, s.mounted_start, start, tiles);
            } else {
                self.tiles.set_vec(tiles);
            }
            s.mounted = mounted;
            s.mounted_start = start;
        }

        // Bottom bar, left: a notice, the selection count while selecting,
        // otherwise when the selected capture was taken.
        let selected_text = s.notice.clone().unwrap_or_else(|| {
            if s.selecting {
                format!("{} selected", s.checked.len())
            } else {
                s.current()
                    .map(|capture| when_text(capture.captured_at()))
                    .unwrap_or_default()
            }
        });
        ui.set_album_selecting(s.selecting);
        ui.set_album_can_delete(s.viewing && s.current().is_some_and(Capture::deletable));
        let game_text = s
            .game_index
            .checked_sub(1)
            .and_then(|index| s.games.get(index))
            .map_or_else(|| "All games".to_owned(), |choice| choice.title.clone());
        ui.set_album_total_count(count as i32);
        ui.set_album_collection_count(s.captures.len() as i32);
        ui.set_album_status(if s.scanned { "No screenshots yet" } else { "" }.into());
        ui.set_album_game_text(game_text.into());
        ui.set_album_scroll_row(s.scroll_top as i32);
        ui.set_album_selected_index(s.selection as i32);
        ui.set_album_selected_text(selected_text.into());
        ui.set_album_visit_revision(s.visit_revision);
        if let Some(capture) = s.current().filter(|_| s.viewing) {
            // A notice (a failed delete) takes the date's place a moment.
            let text = s
                .notice
                .clone()
                .unwrap_or_else(|| when_text(capture.captured_at()));
            ui.set_album_viewer_text(text.into());
        }
        ui.set_album_slide_key(s.slide_key);
        ui.set_album_viewing(s.viewing);
        drop(s);
        self.request_media(ui);
    }

    fn tile(&self, capture: &Capture, absolute_index: usize, checked: bool) -> AlbumTileData {
        let path = capture.path().to_owned();
        let thumbnail = self.thumbnails.borrow_mut().get(&path);
        AlbumTileData {
            absolute_index: absolute_index as i32,
            has_thumbnail: thumbnail.is_some(),
            thumbnail: thumbnail.unwrap_or_default(),
            failed: self.failed.borrow().contains(&path),
            deletable: capture.deletable(),
            checked,
            from_index: -1,
            stays: false,
        }
    }

    /// Update mounted tiles whose thumbnails just arrived.
    fn refresh_tiles(&self, paths: &[PathBuf]) {
        let s = self.state.borrow();
        for (row, mounted) in s.mounted.iter().enumerate() {
            if paths.contains(mounted)
                && let Some(index) = s.order.get(s.mounted_start + row)
            {
                let capture = &s.captures[*index];
                let mut tile = self.tile(
                    capture,
                    s.mounted_start + row,
                    s.checked.contains(capture.path()),
                );
                // A refresh in progress keeps gliding this tile.
                tile.from_index = self.tiles.row_data(row).map_or(-1, |old| old.from_index);
                self.tiles.set_row_data(row, tile);
            }
        }
    }

    /// Tell the loader what the screen needs now: the open photo and its
    /// neighbours first, then thumbnails for mounted tiles, visible rows
    /// before overscan.
    fn request_media(&self, ui: &AppWindow) {
        let s = self.state.borrow();
        let mut display = Vec::new();
        let mut thumbnails = Vec::new();
        let wanted_thumbnail = |capture: &Capture| {
            let path = capture.path().to_owned();
            !self.thumbnails.borrow().contains(&path) && !self.failed.borrow().contains(&path)
        };
        if s.viewing {
            let size = display_size(ui);
            let direction = ui.get_album_slide_direction().signum() as isize;
            for offset in [0, direction, -direction] {
                let Some(capture) = s
                    .selection
                    .checked_add_signed(offset)
                    .and_then(|position| s.order.get(position))
                    .map(|index| &s.captures[*index])
                else {
                    continue;
                };
                if wanted_thumbnail(capture) {
                    thumbnails.push(capture.clone());
                }
                if !self
                    .display
                    .borrow()
                    .contains(&(capture.path().to_owned(), size))
                {
                    display.push((capture.clone(), size));
                }
            }
        }
        let columns = columns(ui);
        let visible_start = s.scroll_top * columns;
        let visible_end = visible_start + visible_rows(ui) * columns;
        let mut mounted: Vec<(bool, &Capture)> = s
            .order
            .iter()
            .enumerate()
            .skip(s.mounted_start)
            .take(s.mounted.len())
            .map(|(position, index)| {
                let on_screen = (visible_start..visible_end).contains(&position);
                (!on_screen, &s.captures[*index])
            })
            .collect();
        mounted.sort_by_key(|(off_screen, _)| *off_screen);
        thumbnails.extend(
            mounted
                .into_iter()
                .map(|(_, capture)| capture)
                .filter(|capture| wanted_thumbnail(capture))
                .cloned(),
        );
        self.loader.want(display, thumbnails);
    }
}

fn game_title(game: Option<&SourceGameRef>, titles: &HashMap<SourceGameRef, String>) -> String {
    match game {
        None => "Horizon".to_owned(),
        Some(game) => titles.get(game).cloned().unwrap_or_else(|| {
            // Not (or no longer) in the library: say what it is, honestly.
            let source = game.source_id().as_str();
            let mut letters = source.chars();
            let source = letters
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + letters.as_str())
                .unwrap_or_default();
            format!("{source} app {}", game.external_id().as_str())
        }),
    }
}

/// Games with captures, the most recently captured first.
fn game_choices(captures: &[Capture], titles: &HashMap<SourceGameRef, String>) -> Vec<GameChoice> {
    let mut seen = HashSet::new();
    captures
        .iter()
        .filter(|capture| seen.insert(capture.game().cloned()))
        .map(|capture| GameChoice {
            game: capture.game().cloned(),
            title: game_title(capture.game(), titles),
        })
        .collect()
}

fn feedback(ok: bool) -> AlbumFeedback {
    if ok {
        AlbumFeedback::Ok
    } else {
        AlbumFeedback::None
    }
}

/// The grid thumbnail the viewer lands on when it closes.
fn set_landing(ui: &AppWindow, thumbnail: Option<Image>) {
    ui.set_album_viewer_landing_has_image(thumbnail.is_some());
    if let Some(thumbnail) = thumbnail {
        ui.set_album_viewer_landing_image(thumbnail);
    }
}

fn wake_overlay(ui: &AppWindow) {
    ui.set_album_overlay_revision(ui.get_album_overlay_revision().wrapping_add(1));
}

fn columns(ui: &AppWindow) -> usize {
    ui.get_album_grid_columns().max(1) as usize
}

fn visible_rows(ui: &AppWindow) -> usize {
    ui.get_album_visible_rows().max(1) as usize
}

/// Full-screen media is decoded at the window's physical size.
fn display_size(ui: &AppWindow) -> (u32, u32) {
    let size = ui.window().size();
    (size.width.max(1), size.height.max(1))
}

fn rgba_image(image: RgbaImage) -> Image {
    let (width, height) = image.dimensions();
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        image.as_raw(),
        width,
        height,
    ))
}

fn when_text(millis: i64) -> String {
    Local
        .timestamp_millis_opt(millis)
        .single()
        .map(|time| time.format("%b %-d, %Y, %-I:%M %p").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        ExternalGameId, SourceId,
        album::{CaptureOrigin, MediaKind},
    };

    fn game(id: &str) -> SourceGameRef {
        SourceGameRef::new(
            SourceId::new("steam").unwrap(),
            ExternalGameId::new(id).unwrap(),
        )
    }

    fn capture(path: &str, kind: MediaKind, game_id: Option<&str>, at: i64) -> Capture {
        Capture::new(
            PathBuf::from(path),
            kind,
            CaptureOrigin::Horizon,
            game_id.map(game),
            at,
        )
    }

    #[test]
    fn game_choices_follow_the_most_recent_capture_and_name_unknown_games() {
        let titles = HashMap::from([(game("10"), "Portal".to_owned())]);
        let captures = vec![
            capture("a", MediaKind::Photo, Some("20"), 5),
            capture("b", MediaKind::Photo, Some("10"), 4),
            capture("c", MediaKind::Photo, None, 3),
            capture("d", MediaKind::Photo, Some("20"), 2),
        ];
        let choices = game_choices(&captures, &titles);
        let names: Vec<_> = choices.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(names, ["Steam app 20", "Portal", "Horizon"]);
    }

    #[test]
    fn game_filter_keeps_newest_first_order() {
        let mut state = State {
            captures: vec![
                capture("a", MediaKind::Photo, Some("20"), 5),
                capture("b", MediaKind::Photo, Some("20"), 4),
                capture("c", MediaKind::Photo, Some("10"), 3),
            ],
            ..State::default()
        };
        state.games = game_choices(&state.captures, &HashMap::new());
        state.rebuild_order();
        assert_eq!(state.order, [0, 1, 2]);
        state.game_index = 1; // Steam app 20
        state.rebuild_order();
        assert_eq!(state.order, [0, 1]);
        state.game_index = 2;
        state.rebuild_order();
        assert_eq!(state.order, [2]);
    }

    #[test]
    fn image_memory_forgets_the_least_recently_used() {
        let mut memory = ImageMemory::new(2);
        memory.insert("a", Image::default());
        memory.insert("b", Image::default());
        assert!(memory.get(&"a").is_some());
        memory.insert("c", Image::default());
        assert!(memory.contains(&"a"));
        assert!(!memory.contains(&"b"));
        assert!(memory.contains(&"c"));
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::*;

    #[test]
    #[ignore = "requires Wayland and a fresh HORIZON_JXL_ANIMATION_TEST_DIR"]
    fn animated_jxl_viewer_stops_and_respects_reduced_motion() {
        let output = PathBuf::from(std::env::var_os("HORIZON_JXL_ANIMATION_TEST_DIR").unwrap());
        let root = output.join("album");
        let folder = root.join("horizon/screenshots");
        std::fs::create_dir_all(&folder).unwrap();
        let frames = vec![
            (
                RgbaImage::from_pixel(240, 160, image::Rgba([240, 16, 32, 255])),
                Duration::from_millis(120),
            ),
            (
                RgbaImage::from_pixel(240, 160, image::Rgba([16, 240, 32, 128])),
                Duration::from_millis(180),
            ),
        ];
        let animation = crate::services::profile::Animation {
            still: frames[0].0.clone(),
            frames,
        };
        crate::services::profile::save_animation(&folder.join("moving.jxl"), &animation).unwrap();
        let ui = AppWindow::new().unwrap();
        let reduced = std::env::var_os("HORIZON_JXL_TEST_REDUCED").is_some();
        ui.global::<crate::Motion>().set_reduced_motion(reduced);
        ui.set_current_route(AppRouteView::Album);
        let album = AlbumController::new(&ui, &[], Some(root), Vec::new(), None);
        ui.window().set_size(slint::PhysicalSize::new(1280, 720));
        ui.show().unwrap();
        album.start_new_visit(&ui);
        let ticks = Cell::new(0);
        let stage = Cell::new(0);
        let paused_at = Cell::new(0);
        let frozen = RefCell::new(Vec::new());
        let weak = ui.as_weak();
        let timer = Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(80),
            move || {
                if stage.get() == 4 {
                    return;
                }
                let ui = weak.upgrade().unwrap();
                ticks.set(ticks.get() + 1);
                assert!(ticks.get() < 100, "JXL viewer timed out");
                assert!(ui.get_album_viewer_problem().is_empty());
                if stage.get() == 0 {
                    if album.tiles.row_count() != 1
                        || album.tiles.iter().any(|tile| !tile.has_thumbnail)
                    {
                        return;
                    }
                    assert!(album.open_viewer(&ui));
                    stage.set(1);
                    paused_at.set(ticks.get());
                    return;
                }
                let Some(frame) = ui.get_album_viewer_image().to_rgba8() else {
                    return;
                };
                assert_eq!((frame.width(), frame.height()), (240, 160));
                let green = frame.as_slice()[0].g > 200;
                if reduced {
                    assert!(!green);
                    assert!(album.animation.borrow().is_none());
                    if ticks.get() - paused_at.get() < 6 {
                        return;
                    }
                } else {
                    match stage.get() {
                        1 if green => {
                            *frozen.borrow_mut() = frame.as_bytes().to_vec();
                            album.handle_application_active_changed(&ui, false);
                            ui.global::<crate::Motion>().set_loops_paused(true);
                            stage.set(2);
                            paused_at.set(ticks.get());
                            return;
                        }
                        1 => return,
                        2 => {
                            assert_eq!(frame.as_bytes(), frozen.borrow().as_slice());
                            if ticks.get() - paused_at.get() < 6 {
                                return;
                            }
                            ui.global::<crate::Motion>().set_loops_paused(false);
                            album.handle_application_active_changed(&ui, true);
                            stage.set(3);
                            return;
                        }
                        3 if green => return,
                        _ => {}
                    }
                }
                // PNG diagnostic output can queue another timer tick before
                // event-loop shutdown; never close the viewer twice.
                stage.set(4);
                let snapshot = ui.window().take_snapshot().unwrap();
                image::save_buffer(
                    output.join("viewer.png"),
                    snapshot.as_bytes(),
                    snapshot.width(),
                    snapshot.height(),
                    image::ColorType::Rgba8,
                )
                .unwrap();
                assert!(album.close_viewer(&ui));
                assert!(album.animation.borrow().is_none());
                slint::quit_event_loop().unwrap();
            },
        );
        slint::run_event_loop().unwrap();
    }

    #[test]
    #[ignore = "requires Wayland and a fresh HORIZON_ALBUM_THUMBNAIL_TEST_DIR"]
    fn album_thumbnail_detail_and_native_pixels() {
        let output = PathBuf::from(
            std::env::var_os("HORIZON_ALBUM_THUMBNAIL_TEST_DIR").expect("test output"),
        );
        let root = output.join("album");
        std::fs::create_dir_all(root.join("horizon/screenshots")).unwrap();
        std::fs::create_dir_all(output.join("cache")).unwrap();
        for (name, width, height) in [
            ("portrait", 1080, 1920),
            ("square", 1920, 1920),
            ("wide", 3840, 1080),
            ("native", 240, 160),
            ("ds-side", 512, 192),
            ("ds-stacked", 256, 384),
        ] {
            let original = image::RgbaImage::from_fn(width, height, |x, y| {
                image::Rgba(if (x / 4 + y / 4) % 2 == 0 {
                    [255, 0, 255, 255]
                } else {
                    [0, 255, 0, 255]
                })
            });
            let file = std::fs::File::create(root.join(format!("horizon/screenshots/{name}.jxl")))
                .unwrap();
            crate::platform::jpeg_xl::write_rgba(&file, original.as_raw(), width, height).unwrap();
        }
        let ui = AppWindow::new().unwrap();
        ui.global::<crate::Motion>().set_reduced_motion(true);
        ui.set_current_route(AppRouteView::Album);
        let album =
            AlbumController::new(&ui, &[], Some(root), Vec::new(), Some(output.join("cache")));
        let weak_album = Rc::downgrade(&album);
        let weak_ui = ui.as_weak();
        ui.on_album_layout_changed(move || {
            if let (Some(album), Some(ui)) = (weak_album.upgrade(), weak_ui.upgrade()) {
                album.on_layout_changed(&ui);
            }
        });
        ui.window().set_size(slint::PhysicalSize::new(1920, 1080));
        ui.show().unwrap();
        album.start_new_visit(&ui);
        let ticks = Cell::new(0);
        let stage = Cell::new(0);
        let final_width: u32 = std::env::var("HORIZON_ALBUM_THUMBNAIL_TEST_WIDTH")
            .unwrap_or_else(|_| "3840".into())
            .parse()
            .unwrap();
        let weak_ui = ui.as_weak();
        let timer = Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(300),
            move || {
                let ui = weak_ui.upgrade().unwrap();
                ticks.set(ticks.get() + 1);
                assert!(ticks.get() < 80, "thumbnail runtime timed out");
                if album.tiles.row_count() != 6
                    || album.tiles.iter().any(|tile| !tile.has_thumbnail)
                {
                    return;
                }
                for capture in &album.state.borrow().captures {
                    let image = album
                        .thumbnails
                        .borrow_mut()
                        .get(&capture.path().to_owned())
                        .unwrap();
                    // Near 16:9 (or small, native): cropped to the tile or
                    // kept; far from it: the whole capture, fitted.
                    let expected = match capture.path().file_stem().unwrap().to_str().unwrap() {
                        "native" => (240, 160),
                        "ds-side" => (512, 192),
                        "ds-stacked" => (256, 384),
                        "portrait" => (304, 540),
                        "square" => (540, 540),
                        _ => (960, 270),
                    };
                    assert_eq!((image.size().width, image.size().height), expected);
                }
                let snapshot = ui.window().take_snapshot().unwrap();
                let width = if stage.get() == 0 { 1920 } else { final_width };
                assert_eq!(snapshot.width(), width);
                assert_eq!(snapshot.height(), width * 9 / 16);
                image::save_buffer(
                    output.join(format!("album-{width}.png")),
                    snapshot.as_bytes(),
                    snapshot.width(),
                    snapshot.height(),
                    image::ColorType::Rgba8,
                )
                .unwrap();
                if stage.replace(1) == 0 && final_width != 1920 {
                    ui.window()
                        .set_size(slint::PhysicalSize::new(final_width, final_width * 9 / 16));
                } else {
                    slint::quit_event_loop().unwrap();
                }
            },
        );
        slint::run_event_loop().unwrap();
    }
}
