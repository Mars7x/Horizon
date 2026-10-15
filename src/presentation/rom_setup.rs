//! First-run and repeat import share one modal and semantic input owner.
use crate::{
    AppWindow,
    domain::LibraryGame,
    input::actions::{UiAction, UiActionEvent},
    persistence::SqliteLibraryRepository,
    platform::file_chooser,
    presentation::{library_reload::LibraryReloader, wake::drain_notify},
    services::{
        library::LibraryRepository,
        rom_import::{self, GbaCatalog, RomCandidate, RomScan, RomSetup},
        worker::{WorkerSender, spawn, worker_channel},
    },
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::mpsc::Receiver,
};

enum ResultMessage {
    Picked(bool, Option<PathBuf>),
    Scanned(RomScan),
    Saved(Vec<LibraryGame>),
    Failed(String),
}

pub struct RomSetupController {
    ui: slint::Weak<AppWindow>,
    database: PathBuf,
    setup: RefCell<RomSetup>,
    scan: RefCell<Option<RomScan>>,
    row: Cell<usize>,
    busy: Cell<bool>,
    notice: Cell<bool>,
    sender: WorkerSender<ResultMessage>,
    receiver: RefCell<Receiver<ResultMessage>>,
    reloader: Rc<LibraryReloader>,
    finished: crate::presentation::CallbackSlot<dyn Fn(bool)>,
    sound: crate::presentation::CallbackSlot<dyn Fn(crate::audio::UiSoundCue)>,
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
            scan: RefCell::new(None),
            row: Cell::new(0),
            busy: Cell::new(false),
            notice: Cell::new(false),
            sender,
            receiver: RefCell::new(receiver),
            reloader,
            finished: RefCell::new(None),
            sound: RefCell::new(None),
        });
        drain.bind(&controller);
        let weak = Rc::downgrade(&controller);
        ui.on_rom_setup_choose(move |index| {
            if let Some(controller) = weak.upgrade() {
                controller.choose(index);
            }
        });
        let weak = Rc::downgrade(&controller);
        ui.on_playback_unavailable(move || {
            if let Some(controller) = weak.upgrade() {
                controller.notice.set(true);
                if let Some(ui) = controller.ui.upgrade() {
                    ui.set_rom_setup_open(true);
                    ui.set_rom_setup_focus(0);
                    ui.set_rom_setup_message("".into());
                }
                controller.publish();
            }
        });
        // First-run onboarding is a separate designed flow; Library can open
        // this reusable importer without replacing Welcome/Profile creation.
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

    pub fn open(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if self.busy.get() {
            return;
        }
        self.notice.set(false);
        self.scan.replace(None);
        self.row.set(0);
        ui.set_rom_setup_open(true);
        ui.set_rom_setup_focus(0);
        ui.set_rom_setup_message("".into());
        self.publish();
    }

    /// Own input before navigation, including while the modal fades out.
    pub fn handle_action(&self, ui: &AppWindow, event: UiActionEvent) -> bool {
        if !ui.get_rom_setup_blocking() {
            if ui.get_current_route() == crate::AppRouteView::Library
                && event.action == UiAction::Secondary
                && !event.repeated
            {
                self.open();
                return true;
            }
            return false;
        }
        if !ui.get_rom_setup_open() || self.busy.get() {
            return true;
        }
        let previous_focus = ui.get_rom_setup_focus();
        match event.action {
            UiAction::Up | UiAction::Left => {
                ui.set_rom_setup_focus((ui.get_rom_setup_focus() - 1).max(0))
            }
            UiAction::Down | UiAction::Right => {
                ui.set_rom_setup_focus((ui.get_rom_setup_focus() + 1).min(if self.notice.get() {
                    0
                } else if self.scan.borrow().is_some() {
                    5
                } else {
                    3
                }))
            }
            UiAction::Accept if !event.repeated => self.choose(ui.get_rom_setup_focus()),
            UiAction::Back if !event.repeated => {
                if self.notice.get() {
                    self.choose(0);
                } else {
                    self.cue(crate::audio::UiSoundCue::Back);
                    self.close(ui, false);
                }
            }
            _ => {}
        }
        if previous_focus != ui.get_rom_setup_focus()
            && matches!(
                event.action,
                UiAction::Up | UiAction::Down | UiAction::Left | UiAction::Right
            )
        {
            self.cue(crate::audio::UiSoundCue::Navigation);
        }
        true
    }

    fn publish(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if self.notice.get() {
            ui.set_rom_setup_title("Playback unavailable".into());
            ui.set_rom_setup_detail("Your games are imported. Playback will become available when Horizon adds its first embedded emulator.".into());
            ui.set_rom_setup_labels(ModelRc::new(VecModel::from(vec![
                slint::SharedString::from("Back"),
            ])));
            ui.set_rom_setup_busy(false);
            return;
        }
        let scan = self.scan.borrow();
        ui.set_rom_setup_title(
            if scan.is_some() {
                "Review games"
            } else {
                "Add games"
            }
            .into(),
        );
        let labels: Vec<String> = if let Some(scan) = scan.as_ref() {
            let selected = scan.candidates.iter().filter(|c| c.selected).count();
            let detail = if let Some(candidate) = scan.candidates.get(self.row.get()) {
                format!(
                    "{} / {} · {} selected\n{}\n{}\n{}",
                    self.row.get() + 1,
                    scan.candidates.len(),
                    selected,
                    candidate.title,
                    candidate
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    candidate.status
                )
            } else {
                "No raw .gba files found in this folder".into()
            };
            ui.set_rom_setup_detail(
                format!(
                    "{detail}\n{} other files or links skipped · Playback is not available yet",
                    scan.skipped
                )
                .into(),
            );
            vec![
                scan.candidates
                    .get(self.row.get())
                    .map(|c| {
                        if c.selected {
                            "Exclude this file"
                        } else {
                            "Include this file"
                        }
                    })
                    .unwrap_or("No file to include")
                    .into(),
                "Previous file".into(),
                "Next file".into(),
                format!("Import {selected} selected files"),
                "Change folder or catalog".into(),
                "Cancel".into(),
            ]
        } else {
            let setup = self.setup.borrow();
            ui.set_rom_setup_detail("Choose a folder, then review your games. A local GBA XML DAT catalog enables exact title and region matching. Without a catalog, games stay unverified. Supported now: raw .gba files.".into());
            vec![
                if setup.folder.is_some() {
                    "Change ROM folder"
                } else {
                    "Choose ROM folder"
                }
                .into(),
                if setup.catalog_path.is_some() {
                    "Change GBA catalog"
                } else {
                    "Choose GBA catalog (optional)"
                }
                .into(),
                "Review games".into(),
                "Set up later".into(),
            ]
        };
        ui.set_rom_setup_labels(ModelRc::new(VecModel::from(
            labels
                .into_iter()
                .map(Into::into)
                .collect::<Vec<slint::SharedString>>(),
        )));
        ui.set_rom_setup_busy(self.busy.get());
    }

    fn run(&self, job: impl FnOnce() -> Result<Option<ResultMessage>, String> + Send + 'static) {
        let sender = self.sender.clone();
        self.busy.set(true);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_rom_setup_message("Working".into());
        }
        self.publish();
        if !spawn("rom-setup", move || {
            let result = match job() {
                Ok(Some(result)) => result,
                Ok(None) => ResultMessage::Picked(false, None),
                Err(error) => ResultMessage::Failed(error),
            };
            let _ = sender.send(result);
        }) {
            self.busy.set(false);
            if let Some(ui) = self.ui.upgrade() {
                ui.set_rom_setup_message("Could not start import worker; try again".into());
            }
            self.publish();
        }
    }

    fn choose(&self, index: i32) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if self.busy.get() || !ui.get_rom_setup_open() {
            return;
        }
        if self.notice.get() {
            self.cue(crate::audio::UiSoundCue::Back);
            ui.set_rom_setup_open(false);
            return;
        }
        self.cue(if self.scan.borrow().is_some() && matches!(index, 4 | 5) {
            crate::audio::UiSoundCue::Back
        } else {
            crate::audio::UiSoundCue::Ok
        });
        ui.set_rom_setup_focus(index);
        ui.set_rom_setup_message("".into());
        if self.scan.borrow().is_some() {
            match index {
                0 => {
                    if let Some(candidate) = self
                        .scan
                        .borrow_mut()
                        .as_mut()
                        .and_then(|s| s.candidates.get_mut(self.row.get()))
                        && candidate.fingerprint.is_some()
                    {
                        candidate.selected = !candidate.selected;
                    }
                }
                1 => self.row.set(self.row.get().saturating_sub(1)),
                2 => {
                    let len = self
                        .scan
                        .borrow()
                        .as_ref()
                        .map_or(0, |s| s.candidates.len());
                    self.row
                        .set((self.row.get() + 1).min(len.saturating_sub(1)));
                }
                3 => {
                    let candidates = self
                        .scan
                        .borrow()
                        .as_ref()
                        .map(|s| {
                            s.candidates
                                .iter()
                                .filter(|c| c.selected)
                                .cloned()
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    if candidates.is_empty() {
                        ui.set_rom_setup_message("Include at least one file to import".into());
                    } else {
                        self.save(candidates);
                    }
                }
                4 => {
                    self.scan.replace(None);
                    ui.set_rom_setup_focus(0);
                }
                5 => self.close(&ui, false),
                _ => {}
            }
        } else {
            match index {
                0 | 1 => self.run(move || {
                    let directory = index == 0;
                    let path =
                        file_chooser::choose_rom_input(directory).map_err(|e| e.to_string())?;
                    if let Some(path) = &path
                        && !directory
                    {
                        GbaCatalog::load(path).map_err(|e| e.to_string())?;
                    }
                    Ok(Some(ResultMessage::Picked(directory, path)))
                }),
                2 => {
                    let setup = self.setup.borrow().clone();
                    if let Some(folder) = setup.folder {
                        self.run(move || {
                            let catalog = setup
                                .catalog_path
                                .as_deref()
                                .map(GbaCatalog::load)
                                .transpose()
                                .map_err(|e| e.to_string())?;
                            rom_import::scan(&folder, catalog.as_ref())
                                .map(|s| Some(ResultMessage::Scanned(s)))
                                .map_err(|e| e.to_string())
                        });
                    } else {
                        ui.set_rom_setup_message("Choose a ROM folder first".into());
                    }
                }
                3 => self.save(Vec::new()),
                _ => {}
            }
        }
        self.publish();
    }

    fn save(&self, candidates: Vec<RomCandidate>) {
        let database = self.database.clone();
        let mut setup = self.setup.borrow().clone();
        setup.completed = true;
        self.run(move || {
            rom_import::revalidate(&candidates).map_err(|e| e.to_string())?;
            let mut repository =
                SqliteLibraryRepository::open(database).map_err(|e| e.to_string())?;
            repository
                .save_rom_import(&setup, &candidates)
                .map_err(|e| e.to_string())?;
            repository
                .list_games()
                .map(|games| Some(ResultMessage::Saved(games)))
                .map_err(|e| e.to_string())
        });
    }

    fn drain(&self) {
        while let Ok(result) = self.receiver.borrow().try_recv() {
            self.busy.set(false);
            let Some(ui) = self.ui.upgrade() else {
                return;
            };
            ui.set_rom_setup_message("".into());
            match result {
                ResultMessage::Picked(directory, Some(path)) => {
                    ui.set_rom_setup_message(
                        format!(
                            "Selected {}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        )
                        .into(),
                    );
                    if directory {
                        self.setup.borrow_mut().folder = Some(path);
                    } else {
                        self.setup.borrow_mut().catalog_path = Some(path);
                    }
                }
                ResultMessage::Picked(_, None) => {}
                ResultMessage::Scanned(scan) => {
                    self.scan.replace(Some(scan));
                    self.row.set(0);
                    ui.set_rom_setup_focus(0);
                }
                ResultMessage::Saved(games) => {
                    self.setup.borrow_mut().completed = true;
                    self.reloader.accept_games(games);
                    self.close(&ui, true);
                }
                ResultMessage::Failed(error) => ui.set_rom_setup_message(error.into()),
            }
            self.publish();
        }
    }
}
