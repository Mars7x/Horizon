//! Album presentation: the capture grid and the full-screen viewer.
//!
//! Everything slow happens elsewhere: scans on a thread per visit, decoding
//! on `MediaLoader` workers, video on GStreamer's threads. Results come back
//! through one channel and a single `album-media-ready` wake-up, so an idle
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
        mpsc::{self, Receiver, Sender},
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
    AlbumTileData, AppRouteView, AppWindow,
    domain::{
        LibraryGame, SourceGameRef,
        album::{Capture, MediaKind},
    },
    input::{UiAction, UiActionEvent},
    navigation::grid::{
        horizontal_step, scroll_to_selection, total_rows, vertical_step, wheel_scroll_top,
    },
    services::album::{
        HorizonAlbum, MediaLoader, MediaResult, ThumbnailCache, VideoBackend, VideoFrame,
        VideoPlayback, VideoSink, scan_all,
    },
    sources::CaptureSource,
};

/// Rows mounted above and below the visible ones, so a camera step never
/// shows an empty row.
const OVERSCAN_ROWS: usize = 2;
/// Decoded thumbnails kept in memory (about 0.5 MiB each).
const THUMBNAIL_MEMORY: usize = 96;
/// Decoded full-screen photos kept: the open one and its neighbours.
const DISPLAY_MEMORY: usize = 4;
const MEDIA_WORKERS: usize = 2;
const SKIP: Duration = Duration::from_secs(10);
/// How often the video clock is re-anchored to the real playback position.
/// Between anchors Slint advances it every frame.
const VIDEO_CLOCK_INTERVAL: Duration = Duration::from_secs(1);
/// Longer than the route transition, so the leaving page keeps its tiles.
const RELEASE_AFTER_LEAVING: Duration = Duration::from_secs(1);

enum AlbumEvent {
    Scanned(Vec<Capture>),
    Media(MediaResult),
    VideoEnded(u64),
    /// The first frame of a playback (or after a seek) is on screen: the
    /// clock starts from the real position.
    VideoShowing(u64),
    VideoFailed(u64, String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum KindFilter {
    #[default]
    All,
    Photos,
    Videos,
}

impl KindFilter {
    fn next(self) -> Self {
        match self {
            Self::All => Self::Photos,
            Self::Photos => Self::Videos,
            Self::Videos => Self::All,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Photos => "Screenshots",
            Self::Videos => "Videos",
        }
    }

    fn admits(self, kind: MediaKind) -> bool {
        match self {
            Self::All => true,
            Self::Photos => kind == MediaKind::Photo,
            Self::Videos => kind == MediaKind::Video,
        }
    }
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
    kind: KindFilter,
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
            .filter(|(_, capture)| self.kind.admits(capture.kind()))
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

struct ActiveVideo {
    path: PathBuf,
    playback: Box<dyn VideoPlayback>,
    playing: bool,
    ended: bool,
    /// A seek's target, reported until the pipeline's own position catches
    /// up (a position query right after a seek can still be the old one).
    seeked_to: Option<Duration>,
    /// Frames are on screen; before that the clock holds still.
    showing: bool,
}

pub struct AlbumController {
    horizon: Option<HorizonAlbum>,
    sources: Vec<Arc<dyn CaptureSource>>,
    cache: ThumbnailCache,
    video: Arc<dyn VideoBackend>,
    loader: MediaLoader,
    titles: HashMap<SourceGameRef, String>,
    sender: Sender<AlbumEvent>,
    receiver: Receiver<AlbumEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
    scan_running: Cell<bool>,
    state: RefCell<State>,
    tiles: Rc<VecModel<AlbumTileData>>,
    thumbnails: RefCell<ImageMemory<PathBuf>>,
    durations: RefCell<HashMap<PathBuf, Duration>>,
    failed: RefCell<HashSet<PathBuf>>,
    display: RefCell<ImageMemory<(PathBuf, (u32, u32))>>,
    active_video: RefCell<Option<ActiveVideo>>,
    /// Which playback may deliver frames; older ones are ignored.
    video_generation: Arc<AtomicU64>,
    video_clock: Timer,
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
        video: Arc<dyn VideoBackend>,
    ) -> Rc<Self> {
        let (sender, receiver) = mpsc::channel();
        let wake = wake_handle(ui);
        let cache = ThumbnailCache::new(cache_dir);
        let media_sender = sender.clone();
        let media_wake = Arc::clone(&wake);
        let loader = MediaLoader::start(
            MEDIA_WORKERS,
            cache.clone(),
            Arc::clone(&video),
            Arc::new(move |result| {
                let _ = media_sender.send(AlbumEvent::Media(result));
                media_wake();
            }),
        );
        let titles = library
            .iter()
            .flat_map(|game| {
                game.sources()
                    .iter()
                    .map(|source| (source.clone(), game.game().title().as_str().to_owned()))
            })
            .collect();
        let tiles = Rc::new(VecModel::from(Vec::<AlbumTileData>::new()));
        ui.set_album_tiles(ModelRc::from(Rc::clone(&tiles)));
        let controller = Rc::new_cyclic(|this| Self {
            horizon: horizon_dir.map(HorizonAlbum::new),
            sources,
            cache,
            video,
            loader,
            titles,
            sender,
            receiver,
            wake,
            scan_running: Cell::new(false),
            state: RefCell::new(State::default()),
            tiles,
            thumbnails: RefCell::new(ImageMemory::new(THUMBNAIL_MEMORY)),
            durations: RefCell::new(HashMap::new()),
            failed: RefCell::new(HashSet::new()),
            display: RefCell::new(ImageMemory::new(DISPLAY_MEMORY)),
            active_video: RefCell::new(None),
            video_generation: Arc::new(AtomicU64::new(0)),
            video_clock: Timer::default(),
            ui: ui.as_weak(),
            this: this.clone(),
        });
        let weak = Rc::downgrade(&controller);
        ui.on_album_media_ready(move || {
            if let Some(controller) = weak.upgrade() {
                controller.drain();
            }
        });
        controller.publish(ui);
        controller
    }

    /// Every Album visit is fresh: the newest capture, scrolled to the top.
    /// The Game and Type filters are view choices and are kept. The folder
    /// is rescanned so new captures appear.
    pub fn start_new_visit(&self, ui: &AppWindow) {
        self.stop_video(ui);
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
    pub fn on_leave(&self, ui: &AppWindow) {
        self.stop_video(ui);
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
        let mut s = self.state.borrow_mut();
        s.mounted.clear();
        s.mounted_start = 0;
    }

    /// Horizon lost the foreground (a game started): pause, never play on.
    pub fn handle_application_active_changed(&self, ui: &AppWindow, active: bool) {
        if !active {
            self.set_playing(ui, false);
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
        let wake = Arc::clone(&self.wake);
        let spawned = std::thread::Builder::new()
            .name("album-scan".into())
            .spawn(move || {
                let captures = scan_all(horizon.as_ref(), &sources);
                cache.prune(&captures);
                let _ = sender.send(AlbumEvent::Scanned(captures));
                wake();
            });
        if let Err(error) = spawned {
            warn!(%error, "Album scan could not start");
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
                        if let Some(duration) = thumbnail.duration {
                            self.durations.borrow_mut().insert(path.clone(), duration);
                        }
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
                            self.show(&ui, Some(image));
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
                AlbumEvent::VideoEnded(generation) => {
                    if self.video_generation.load(Ordering::SeqCst) == generation
                        && let Some(video) = self.active_video.borrow_mut().as_mut()
                    {
                        video.playing = false;
                        video.ended = true;
                    }
                    self.publish_video(&ui);
                }
                AlbumEvent::VideoShowing(generation) => {
                    if self.video_generation.load(Ordering::SeqCst) == generation
                        && let Some(video) = self.active_video.borrow_mut().as_mut()
                    {
                        video.showing = true;
                    }
                    self.publish_video(&ui);
                }
                AlbumEvent::VideoFailed(generation, error) => {
                    if self.video_generation.load(Ordering::SeqCst) == generation {
                        warn!(%error, "Album video playback failed");
                        self.stop_video(&ui);
                        ui.set_album_viewer_problem("Can't play this video".into());
                    }
                }
            }
        }
        if republish {
            self.publish(&ui);
        }
        if !thumbnails.is_empty() {
            self.refresh_tiles(&thumbnails);
            // The open capture's thumbnail fills an empty viewer; it never
            // replaces a sharper photo or a video frame already on screen.
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
        s.games = game_choices(&captures, &self.titles);
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
            UiAction::RightBumper if !event.repeated && !selecting => self.cycle_kind(ui),
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
        let (title, text) = {
            let s = self.state.borrow();
            let kind = |path: &PathBuf| {
                s.captures
                    .iter()
                    .find(|c| c.path() == path)
                    .map(Capture::kind)
            };
            match targets.as_slice() {
                [one] => (
                    match kind(one) {
                        Some(MediaKind::Video) => "Delete this video?",
                        _ => "Delete this screenshot?",
                    }
                    .to_owned(),
                    "It will be permanently deleted.".to_owned(),
                ),
                many => (
                    format!("Delete {} captures?", many.len()),
                    "They will be permanently deleted.".to_owned(),
                ),
            }
        };
        self.set_playing(ui, false);
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
            self.stop_video(ui);
        }
        let mut deleted = HashSet::new();
        let mut failures = 0;
        for path in targets {
            let deletable = self
                .state
                .borrow()
                .captures
                .iter()
                .any(|c| c.path() == path && c.deletable());
            let result = match (&self.horizon, deletable) {
                (Some(album), true) => album.delete(path),
                _ => Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "not one of Horizon's captures",
                )),
            };
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
            s.games = game_choices(&s.captures, &self.titles);
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
            self.durations.borrow_mut().remove(path);
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

    /// A click selects a tile; clicking the selected tile opens it.
    pub fn choose(&self, ui: &AppWindow, index: i32) -> bool {
        let Ok(index) = usize::try_from(index) else {
            return false;
        };
        let (open, selecting) = {
            let mut s = self.state.borrow_mut();
            if s.viewing || s.dialog.is_some() || index >= s.order.len() {
                return false;
            }
            let open = s.selection == index;
            s.selection = index;
            s.wheel_scrolled = false;
            s.notice = None;
            (open, s.selecting)
        };
        // While selecting, a click ticks the tile instead of opening it.
        if selecting {
            return self.toggle_checked(ui) == AlbumFeedback::Ok;
        }
        if open {
            return self.open_viewer(ui);
        }
        self.publish(ui);
        false
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

    pub fn cycle_game(&self, ui: &AppWindow) {
        {
            let mut s = self.state.borrow_mut();
            if s.viewing || s.captures.is_empty() {
                return;
            }
            s.game_index = (s.game_index + 1) % (s.games.len() + 1);
            s.selection = 0;
            s.scroll_top = 0;
            s.wheel_scrolled = false;
            s.rebuild_order();
        }
        self.publish(ui);
    }

    pub fn cycle_kind(&self, ui: &AppWindow) {
        {
            let mut s = self.state.borrow_mut();
            if s.viewing || s.captures.is_empty() {
                return;
            }
            s.kind = s.kind.next();
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
            UiAction::Accept if !event.repeated => self.toggle_play(ui),
            UiAction::LeftBumper if !event.repeated => self.skip(ui, false),
            UiAction::RightBumper if !event.repeated => self.skip(ui, true),
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
        self.stop_video(ui);
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
        // What is on screen slides out: for a video, its current frame.
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
    /// ask for better; start its video.
    fn present_current(&self, ui: &AppWindow) {
        self.stop_video(ui);
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
            ui.set_album_viewer_problem(
                match capture.kind() {
                    MediaKind::Photo => "Can't open this image",
                    MediaKind::Video => "Can't play this video",
                }
                .into(),
            );
            return;
        }
        if capture.kind() == MediaKind::Video {
            self.start_video(ui, &capture);
        }
    }

    fn show(&self, ui: &AppWindow, image: Option<Image>) {
        ui.set_album_viewer_has_image(image.is_some());
        if let Some(image) = &image {
            ui.set_album_viewer_image(image.clone());
        }
    }

    fn start_video(&self, ui: &AppWindow, capture: &Capture) {
        let generation = self.video_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let sink = Arc::new(UiVideoSink {
            generation,
            current: Arc::clone(&self.video_generation),
            pending: Arc::new(AtomicBool::new(false)),
            showing: AtomicBool::new(false),
            ui: self.ui.clone(),
            sender: self.sender.clone(),
            wake: Arc::clone(&self.wake),
        });
        match self.video.play(capture.path(), display_size(ui), sink) {
            Ok(playback) => {
                playback.set_playing(true);
                *self.active_video.borrow_mut() = Some(ActiveVideo {
                    path: capture.path().to_owned(),
                    playback,
                    playing: true,
                    ended: false,
                    seeked_to: None,
                    showing: false,
                });
                let this = self.this.clone();
                // Only the clock text needs polling; frames push themselves.
                self.video_clock.start(
                    slint::TimerMode::Repeated,
                    VIDEO_CLOCK_INTERVAL,
                    move || {
                        if let Some(controller) = this.upgrade()
                            && let Some(ui) = controller.ui.upgrade()
                        {
                            controller.publish_video(&ui);
                        }
                    },
                );
            }
            Err(error) => {
                warn!(%error, "Album video could not start");
                ui.set_album_viewer_problem("Can't play this video".into());
            }
        }
        self.publish_video(ui);
    }

    fn stop_video(&self, ui: &AppWindow) {
        self.video_generation.fetch_add(1, Ordering::SeqCst);
        self.video_clock.stop();
        // Dropping the playback stops it and frees the decoder.
        *self.active_video.borrow_mut() = None;
        self.publish_video(ui);
    }

    fn set_playing(&self, ui: &AppWindow, playing: bool) {
        if let Some(video) = self.active_video.borrow_mut().as_mut()
            && video.playing != playing
        {
            if playing && video.ended {
                video.playback.seek(Duration::ZERO);
                video.ended = false;
                video.seeked_to = Some(Duration::ZERO);
            }
            video.playback.set_playing(playing);
            video.playing = playing;
        }
        self.publish_video(ui);
    }

    pub fn toggle_play(&self, ui: &AppWindow) {
        let playing = self
            .active_video
            .borrow()
            .as_ref()
            .map(|video| video.playing);
        if let Some(playing) = playing {
            self.set_playing(ui, !playing);
        }
    }

    fn skip(&self, ui: &AppWindow, forward: bool) {
        if let Some(video) = self.active_video.borrow_mut().as_mut() {
            let position = video.playback.position().unwrap_or_default();
            let target = if forward {
                position + SKIP
            } else {
                position.saturating_sub(SKIP)
            };
            let target = video
                .playback
                .duration()
                .map_or(target, |duration| target.min(duration));
            video.playback.seek(target);
            video.ended = false;
            video.seeked_to = Some(target);
        }
        self.publish_video(ui);
    }

    /// Pointer seek on the timeline, 0..1.
    pub fn seek_fraction(&self, ui: &AppWindow, fraction: f32) {
        if let Some(video) = self.active_video.borrow_mut().as_mut()
            && let Some(duration) = video.playback.duration()
        {
            let target = duration.mul_f32(fraction.clamp(0.0, 1.0));
            video.playback.seek(target);
            video.ended = false;
            video.seeked_to = Some(target);
        }
        self.publish_video(ui);
    }

    /// Anchor the video clock to the real playback position. Slint runs it
    /// on from here each frame while playing (`video-anchor`).
    fn publish_video(&self, ui: &AppWindow) {
        let mut video = self.active_video.borrow_mut();
        let Some(video) = video.as_mut() else {
            ui.set_album_viewer_playing(false);
            ui.set_album_video_duration_ms(0);
            ui.set_album_video_position_ms(0);
            bump_anchor(ui);
            return;
        };
        let duration = video
            .playback
            .duration()
            .or_else(|| self.durations.borrow().get(&video.path).copied());
        let reported = video.playback.position();
        let position = if video.ended {
            duration
        } else if let Some(target) = video.seeked_to {
            // Trust the pipeline again once it reports the new place.
            if reported.is_some_and(|p| p.abs_diff(target) < Duration::from_millis(500)) {
                video.seeked_to = None;
                reported
            } else {
                Some(target)
            }
        } else {
            reported
        };
        // The clock runs only while frames are actually flowing.
        ui.set_album_viewer_playing(video.playing);
        ui.set_album_video_clock_running(video.playing && video.showing && !video.ended);
        ui.set_album_video_duration_ms(millis(duration.unwrap_or_default()));
        ui.set_album_video_position_ms(millis(position.unwrap_or_default()));
        bump_anchor(ui);
    }

    // ----- publishing -------------------------------------------------

    fn publish(&self, ui: &AppWindow) {
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
            let tiles: Vec<AlbumTileData> = s.order[start..end]
                .iter()
                .enumerate()
                .map(|(offset, index)| {
                    let capture = &s.captures[*index];
                    self.tile(capture, start + offset, s.checked.contains(capture.path()))
                })
                .collect();
            self.tiles.set_vec(tiles);
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
        ui.set_album_selected_count(s.checked.len() as i32);
        ui.set_album_has_deletable(s.order.iter().any(|i| s.captures[*i].deletable()));
        ui.set_album_can_delete(s.viewing && s.current().is_some_and(Capture::deletable));
        let game_text = s
            .game_index
            .checked_sub(1)
            .and_then(|index| s.games.get(index))
            .map_or_else(|| "All games".to_owned(), |choice| choice.title.clone());
        ui.set_album_total_count(count as i32);
        ui.set_album_collection_count(s.captures.len() as i32);
        ui.set_album_status(
            if s.scanned {
                "No screenshots or videos yet. Steam screenshots appear here, as will captures \
                 Horizon takes."
            } else {
                ""
            }
            .into(),
        );
        ui.set_album_game_text(game_text.into());
        ui.set_album_kind_text(s.kind.label().into());
        ui.set_album_scroll_row(s.scroll_top as i32);
        ui.set_album_selected_index(s.selection as i32);
        ui.set_album_selected_text(selected_text.into());
        ui.set_album_visit_revision(s.visit_revision);
        if let Some(capture) = s.current().filter(|_| s.viewing) {
            ui.set_album_viewer_text(when_text(capture.captured_at()).into());
            ui.set_album_viewer_is_video(capture.kind() == MediaKind::Video);
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
            is_video: capture.kind() == MediaKind::Video,
            deletable: capture.deletable(),
            checked,
            duration: self
                .durations
                .borrow()
                .get(&path)
                .map(|duration| clock_text(*duration))
                .unwrap_or_default()
                .into(),
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
                self.tiles.set_row_data(
                    row,
                    self.tile(
                        capture,
                        s.mounted_start + row,
                        s.checked.contains(capture.path()),
                    ),
                );
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
                if capture.kind() == MediaKind::Photo
                    && !self
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

fn millis(duration: Duration) -> i32 {
    i32::try_from(duration.as_millis()).unwrap_or(i32::MAX)
}

fn bump_anchor(ui: &AppWindow) {
    ui.set_album_video_anchor(ui.get_album_video_anchor().wrapping_add(1));
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

fn wake_handle(ui: &AppWindow) -> Arc<dyn Fn() + Send + Sync> {
    let weak = ui.as_weak();
    // Coalesce: many results finishing together cause one drain.
    let scheduled = Arc::new(AtomicBool::new(false));
    Arc::new(move || {
        if scheduled.swap(true, Ordering::SeqCst) {
            return;
        }
        let scheduled = Arc::clone(&scheduled);
        let _ = weak.upgrade_in_event_loop(move |ui| {
            scheduled.store(false, Ordering::SeqCst);
            ui.invoke_album_media_ready();
        });
    })
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

fn clock_text(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

/// Hands decoded frames to the UI thread, at most one in flight: while a
/// frame waits to be shown, newer ones are dropped before being copied.
struct UiVideoSink {
    generation: u64,
    current: Arc<AtomicU64>,
    pending: Arc<AtomicBool>,
    /// The first frame has been handed over.
    showing: AtomicBool,
    ui: slint::Weak<AppWindow>,
    sender: Sender<AlbumEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl UiVideoSink {
    fn live(&self) -> bool {
        self.current.load(Ordering::SeqCst) == self.generation
    }
}

impl VideoSink for UiVideoSink {
    fn wants_frame(&self) -> bool {
        self.live() && !self.pending.load(Ordering::SeqCst)
    }

    fn frame(&self, frame: VideoFrame<'_>) {
        let row = frame.width as usize * 4;
        if frame.stride < row {
            return;
        }
        let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(frame.width, frame.height);
        let target = buffer.make_mut_bytes();
        for (out, line) in target
            .chunks_exact_mut(row)
            .zip(frame.data.chunks(frame.stride))
        {
            let Some(line) = line.get(..row) else {
                return;
            };
            out.copy_from_slice(line);
        }
        self.pending.store(true, Ordering::SeqCst);
        let pending = Arc::clone(&self.pending);
        let current = Arc::clone(&self.current);
        let generation = self.generation;
        let _ = self.ui.upgrade_in_event_loop(move |ui| {
            pending.store(false, Ordering::SeqCst);
            if current.load(Ordering::SeqCst) == generation {
                ui.set_album_viewer_image(Image::from_rgba8(buffer));
                ui.set_album_viewer_has_image(true);
            }
        });
        if !self.showing.swap(true, Ordering::SeqCst) {
            let _ = self.sender.send(AlbumEvent::VideoShowing(self.generation));
            (self.wake)();
        }
    }

    fn ended(&self) {
        let _ = self.sender.send(AlbumEvent::VideoEnded(self.generation));
        (self.wake)();
    }

    fn failed(&self, message: String) {
        let _ = self
            .sender
            .send(AlbumEvent::VideoFailed(self.generation, message));
        (self.wake)();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ExternalGameId, SourceId, album::CaptureOrigin};

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
            capture("c", MediaKind::Video, None, 3),
            capture("d", MediaKind::Photo, Some("20"), 2),
        ];
        let choices = game_choices(&captures, &titles);
        let names: Vec<_> = choices.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(names, ["Steam app 20", "Portal", "Horizon"]);
    }

    #[test]
    fn filters_combine_and_keep_newest_first_order() {
        let mut state = State {
            captures: vec![
                capture("a", MediaKind::Photo, Some("20"), 5),
                capture("b", MediaKind::Video, Some("20"), 4),
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
        state.kind = KindFilter::Videos;
        state.rebuild_order();
        assert_eq!(state.order, [1]);
        state.game_index = 2; // Steam app 10 has no videos
        state.rebuild_order();
        assert!(state.order.is_empty());
        assert_eq!(state.selection, 0);
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

    #[test]
    fn clock_text_reads_like_a_player() {
        assert_eq!(clock_text(Duration::from_secs(42)), "0:42");
        assert_eq!(clock_text(Duration::from_secs(605)), "10:05");
        assert_eq!(clock_text(Duration::from_secs(3725)), "1:02:05");
    }
}
