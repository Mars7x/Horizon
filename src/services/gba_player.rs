//! mGBA sessions (GB, GBC and GBA): validated ROM snapshots, core lifecycle
//! and battery saves.
use crate::domain::core_settings::PlaybackSpeed;
use crate::{
    domain::{GameId, SessionTrackingMethod, SourceId},
    persistence::SqliteLibraryRepository,
    platform::{
        gba_core::{AudioBuffer, Core, DisplaySize, LatestFrame},
        gba_present::FrameStage,
        pipewire::{AudioStream, Role},
    },
    services::{activity::ActivityRepository, rom_import::RomSystem, worker::Notify},
};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU16, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
static CORE_RUNNING: AtomicBool = AtomicBool::new(false);
pub struct RomLocation {
    pub game_id: GameId,
    pub title: String,
    pub path: PathBuf,
}
#[derive(Clone)]
pub struct PlayerPaths {
    pub database: PathBuf,
    pub core: PathBuf,
    pub ds_core: PathBuf,
    pub saves: PathBuf,
    pub settings: crate::platform::data_paths::CoreSettingsPaths,
    pub album: PathBuf,
}
pub enum Command {
    Pause(bool),
    Stop,
    Shutdown,
    Speed(PlaybackSpeed),
    Screenshot,
    Restart,
    ListStates,
    SaveState(u8),
    /// Load a slot; `quick` reports trouble as a notice over the game.
    LoadState {
        slot: u8,
        quick: bool,
    },
    /// Save the quick slot without pausing (its writing doesn't stall play).
    QuickSave,
    /// Pause and project volatile history without restoring any core state.
    OpenRewind,
    /// Commit a selected point from the paused history.
    RewindTo(usize),
    /// Refresh the automatic resume point now (Horizon lost focus).
    Autosave,
    /// Close (true) or open the DS lid.
    Lid(bool),
    /// Remember how this game is shown (DS layout and swap) for next time.
    RememberView(crate::services::game_storage::GameView),
}

/// Quick save and quick load use the first manual slot.
pub const QUICK_SLOT: u8 = 1;

/// One background thread for everything the player writes while a game may
/// be running: state bundles (the automatic resume point, quick saves) and
/// screenshots. The game thread only takes the snapshot (a few milliseconds);
/// encoding and synced writes happen here, in order. The game thread waits
/// for it to be idle before it touches states itself.
struct Writer {
    jobs: Option<Sender<WriteJob>>,
    thread: Option<JoinHandle<()>>,
    /// Jobs queued or being written.
    pending: Arc<(std::sync::Mutex<usize>, std::sync::Condvar)>,
}
enum WriteJob {
    State {
        store: crate::services::save_states::Store,
        slot: u8,
        bytes: Vec<u8>,
        frame: crate::platform::gba_core::Frame,
        /// A quick save: the player is told how it went.
        quick: bool,
    },
    Screenshot {
        target: ScreenshotFile,
        frame: crate::platform::gba_core::Frame,
    },
}
impl Writer {
    fn new(events: Sender<Event>) -> Self {
        let (jobs, queue) = mpsc::channel::<WriteJob>();
        let pending = Arc::new((std::sync::Mutex::new(0usize), std::sync::Condvar::new()));
        let done = Arc::clone(&pending);
        let thread = thread::Builder::new()
            .name("horizon-writer".into())
            .spawn(move || {
                for job in queue {
                    let started = Instant::now();
                    match job {
                        WriteJob::State {
                            store,
                            slot,
                            bytes,
                            frame,
                            quick,
                        } => {
                            let result = store.save(slot, &bytes, &frame);
                            if quick {
                                let _ = events.send(Event::QuickSaved(
                                    result.map_err(|_| "Couldn't save the state".to_owned()),
                                ));
                            } else {
                                match result {
                                    Ok(()) => tracing::debug!(
                                        write_ms = started.elapsed().as_millis(),
                                        "automatic resume point refreshed"
                                    ),
                                    Err(error) => tracing::warn!(
                                        %error,
                                        "automatic resume point not refreshed"
                                    ),
                                }
                            }
                        }
                        WriteJob::Screenshot { target, frame } => {
                            // Success was confirmed when it was taken.
                            if let Err(error) = finish_screenshot(target, &frame) {
                                let _ = events.send(Event::ScreenshotFailed(error));
                            }
                        }
                    }
                    let (count, idle) = &*done;
                    let mut count = count
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    *count = count.saturating_sub(1);
                    idle.notify_all();
                }
            })
            .map_err(|error| tracing::warn!(%error, "the background writer could not start"))
            .ok();
        Self {
            jobs: thread.is_some().then_some(jobs),
            thread,
            pending,
        }
    }

    fn busy(&self) -> bool {
        *self
            .pending
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            > 0
    }

    /// Queue a job; false if the writer isn't running (nothing was queued).
    fn send(&self, job: WriteJob) -> bool {
        let Some(jobs) = &self.jobs else {
            return false;
        };
        let (count, _) = &*self.pending;
        *count
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
        if jobs.send(job).is_err() {
            *count
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) -= 1;
            return false;
        }
        true
    }

    /// Wait until everything queued is written.
    fn wait(&self) {
        let (count, idle) = &*self.pending;
        let _idle = idle
            .wait_while(
                count
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                |count| *count > 0,
            )
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        self.jobs.take();
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::warn!("the background writer stopped unexpectedly");
        }
    }
}

/// Refreshes the automatic resume point while a game is played, so a crash
/// or power cut loses at most about a minute (through the `Writer`).
struct Autosave {
    folder: crate::services::game_storage::GameFolder,
    identity: String,
    /// Unpaused play since the last refresh.
    played: Duration,
}
impl Autosave {
    const EVERY: Duration = Duration::from_secs(60);
    /// Leaving Horizon refreshes only after this much play, so switching
    /// windows back and forth doesn't keep writing.
    const AT_LEAST: Duration = Duration::from_secs(5);

    fn due(&self) -> bool {
        // Measurements can refresh more often (HORIZON_AUTOSAVE_SECONDS).
        #[cfg(test)]
        if let Some(seconds) = std::env::var("HORIZON_AUTOSAVE_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
        {
            return self.played >= Duration::from_secs(seconds);
        }
        self.played >= Self::EVERY
    }

    fn store(&self) -> crate::services::save_states::Store {
        crate::services::save_states::Store::new(self.folder.clone(), self.identity.clone())
    }

    /// Snapshot `core` into `slot` through `writer`. An automatic refresh
    /// is skipped while the writer is still busy; a quick save queues.
    fn start(&mut self, core: &Core, writer: &Writer, slot: u8, quick: bool) {
        if !quick && writer.busy() {
            return;
        }
        let taken = Instant::now();
        let snapshot = core
            .serialize_state()
            .and_then(|bytes| core.capture_frame().map(|frame| (bytes, frame)));
        let (bytes, frame) = match snapshot {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(%error, slot, "state not saved");
                return;
            }
        };
        tracing::debug!(
            snapshot_ms = taken.elapsed().as_secs_f64() * 1000.0,
            slot,
            "state snapshot"
        );
        if slot == 0 {
            self.played = Duration::ZERO;
        }
        writer.send(WriteJob::State {
            store: self.store(),
            slot,
            bytes,
            frame,
            quick,
        });
    }
}
pub enum Event {
    RewindOpened(Vec<RewindPoint>),
    Rewound {
        lid_closed: bool,
    },
    Ready {
        title: String,
        freeze_when_unfocused: bool,
        /// A DS game's screen layout, and whether its screens are swapped;
        /// None for other systems.
        ds_layout: Option<crate::domain::core_settings::DsLayout>,
        ds_swapped: bool,
    },
    /// Saving on exit has begun: the last frame, blurred, for the player to
    /// show if saving takes a moment.
    Exiting(crate::platform::gba_core::Frame),
    /// While the game loads: the newest state's picture (if resuming) and,
    /// for DS, the layout to show it in.
    Loading {
        preview: Option<crate::platform::gba_core::Frame>,
        ds_layout: Option<crate::domain::core_settings::DsLayout>,
        ds_swapped: bool,
    },
    Started(String),
    Stopped,
    Failed(String),
    SaveFailed(String),
    SpeedChanged(PlaybackSpeed),
    ScreenshotSaved(crate::platform::gba_core::Frame),
    Restarted,
    States(Vec<crate::services::save_states::Entry>),
    StateSaved,
    StateLoaded,
    /// A quick save finished.
    QuickSaved(Result<(), String>),
    /// A short message to show over the game.
    Notice(String),
    /// A screenshot already confirmed could not be written after all.
    ScreenshotFailed(String),
    ActionFailed(String),
}

pub struct RewindPoint {
    pub behind: Duration,
    pub preview: crate::platform::gba_core::Frame,
}

struct RewindCapture {
    frame: crate::platform::gba_core::Frame,
    lid_closed: bool,
}

fn record_rewind(
    core: &Core,
    history: &mut crate::domain::rewind::History<RewindCapture>,
    at: Duration,
    lid_closed: bool,
) -> Result<(), String> {
    let state = core.serialize_state()?;
    let raw = core.capture_frame()?;
    let preview = crate::services::save_states::preview_size(&raw).unwrap_or(raw);
    let cost = preview.rgba.len();
    // A useful history must fit a prior point and the exact paused moment.
    if state.len().saturating_add(cost) > crate::domain::rewind::MEMORY_LIMIT / 2 {
        return Err("This game's rewind states are too large for the memory limit".into());
    }
    if history.push(
        at,
        state,
        RewindCapture {
            frame: preview,
            lid_closed,
        },
        cost,
    ) {
        Ok(())
    } else {
        Err("This game's rewind state exceeds the memory limit".into())
    }
}

/// Compose on the game worker, in the current DS layout, at bounded preview size.
fn rewind_picture(
    frame: &crate::platform::gba_core::Frame,
    view: Option<(crate::domain::core_settings::DsLayout, bool)>,
) -> crate::platform::gba_core::Frame {
    let frame = match view {
        Some((layout, swapped)) => {
            crate::platform::gba_video::ds_screenshot(frame, layout, swapped)
        }
        None => frame.clone(),
    };
    if frame.width <= 512 && frame.height <= 384 {
        return frame;
    }
    let size = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone());
    let Some(size) = size else {
        return frame;
    };
    let scale = (512.0 / f64::from(frame.width)).min(384.0 / f64::from(frame.height));
    let resized = image::imageops::resize(
        &size,
        (f64::from(frame.width) * scale).round() as u32,
        (f64::from(frame.height) * scale).round() as u32,
        image::imageops::FilterType::Triangle,
    );
    crate::platform::gba_core::Frame {
        width: resized.width(),
        height: resized.height(),
        rgba: resized.into_raw(),
        smooth: frame.smooth,
    }
}
pub struct RunningGame {
    pub commands: Sender<Command>,
    pub events: Receiver<Event>,
    pub frame: LatestFrame,
    pub buttons: Arc<AtomicU16>,
    pub ds_input: Option<Arc<crate::platform::gba_core::DsInput>>,
    /// The player's size in physical pixels, for xBRZ's screen-size output.
    pub display: DisplaySize,
    /// Tell it when the screen has drawn a frame, so the next is filtered.
    pub stage: Arc<FrameStage>,
    worker: Option<JoinHandle<Result<(), String>>>,
}
impl RunningGame {
    /// `frame_ready` is called on the worker each time a new frame is ready
    /// in `frame`, so the UI shows it as soon as it exists.
    pub fn start(
        paths: PlayerPaths,
        system: RomSystem,
        sha256: String,
        frame_ready: Notify,
    ) -> Result<Self, String> {
        if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid ROM identity".into());
        }
        if CORE_RUNNING.swap(true, Ordering::AcqRel) {
            return Err("A game is already running".into());
        }
        let (commands, receiver) = mpsc::channel();
        let (sender, events) = mpsc::channel();
        let frame = Arc::new(Mutex::new(None));
        let buttons = Arc::new(AtomicU16::new(0));
        let ds_input = (system == RomSystem::NintendoDs)
            .then(|| Arc::new(crate::platform::gba_core::DsInput::default()));
        let worker_ds_input = ds_input.clone();
        let display = DisplaySize::default();
        let stage = Arc::new(FrameStage::new());
        let (worker_frame, worker_buttons) = (Arc::clone(&frame), Arc::clone(&buttons));
        let (worker_display, worker_stage) = (Arc::clone(&display), Arc::clone(&stage));
        let worker = thread::Builder::new()
            .name("horizon-emulator".into())
            .spawn(move || {
                struct CoreLease;
                impl Drop for CoreLease {
                    fn drop(&mut self) {
                        CORE_RUNNING.store(false, Ordering::Release);
                    }
                }
                let _lease = CoreLease;
                let result = play(
                    paths,
                    (system, &sha256),
                    receiver,
                    &sender,
                    (worker_frame, worker_display, worker_stage),
                    (worker_buttons, worker_ds_input),
                    frame_ready,
                );
                if let Err(error) = &result {
                    let _ = sender.send(Event::Failed(error.clone()));
                }
                result
            });
        match worker {
            Ok(worker) => Ok(Self {
                commands,
                events,
                frame,
                buttons,
                ds_input,
                display,
                stage,
                worker: Some(worker),
            }),
            Err(_) => {
                CORE_RUNNING.store(false, Ordering::Release);
                Err("Could not start the game worker".into())
            }
        }
    }
    pub fn shutdown(&mut self) -> Result<(), String> {
        let _ = self.commands.send(Command::Shutdown);
        match self.worker.take() {
            Some(worker) => worker
                .join()
                .map_err(|_| "The game worker stopped unexpectedly".to_owned())?,
            None => Ok(()),
        }
    }
    pub fn finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }
}
impl Drop for RunningGame {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}

pub fn read_rom_snapshot(path: &Path, sha256: &str) -> Result<Vec<u8>, String> {
    let bytes = read_rom(path)?;
    if !rom_matches(&bytes, sha256) {
        return Err(CHANGED_ROM.into());
    }
    Ok(bytes)
}

const CHANGED_ROM: &str = "The game file has changed. Add its folder again before playing.";

/// The imported file's bytes, checked for size and (DS) header, but not yet
/// against its imported identity: see `rom_matches`.
fn read_rom(path: &Path) -> Result<Vec<u8>, String> {
    let system = RomSystem::from_extension(path).unwrap_or(RomSystem::GameBoyAdvance);
    let maximum = system.max_rom_size();
    let mut file = File::open(path)
        .map_err(|_| "Game folder access is unavailable. Choose its folder again.".to_owned())?;
    let metadata = file
        .metadata()
        .map_err(|_| "Could not read the game file".to_owned())?;
    if !metadata.is_file() || !(192..=maximum).contains(&metadata.len()) {
        return Err("This game file is not a supported cartridge file".into());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    (&mut file)
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read the game file".to_owned())?;
    if system == RomSystem::NintendoDs {
        crate::sources::gba::recognition::validate_ds_header(&bytes, bytes.len() as u64)
            .map_err(|_| "This is not a supported Nintendo DS cartridge".to_owned())?;
    }
    if bytes.len() as u64 != metadata.len() {
        return Err(CHANGED_ROM.into());
    }
    Ok(bytes)
}

/// The bytes are the game that was imported.
fn rom_matches(bytes: &[u8], sha256: &str) -> bool {
    let actual = ring::digest::digest(&ring::digest::SHA256, bytes);
    actual
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        == sha256
}

fn play(
    paths: PlayerPaths,
    (system, sha256): (RomSystem, &str),
    commands: Receiver<Command>,
    events: &Sender<Event>,
    (frame, display, stage): (LatestFrame, DisplaySize, Arc<FrameStage>),
    (buttons, ds_input): (
        Arc<AtomicU16>,
        Option<Arc<crate::platform::gba_core::DsInput>>,
    ),
    frame_ready: Notify,
) -> Result<(), String> {
    let launch = Instant::now();
    fs::create_dir_all(&paths.saves)
        .map_err(|_| "Could not prepare the game save directory".to_owned())?;
    let mut repository =
        SqliteLibraryRepository::open(&paths.database).map_err(|e| e.to_string())?;
    let locations = repository
        .rom_locations(system, sha256)
        .map_err(|e| e.to_string())?;
    let mut failure =
        "This game has no available imported ROM. Choose its folder again.".to_owned();
    let ds = system == RomSystem::NintendoDs;
    // DS state serialization and GPU readback can take 30–50 ms at 8×.
    // Its 100 ms queue starts around 62 ms; mGBA retains 31 ms headroom.
    let audio: AudioBuffer = Arc::new(crate::platform::gba_audio::GameAudio::with_buffer_ms(
        if ds { 100 } else { 50 },
    ));
    let preferences = crate::persistence::core_settings::CoreSettingsStore::new(paths.settings)
        .load()
        .map_err(|_| "Could not read playback or core settings".to_owned())?;
    let mut mgba = if ds {
        // DS takes only its filter; its layout is the player's.
        crate::domain::core_settings::MgbaPreferences {
            video_filter: preferences.ds.video_filter,
            ..Default::default()
        }
    } else {
        preferences.mgba
    };
    // The correction models the GBA and GBC screens (one shared profile);
    // original Game Boy games keep their chosen palette.
    mgba.color_correction &= system != RomSystem::GameBoy;
    let source = SourceId::new(system.source_id()).map_err(|e| e.to_string())?;
    let game = crate::domain::SourceGameRef::new(
        source.clone(),
        crate::domain::ExternalGameId::new(format!("sha256:{sha256}"))
            .map_err(|e| e.to_string())?,
    );
    let early_folder = locations.first().and_then(|location| {
        crate::services::game_storage::GameFolder::prepare(&paths.saves, &game, &location.title)
            .ok()
    });
    // A DS game keeps the layout and swap it was last shown with; Settings'
    // layout is for games never changed.
    let view = early_folder
        .as_ref()
        .map(crate::services::game_storage::GameFolder::view)
        .unwrap_or_default();
    let ds_layout = ds.then(|| view.ds_layout.unwrap_or(preferences.ds.layout));
    // Something to look at while a large game loads: the state it resumes.
    let preview = early_folder
        .filter(|_| preferences.resume_last_state)
        .and_then(crate::services::save_states::Store::latest_preview)
        .map(|frame| crate::platform::gba_video::loading_picture(&frame, ds));
    let _ = events.send(Event::Loading {
        preview,
        ds_layout,
        ds_swapped: view.ds_swapped,
    });
    let mut read_at = Duration::ZERO;
    let mut selected = None;
    for location in locations {
        let rom = match read_rom(&location.path) {
            Ok(bytes) => bytes,
            Err(error) => {
                failure = error;
                continue;
            }
        };
        read_at = launch.elapsed();
        let game_folder = crate::services::game_storage::GameFolder::prepare(
            &paths.saves,
            &game,
            &location.title,
        )
        .map_err(|_| {
            "Could not prepare the game's save folder. Existing files have been preserved."
                .to_owned()
        })?;
        let ds_directory =
            crate::platform::data_paths::ds_support_directory(&game_folder.directory);
        if ds {
            crate::services::game_storage::ensure_directory(&game_folder.directory, &ds_directory)
                .map_err(|_| "Could not prepare DS core data")?;
        }
        // The identity check (a large DS cartridge takes a few hundred
        // milliseconds to hash) runs while the core loads. Nothing runs, and
        // no save is touched, until both are done and the file matched.
        let (matched, core) = thread::scope(|scope| {
            let check = scope.spawn(|| rom_matches(&rom, sha256));
            let core = Core::load_system(
                if ds { &paths.ds_core } else { &paths.core },
                &rom,
                mgba,
                Arc::clone(&audio),
                Arc::clone(&buttons),
                ds_input.clone().map(|input| {
                    (
                        input,
                        ds_directory.as_path(),
                        preferences.ds.resolution.scale(),
                    )
                }),
            );
            (check.join().unwrap_or(false), core)
        });
        if !matched {
            drop(core);
            failure = CHANGED_ROM.into();
            continue;
        }
        selected = Some((location, game_folder, core?));
        break;
    }
    let Some((location, game_folder, mut core)) = selected else {
        return Err(failure);
    };
    let core_at = launch.elapsed();
    // Filtering runs beside the core, so it never slows the game down.
    struct Filter {
        stage: Arc<FrameStage>,
        thread: Option<JoinHandle<()>>,
    }
    impl Drop for Filter {
        fn drop(&mut self) {
            self.stage.stop();
            if let Some(thread) = self.thread.take()
                && thread.join().is_err()
            {
                tracing::warn!("the game's video filter stopped unexpectedly");
            }
        }
    }
    let _filter = Filter {
        stage: Arc::clone(&stage),
        thread: Some(
            thread::Builder::new()
                .name("horizon-frames".into())
                .spawn({
                    let stage = Arc::clone(&stage);
                    let ds_input = ds_input.clone();
                    move || {
                        let view = ds_input.as_ref().map(|input| &input.view);
                        stage.run(mgba, view, &display, &frame, &*frame_ready)
                    }
                })
                .map_err(|_| "Could not start the game's video".to_owned())?,
        ),
    };
    let save_path = game_folder.save_path();
    // A game new to Horizon may have a save from another emulator beside it.
    if !save_path.exists()
        && let Some((found, bytes)) = save_beside(&location.path, ds, core.max_save_size())
    {
        match write_save_bounded(&save_path, &bytes, core.max_save_size()) {
            Ok(()) => {
                tracing::info!(file = %found.display(), "imported the save found beside the game")
            }
            Err(error) => tracing::warn!(%error, "could not import the save beside the game"),
        }
    }
    let mut last_save = read_save_bounded(&save_path, core.max_save_size())?.unwrap_or_default();
    if !last_save.is_empty() {
        core.restore_save(&last_save)?;
    }
    // melonDS installs the frontend SRAM buffer on its first run. Complete
    // that initialization before restoring a state, or it can overwrite the
    // state's cartridge memory on the next frame.
    if ds {
        core.run_frame();
        audio.clear();
    }
    let save_at = launch.elapsed();
    let states =
        crate::services::save_states::Store::new(game_folder.clone(), core.state_identity.clone());
    if preferences.resume_last_state && core.supports_states()
        && let Some(snapshot) = states.latest().map_err(|_| "Saved states are unavailable or incompatible. Disable Resume last state in Settings to start normally. Existing states have been preserved.".to_owned())? {
            core.restore_state(&snapshot.bytes)?;
    }
    let state_at = launch.elapsed();
    let mut autosave = Autosave {
        folder: game_folder.clone(),
        identity: core.state_identity.clone(),
        played: Duration::ZERO,
    };
    let writer = Writer::new(events.clone());
    let mut rewind = crate::domain::rewind::History::default();
    let mut rewind_clock = Duration::ZERO;
    let mut rewind_since = Duration::ZERO;
    let mut rewind_enabled = core.supports_states();
    let mut last_tick = Instant::now();
    let mut lid_closed = false;
    let mut view_writer: Option<JoinHandle<()>> = None;
    let mut audio_reader = crate::platform::gba_audio::AudioReader::new(
        Arc::clone(&audio),
        core.av.sample_rate.round() as u32,
    );
    let output_started = Arc::new(AtomicBool::new(false));
    let output_ready = Arc::clone(&output_started);
    // Keep the PipeWire graph connected until this session ends. Pauses gate
    // the reader instead of paying device/graph activation latency on resume.
    let _stream = AudioStream::open(
        "Horizon game",
        Role::Game,
        core.av.sample_rate.round() as u32,
        2,
        256,
        Box::new(move |output, _| {
            audio_reader.render(output);
            output_ready.store(true, Ordering::Release);
        }),
    )
    .map_err(|_| "Could not open game audio. Check the audio output and try again.".to_owned())?;
    let audio_opened = Instant::now();
    let (fps, rate) = (core.av.fps, core.av.sample_rate.round() as u32);
    let ms = |at: Duration| at.as_millis();
    tracing::info!(
        read_and_verify_ms = ms(read_at),
        core_ms = ms(core_at - read_at),
        save_ms = ms(save_at - core_at),
        state_ms = ms(state_at - save_at),
        audio_ms = ms(launch.elapsed() - state_at),
        "game ready in {} ms",
        ms(launch.elapsed())
    );
    let mut reported = (0u64, 0u64);
    // Frames wait as long as the audio they produced (see FramePacer).
    let mut pacer = crate::platform::gba_audio::FramePacer::new(rate, fps, 1);
    let mut deadline = Instant::now();
    let mut checkpoint = Instant::now();
    let mut checkpoint_frames = 0u64;
    let mut paused = false;
    let mut speed = PlaybackSpeed::Normal;
    let mut pending = None;
    let mut session = None;
    let mut frame_attempts = 0u32;
    // Hand each new frame to the filter thread, which shows the newest one
    // whenever the screen has drawn the last.
    let present = |core: &Core| core.hand_off(&stage);
    let _ = events.send(Event::Ready {
        title: location.title.clone(),
        freeze_when_unfocused: preferences.freeze_when_unfocused,
        ds_layout,
        ds_swapped: view.ds_swapped,
    });
    let result = (|| -> Result<(), String> {
        loop {
            while let Some(command) = pending.take().or_else(|| commands.try_recv().ok()) {
                // No more frames will be produced while waiting for exit saves.
                // Gate output before that wait, capture and serialization.
                if matches!(command, Command::Stop | Command::Shutdown) {
                    audio.set_paused(true);
                }
                // States are touched here only after an autosave finished.
                if matches!(
                    command,
                    Command::ListStates
                        | Command::SaveState(_)
                        | Command::LoadState { .. }
                        | Command::Stop
                        | Command::Shutdown
                ) {
                    writer.wait();
                }
                match command {
                    Command::OpenRewind => {
                        let result = if rewind_enabled {
                            record_rewind(&core, &mut rewind, rewind_clock, lid_closed)
                        } else {
                            Err("Rewind is unavailable for this game".into())
                        };
                        if let Err(error) = result {
                            let _ = events.send(Event::ActionFailed(error));
                            continue;
                        }
                        if rewind.len() < 2 {
                            let _ = events.send(Event::ActionFailed(
                                "Play a little longer to build rewind history".into(),
                            ));
                            continue;
                        }
                        paused = true;
                        audio.set_paused(true);
                        buttons.store(0, Ordering::Release);
                        let view = ds_input.as_ref().map(|input| {
                            crate::platform::gba_core::unpack_ds_view(
                                input.view.load(Ordering::Acquire),
                            )
                        });
                        let points = rewind
                            .iter()
                            .map(|point| RewindPoint {
                                behind: rewind_clock.saturating_sub(point.at),
                                preview: rewind_picture(&point.preview.frame, view),
                            })
                            .collect();
                        let _ = events.send(Event::RewindOpened(points));
                    }
                    Command::RewindTo(index) => {
                        if !paused {
                            continue;
                        }
                        let Some(point) = rewind.get(index) else {
                            let _ = events.send(Event::ActionFailed(
                                "That rewind moment is no longer available".into(),
                            ));
                            continue;
                        };
                        writer.wait();
                        if let Err(error) = save_current(&core, &save_path, &mut last_save) {
                            let _ = events.send(Event::ActionFailed(error));
                            continue;
                        }
                        buttons.store(0, Ordering::Release);
                        if let Some(input) = ds_input.as_ref() {
                            input.pointer.store(0, Ordering::Release);
                        }
                        core.restore_state(&point.state)?;
                        lid_closed = point.preview.lid_closed;
                        rewind_clock = point.at;
                        rewind_since = Duration::ZERO;
                        rewind.truncate_after(index);
                        core.set_speed(speed);
                        audio.clear();
                        core.run_frame();
                        rewind_clock += Duration::from_secs_f64(1.0 / fps);
                        present(&core);
                        audio.clear();
                        pacer =
                            crate::platform::gba_audio::FramePacer::new(rate, fps, speed.factor());
                        deadline = Instant::now();
                        checkpoint = Instant::now();
                        checkpoint_frames = core.frames.load(Ordering::Acquire);
                        let _ = events.send(Event::Rewound { lid_closed });
                    }
                    Command::Pause(value) => {
                        paused = value;
                        buttons.store(0, Ordering::Release);
                        audio.set_paused(value);
                        deadline = Instant::now();
                    }
                    Command::Speed(next) => {
                        speed = next;
                        core.set_speed(next);
                        pacer.reset(fps, next.factor());
                        audio.set_paused(true);
                        audio.clear();
                        audio.set_paused(paused);
                        deadline = Instant::now();
                        let _ = events.send(Event::SpeedChanged(next));
                    }
                    Command::Screenshot => {
                        // A DS screenshot is what the screen shows: its
                        // layout and swap. It's taken now (confirmed at
                        // once) and written by the writer, so it's instant.
                        let queued = core.capture_frame().and_then(|frame| {
                            let frame = match ds_input.as_ref() {
                                Some(input) => {
                                    let (layout, swapped) =
                                        crate::platform::gba_core::unpack_ds_view(
                                            input.view.load(Ordering::Acquire),
                                        );
                                    crate::platform::gba_video::ds_screenshot(
                                        &frame, layout, swapped,
                                    )
                                }
                                None => frame,
                            };
                            let target = prepare_screenshot(
                                &paths.album,
                                &game,
                                &game_folder.manifest.title,
                            )?;
                            writer
                                .send(WriteJob::Screenshot {
                                    target,
                                    frame: frame.clone(),
                                })
                                .then_some(frame)
                                .ok_or_else(|| "Could not save the screenshot".to_owned())
                        });
                        let _ = events.send(match queued {
                            Ok(frame) => Event::ScreenshotSaved(frame),
                            Err(error) => Event::ActionFailed(error),
                        });
                    }
                    Command::RememberView(view) => {
                        // Written beside the game, one at a time and in
                        // order, off the game thread.
                        if let Some(writer) = view_writer.take() {
                            let _ = writer.join();
                        }
                        let folder = game_folder.clone();
                        view_writer = thread::Builder::new()
                            .name("horizon-view".into())
                            .spawn(move || {
                                if let Err(error) = folder.remember_view(view) {
                                    tracing::warn!(%error, "could not remember the game's view");
                                }
                            })
                            .ok();
                    }
                    Command::QuickSave => {
                        if session.is_some() && core.supports_states() {
                            autosave.start(&core, &writer, QUICK_SLOT, true);
                        } else {
                            let _ = events
                                .send(Event::QuickSaved(Err("This game can't save states".into())));
                        }
                    }
                    Command::Lid(closed) => {
                        if ds && closed != lid_closed {
                            // melonDS toggles its lid on L2 + Y.
                            core.pulse_buttons((1 << 12) | (1 << 1));
                            lid_closed = closed;
                        }
                    }
                    Command::Autosave => {
                        if session.is_some()
                            && core.supports_states()
                            && autosave.played >= Autosave::AT_LEAST
                        {
                            autosave.start(&core, &writer, 0, false);
                        }
                    }
                    Command::ListStates => {
                        let _ = events.send(Event::States(states.list()));
                    }
                    Command::SaveState(slot) => {
                        let result = core.serialize_state().and_then(|bytes| core.capture_frame().and_then(|frame|
                            states.save(slot, &bytes, &frame).map_err(|_| "Could not save the state. Existing checkpoints have been preserved.".to_owned())));
                        if let Err(error) = result {
                            let _ = events.send(Event::ActionFailed(error));
                        } else {
                            let _ = events.send(Event::States(states.list()));
                            let _ = events.send(Event::StateSaved);
                        }
                    }
                    Command::LoadState { slot, quick } => match states.load_state(slot) {
                        Ok(Some(snapshot)) => {
                            save_current(&core, &save_path, &mut last_save)?;
                            audio.set_paused(true);
                            buttons.store(0, Ordering::Release);
                            audio.clear();
                            core.restore_state(&snapshot.bytes)?;
                            rewind.clear();
                            rewind_clock = Duration::ZERO;
                            rewind_since = Duration::ZERO;
                            lid_closed = false; // A fresh or loaded console has its lid open.
                            core.set_speed(speed);
                            core.run_frame();
                            present(&core);
                            audio.clear();
                            deadline = Instant::now();
                            let _ = events.send(Event::StateLoaded);
                        }
                        Ok(None) => {
                            let message = format!("Slot {slot} is empty");
                            let _ = events.send(if quick {
                                Event::Notice(message)
                            } else {
                                Event::ActionFailed("This state slot is empty".into())
                            });
                        }
                        Err(error) => {
                            let _ = events.send(if quick {
                                Event::Notice(format!("Couldn't load slot {slot}"))
                            } else {
                                Event::ActionFailed(error.to_string())
                            });
                        }
                    },
                    Command::Restart => {
                        if let Err(error) = save_current(&core, &save_path, &mut last_save) {
                            paused = true;
                            audio.set_paused(true);
                            let _ = events.send(Event::SaveFailed(error));
                            continue;
                        }
                        core.restart();
                        rewind.clear();
                        rewind_clock = Duration::ZERO;
                        rewind_since = Duration::ZERO;
                        lid_closed = false; // A fresh or loaded console has its lid open.
                        core.set_speed(speed);
                        if !last_save.is_empty()
                            && let Err(error) = core.restore_save(&last_save)
                        {
                            paused = true;
                            audio.set_paused(true);
                            let _ = events.send(Event::ActionFailed(error));
                            continue;
                        }
                        buttons.store(0, Ordering::Release);
                        audio.clear();
                        paused = false;
                        audio.set_paused(false);
                        deadline = Instant::now();
                        let _ = events.send(Event::Restarted);
                    }
                    Command::Stop | Command::Shutdown => {
                        let stopping = matches!(command, Command::Stop);
                        if stopping && let Ok(frame) = core.capture_frame() {
                            let _ = events.send(Event::Exiting(
                                crate::platform::gba_video::loading_picture(&frame, ds),
                            ));
                        }
                        // Tests can make the exit save slow.
                        #[cfg(test)]
                        if let Some(delay) = std::env::var("HORIZON_TEST_EXIT_DELAY_MS")
                            .ok()
                            .and_then(|value| value.parse().ok())
                        {
                            thread::sleep(Duration::from_millis(delay));
                        }
                        if let Err(error) = if session.is_some() {
                            save_current(&core, &save_path, &mut last_save).and_then(|()| {
                                if !core.supports_states() { return Ok(()); }
                                let bytes=core.serialize_state()?;
                                let frame=core.capture_frame()?;
                                states.save(0,&bytes,&frame).map_err(|_| "Could not save the automatic resume point. Free storage or restore folder access, then retry.".to_owned())
                            })
                        } else {
                            Ok(())
                        } {
                            if !stopping {
                                return Err(error);
                            }
                            paused = true;
                            audio.set_paused(true);
                            let _ = events.send(Event::SaveFailed(error));
                            continue;
                        }
                        return Ok(());
                    }
                }
            }
            // Opening is asynchronous. Do not fill (and overflow) the queue
            // while PipeWire is still connecting its first consumer. Remain
            // responsive to stop/pause commands and fail visibly if it stalls.
            if !output_started.load(Ordering::Acquire) {
                if audio_opened.elapsed() >= Duration::from_secs(2) {
                    return Err(
                        "Game audio did not start. Check the audio output and try again.".into(),
                    );
                }
                match commands.recv_timeout(Duration::from_millis(5)) {
                    Ok(command) => pending = Some(command),
                    Err(mpsc::RecvTimeoutError::Disconnected) => pending = Some(Command::Shutdown),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                deadline = Instant::now();
                continue;
            }
            let now = Instant::now();
            if !paused {
                autosave.played += now
                    .duration_since(last_tick)
                    .min(Duration::from_millis(100));
            }
            last_tick = now;
            if !paused && session.is_some() && core.supports_states() && autosave.due() {
                autosave.start(&core, &writer, 0, false);
            }
            if !paused {
                let before = audio.written();
                core.run_frame();
                let pushed = audio.written().wrapping_sub(before);
                let presented = present(&core);
                let elapsed = Duration::from_secs_f64(1.0 / fps);
                rewind_clock += elapsed;
                rewind_since += elapsed;
                // Reuse the GPU readback of a frame already being shown.
                // At 2×, taking an extra full-resolution readback just for
                // rewind can stall audio. In the background, still retain
                // history even when the display stops requesting frames.
                if rewind_enabled
                    && rewind_since >= crate::domain::rewind::INTERVAL
                    && (!ds || presented || rewind_since >= crate::domain::rewind::INTERVAL * 2)
                {
                    rewind_since = Duration::ZERO;
                    if let Err(error) = record_rewind(&core, &mut rewind, rewind_clock, lid_closed)
                    {
                        rewind_enabled = false;
                        rewind.clear();
                        let _ = events.send(Event::Notice(format!("Rewind unavailable: {error}")));
                    }
                }
                // The first GPU readback can warm shaders for longer than a
                // frame. That is startup, not emulation time to catch up in
                // an audio-overfilling burst after the first picture.
                if frame_attempts == 0 {
                    deadline = Instant::now();
                }
                frame_attempts = frame_attempts.saturating_add(1);
                if frame_attempts >= 120 && core.frames.load(Ordering::Acquire) == 0 {
                    return Err("mGBA could not produce a game frame".into());
                }
                if session.is_none() && core.frames.load(Ordering::Acquire) > 0 {
                    session = Some(
                        repository
                            .begin_play_session(
                                location.game_id,
                                &source,
                                chrono::Utc::now().timestamp(),
                                SessionTrackingMethod::SourceRuntime,
                            )
                            .map_err(|e| e.to_string())?,
                    );
                    let _ = events.send(Event::Started(location.title.clone()));
                }
                deadline += if ds && speed != PlaybackSpeed::Normal {
                    pacer.fast_forward_interval(pushed, audio.len(), audio.limit())
                } else {
                    pacer.interval(pushed, audio.len(), audio.limit())
                };
                if Instant::now().saturating_duration_since(deadline) > Duration::from_millis(100) {
                    deadline = Instant::now();
                }
            }
            if checkpoint.elapsed() >= Duration::from_secs(5) {
                // Fast-forward that falls short is worth knowing about.
                let emulated = core.frames.load(Ordering::Acquire);
                let reached = emulated.saturating_sub(checkpoint_frames) as f64
                    / checkpoint.elapsed().as_secs_f64()
                    / fps.max(1.0);
                if speed != PlaybackSpeed::Normal {
                    tracing::info!(
                        chosen = speed.factor(),
                        reached = format!("{reached:.2}"),
                        "fast-forward speed in the last 5 seconds"
                    );
                }
                checkpoint_frames = emulated;
                // Report audible audio trouble, so a hitch can be told apart
                // from anything else.
                let now = audio.problems();
                if now != reported {
                    tracing::warn!(
                        gaps = now.0 - reported.0,
                        dropped_ms = (now.1 - reported.1) * 1000 / u64::from(rate.max(1)),
                        "game audio ran dry or overflowed in the last 5 seconds"
                    );
                    reported = now;
                }
                if let Err(error) = if session.is_some() {
                    save_current(&core, &save_path, &mut last_save)
                } else {
                    Ok(())
                } {
                    paused = true;
                    audio.set_paused(true);
                    buttons.store(0, Ordering::Release);
                    let _ = events.send(Event::SaveFailed(error));
                }
                repository
                    .checkpoint_open_play_sessions(chrono::Utc::now().timestamp())
                    .map_err(|e| e.to_string())?;
                checkpoint = Instant::now();
            }
            let delay = if paused {
                Duration::from_millis(100)
            } else {
                deadline.saturating_duration_since(Instant::now())
            };
            match commands.recv_timeout(delay) {
                Ok(command) => pending = Some(command),
                Err(mpsc::RecvTimeoutError::Disconnected) => pending = Some(Command::Shutdown),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    })();
    // A remembered view is small and quick: let it finish before leaving.
    if let Some(writer) = view_writer.take() {
        let _ = writer.join();
    }
    let (gaps, dropped) = audio.problems();
    tracing::info!(
        gaps,
        dropped_ms = dropped * 1000 / u64::from(rate.max(1)),
        "game audio for this session"
    );
    if let Some(session) = session {
        let end = if result.is_ok() {
            repository.complete_play_session(session, chrono::Utc::now().timestamp())
        } else {
            repository.interrupt_play_session(session)
        };
        end.map_err(|e| e.to_string())?;
    }
    if result.is_ok() {
        let _ = events.send(Event::Stopped);
    }
    result
}

/// A screenshot's place in Album, claimed (its partial file created) before
/// it is confirmed, so a missing or blocked Album is reported at once; the
/// slow part, `finish_screenshot`, runs on the writer.
struct ScreenshotFile {
    path: PathBuf,
    partial: PathBuf,
    file: File,
}

fn prepare_screenshot(
    album: &Path,
    game: &crate::domain::SourceGameRef,
    title: &str,
) -> Result<ScreenshotFile, String> {
    use crate::services::album::catalog::{HorizonAlbum, part_path};
    let path = HorizonAlbum::new(album.to_owned())
        .new_capture_path(Some(game), title, "jxl", chrono::Utc::now())
        .map_err(|_| "Could not prepare the screenshot in Album".to_owned())?;
    let partial = part_path(&path);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&partial)
        .map_err(|_| "Could not create the screenshot".to_owned())?;
    Ok(ScreenshotFile {
        path,
        partial,
        file,
    })
}

fn finish_screenshot(
    target: ScreenshotFile,
    frame: &crate::platform::gba_core::Frame,
) -> Result<(), String> {
    let ScreenshotFile {
        path,
        partial,
        file,
    } = target;
    let result = (|| -> Result<(), String> {
        crate::platform::jpeg_xl::write_rgba_quickly(&file, &frame.rgba, frame.width, frame.height)
            .map_err(|_| "Could not encode the screenshot".to_owned())?;
        file.sync_all()
            .map_err(|_| "Could not save the screenshot".to_owned())?;
        fs::hard_link(&partial, &path)
            .map_err(|_| "Could not publish the screenshot in Album".to_owned())?;
        Ok(())
    })();
    if let Err(error) = fs::remove_file(&partial) {
        tracing::warn!(%error, "could not remove screenshot partial file");
    }
    result
}

#[cfg(test)]
fn write_screenshot(
    album: &Path,
    game: &crate::domain::SourceGameRef,
    title: &str,
    frame: &crate::platform::gba_core::Frame,
) -> Result<(), String> {
    finish_screenshot(prepare_screenshot(album, game, title)?, frame)
}

#[cfg(test)]
fn read_save(path: &Path) -> Result<Option<Vec<u8>>, String> {
    read_save_bounded(path, 128 * 1024)
}
/// A save with the game file's name beside it: `.sav` (mGBA, melonDS and most
/// emulators), `.srm` (RetroArch) or, for DS, DeSmuME's `.dsv` without its
/// footer. Read only; the file itself is never changed. The first that reads
/// as a plausible save wins.
fn save_beside(rom: &Path, ds: bool, maximum: usize) -> Option<(PathBuf, Vec<u8>)> {
    let extensions: &[&str] = if ds {
        &["sav", "dsv", "srm"]
    } else {
        &["sav", "srm"]
    };
    extensions.iter().find_map(|extension| {
        let path = rom.with_extension(extension);
        let mut bytes = read_save_bounded(&path, maximum + 1024).ok()??;
        if *extension == "dsv" {
            bytes.truncate(dsv_payload(&bytes)?);
        }
        (!bytes.is_empty() && bytes.len() <= maximum).then_some((path, bytes))
    })
}

/// DeSmuME appends a footer to the raw save, introduced by a text marker.
fn dsv_payload(bytes: &[u8]) -> Option<usize> {
    const MARKER: &[u8] = b"|<--Snip above here to create a raw sav";
    bytes
        .windows(MARKER.len())
        .rposition(|window| window == MARKER)
}

fn read_save_bounded(path: &Path, maximum: usize) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => {
            return Err("The existing save is not a regular file. It has been preserved.".into());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Could not inspect the existing save. It has been preserved.".into()),
        _ => {}
    }
    match File::open(path) {
        Ok(file) => {
            let size = file
                .metadata()
                .map_err(|_| "Could not inspect the existing save".to_owned())?
                .len();
            if size == 0 || size > maximum as u64 {
                return Err("The existing save is invalid. It has been preserved.".into());
            }
            let mut bytes = Vec::with_capacity(size as usize);
            file.take(maximum as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "Could not read the existing save".to_owned())?;
            if bytes.len() as u64 != size {
                return Err("The save changed while reading. It has been preserved.".into());
            }
            Ok(Some(bytes))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Could not read the existing save. It has been preserved.".into()),
    }
}
fn save_current(core: &Core, path: &Path, last: &mut Vec<u8>) -> Result<(), String> {
    let bytes = core.save_ram()?;
    if bytes.is_empty() || bytes == *last {
        return Ok(());
    }
    write_save_bounded(path, &bytes, core.max_save_size())?;
    *last = bytes;
    Ok(())
}
#[cfg(test)]
pub fn write_save(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_save_bounded(path, bytes, 128 * 1024)
}
fn write_save_bounded(path: &Path, bytes: &[u8], maximum: usize) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > maximum {
        return Err("Invalid game save size".into());
    }
    fn replace(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let temporary = crate::platform::data_paths::save_temporary_path(path);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        let result = (|| {
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            if let Some(parent) = path.parent() {
                File::open(parent)?.sync_all()?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
    if let Some(previous) = read_save_bounded(path, maximum)? {
        let backup =
            crate::platform::data_paths::save_backup_path(path).map_err(|e| e.to_string())?;
        let parent = path.parent().ok_or("The save has no directory")?;
        let backup_parent = backup.parent().ok_or("The backup has no directory")?;
        crate::services::game_storage::ensure_directory(parent, backup_parent)
            .map_err(|_| "Could not prepare the save backup directory".to_owned())?;
        replace(&backup, &previous)
            .map_err(|_| "Could not preserve the previous save".to_owned())?;
    }
    replace(path,bytes).map_err(|_| "Could not write the game save. Free storage or restore folder access, then try returning to Horizon again.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires the built pinned mGBA core (HORIZON_GBA_CORE)"]
    fn real_core_rewind_previews_do_not_restore_until_commit() {
        let path = std::env::var_os("HORIZON_GBA_CORE").unwrap();
        let mut core = Core::load(
            Path::new(&path),
            &crate::platform::gba_core::test_rom(),
            Default::default(),
            Arc::new(crate::platform::gba_audio::GameAudio::default()),
            Arc::new(AtomicU16::new(0)),
        )
        .unwrap();
        let mut history = crate::domain::rewind::History::default();
        for i in 0..3 {
            for _ in 0..30 {
                core.run_frame();
            }
            record_rewind(&core, &mut history, Duration::from_millis(i * 500), false).unwrap();
        }
        // mGBA's state frame counter: compare machine progress, not audio bookkeeping.
        let counter = |bytes: &[u8]| u32::from_le_bytes(bytes[0x1fc..0x200].try_into().unwrap());
        let live = counter(&core.serialize_state().unwrap());
        let point = history.get(0).unwrap();
        let earlier = counter(&point.state);
        assert!(earlier < live);
        let preview = rewind_picture(&point.preview.frame, None);
        assert_eq!((preview.width, preview.height), (240, 160));
        assert_eq!(counter(&core.serialize_state().unwrap()), live);
        assert_eq!(history.len(), 3);
        core.restore_state(&point.state).unwrap();
        assert_eq!(counter(&core.serialize_state().unwrap()), earlier);
        history.truncate_after(0);
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn rewind_ds_previews_follow_the_current_view_and_bound_upscaled_images() {
        use crate::{domain::core_settings::DsLayout, platform::gba_core::Frame};
        let mut rgba = [255, 0, 0, 255].repeat(256 * 192);
        rgba.extend([0, 0, 255, 255].repeat(256 * 192));
        let raw = Frame {
            width: 256,
            height: 384,
            rgba,
            smooth: false,
        };
        let side = rewind_picture(&raw, Some((DsLayout::SideBySide, true)));
        assert_eq!((side.width, side.height), (512, 192));
        assert_eq!(&side.rgba[..4], &[0, 0, 255, 255]);
        let large = rewind_picture(&raw, Some((DsLayout::LargeAndSmall, false)));
        assert!(large.width <= 512 && large.height <= 384);
        assert_eq!(&large.rgba[..4], &[255, 0, 0, 255]);
    }
    fn directory(name: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("horizon-gba-{name}-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }
    #[test]
    fn launch_rejects_changed_content_even_when_filename_is_unchanged() {
        let dir = directory("hash");
        let path = dir.join("same.gba");
        let bytes = vec![0u8; 192];
        fs::write(&path, &bytes).unwrap();
        let hash = ring::digest::digest(&ring::digest::SHA256, &bytes)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(read_rom_snapshot(&path, &hash).unwrap(), bytes);
        fs::write(&path, vec![1u8; 192]).unwrap();
        assert!(read_rom_snapshot(&path, &hash).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn battery_save_replacement_preserves_prior_data_and_rejects_invalid_existing_data() {
        let dir = directory("save");
        let path = dir.join("game.sav");
        write_save(&path, &[1; 512]).unwrap();
        write_save(&path, &[2; 512]).unwrap();
        assert_eq!(read_save(&path).unwrap().unwrap(), vec![2; 512]);
        assert_eq!(
            fs::read(crate::platform::data_paths::save_backup_path(&path).unwrap()).unwrap(),
            vec![1; 512]
        );
        fs::write(&path, vec![0u8; 128 * 1024 + 1]).unwrap();
        assert!(write_save(&path, &[3; 512]).is_err());
        assert_eq!(fs::metadata(&path).unwrap().len(), 128 * 1024 + 1);
        assert_eq!(
            fs::read(crate::platform::data_paths::save_backup_path(&path).unwrap()).unwrap(),
            vec![1; 512]
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn screenshots_publish_native_pixels_and_preserve_unavailable_album_data() {
        let dir = directory("screenshots");
        let frame = crate::platform::gba_core::Frame {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
            smooth: false,
        };
        write_screenshot(
            &dir,
            &crate::domain::SourceGameRef::new(
                SourceId::new("gba").unwrap(),
                crate::domain::ExternalGameId::new(format!("sha256:{}", "a".repeat(64))).unwrap(),
            ),
            "Test game",
            &frame,
        )
        .unwrap();
        let shots = crate::services::album::catalog::HorizonAlbum::new(dir.clone()).scan();
        assert_eq!(shots.len(), 1);
        assert_eq!(
            crate::services::image::decode_file(shots[0].path())
                .unwrap()
                .to_rgba8()
                .as_raw(),
            &frame.rgba
        );
        assert_eq!(shots[0].path().extension().unwrap(), "jxl");
        let blocked = dir.join("unavailable");
        fs::write(&blocked, b"preserved").unwrap();
        assert!(
            write_screenshot(
                &blocked,
                &crate::domain::SourceGameRef::new(
                    SourceId::new("gba").unwrap(),
                    crate::domain::ExternalGameId::new(format!("sha256:{}", "a".repeat(64)))
                        .unwrap()
                ),
                "Test game",
                &frame
            )
            .is_err()
        );
        assert_eq!(fs::read(blocked).unwrap(), b"preserved");
        fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod storage_failure_tests {
    use super::*;
    #[test]
    fn failed_backup_and_occupied_partial_preserve_save_and_unrelated_files() {
        let root =
            std::env::temp_dir().join(format!("horizon-save-failure-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let save = root.join("Game.sav");
        write_save(&save, &[1; 512]).unwrap();
        let backup = crate::platform::data_paths::save_backup_path(&save).unwrap();
        fs::write(backup.parent().unwrap(), b"keep directory obstruction").unwrap();
        assert!(write_save(&save, &[2; 512]).is_err());
        assert_eq!(read_save(&save).unwrap().unwrap(), vec![1; 512]);
        fs::remove_file(backup.parent().unwrap()).unwrap();
        let partial = crate::platform::data_paths::save_temporary_path(&save);
        fs::write(&partial, b"keep unfinished file").unwrap();
        assert!(write_save(&save, &[2; 512]).is_err());
        assert_eq!(fs::read(&partial).unwrap(), b"keep unfinished file");
        assert_eq!(read_save(&save).unwrap().unwrap(), vec![1; 512]);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn save_symlinks_are_rejected_without_reading_or_replacing_the_target() {
        let root = std::env::temp_dir().join(format!("horizon-save-link-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let original = root.join("outside.sav");
        fs::write(&original, [1; 512]).unwrap();
        let save = root.join("Game.sav");
        std::os::unix::fs::symlink(&original, &save).unwrap();
        assert!(read_save(&save).is_err());
        assert!(write_save(&save, &[2; 512]).is_err());
        assert_eq!(fs::read(original).unwrap(), vec![1; 512]);
        assert!(fs::symlink_metadata(save).unwrap().file_type().is_symlink());
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod save_import_tests {
    use super::*;

    #[test]
    fn saves_beside_the_game_are_found_and_desmume_footers_dropped() {
        let directory =
            std::env::temp_dir().join(format!("horizon-save-import-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let rom = directory.join("Game (USA).nds");
        fs::write(&rom, b"rom").unwrap();
        assert!(save_beside(&rom, true, 1024).is_none());
        // DeSmuME: the raw save, then its footer.
        let mut dsv = vec![7u8; 512];
        dsv.extend_from_slice(
            b"|<--Snip above here to create a raw sav by excluding this DeSmuME savedata footer:",
        );
        dsv.extend_from_slice(&[0; 40]);
        dsv.extend_from_slice(b"|-DESMUME SAVE-|");
        fs::write(directory.join("Game (USA).dsv"), &dsv).unwrap();
        let (found, bytes) = save_beside(&rom, true, 1024).unwrap();
        assert!(found.ends_with("Game (USA).dsv"));
        assert_eq!(bytes, vec![7u8; 512]);
        // The file itself is left as it was.
        assert_eq!(fs::read(directory.join("Game (USA).dsv")).unwrap(), dsv);
        // A plain .sav is preferred; other systems don't read .dsv.
        fs::write(directory.join("Game (USA).sav"), [1u8; 256]).unwrap();
        assert_eq!(save_beside(&rom, true, 1024).unwrap().1, vec![1u8; 256]);
        fs::remove_file(directory.join("Game (USA).sav")).unwrap();
        assert!(save_beside(&rom, false, 1024).is_none());
        // Too large for the cartridge: not a save of this game.
        fs::write(directory.join("Game (USA).sav"), vec![1u8; 4096]).unwrap();
        assert_eq!(save_beside(&rom, true, 1024).unwrap().1, vec![7u8; 512]);
        fs::remove_dir_all(directory).unwrap();
    }
}
