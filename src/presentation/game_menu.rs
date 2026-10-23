//! Identity-bound game options shared by Home and Library.
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
        worker::{WorkerSender, spawn, worker_channel},
    },
};
use slint::ComponentHandle;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::mpsc::Receiver,
};

enum Reply {
    Details(u64, Result<GameDetails, ManagementError>),
    Mutation(u64, Result<UninstallOutcome, ManagementError>),
}

pub struct GameMenuController {
    ui: slint::Weak<AppWindow>,
    database: PathBuf,
    catalog: SharedCatalog,
    home: Rc<HomeController>,
    reloader: Rc<LibraryReloader>,
    target: Cell<Option<GameId>>,
    details: RefCell<Option<GameDetails>>,
    generation: Cell<u64>,
    mutation: RefCell<Option<std::thread::JoinHandle<()>>>,
    close_requested: Cell<bool>,
    sender: WorkerSender<Reply>,
    receiver: RefCell<Receiver<Reply>>,
    sound: CallbackSlot<dyn Fn(UiSoundCue)>,
}

impl GameMenuController {
    pub fn new(
        ui: &AppWindow,
        database: PathBuf,
        catalog: SharedCatalog,
        home: Rc<HomeController>,
        reloader: Rc<LibraryReloader>,
    ) -> Rc<Self> {
        let (notify, drain) = drain_notify(|controller: &Self| controller.drain());
        let (sender, receiver) = worker_channel(notify);
        let controller = Rc::new(Self {
            ui: ui.as_weak(),
            database,
            catalog,
            home,
            reloader,
            target: Cell::new(None),
            details: RefCell::new(None),
            generation: Cell::new(0),
            mutation: RefCell::new(None),
            close_requested: Cell::new(false),
            sender,
            receiver: RefCell::new(receiver),
            sound: RefCell::new(None),
        });
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
                && ui.get_game_menu_view() == 0
                && !ui.get_game_menu_busy()
                && (0..4).contains(&index)
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
        self.target.set(Some(id));
        *self.details.borrow_mut() = None;
        ui.set_game_menu_game(card);
        ui.set_game_menu_hidden(catalog[index].hidden());
        drop(catalog);
        ui.set_game_menu_selected(0);
        ui.set_game_menu_view(0);
        ui.set_game_menu_offset(0.0);
        ui.set_game_menu_details("Loading game details…".into());
        ui.set_game_menu_message("".into());
        ui.set_game_menu_open(true);
        self.reposition(&ui);
        self.load_details(id);
        self.cue(UiSoundCue::Ok);
        true
    }

    pub fn reposition(&self, ui: &AppWindow) {
        if !ui.get_game_menu_open() {
            return;
        }
        let anchor = ui.invoke_player_capture_origin();
        let (x, y) = popup_position(
            anchor,
            ui.get_logical_viewport_width_px(),
            ui.get_logical_viewport_height_px(),
        );
        ui.set_game_menu_x(x);
        ui.set_game_menu_y(y);
    }

    fn load_details(&self, id: GameId) {
        let generation = self.next_generation();
        let database = self.database.clone();
        let sender = self.sender.clone();
        if !spawn("game-properties", move || {
            let result = game_management::details(&database, id);
            let _ = sender.send(Reply::Details(generation, result));
        }) {
            self.error("Could not start reading game details. Close the menu and try again.");
        }
    }

    fn close(&self) {
        if let Some(ui) = self.ui.upgrade() {
            if ui.get_game_menu_busy() {
                return;
            }
            ui.set_game_menu_open(false);
        }
        self.next_generation();
        self.target.set(None);
        self.details.borrow_mut().take();
    }

    fn back(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if !ui.get_game_menu_open() || ui.get_game_menu_busy() {
            return;
        }
        if ui.get_game_menu_view() != 0 {
            let previous = ui.get_game_menu_view();
            ui.set_game_menu_view(0);
            ui.set_game_menu_selected(if previous == 2 { 3 } else { 1 });
            ui.set_game_menu_message("".into());
        } else {
            self.close();
        }
        self.cue(UiSoundCue::Back);
    }

    fn scroll(&self, direction: i32) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if !ui.get_game_menu_open() || ui.get_game_menu_view() != 1 || ui.get_game_menu_busy() {
            return;
        }
        let maximum = ui.get_game_menu_max_offset();
        ui.set_game_menu_offset(
            (ui.get_game_menu_offset() + direction.signum() as f32 * 54.0)
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
            0 => match index {
                0 => {
                    let slot = self.catalog.borrow().position(id);
                    self.close();
                    if let Some(slot) = slot {
                        self.home.launch_from_library(&ui, slot);
                    }
                }
                1 => {
                    ui.set_game_menu_view(1);
                    ui.set_game_menu_offset(0.0);
                    self.cue(UiSoundCue::Ok);
                }
                2 => {
                    self.mutate(id, false);
                }
                3 if self
                    .details
                    .borrow()
                    .as_ref()
                    .is_some_and(|details| !details.roms.is_empty()) =>
                {
                    let count = self
                        .details
                        .borrow()
                        .as_ref()
                        .map_or(0, |details| details.roms.len());
                    let title = ui
                        .get_game_menu_game()
                        .title
                        .chars()
                        .take(80)
                        .collect::<String>();
                    ui.set_game_menu_confirm_text(format!("{title}\n\nDeletes {count} imported ROM {}. Saves, states, screenshots and history stay.", if count == 1 { "file" } else { "files" }).into());
                    ui.set_game_menu_view(2);
                    ui.set_game_menu_selected(0);
                    self.cue(UiSoundCue::Ok);
                }
                3 => self
                    .error("Game details are unavailable. Reopen this menu before uninstalling."),
                _ => {}
            },
            2 => match index {
                0 => self.back(),
                1 => self.mutate(id, true),
                _ => {}
            },
            _ => {}
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

    fn error(&self, text: &str) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_game_menu_message(text.into());
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
                            ui.set_game_menu_details(properties_text(&details).into());
                            *self.details.borrow_mut() = Some(details);
                        }
                        Err(error) => {
                            ui.set_game_menu_details("Game details unavailable.".into());
                            self.error(&error.to_string());
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
                                ui.set_game_menu_view(0);
                                self.error(&format!("Uninstall stopped: {error}"));
                                if let Some(id) = self.target.get() {
                                    self.load_details(id);
                                }
                            } else {
                                self.close();
                                self.cue(UiSoundCue::Ok);
                                if self.close_requested.replace(false)
                                    && let Err(error) = slint::quit_event_loop()
                                {
                                    tracing::warn!(%error, "could not finish the requested close");
                                }
                            }
                        }
                        Err(error) => {
                            self.close_requested.set(false);
                            ui.set_game_menu_view(0);
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
        match event.action {
            UiAction::Back | UiAction::Menu if !event.repeated => self.back(),
            UiAction::Home if !event.repeated => {
                self.close();
                return false; // Global Home still clears history and resets selection.
            }
            UiAction::Accept if !event.repeated && ui.get_game_menu_view() != 1 => {
                self.choose(ui.get_game_menu_selected())
            }
            UiAction::Up | UiAction::Down if ui.get_game_menu_view() == 1 => {
                self.scroll(if event.action == UiAction::Up { -1 } else { 1 })
            }
            UiAction::Up | UiAction::Down if ui.get_game_menu_view() == 0 => {
                let next = (ui.get_game_menu_selected()
                    + if event.action == UiAction::Up { -1 } else { 1 })
                .clamp(0, 3);
                if next != ui.get_game_menu_selected() {
                    ui.set_game_menu_selected(next);
                    self.cue(UiSoundCue::Navigation);
                }
            }
            UiAction::Left | UiAction::Right if ui.get_game_menu_view() == 2 => {
                let next = if event.action == UiAction::Left { 0 } else { 1 };
                if next != ui.get_game_menu_selected() {
                    ui.set_game_menu_selected(next);
                    self.cue(UiSoundCue::Navigation);
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

/// Keep the compact commands next to their game, flipping left at the right
/// edge. Reserve the maximum error/busy height so a failure never leaves screen.
fn popup_position(anchor: crate::CardRect, width: f32, height: f32) -> (f32, f32) {
    let width = if width.is_finite() {
        width.max(32.0)
    } else {
        1280.0
    };
    let height = if height.is_finite() {
        height.max(32.0)
    } else {
        720.0
    };
    let menu_width = 304.0_f32.min((width - 32.0).max(0.0));
    let menu_height = 292.0_f32.min((height - 32.0).max(0.0));
    let x = if anchor.x.is_finite() {
        anchor.x * width
    } else {
        16.0
    };
    let y = if anchor.y.is_finite() {
        anchor.y * height
    } else {
        16.0
    };
    let card_width = if anchor.width.is_finite() {
        (anchor.width * width).max(0.0)
    } else {
        0.0
    };
    let right = x + card_width + 12.0;
    let candidate = if right + menu_width <= width - 16.0 {
        right
    } else {
        x - menu_width - 12.0
    };
    (
        candidate.clamp(16.0, (width - 16.0 - menu_width).max(16.0)),
        y.clamp(16.0, (height - 16.0 - menu_height).max(16.0)),
    )
}

fn properties_text(details: &GameDetails) -> String {
    let system = details
        .game
        .sources()
        .iter()
        .map(|source| source.source_id().as_str().to_uppercase())
        .collect::<Vec<_>>()
        .join(" · ");
    let mut text = format!(
        "Imported title\n{}\n\nSystem\n{}",
        details.game.game().title().as_str(),
        system
    );
    if details.roms.is_empty() {
        text.push_str("\n\nROM information unavailable");
    }
    for (index, rom) in details.roms.iter().enumerate() {
        text.push_str(&format!(
            "\n\nROM {} · {:.2} MiB\n{}\n\nSHA-256\n{}\n{}\n\nTitle metadata\n{}",
            index + 1,
            rom.size as f64 / 1_048_576.0,
            rom.path.to_string_lossy(),
            rom.sha256.chars().take(32).collect::<String>(),
            rom.sha256.chars().skip(32).collect::<String>(),
            if rom.catalog.is_some() {
                "Verified catalog match"
            } else {
                "Imported filename"
            }
        ));
    }
    text
}

#[cfg(test)]
mod placement_tests {
    use super::*;
    #[test]
    fn placement_follows_the_game_and_flips_at_screen_edges() {
        let at = |x, y, w| crate::CardRect {
            x,
            y,
            width: w,
            height: 0.3,
        };
        assert_eq!(
            popup_position(at(0.1, 0.2, 0.2), 1280.0, 720.0),
            (396.0, 144.0)
        );
        assert_eq!(
            popup_position(at(0.8, 0.8, 0.15), 1280.0, 720.0),
            (708.0, 412.0)
        );
        for (width, height) in [(800.0, 600.0), (1280.0, 720.0), (3440.0, 1440.0)] {
            let (x, y) = popup_position(at(0.99, 0.99, 0.2), width, height);
            assert!(x >= 16.0 && x + 304.0 <= width - 16.0);
            assert!(y >= 16.0 && y + 292.0 <= height - 16.0);
        }
    }
}
