//! Reading the library again while Horizon runs.
//!
//! The sources are asked on a worker thread, with a registry of its own;
//! what they found is saved on the UI thread, where the library's database
//! connection lives, the same way startup saves it. The fresh library is then
//! merged into the shared catalog (indices never move) and each page is told
//! what changed.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::mpsc::Receiver,
};

use slint::ComponentHandle;
use tracing::{debug, warn};

use crate::{
    AppWindow,
    persistence::SqliteLibraryRepository,
    presentation::{
        activity::{ActivityController, ActivityShowcaseController},
        album::AlbumController,
        catalog::SharedCatalog,
        home::HomeController,
        library::LibraryController,
        wake::drain_notify,
    },
    services::{
        activity::ActivityService,
        artwork::ArtworkService,
        import::{SourceDiscoveryResult, SourceImportOutcome, SourceImportService},
        library::LibraryService,
        settings::ArtworkPreferences,
        worker::{WorkerSender, spawn, worker_channel},
    },
    sources::{SourceRegistry, production_source_registry},
};

/// What the pages a reload updates need from the app.
pub struct LibraryReloadParts {
    pub catalog: SharedCatalog,
    pub repository: Rc<RefCell<SqliteLibraryRepository>>,
    pub activity: Rc<ActivityService<SqliteLibraryRepository>>,
    pub registry: Rc<SourceRegistry>,
    pub artwork: Rc<ArtworkService>,
    pub home: Rc<HomeController>,
    pub library: Rc<LibraryController>,
    pub showcase: Rc<ActivityShowcaseController>,
    pub album: Rc<AlbumController>,
    /// The current SteamGridDB settings, for new games' covers.
    pub artwork_preferences: Rc<dyn Fn() -> ArtworkPreferences>,
}

pub struct LibraryReloader {
    parts: LibraryReloadParts,
    ui: slint::Weak<AppWindow>,
    sender: WorkerSender<Vec<SourceDiscoveryResult>>,
    receiver: RefCell<Receiver<Vec<SourceDiscoveryResult>>>,
    /// A reload's discovery is on its worker.
    running: Cell<bool>,
    /// Asked again while running: go once more when it finishes, so the
    /// last change is never missed.
    again: Cell<bool>,
}

impl LibraryReloader {
    pub fn new(ui: &AppWindow, parts: LibraryReloadParts) -> Rc<Self> {
        let (notify, drain) = drain_notify(|reloader: &Self| reloader.drain());
        let (sender, receiver) = worker_channel(notify);
        let reloader = Rc::new(Self {
            parts,
            ui: ui.as_weak(),
            sender,
            receiver: RefCell::new(receiver),
            running: Cell::new(false),
            again: Cell::new(false),
        });
        drain.bind(&reloader);
        reloader
    }

    /// Read every source again, in the background.
    pub fn reload(&self) {
        if self.running.get() {
            self.again.set(true);
            return;
        }
        let sender = self.sender.clone();
        let started = spawn("library-reload", move || {
            let found = match production_source_registry() {
                Ok(registry) => SourceImportService::discover_all(&registry),
                Err(error) => {
                    warn!(%error, "library reload could not set up its sources");
                    Vec::new()
                }
            };
            let _ = sender.send(found);
        });
        self.running.set(started);
    }

    fn drain(&self) {
        loop {
            let Ok(found) = self.receiver.borrow().try_recv() else {
                break;
            };
            self.running.set(false);
            self.apply(found);
            if self.again.replace(false) {
                self.reload();
            }
        }
    }

    fn apply(&self, found: Vec<SourceDiscoveryResult>) {
        if found.is_empty() {
            return;
        }
        let parts = &self.parts;
        let fresh = {
            let mut repository = parts.repository.borrow_mut();
            let mut library = LibraryService::new(&mut *repository);
            match SourceImportService::save_all(found, &mut library) {
                Ok(summary) => {
                    for report in summary.reports() {
                        if let SourceImportOutcome::Failed(error) = report.outcome() {
                            warn!(
                                source = %report.source_id(),
                                %error,
                                "source reload failed; keeping its saved games"
                            );
                        }
                    }
                }
                Err(error) => {
                    warn!(%error, "library reload could not be saved");
                    return;
                }
            }
            match library.games() {
                Ok(games) => games,
                Err(error) => {
                    warn!(%error, "library reload could not be read back");
                    return;
                }
            }
        };

        self.accept_games(fresh);
    }

    pub fn accept_games(&self, fresh: Vec<crate::domain::LibraryGame>) {
        let parts = &self.parts;
        let changes = parts.catalog.borrow_mut().merge(fresh);
        if changes.is_empty() {
            debug!("library reloaded; nothing changed");
            return;
        }
        debug!(
            added = changes.added.len(),
            updated = changes.updated.len(),
            removed = changes.removed.len(),
            "library reloaded"
        );
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let recent = parts
            .activity
            .recent_game_ids(crate::presentation::home::HOME_RECENT_LIMIT)
            .unwrap_or_else(|error| {
                warn!(%error, "recent games unavailable after a library reload");
                Vec::new()
            });
        parts.home.catalog_changed(
            &ui,
            &changes,
            &parts.artwork,
            &parts.registry,
            (parts.artwork_preferences)(),
            &recent,
        );
        parts.library.catalog_changed(&ui);
        let catalog = parts.catalog.borrow();
        parts.album.add_titles(
            changes
                .added
                .iter()
                .chain(&changes.updated)
                .map(|index| &catalog[*index]),
        );
        drop(catalog);
        if let Some(overview) = ActivityController::refresh(&ui, &parts.activity) {
            parts.showcase.refresh(&ui, &overview);
        }
    }
}
