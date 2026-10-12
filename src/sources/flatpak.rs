//! Flatpak games: apps installed through Flatpak whose desktop entry is in
//! the Game category (ADR 0183).
//!
//! Discovery reads only the host's Flatpak exports, read-only: the exported
//! `.desktop` files and hicolor icons of the user and system installations.
//! Exported files are links into each installation's `app` folder, so both
//! folders are granted read-only. Launching (`flatpak run`) and noticing a
//! running game (`flatpak ps`) go through the host's `flatpak`, which only
//! `platform::flatpak_host` runs; this adapter reaches it through
//! [`FlatpakHost`] and never runs anything itself.

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use thiserror::Error;
use tracing::{debug, warn};

use crate::domain::{DomainValidationError, ExternalGameId, GameTitle, SourceId};

use super::{
    GameSource, SourceArtworkCandidate, SourceCapability, SourceDescriptor, SourceDiscovery,
    SourceError, SourceGame, SourceInitializationError, SourceLaunchTarget, SourceRuntimeState,
    SourceSnapshot, SourceSnapshotError, SourceUnavailableReason,
    support::{dedupe_paths, home_dir},
};

const FLATPAK_SOURCE_ID: &str = "flatpak";
const FLATPAK_DISPLAY_NAME: &str = "Flatpak";
/// Horizon itself, which is never one of its own games.
const OWN_APP_ID: &str = "io.github.Mars7x.Horizon";
const SYSTEM_EXPORTS: &str = "/var/lib/flatpak/exports/share";

/// Apps in the Game category that are not games: stores and launchers whose
/// games are their own sources or their own business (Steam and Heroic are
/// Horizon sources already), streaming clients, and game tools. Matched by
/// exact app ID.
const NOT_GAMES: &[&str] = &[
    // Stores and multi-game launchers.
    "com.valvesoftware.Steam",
    "com.heroicgameslauncher.hgl",
    "net.lutris.Lutris",
    "com.usebottles.bottles",
    "io.itch.itch",
    "io.github.sharkwouter.Minigalaxy",
    "com.github.tkashkin.gamehub",
    "com.gamejolt.Client",
    "com.github.Faugus.faugus-launcher",
    "io.github.hedge_dev.hedgemodmanager",
    // Game streaming clients.
    "com.valvesoftware.SteamLink",
    "com.moonlight_stream.Moonlight",
    "io.github.unknownskl.greenlight",
    "io.github.streetpea.Chiaki4deck",
    // Game tools.
    "net.davidotek.pupgui2",
    "com.vysp3r.ProtonPlus",
    "com.github.Matoking.protontricks",
    "io.github.benjamimgois.goverlay",
];

/// How long a running-apps answer stays current, and how long the host is
/// asked again after the last question.
const RUNNING_REFRESH: Duration = Duration::from_secs(1);
const RUNNING_IDLE: Duration = Duration::from_secs(5);

/// A Flatpak app ID as Flatpak itself accepts one: at least three dot-separated
/// parts of ASCII letters, digits, `_` and `-`, starting with a letter. Also
/// what makes an ID safe to pass to the host's `flatpak` as one argument.
pub fn valid_app_id(app_id: &str) -> bool {
    let parts: Vec<&str> = app_id.split('.').collect();
    parts.len() >= 3
        && app_id.starts_with(|c: char| c.is_ascii_alphabetic())
        && app_id.len() <= 255
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}

/// The host's Flatpak, as this source needs it. `platform::flatpak_host`
/// implements it; nothing here runs a program.
pub trait FlatpakHost: Send + Sync {
    /// The IDs of the apps running now. Blocking.
    fn running_apps(&self) -> Result<BTreeSet<String>, String>;
}

#[derive(Debug, Error)]
enum FlatpakSourceError {
    #[error("not a Flatpak app ID: {0}")]
    InvalidAppId(String),
    #[error("could not read Flatpak exports at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not ask Flatpak what is running: {0}")]
    Running(String),
    #[error(transparent)]
    Domain(#[from] DomainValidationError),
    #[error(transparent)]
    Snapshot(#[from] SourceSnapshotError),
}

/// One game's exported desktop entry.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DesktopGame {
    app_id: String,
    title: String,
    icon: Option<String>,
}

/// What one exports folder holds.
struct RootDiscovery {
    games: Vec<DesktopGame>,
    /// Every entry could be read, so an app missing here isn't installed.
    complete: bool,
}

pub struct FlatpakSource {
    descriptor: SourceDescriptor,
    /// `…/exports/share` folders, the user's installation first.
    export_roots: Vec<PathBuf>,
    running: Arc<RunningApps>,
}

impl FlatpakSource {
    pub fn new(host: Arc<dyn FlatpakHost>) -> Result<Self, SourceInitializationError> {
        let descriptor = SourceDescriptor::new(
            SourceId::new(FLATPAK_SOURCE_ID)?,
            FLATPAK_DISPLAY_NAME,
            vec![
                SourceCapability::Launch,
                SourceCapability::RuntimeObservation,
                SourceCapability::Artwork,
            ],
        )?;
        Ok(Self {
            descriptor,
            export_roots: default_export_roots(),
            running: Arc::new(RunningApps::new(host)),
        })
    }

    #[cfg(test)]
    fn with_roots(
        roots: Vec<PathBuf>,
        host: Arc<dyn FlatpakHost>,
    ) -> Result<Self, SourceInitializationError> {
        let mut source = Self::new(host)?;
        source.export_roots = roots;
        Ok(source)
    }

    fn checked_app_id(external_id: &ExternalGameId) -> Result<&str, SourceError> {
        let app_id = external_id.as_str();
        if valid_app_id(app_id) {
            Ok(app_id)
        } else {
            Err(SourceError::new(FlatpakSourceError::InvalidAppId(
                app_id.to_owned(),
            )))
        }
    }

    /// The game's exported desktop entry, from the first root that has one.
    fn desktop_game(&self, app_id: &str) -> Option<DesktopGame> {
        self.export_roots.iter().find_map(|root| {
            discover_root(root)
                .ok()?
                .games
                .into_iter()
                .find(|game| game.app_id == app_id)
        })
    }
}

impl GameSource for FlatpakSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    fn discover(&self) -> Result<SourceDiscovery, SourceError> {
        let roots = self
            .export_roots
            .iter()
            .filter(|root| root.is_dir())
            .collect::<Vec<_>>();
        if roots.is_empty() {
            return Ok(SourceDiscovery::Unavailable(
                SourceUnavailableReason::NotInstalled,
            ));
        }

        let mut merged = BTreeMap::<String, DesktopGame>::new();
        let mut complete = true;
        let mut first_error = None;
        let mut read_any = false;
        for root in roots {
            match discover_root(root) {
                Ok(found) => {
                    read_any = true;
                    complete &= found.complete;
                    for game in found.games {
                        // The user's installation comes first and wins.
                        merged.entry(game.app_id.clone()).or_insert(game);
                    }
                }
                Err(error) => {
                    complete = false;
                    warn!(root = %root.display(), %error, "Flatpak exports could not be read");
                    first_error.get_or_insert(error);
                }
            }
        }
        if !read_any && let Some(error) = first_error {
            return Err(SourceError::new(error));
        }

        let mut games = Vec::with_capacity(merged.len());
        for game in merged.into_values() {
            let external_id = ExternalGameId::new(game.app_id)
                .map_err(FlatpakSourceError::from)
                .map_err(SourceError::new)?;
            let title = GameTitle::new(game.title)
                .map_err(FlatpakSourceError::from)
                .map_err(SourceError::new)?;
            games.push(SourceGame::new(external_id, title));
        }
        games.sort_by(|left, right| {
            left.title()
                .as_str()
                .to_lowercase()
                .cmp(&right.title().as_str().to_lowercase())
                .then_with(|| {
                    left.external_id()
                        .as_str()
                        .cmp(right.external_id().as_str())
                })
        });

        // An entry that couldn't be read (an app being installed or removed,
        // or a missing grant) may still be an installed game: keep it.
        let snapshot = if complete {
            SourceSnapshot::authoritative(games)
        } else {
            SourceSnapshot::new(games)
        }
        .map_err(FlatpakSourceError::from)
        .map_err(SourceError::new)?;
        Ok(SourceDiscovery::Available(snapshot))
    }

    fn launch_target(
        &self,
        external_id: &ExternalGameId,
    ) -> Result<Option<SourceLaunchTarget>, SourceError> {
        let app_id = Self::checked_app_id(external_id)?;
        Ok(Some(SourceLaunchTarget::FlatpakApp(app_id.to_owned())))
    }

    /// Whether `flatpak ps` lists the game. The answer comes from a list a
    /// background thread keeps current while it's being asked for, so this
    /// never waits for the host (it's called on the UI thread).
    fn runtime_state(
        &self,
        external_id: &ExternalGameId,
    ) -> Result<Option<SourceRuntimeState>, SourceError> {
        let app_id = Self::checked_app_id(external_id)?;
        match RunningApps::ask(&self.running) {
            // Not known yet: not running yet, as far as anyone can tell.
            None => Ok(Some(SourceRuntimeState::Stopped)),
            Some(Ok(apps)) => Ok(Some(if apps.contains(app_id) {
                SourceRuntimeState::Running
            } else {
                SourceRuntimeState::Stopped
            })),
            Some(Err(error)) => Err(SourceError::new(FlatpakSourceError::Running(error))),
        }
    }

    fn artwork_candidates(
        &self,
        external_id: &ExternalGameId,
    ) -> Result<Vec<SourceArtworkCandidate>, SourceError> {
        let app_id = Self::checked_app_id(external_id)?;
        let Some(icon) = self.desktop_game(app_id).and_then(|game| game.icon) else {
            return Ok(vec![]);
        };
        Ok(self
            .export_roots
            .iter()
            .flat_map(|root| icon_paths(root, &icon))
            .filter(|path| path.is_file())
            .map(SourceArtworkCandidate::local_square_icon)
            .collect())
    }
}

/// The user's and the system's exports. Inside Horizon's sandbox HOME is the
/// host's, and both are granted read-only at their host paths.
fn default_export_roots() -> Vec<PathBuf> {
    let user_data = if env::var_os("FLATPAK_ID").is_some() {
        env::var_os("HOST_XDG_DATA_HOME").map(PathBuf::from)
    } else {
        env::var_os("XDG_DATA_HOME").map(PathBuf::from)
    }
    .or_else(|| home_dir().map(|home| home.join(".local/share")));
    dedupe_paths(
        user_data
            .map(|data| data.join("flatpak/exports/share"))
            .into_iter()
            .chain([PathBuf::from(SYSTEM_EXPORTS)]),
    )
}

fn discover_root(root: &Path) -> Result<RootDiscovery, FlatpakSourceError> {
    let folder = root.join("applications");
    let entries = match fs::read_dir(&folder) {
        Ok(entries) => entries,
        // An installation with no apps exported.
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(RootDiscovery {
                games: vec![],
                complete: true,
            });
        }
        Err(source) => {
            return Err(FlatpakSourceError::Read {
                path: folder,
                source,
            });
        }
    };

    let mut complete = true;
    let mut by_app = BTreeMap::<String, Vec<(String, DesktopGame)>>::new();
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(error) => {
                warn!(folder = %folder.display(), %error, "Flatpak exports listing was cut short");
                complete = false;
                continue;
            }
        };
        let Some(stem) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".desktop"))
        else {
            continue;
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                debug!(path = %path.display(), %error, "Flatpak desktop entry could not be read");
                complete = false;
                continue;
            }
        };
        if let Some(game) = parse_desktop_game(&text, stem) {
            by_app
                .entry(game.app_id.clone())
                .or_default()
                .push((stem.to_owned(), game));
        }
    }

    // An app can export several entries; its main one (named for the app)
    // is the game, else the first by name.
    let games = by_app
        .into_iter()
        .map(|(app_id, mut entries)| {
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            let main = entries.iter().position(|(stem, _)| *stem == app_id);
            entries.swap_remove(main.unwrap_or(0)).1
        })
        .collect();
    Ok(RootDiscovery { games, complete })
}

/// The game in an exported desktop entry, or `None` when the entry isn't a
/// shown application in the Game category, or is one of [`NOT_GAMES`].
fn parse_desktop_game(text: &str, file_stem: &str) -> Option<DesktopGame> {
    let mut in_entry = false;
    let mut values = BTreeMap::<&str, &str>::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            // The first value wins, as the spec reads it.
            values.entry(key.trim()).or_insert(value.trim());
        }
    }

    let flag = |key: &str| values.get(key).is_some_and(|value| *value == "true");
    let is_game = values
        .get("Categories")
        .is_some_and(|categories| categories.split(';').any(|category| category == "Game"));
    if values.get("Type") != Some(&"Application") || flag("NoDisplay") || flag("Hidden") || !is_game
    {
        return None;
    }

    // Flatpak marks every exported entry with its app; an entry without the
    // mark is named for it.
    let app_id = values.get("X-Flatpak").copied().unwrap_or(file_stem);
    if !valid_app_id(app_id) || app_id == OWN_APP_ID || NOT_GAMES.contains(&app_id) {
        return None;
    }
    let title = unescape(values.get("Name")?);
    if title.trim().is_empty() {
        return None;
    }
    let icon = values
        .get("Icon")
        .filter(|icon| !icon.is_empty() && !icon.contains('/'))
        .map(|icon| (*icon).to_owned());
    Some(DesktopGame {
        app_id: app_id.to_owned(),
        title: title.trim().to_owned(),
        icon,
    })
}

/// A desktop entry string's escapes.
fn unescape(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => text.push(' '),
            Some('n' | 't' | 'r') => text.push(' '),
            Some(other) => text.push(other),
            None => {}
        }
    }
    text
}

/// Exported PNG icons for `icon`, largest first. Flatpak exports hicolor
/// icons in square size folders; the artwork service checks each anyway.
fn icon_paths(root: &Path, icon: &str) -> Vec<PathBuf> {
    const SIZES: &[u32] = &[512, 256, 192, 128, 96, 72, 64, 48];
    SIZES
        .iter()
        .map(|size| {
            root.join("icons/hicolor")
                .join(format!("{size}x{size}"))
                .join("apps")
                .join(format!("{icon}.png"))
        })
        .collect()
}

/// What's running, kept current by a background thread for as long as it's
/// being asked (a session being watched), then left alone.
struct RunningApps {
    host: Arc<dyn FlatpakHost>,
    state: Mutex<RunningState>,
}

#[derive(Default)]
struct RunningState {
    apps: Option<Result<BTreeSet<String>, String>>,
    asked: Option<Instant>,
    polling: bool,
}

impl RunningApps {
    fn new(host: Arc<dyn FlatpakHost>) -> Self {
        Self {
            host,
            state: Mutex::new(RunningState::default()),
        }
    }

    /// The latest answer, starting the poller when it isn't running.
    fn ask(this: &Arc<Self>) -> Option<Result<BTreeSet<String>, String>> {
        let mut state = this.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.asked = Some(Instant::now());
        if !state.polling {
            let poller = Arc::clone(this);
            state.polling = std::thread::Builder::new()
                .name("flatpak-running".into())
                .spawn(move || poller.poll())
                .inspect_err(|error| warn!(%error, "Flatpak running-apps thread did not start"))
                .is_ok();
        }
        state.apps.clone()
    }

    fn poll(&self) {
        loop {
            let apps = self.host.running_apps();
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.apps = Some(apps);
            if state
                .asked
                .is_none_or(|asked| asked.elapsed() >= RUNNING_IDLE)
            {
                // Nobody's watching: the next question starts over.
                state.polling = false;
                state.apps = None;
                return;
            }
            drop(state);
            std::thread::sleep(RUNNING_REFRESH);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_ID: AtomicU64 = AtomicU64::new(0);

    struct FakeHost(Result<BTreeSet<String>, String>);

    impl FlatpakHost for FakeHost {
        fn running_apps(&self) -> Result<BTreeSet<String>, String> {
            self.0.clone()
        }
    }

    fn host(running: &[&str]) -> Arc<dyn FlatpakHost> {
        Arc::new(FakeHost(Ok(running
            .iter()
            .map(|id| (*id).to_owned())
            .collect())))
    }

    fn temp_root(name: &str) -> PathBuf {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!(
            "horizon-flatpak-{name}-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("applications")).expect("applications");
        root
    }

    fn entry(name: &str, categories: &str, extra: &str) -> String {
        format!(
            "[Desktop Entry]\nType=Application\nName={name}\nCategories={categories}\nIcon=org.example.Icon\n{extra}\n"
        )
    }

    fn write(root: &Path, file: &str, text: &str) {
        fs::write(root.join("applications").join(file), text).expect("desktop entry");
    }

    fn snapshot(source: &FlatpakSource) -> SourceSnapshot {
        match source.discover().expect("discovery") {
            SourceDiscovery::Available(snapshot) => snapshot,
            other => panic!("expected an available snapshot, got {other:?}"),
        }
    }

    fn ids(snapshot: &SourceSnapshot) -> Vec<&str> {
        snapshot
            .games()
            .iter()
            .map(|game| game.external_id().as_str())
            .collect()
    }

    #[test]
    fn app_ids_are_checked_like_flatpak() {
        assert!(valid_app_id("org.example.Game"));
        assert!(valid_app_id("com.example.my_game-2"));
        assert!(!valid_app_id("org.example"));
        assert!(!valid_app_id("-org.example.Game"));
        assert!(!valid_app_id("org..Game"));
        assert!(!valid_app_id("org.example.Game --command=sh"));
        assert!(!valid_app_id("../../etc.passwd.x"));
    }

    #[test]
    fn only_shown_game_entries_are_games() {
        let game = parse_desktop_game(
            &entry(
                "Super\\sTux",
                "Game;ArcadeGame;",
                "X-Flatpak=org.supertux.SuperTux",
            ),
            "org.supertux.SuperTux",
        )
        .expect("game");
        assert_eq!(game.app_id, "org.supertux.SuperTux");
        assert_eq!(game.title, "Super Tux");
        assert_eq!(game.icon.as_deref(), Some("org.example.Icon"));

        let stem = "org.example.App";
        assert!(parse_desktop_game(&entry("Editor", "Graphics;", ""), stem).is_none());
        // "Game" must be a whole category, not part of one.
        assert!(parse_desktop_game(&entry("Dev", "GameDevelopment;", ""), stem).is_none());
        assert!(parse_desktop_game(&entry("Hidden", "Game;", "NoDisplay=true"), stem).is_none());
        assert!(parse_desktop_game(&entry("Gone", "Game;", "Hidden=true"), stem).is_none());
        assert!(parse_desktop_game(&entry("", "Game;", ""), stem).is_none());
        // Keys outside [Desktop Entry] (actions) don't count.
        let action =
            "[Desktop Entry]\nType=Application\nName=App\n[Desktop Action x]\nCategories=Game;\n";
        assert!(parse_desktop_game(action, stem).is_none());
    }

    #[test]
    fn launchers_tools_and_horizon_are_not_games() {
        for app_id in [
            "com.valvesoftware.Steam",
            "com.heroicgameslauncher.hgl",
            OWN_APP_ID,
        ] {
            let text = entry("Launcher", "Game;", &format!("X-Flatpak={app_id}"));
            assert!(parse_desktop_game(&text, app_id).is_none(), "{app_id}");
        }
    }

    #[test]
    fn discovery_merges_installations_and_prefers_the_main_entry() {
        let user = temp_root("user");
        let system = temp_root("system");
        write(
            &user,
            "org.example.Both.desktop",
            &entry("Both (user)", "Game;", ""),
        );
        write(
            &system,
            "org.example.Both.desktop",
            &entry("Both (system)", "Game;", ""),
        );
        write(
            &system,
            "org.example.Multi.Server.desktop",
            &entry("Multi Server", "Game;", "X-Flatpak=org.example.Multi"),
        );
        write(
            &system,
            "org.example.Multi.desktop",
            &entry("Multi", "Game;", "X-Flatpak=org.example.Multi"),
        );
        write(
            &system,
            "org.example.Paint.desktop",
            &entry("Paint", "Graphics;", ""),
        );
        write(&system, "notes.txt", "not an entry");

        let source = FlatpakSource::with_roots(vec![user, system], host(&[])).expect("source");
        let snapshot = snapshot(&source);
        assert!(snapshot.is_authoritative());
        assert_eq!(
            ids(&snapshot),
            vec!["org.example.Both", "org.example.Multi"]
        );
        assert_eq!(snapshot.games()[0].title().as_str(), "Both (user)");
        assert_eq!(snapshot.games()[1].title().as_str(), "Multi");
    }

    #[test]
    fn missing_exports_are_unavailable_not_empty() {
        let root = env::temp_dir().join("horizon-flatpak-missing-exports");
        let source = FlatpakSource::with_roots(vec![root], host(&[])).expect("source");
        assert_eq!(
            source.discover().expect("discovery"),
            SourceDiscovery::Unavailable(SourceUnavailableReason::NotInstalled)
        );
    }

    #[test]
    fn an_unreadable_entry_keeps_installed_games() {
        let root = temp_root("dangling");
        write(
            &root,
            "org.example.Game.desktop",
            &entry("Game", "Game;", ""),
        );
        // An exported link whose app folder can't be read.
        std::os::unix::fs::symlink(
            root.join("nowhere/org.example.Other.desktop"),
            root.join("applications/org.example.Other.desktop"),
        )
        .expect("link");

        let source = FlatpakSource::with_roots(vec![root], host(&[])).expect("source");
        let snapshot = snapshot(&source);
        assert!(!snapshot.is_authoritative());
        assert_eq!(ids(&snapshot), vec!["org.example.Game"]);
    }

    #[test]
    fn launch_runs_the_checked_app_and_nothing_else() {
        let source = FlatpakSource::with_roots(vec![], host(&[])).expect("source");
        let game = ExternalGameId::new("org.example.Game").expect("id");
        assert_eq!(
            source.launch_target(&game).expect("target"),
            Some(SourceLaunchTarget::FlatpakApp("org.example.Game".into()))
        );
        let bad = ExternalGameId::new("org.example.Game --command=sh").expect("id");
        assert!(source.launch_target(&bad).is_err());
        assert!(source.runtime_state(&bad).is_err());
    }

    #[test]
    fn running_state_follows_flatpak_ps() {
        let source =
            FlatpakSource::with_roots(vec![], host(&["org.example.Game"])).expect("source");
        let game = ExternalGameId::new("org.example.Game").expect("id");
        let other = ExternalGameId::new("org.example.Other").expect("id");
        // Before the first answer, nothing is known to be running.
        let first = source.runtime_state(&game).expect("state");
        assert!(matches!(
            first,
            Some(SourceRuntimeState::Stopped | SourceRuntimeState::Running)
        ));
        let deadline = Instant::now() + Duration::from_secs(5);
        while source.runtime_state(&game).expect("state") != Some(SourceRuntimeState::Running) {
            assert!(Instant::now() < deadline, "poller never answered");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            source.runtime_state(&other).expect("state"),
            Some(SourceRuntimeState::Stopped)
        );
    }

    #[test]
    fn a_failing_host_is_an_error_not_stopped() {
        let host: Arc<dyn FlatpakHost> = Arc::new(FakeHost(Err("no host".into())));
        let source = FlatpakSource::with_roots(vec![], host).expect("source");
        let game = ExternalGameId::new("org.example.Game").expect("id");
        let deadline = Instant::now() + Duration::from_secs(5);
        while source.runtime_state(&game).is_ok() {
            assert!(Instant::now() < deadline, "poller never answered");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn icons_come_from_the_exported_hicolor_theme() {
        let root = temp_root("icons");
        write(
            &root,
            "org.example.Game.desktop",
            &entry("Game", "Game;", ""),
        );
        let icons = root.join("icons/hicolor/256x256/apps");
        fs::create_dir_all(&icons).expect("icons");
        fs::write(icons.join("org.example.Icon.png"), b"png").expect("icon");

        let source = FlatpakSource::with_roots(vec![root], host(&[])).expect("source");
        let game = ExternalGameId::new("org.example.Game").expect("id");
        let candidates = source.artwork_candidates(&game).expect("candidates");
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            source
                .artwork_candidates(&ExternalGameId::new("org.example.None").expect("id"))
                .expect("candidates"),
            vec![]
        );
    }
}
