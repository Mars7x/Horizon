//! Choose a folder, then scan and import without a configuration/review menu.
//! The chosen folder is then checked again at startup and every few seconds,
//! so games added to it appear, and games deleted from it leave, without asking.
use crate::{
    AppWindow,
    domain::LibraryGame,
    input::{UiAction, UiActionEvent},
    persistence::SqliteLibraryRepository,
    platform::file_chooser,
    presentation::{library_reload::LibraryReloader, wake::drain_notify},
    services::{
        library::LibraryRepository,
        rom_import::{self, RomCatalog, RomSetup},
        worker::{WorkerSender, spawn, worker_channel},
    },
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::mpsc::Receiver,
    time::Duration,
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone)]
enum View {
    Picking,
    Scanning,
    Error(String),
    Skipped(usize),
    PlaybackUnavailable,
}

impl View {
    fn busy(&self) -> bool {
        matches!(self, Self::Picking | Self::Scanning)
    }
    fn last_focus(&self) -> i32 {
        if matches!(self, Self::Error(_)) { 1 } else { 0 }
    }
}

enum ResultMessage {
    Scanning,
    Cancelled,
    Saved {
        games: Vec<LibraryGame>,
        setup: RomSetup,
        unreadable: usize,
    },
    Failed(String),
    /// A folder refresh finished: the library when it added games.
    Refreshed(Result<Option<Vec<LibraryGame>>, String>),
}

pub struct RomSetupController {
    ui: slint::Weak<AppWindow>,
    database: PathBuf,
    setup: RefCell<RomSetup>,
    view: RefCell<View>,
    sender: WorkerSender<ResultMessage>,
    receiver: RefCell<Receiver<ResultMessage>>,
    reloader: Rc<LibraryReloader>,
    finished: crate::presentation::CallbackSlot<dyn Fn(bool)>,
    sound: crate::presentation::CallbackSlot<dyn Fn(crate::audio::UiSoundCue)>,
    running: Cell<bool>,
    refreshing: Cell<bool>,
    /// The last refresh failed; report the next failure only after a success.
    refresh_failed: Cell<bool>,
    watch: slint::Timer,
}

impl RomSetupController {
    pub fn new(
        ui: &AppWindow,
        database: PathBuf,
        setup: RomSetup,
        reloader: Rc<LibraryReloader>,
    ) -> Rc<Self> {
        let (notify, drain) = drain_notify(|controller: &Self| controller.drain());
        let (sender, receiver) = worker_channel(notify);
        let controller = Rc::new(Self {
            ui: ui.as_weak(),
            database,
            setup: RefCell::new(setup),
            view: RefCell::new(View::Picking),
            sender,
            receiver: RefCell::new(receiver),
            reloader,
            finished: RefCell::new(None),
            sound: RefCell::new(None),
            running: Cell::new(false),
            refreshing: Cell::new(false),
            refresh_failed: Cell::new(false),
            watch: slint::Timer::default(),
        });
        drain.bind(&controller);
        let weak = Rc::downgrade(&controller);
        controller
            .watch
            .start(slint::TimerMode::Repeated, REFRESH_INTERVAL, move || {
                if let Some(controller) = weak.upgrade() {
                    controller.refresh();
                }
            });
        controller.refresh();
        let weak = Rc::downgrade(&controller);
        ui.on_rom_setup_choose(move |index| {
            if let Some(controller) = weak.upgrade() {
                controller.choose(index);
            }
        });
        let weak = Rc::downgrade(&controller);
        ui.on_playback_unavailable(move || {
            if let Some(controller) = weak.upgrade()
                && !controller.running.get()
            {
                controller.show(View::PlaybackUnavailable);
            }
        });
        controller
    }

    pub fn set_sound(&self, sound: Rc<dyn Fn(crate::audio::UiSoundCue)>) {
        *self.sound.borrow_mut() = Some(sound);
    }
    fn cue(&self, cue: crate::audio::UiSoundCue) {
        if let Some(sound) = self.sound.borrow().as_ref() {
            sound(cue);
        }
    }
    pub fn set_finished(&self, callback: Rc<dyn Fn(bool)>) {
        *self.finished.borrow_mut() = Some(callback);
    }

    fn close(&self, ui: &AppWindow, saved: bool) {
        ui.set_rom_setup_open(false);
        if let Some(callback) = self.finished.borrow().as_ref() {
            callback(saved);
        }
    }

    /// Add new games from the chosen folder, and retire games whose files
    /// are gone (history and saves kept), in the background without a dialog.
    fn refresh(&self) {
        let setup = self.setup.borrow().clone();
        let Some(folder) = setup.folder.clone() else {
            return;
        };
        if self.running.get() || self.refreshing.replace(true) {
            return;
        }
        let sender = self.sender.clone();
        let database = self.database.clone();
        let started = spawn("rom-folder-refresh", move || {
            let result = (|| -> Result<Option<Vec<LibraryGame>>, String> {
                let catalog = setup
                    .catalog_path
                    .as_deref()
                    .and_then(|path| RomCatalog::load(path).ok());
                let mut repository =
                    SqliteLibraryRepository::open(database).map_err(|e| e.to_string())?;
                let imported = repository.imported_rom_files().map_err(|e| e.to_string())?;
                let scan = rom_import::scan_folder_for_new(
                    &folder,
                    catalog,
                    &imported,
                    std::time::SystemTime::now(),
                )
                .map_err(|e| e.to_string())?;
                let added = scan.candidates.iter().any(|candidate| candidate.selected);
                if added {
                    rom_import::revalidate(&scan.candidates).map_err(|e| e.to_string())?;
                    repository
                        .add_rom_files(&scan.candidates)
                        .map_err(|e| e.to_string())?;
                }
                // Only a complete scan gets here, so missing files are gone.
                let retired = repository
                    .retire_missing_rom_files(&folder, &scan.seen.into_iter().collect())
                    .map_err(|e| e.to_string())?;
                if !added && retired == 0 {
                    return Ok(None);
                }
                repository.list_games().map(Some).map_err(|e| e.to_string())
            })();
            let _ = sender.send(ResultMessage::Refreshed(result));
        });
        if !started {
            self.refreshing.set(false);
        }
    }

    /// Both onboarding and Library open the system picker directly.
    pub fn open(&self) {
        if self.running.replace(true) {
            return;
        }
        self.show(View::Picking);
        let sender = self.sender.clone();
        let database = self.database.clone();
        let mut setup = self.setup.borrow().clone();
        if !spawn("rom-folder-import", move || {
            let result = (|| -> Result<ResultMessage, String> {
                let Some(folder) = file_chooser::choose_game_folder().map_err(|e| e.to_string())? else {
                    return Ok(ResultMessage::Cancelled);
                };
                let _ = sender.send(ResultMessage::Scanning);
                // Existing catalog configuration is automatic; an unavailable
                // catalog must not stop folder import or fabricate an exact match.
                let catalog = setup.catalog_path.as_deref().and_then(|path| {
                    RomCatalog::load(path).inspect_err(|error| {
                        tracing::warn!(%error, "saved ROM catalog unavailable; using unverified filenames");
                    }).ok()
                });
                let scan = rom_import::scan_folder(&folder, catalog).map_err(|e| e.to_string())?;
                if !scan.candidates.iter().any(|candidate| candidate.selected) {
                    return Err(if scan.candidates.is_empty() { "No supported games were found in this folder" } else { "The game files in this folder could not be read" }.into());
                }
                let unreadable = scan.candidates.iter().filter(|candidate| candidate.fingerprint.is_none()).count();
                rom_import::revalidate(&scan.candidates).map_err(|e| e.to_string())?;
                setup.folder = Some(folder);
                setup.completed = true;
                let mut repository = SqliteLibraryRepository::open(database).map_err(|e| e.to_string())?;
                repository.save_rom_import(&setup, &scan.candidates).map_err(|e| e.to_string())?;
                let games = repository.list_games().map_err(|e| e.to_string())?;
                Ok(ResultMessage::Saved {games,setup,unreadable})
            })().unwrap_or_else(ResultMessage::Failed);
            let _ = sender.send(result);
        }) {
            self.running.set(false);
            self.show(View::Error("Could not start the scan. Try again.".into()));
        }
    }

    pub fn handle_action(&self, ui: &AppWindow, event: UiActionEvent) -> bool {
        if !ui.get_rom_setup_blocking() {
            return false;
        }
        if !ui.get_rom_setup_open() || self.running.get() {
            return true;
        }
        let view = self.view.borrow().clone();
        let previous = ui.get_rom_setup_focus();
        match event.action {
            UiAction::Up | UiAction::Left => ui.set_rom_setup_focus(0),
            UiAction::Down | UiAction::Right => ui.set_rom_setup_focus(view.last_focus()),
            UiAction::Accept if !event.repeated => self.choose(previous),
            UiAction::Back if !event.repeated => {
                self.cue(crate::audio::UiSoundCue::Back);
                self.close(ui, matches!(view, View::Skipped(_)));
            }
            _ => {}
        }
        if previous != ui.get_rom_setup_focus()
            && matches!(
                event.action,
                UiAction::Up | UiAction::Down | UiAction::Left | UiAction::Right
            )
        {
            self.cue(crate::audio::UiSoundCue::Navigation);
        }
        true
    }

    fn show(&self, view: View) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let (title, detail, labels): (&str, String, Vec<&str>) = match &view {
            View::Picking => (
                "Choose game folder",
                "Choose your game folder in the system window.".into(),
                vec![],
            ),
            View::Scanning => (
                "Scanning games",
                "Adding games to your library.".into(),
                vec![],
            ),
            View::Error(error) => (
                "Couldn’t add games",
                error.clone(),
                vec!["Choose another folder", "Back"],
            ),
            View::Skipped(count) => (
                "Some files couldn’t be added",
                format!("Your games have been added. {count} files could not be read."),
                vec!["Continue"],
            ),
            View::PlaybackUnavailable => (
                "Playback unavailable",
                "Playback is not available for this game yet.".into(),
                vec!["Back"],
            ),
        };
        ui.set_rom_setup_open(true);
        ui.set_rom_setup_focus(0);
        ui.set_rom_setup_busy(view.busy());
        ui.set_rom_setup_title(title.into());
        ui.set_rom_setup_detail(detail.into());
        ui.set_rom_setup_labels(ModelRc::new(VecModel::from(
            labels
                .into_iter()
                .map(Into::into)
                .collect::<Vec<slint::SharedString>>(),
        )));
        *self.view.borrow_mut() = view;
    }

    fn choose(&self, index: i32) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if !ui.get_rom_setup_open() || self.running.get() {
            return;
        }
        let view = self.view.borrow().clone();
        match view {
            View::Error(_) if index == 0 => {
                self.cue(crate::audio::UiSoundCue::Ok);
                self.open();
            }
            View::Error(_) if index == 1 => {
                self.cue(crate::audio::UiSoundCue::Back);
                self.close(&ui, false);
            }
            View::Skipped(_) if index == 0 => {
                self.cue(crate::audio::UiSoundCue::Ok);
                self.close(&ui, true);
            }
            View::PlaybackUnavailable if index == 0 => {
                self.cue(crate::audio::UiSoundCue::Back);
                self.close(&ui, false);
            }
            _ => {}
        }
    }

    fn drain(&self) {
        while let Ok(result) = self.receiver.borrow().try_recv() {
            let Some(ui) = self.ui.upgrade() else {
                return;
            };
            match result {
                ResultMessage::Scanning => self.show(View::Scanning),
                ResultMessage::Cancelled => {
                    self.running.set(false);
                    self.close(&ui, false);
                }
                ResultMessage::Failed(error) => {
                    self.running.set(false);
                    self.show(View::Error(error));
                }
                ResultMessage::Refreshed(result) => {
                    self.refreshing.set(false);
                    match result {
                        Ok(games) => {
                            self.refresh_failed.set(false);
                            if let Some(games) = games {
                                self.reloader.accept_games(games);
                            }
                        }
                        Err(error) => {
                            if !self.refresh_failed.replace(true) {
                                tracing::warn!(%error, "game folder refresh failed; keeping the library");
                            }
                        }
                    }
                }
                ResultMessage::Saved {
                    games,
                    setup,
                    unreadable,
                } => {
                    self.running.set(false);
                    *self.setup.borrow_mut() = setup;
                    self.reloader.accept_games(games);
                    if unreadable == 0 {
                        self.close(&ui, true);
                    } else {
                        self.show(View::Skipped(unreadable));
                    }
                }
            }
        }
    }
}
