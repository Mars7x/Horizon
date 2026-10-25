//! Identity-bound game options shared by Home and Library: a side panel with
//! pages for save data, mods (3DS), properties and confirmations. Rust owns
//! the page, focus and every operation; Slint only presents them.
use super::{
    CallbackSlot, catalog::SharedCatalog, home::HomeController, library_reload::LibraryReloader,
    wake::drain_notify,
};
use crate::{
    AppWindow,
    audio::UiSoundCue,
    domain::GameId,
    input::{UiAction, UiActionEvent},
    services::{
        game_management::{self, GameDetails, ManagementError, UninstallOutcome},
        mods::ModEntry,
        rom_import::RomSystem,
        worker::{WorkerSender, spawn, worker_channel},
    },
};
use slint::{ComponentHandle, Model};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::mpsc::Receiver,
};

/// The panel's pages (the `game-menu-view` values).
const OPTIONS: i32 = 0;
const PROPERTIES: i32 = 1;
const CONFIRM_UNINSTALL: i32 = 2;
const SAVES: i32 = 4;
const CONFIRM_DELETE_SAVES: i32 = 5;
const MODS: i32 = 6;

/// The options page's rows, in order. Manage mods is for 3DS games, the
/// only system Horizon applies mods to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Choice {
    Saves,
    Mods,
    Properties,
    Hide,
    Uninstall,
}
fn choices(three_ds: bool) -> &'static [Choice] {
    if three_ds {
        &[
            Choice::Saves,
            Choice::Mods,
            Choice::Properties,
            Choice::Hide,
            Choice::Uninstall,
        ]
    } else {
        &[
            Choice::Saves,
            Choice::Properties,
            Choice::Hide,
            Choice::Uninstall,
        ]
    }
}
fn row_of(three_ds: bool, choice: Choice) -> i32 {
    choices(three_ds)
        .iter()
        .position(|&c| c == choice)
        .map_or(0, |row| row as i32)
}

/// Save data page rows.
const OPEN_SAVES: i32 = 0;
const BACK_UP_SAVES: i32 = 1;
const DELETE_SAVES: i32 = 2;

enum Reply {
    Details(u64, Result<GameDetails, ManagementError>),
    Mutation(u64, Result<UninstallOutcome, ManagementError>),
    Save(u64, i32, Result<String, String>),
    Mods(u64, Result<(Vec<ModEntry>, String), String>),
}

/// What the mods page asks its worker to do.
enum ModsAction {
    List,
    Toggle(String, bool),
    Open,
}

pub struct GameMenuController {
    ui: slint::Weak<AppWindow>,
    database: PathBuf,
    saves: PathBuf,
    catalog: SharedCatalog,
    home: Rc<HomeController>,
    reloader: Rc<LibraryReloader>,
    target: Cell<Option<GameId>>,
    three_ds: Cell<bool>,
    details: RefCell<Option<GameDetails>>,
    generation: Cell<u64>,
    mutation: RefCell<Option<std::thread::JoinHandle<()>>>,
    close_requested: Cell<bool>,
    sender: WorkerSender<Reply>,
    receiver: RefCell<Receiver<Reply>>,
    sound: CallbackSlot<dyn Fn(UiSoundCue)>,
    mods: RefCell<Vec<ModEntry>>,
    mods_model: Rc<slint::VecModel<crate::ModRow>>,
    properties_model: Rc<slint::VecModel<crate::PropertyRow>>,
}

impl GameMenuController {
    pub fn new(
        ui: &AppWindow,
        database: PathBuf,
        saves: PathBuf,
        catalog: SharedCatalog,
        home: Rc<HomeController>,
        reloader: Rc<LibraryReloader>,
    ) -> Rc<Self> {
        let (notify, drain) = drain_notify(|controller: &Self| controller.drain());
        let (sender, receiver) = worker_channel(notify);
        let controller = Rc::new(Self {
            ui: ui.as_weak(),
            database,
            saves,
            catalog,
            home,
            reloader,
            target: Cell::new(None),
            three_ds: Cell::new(false),
            details: RefCell::new(None),
            generation: Cell::new(0),
            mutation: RefCell::new(None),
            close_requested: Cell::new(false),
            sender,
            receiver: RefCell::new(receiver),
            sound: RefCell::new(None),
            mods: RefCell::new(Vec::new()),
            mods_model: Rc::new(slint::VecModel::default()),
            properties_model: Rc::new(slint::VecModel::default()),
        });
        ui.set_game_menu_mods(slint::ModelRc::from(controller.mods_model.clone()));
        ui.set_game_menu_properties(slint::ModelRc::from(controller.properties_model.clone()));
        drain.bind(&controller);
        let weak = Rc::downgrade(&controller);
        ui.on_game_menu_choose(move |index| {
            if let Some(controller) = weak.upgrade() {
                controller.choose(index);
            }
        });
        let weak = Rc::downgrade(&controller);
        ui.on_game_menu_focus(move |index| {
            if let Some(controller) = weak.upgrade()
                && let Some(ui) = controller.ui.upgrade()
                && ui.get_game_menu_open()
                && !ui.get_game_menu_busy()
                && controller
                    .last_row(ui.get_game_menu_view())
                    .is_some_and(|last| (0..=last).contains(&index))
            {
                ui.set_game_menu_selected(index);
            }
        });
        let weak = Rc::downgrade(&controller);
        ui.on_game_menu_back(move || {
            if let Some(controller) = weak.upgrade() {
                controller.back();
            }
        });
        let weak = Rc::downgrade(&controller);
        ui.on_game_menu_dismiss(move || {
            if let Some(controller) = weak.upgrade()
                && controller.close()
            {
                controller.cue(UiSoundCue::Back);
            }
        });
        let weak = Rc::downgrade(&controller);
        ui.on_game_menu_scroll(move |direction| {
            if let Some(controller) = weak.upgrade() {
                controller.scroll(direction);
            }
        });
        controller
    }

    /// Keep a normal window close pending until a durable mutation finishes.
    pub fn delay_close(&self) -> bool {
        if self.ui.upgrade().is_some_and(|ui| ui.get_game_menu_busy()) {
            self.close_requested.set(true);
            true
        } else {
            false
        }
    }

    pub fn set_sound(&self, sound: Rc<dyn Fn(UiSoundCue)>) {
        *self.sound.borrow_mut() = Some(sound);
    }
    fn cue(&self, cue: UiSoundCue) {
        if let Some(sound) = self.sound.borrow().as_ref() {
            sound(cue);
        }
    }
    fn next_generation(&self) -> u64 {
        let next = self.generation.get().wrapping_add(1);
        self.generation.set(next);
        next
    }

    /// The last row a page can focus; None for pages without rows.
    fn last_row(&self, view: i32) -> Option<i32> {
        match view {
            OPTIONS => Some(choices(self.three_ds.get()).len() as i32 - 1),
            SAVES => Some(DELETE_SAVES),
            MODS => Some(i32::try_from(self.mods.borrow().len()).unwrap_or(i32::MAX - 1)),
            CONFIRM_UNINSTALL | CONFIRM_DELETE_SAVES => Some(1),
            _ => None,
        }
    }

    fn show(&self, ui: &AppWindow, view: i32, selected: i32) {
        ui.set_game_menu_view(view);
        ui.set_game_menu_selected(selected);
        ui.set_game_menu_message("".into());
    }

    pub fn open(&self, id: i64) -> bool {
        let Some(ui) = self.ui.upgrade() else {
            return false;
        };
        if ui.get_player_open() || ui.get_game_menu_open() || ui.get_game_menu_busy() {
            return false;
        }
        let Ok(id) = GameId::new(id) else {
            return false;
        };
        let catalog = self.catalog.borrow();
        let Some(index) = catalog.position(id) else {
            return false;
        };
        let Some(card) = self.home.card_at(index) else {
            return false;
        };
        let three_ds = catalog[index].sources().iter().any(|source| {
            RomSystem::from_source_id(source.source_id().as_str()) == Some(RomSystem::Nintendo3ds)
        });
        self.target.set(Some(id));
        self.three_ds.set(three_ds);
        *self.details.borrow_mut() = None;
        self.properties_model.set_vec(Vec::new());
        self.mods.borrow_mut().clear();
        self.mods_model.set_vec(Vec::new());
        ui.set_game_menu_game(card);
        ui.set_game_menu_hidden(catalog[index].hidden());
        ui.set_game_menu_mods_available(three_ds);
        drop(catalog);
        self.show(&ui, OPTIONS, 0);
        ui.set_game_menu_offset(0.0);
        ui.set_game_menu_open(true);
        self.load_details(id);
        self.cue(UiSoundCue::Ok);
        true
    }

    fn load_details(&self, id: GameId) {
        let generation = self.next_generation();
        let database = self.database.clone();
        let sender = self.sender.clone();
        if !spawn("game-properties", move || {
            let result = game_management::details(&database, id);
            let _ = sender.send(Reply::Details(generation, result));
        }) {
            self.error("Could not read this game's details. Close and try again.");
        }
    }

    /// Close the panel; false while an operation must finish first.
    fn close(&self) -> bool {
        let Some(ui) = self.ui.upgrade() else {
            return false;
        };
        if !ui.get_game_menu_open() || ui.get_game_menu_busy() {
            return false;
        }
        ui.set_game_menu_open(false);
        self.next_generation();
        self.target.set(None);
        self.details.borrow_mut().take();
        true
    }

    /// Up one page; on the options page, close.
    fn back(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if !ui.get_game_menu_open() || ui.get_game_menu_busy() {
            return;
        }
        let three_ds = self.three_ds.get();
        match ui.get_game_menu_view() {
            OPTIONS => {
                self.close();
            }
            PROPERTIES => self.show(&ui, OPTIONS, row_of(three_ds, Choice::Properties)),
            SAVES => self.show(&ui, OPTIONS, row_of(three_ds, Choice::Saves)),
            MODS => self.show(&ui, OPTIONS, row_of(three_ds, Choice::Mods)),
            CONFIRM_UNINSTALL => self.show(&ui, OPTIONS, row_of(three_ds, Choice::Uninstall)),
            CONFIRM_DELETE_SAVES => self.show(&ui, SAVES, DELETE_SAVES),
            _ => {
                self.close();
            }
        }
        self.cue(UiSoundCue::Back);
    }

    fn scroll(&self, direction: i32) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if !ui.get_game_menu_open() || ui.get_game_menu_view() != PROPERTIES {
            return;
        }
        let maximum = ui.get_game_menu_max_offset();
        ui.set_game_menu_offset(
            (ui.get_game_menu_offset() + direction.signum() as f32 * 48.0)
                .clamp(0.0, maximum.max(0.0)),
        );
    }

    fn choose(&self, index: i32) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if !ui.get_game_menu_open() || ui.get_game_menu_busy() {
            return;
        }
        let Some(id) = self.target.get() else {
            return;
        };
        match ui.get_game_menu_view() {
            OPTIONS => {
                let Some(&choice) = usize::try_from(index)
                    .ok()
                    .and_then(|row| choices(self.three_ds.get()).get(row))
                else {
                    return;
                };
                match choice {
                    Choice::Saves => {
                        self.show(&ui, SAVES, OPEN_SAVES);
                        self.cue(UiSoundCue::Ok);
                    }
                    Choice::Mods => {
                        self.show(&ui, MODS, 0);
                        self.cue(UiSoundCue::Ok);
                        self.manage_mods(id, ModsAction::List);
                    }
                    Choice::Properties => {
                        self.show(&ui, PROPERTIES, 0);
                        ui.set_game_menu_offset(0.0);
                        if self.details.borrow().is_none() {
                            ui.set_game_menu_message("Loading game details…".into());
                        }
                        self.cue(UiSoundCue::Ok);
                    }
                    Choice::Hide => self.mutate(id, false),
                    Choice::Uninstall => {
                        let count = self
                            .details
                            .borrow()
                            .as_ref()
                            .map_or(0, |details| details.roms.len());
                        if count == 0 {
                            self.error("Game details aren't available. Close and reopen game options before uninstalling.");
                            return;
                        }
                        ui.set_game_menu_confirm_text(
                            format!(
                                "Delete {count} imported ROM {}?\n\nSaves, states, screenshots and history stay.",
                                if count == 1 { "file" } else { "files" }
                            )
                            .into(),
                        );
                        self.show(&ui, CONFIRM_UNINSTALL, 0);
                        self.cue(UiSoundCue::Ok);
                    }
                }
            }
            CONFIRM_UNINSTALL => match index {
                0 => self.back(),
                1 => self.mutate(id, true),
                _ => {}
            },
            SAVES => match index {
                OPEN_SAVES | BACK_UP_SAVES => self.manage_saves(id, index),
                DELETE_SAVES => {
                    ui.set_game_menu_confirm_text(
                        "Delete the battery save, save states and resume point?\n\nBackups, ROMs, screenshots and history stay. Back up first to keep your progress."
                            .into(),
                    );
                    self.show(&ui, CONFIRM_DELETE_SAVES, 0);
                    self.cue(UiSoundCue::Ok);
                }
                _ => {}
            },
            CONFIRM_DELETE_SAVES => match index {
                0 => self.back(),
                1 => self.manage_saves(id, DELETE_SAVES),
                _ => {}
            },
            MODS => {
                let count = self.mods.borrow().len();
                match usize::try_from(index) {
                    Ok(row) if row < count => {
                        let entry = self.mods.borrow()[row].clone();
                        // The switch moves now (and animates); the reply
                        // confirms it, or moves it back if the choice failed.
                        self.mods_model.set_row_data(
                            row,
                            crate::ModRow {
                                title: entry.name.as_str().into(),
                                enabled: !entry.enabled,
                            },
                        );
                        self.cue(UiSoundCue::Ok);
                        self.manage_mods(id, ModsAction::Toggle(entry.name, !entry.enabled));
                    }
                    Ok(row) if row == count => self.manage_mods(id, ModsAction::Open),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// Show `mods`, changing rows in place when only their switches differ,
    /// so a switch animates instead of its row being rebuilt.
    fn show_mods(&self, mods: &[ModEntry]) {
        let row = |entry: &ModEntry| crate::ModRow {
            title: entry.name.as_str().into(),
            enabled: entry.enabled,
        };
        let same_mods = self.mods_model.row_count() == mods.len()
            && self
                .mods_model
                .iter()
                .zip(mods)
                .all(|(shown, entry)| shown.title.as_str() == entry.name);
        if !same_mods {
            self.mods_model
                .set_vec(mods.iter().map(row).collect::<Vec<_>>());
            return;
        }
        for (index, entry) in mods.iter().enumerate() {
            if self
                .mods_model
                .row_data(index)
                .is_none_or(|shown| shown.enabled != entry.enabled)
            {
                self.mods_model.set_row_data(index, row(entry));
            }
        }
    }

    fn manage_mods(&self, id: GameId, action: ModsAction) {
        let Some(ui) = self.ui.upgrade() else { return };
        let database = self.database.clone();
        let saves = self.saves.clone();
        let generation = self.next_generation();
        let sender = self.sender.clone();
        ui.set_game_menu_busy(true);
        if matches!(action, ModsAction::List) {
            ui.set_game_menu_message("Loading mods…".into());
        }
        let started = std::thread::Builder::new()
            .name("mods".into())
            .spawn(move || {
                let result = (|| {
                    let folder = crate::services::save_data::folder(&database, &saves, id)?;
                    let game = &folder.directory;
                    let message = match action {
                        ModsAction::List => String::new(),
                        ModsAction::Toggle(name, enabled) => {
                            crate::services::mods::set_enabled(game, &name, enabled)
                                .map_err(|e| format!("Could not save the mod choice: {e}"))?;
                            "Changes apply the next time the game starts".into()
                        }
                        ModsAction::Open => {
                            let directory = crate::services::mods::directory(game)
                                .map_err(|e| format!("Could not prepare the mods folder: {e}"))?;
                            crate::platform::launcher::open_directory(&directory)
                                .map_err(|e| format!("Could not open the mods folder: {e}"))?;
                            "Mods folder opened. Reopen this page after adding mods".into()
                        }
                    };
                    let mods = crate::services::mods::list(game)
                        .map_err(|e| format!("Could not read this game's mods: {e}"))?;
                    Ok((mods, message))
                })();
                let _ = sender.send(Reply::Mods(generation, result));
            });
        match started {
            Ok(worker) => *self.mutation.borrow_mut() = Some(worker),
            Err(_) => {
                ui.set_game_menu_busy(false);
                self.error("Could not start reading mods. Try again.");
            }
        }
    }

    fn manage_saves(&self, id: GameId, action: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        let database = self.database.clone();
        let saves = self.saves.clone();
        let generation = self.next_generation();
        let sender = self.sender.clone();
        ui.set_game_menu_busy(true);
        ui.set_game_menu_message("Working…".into());
        let started = std::thread::Builder::new().name("save-management".into()).spawn(move || {
            let result = (|| {
                let folder = crate::services::save_data::folder(&database, &saves, id)?;
                match action {
                    OPEN_SAVES => {
                        crate::platform::launcher::open_directory(&folder.directory)
                            .map_err(|e| format!("Could not open the save folder: {e}"))?;
                        Ok("Save folder opened".into())
                    }
                    BACK_UP_SAVES => {
                        let backup = crate::services::save_data::backup(&folder)
                            .map_err(|e| format!("Could not back up save data: {e}"))?;
                        Ok(format!("Backup saved in {}", backup.display()))
                    }
                    DELETE_SAVES => {
                        crate::services::save_data::delete(&folder)
                            .map_err(|e| format!("Could not finish deleting save data: {e}. Any remaining files were kept."))?;
                        Ok("Save data deleted. The next launch starts fresh".into())
                    }
                    _ => Err("Unknown save operation.".into()),
                }
            })();
            let _ = sender.send(Reply::Save(generation, action, result));
        });
        match started {
            Ok(worker) => *self.mutation.borrow_mut() = Some(worker),
            Err(_) => {
                ui.set_game_menu_busy(false);
                self.show(&ui, SAVES, OPEN_SAVES);
                self.error("Could not start managing save data. Try again.");
            }
        }
    }

    fn mutate(&self, id: GameId, uninstall: bool) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let database = self.database.clone();
        let generation = self.next_generation();
        let sender = self.sender.clone();
        let hidden = !ui.get_game_menu_hidden();
        let roms = self
            .details
            .borrow()
            .as_ref()
            .map(|details| details.roms.clone())
            .unwrap_or_default();
        ui.set_game_menu_busy(true);
        ui.set_game_menu_message("".into());
        let started = std::thread::Builder::new()
            .name("game-management".into())
            .spawn(move || {
                let result = if uninstall {
                    game_management::uninstall(&database, id, &roms)
                } else {
                    game_management::set_hidden(&database, id, hidden)
                        .map(|games| UninstallOutcome { games, error: None })
                };
                let _ = sender.send(Reply::Mutation(generation, result));
            });
        match started {
            Ok(worker) => {
                *self.mutation.borrow_mut() = Some(worker);
            }
            Err(_) => {
                ui.set_game_menu_busy(false);
                self.error("Could not start saving. Try again.");
            }
        }
    }

    /// Failures show on the message line of the page they happened on.
    fn error(&self, text: &str) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_game_menu_message(text.into());
        }
    }

    fn finish_requested_close(&self) {
        if self.close_requested.replace(false)
            && let Err(error) = slint::quit_event_loop()
        {
            tracing::warn!(%error, "could not finish the requested close");
        }
    }

    fn drain(&self) {
        while let Ok(reply) = self.receiver.borrow().try_recv() {
            let Some(ui) = self.ui.upgrade() else {
                return;
            };
            match reply {
                Reply::Details(generation, result)
                    if generation == self.generation.get() && ui.get_game_menu_open() =>
                {
                    match result {
                        Ok(details) => {
                            self.properties_model.set_vec(property_rows(&details));
                            *self.details.borrow_mut() = Some(details);
                            if ui.get_game_menu_view() == PROPERTIES {
                                ui.set_game_menu_message("".into());
                            }
                        }
                        Err(error) => self.error(&error.to_string()),
                    }
                }
                Reply::Save(generation, action, result) if generation == self.generation.get() => {
                    ui.set_game_menu_busy(false);
                    // A deletion returns from its confirmation to Save data.
                    if action == DELETE_SAVES {
                        ui.set_game_menu_view(SAVES);
                        ui.set_game_menu_selected(DELETE_SAVES);
                    }
                    match result {
                        Ok(message) => {
                            ui.set_game_menu_message(message.into());
                            self.cue(UiSoundCue::Ok);
                            self.finish_requested_close();
                        }
                        Err(error) => {
                            self.close_requested.set(false);
                            self.error(&error);
                        }
                    }
                }
                Reply::Mods(generation, result) if generation == self.generation.get() => {
                    ui.set_game_menu_busy(false);
                    match result {
                        Ok((mods, message)) => {
                            self.show_mods(&mods);
                            *self.mods.borrow_mut() = mods;
                            if let Some(last) = self.last_row(MODS) {
                                ui.set_game_menu_selected(ui.get_game_menu_selected().min(last));
                            }
                            let message = if !message.is_empty() {
                                message
                            } else if self.mods_model.row_count() == 0 {
                                "No mods yet. Put each mod's folder (with romfs, exefs or code.ips inside) in the mods folder".into()
                            } else {
                                "Mods apply in name order; later ones win".into()
                            };
                            ui.set_game_menu_message(message.into());
                            self.finish_requested_close();
                        }
                        Err(error) => {
                            self.close_requested.set(false);
                            // A switch moved ahead of a failed save goes back.
                            self.show_mods(&self.mods.borrow());
                            self.error(&error);
                        }
                    }
                }
                Reply::Mutation(generation, result) if generation == self.generation.get() => {
                    ui.set_game_menu_busy(false);
                    match result {
                        Ok(UninstallOutcome { games, error }) => {
                            self.reloader.accept_games(games);
                            if let Some(error) = error {
                                self.close_requested.set(false);
                                self.show(
                                    &ui,
                                    OPTIONS,
                                    row_of(self.three_ds.get(), Choice::Uninstall),
                                );
                                self.error(&format!("Uninstall stopped: {error}"));
                                if let Some(id) = self.target.get() {
                                    self.load_details(id);
                                }
                            } else {
                                self.close();
                                self.cue(UiSoundCue::Ok);
                                self.finish_requested_close();
                            }
                        }
                        Err(error) => {
                            self.close_requested.set(false);
                            if ui.get_game_menu_view() == CONFIRM_UNINSTALL {
                                self.show(
                                    &ui,
                                    OPTIONS,
                                    row_of(self.three_ds.get(), Choice::Uninstall),
                                );
                            }
                            self.error(&error.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub fn handle_action(&self, ui: &AppWindow, event: UiActionEvent) -> bool {
        if !ui.get_game_menu_open() {
            return false;
        }
        if ui.get_game_menu_busy() {
            return true;
        }
        let view = ui.get_game_menu_view();
        match event.action {
            UiAction::Back if !event.repeated => self.back(),
            // The button that opened the panel closes it, from any page.
            UiAction::Menu if !event.repeated => {
                if self.close() {
                    self.cue(UiSoundCue::Back);
                }
            }
            UiAction::Home if !event.repeated => {
                self.close();
                return false; // Global Home still clears history and resets selection.
            }
            UiAction::Accept if !event.repeated && view != PROPERTIES => {
                self.choose(ui.get_game_menu_selected())
            }
            UiAction::Up | UiAction::Down if view == PROPERTIES => {
                self.scroll(if event.action == UiAction::Up { -1 } else { 1 })
            }
            UiAction::Up | UiAction::Down => {
                if let Some(last) = self.last_row(view) {
                    let next = (ui.get_game_menu_selected()
                        + if event.action == UiAction::Up { -1 } else { 1 })
                    .clamp(0, last);
                    if next != ui.get_game_menu_selected() {
                        ui.set_game_menu_selected(next);
                        self.cue(UiSoundCue::Navigation);
                    }
                }
            }
            _ => {}
        }
        true
    }
}

impl Drop for GameMenuController {
    fn drop(&mut self) {
        // SIGINT/TERM can end the event loop directly. Finish any accepted
        // filesystem/database operation before process teardown in that case.
        if let Some(worker) = self.mutation.get_mut().take()
            && worker.join().is_err()
        {
            tracing::warn!("game-management worker ended unexpectedly during shutdown");
        }
    }
}

/// The facts on the properties page, in reading order.
fn property_rows(details: &GameDetails) -> Vec<crate::PropertyRow> {
    let row = |label: &str, value: String| crate::PropertyRow {
        label: label.into(),
        value: value.into(),
    };
    let system = details
        .game
        .sources()
        .iter()
        .map(|source| {
            RomSystem::from_source_id(source.source_id().as_str()).map_or_else(
                || source.source_id().as_str().to_uppercase(),
                |system| system.name().to_owned(),
            )
        })
        .collect::<Vec<_>>()
        .join(" · ");
    let mut rows = vec![
        row("Title", details.game.game().title().as_str().to_owned()),
        row("System", system),
    ];
    if details.roms.is_empty() {
        rows.push(row("ROM", "Unavailable".into()));
    }
    let numbered = details.roms.len() > 1;
    for (index, rom) in details.roms.iter().enumerate() {
        let label = |name: &str| {
            if numbered {
                format!("{name} {}", index + 1)
            } else {
                name.to_owned()
            }
        };
        rows.push(row(&label("File"), rom.path.to_string_lossy().into_owned()));
        rows.push(row(
            &label("Size"),
            format!("{:.2} MiB", rom.size as f64 / 1_048_576.0),
        ));
        rows.push(row(&label("SHA-256"), rom.sha256.clone()));
        rows.push(row(
            &label("Title from"),
            if rom.catalog.is_some() {
                "Verified catalog match"
            } else {
                "File name"
            }
            .into(),
        ));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manage_mods_is_offered_only_for_3ds_games() {
        assert!(!choices(false).contains(&Choice::Mods));
        assert_eq!(choices(true)[1], Choice::Mods);
        assert_eq!(row_of(false, Choice::Properties), 1);
        assert_eq!(row_of(true, Choice::Properties), 2);
        assert_eq!(row_of(false, Choice::Uninstall), 3);
        assert_eq!(row_of(true, Choice::Uninstall), 4);
    }
}
