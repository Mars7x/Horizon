use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};

use steam_vdf_parser::{Obj, parse_appinfo, parse_text};
use thiserror::Error;
use tracing::{debug, warn};
use zip::ZipArchive;

use crate::domain::{DomainValidationError, ExternalGameId, GameTitle, PlaytimeSeconds, SourceId};

use super::{
    GameSource, SourceArtworkCandidate, SourceCapability, SourceDescriptor, SourceDiscovery,
    SourceError, SourceGame, SourceInitializationError, SourceLaunchTarget, SourceRuntimeState,
    SourceSnapshot, SourceSnapshotError, SourceUnavailableReason,
};

const STEAM_SOURCE_ID: &str = "steam";
const STEAM_DISPLAY_NAME: &str = "Steam";

pub type SteamSourceInitError = SourceInitializationError;

#[derive(Debug, Error)]
enum SteamDiscoveryError {
    #[error("failed to read Steam metadata at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse Steam VDF at {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("Steam metadata at {0} did not contain an object root")]
    InvalidRoot(PathBuf),
    #[error("Steam was detected but no installation could be discovered successfully")]
    NoUsableInstallation,
    #[error("Steam app id is not numeric: {0}")]
    InvalidAppId(String),
    #[error("Steam runtime log cache mutex was poisoned")]
    RuntimeLogCachePoisoned,
    #[error(transparent)]
    Domain(#[from] DomainValidationError),
    #[error(transparent)]
    Snapshot(#[from] SourceSnapshotError),
}

#[derive(Debug, Clone)]
struct SteamLibrary {
    path: PathBuf,
    app_ids: BTreeSet<String>,
}

#[derive(Debug)]
struct SteamRootDiscovery {
    games: Vec<SourceGame>,
    present_game_ids: BTreeSet<ExternalGameId>,
}

#[derive(Debug, Clone)]
struct SteamRuntimeLogCache {
    length: u64,
    modified: Option<SystemTime>,
    states: BTreeMap<String, bool>,
}

pub struct SteamSource {
    descriptor: SourceDescriptor,
    roots: Vec<PathBuf>,
    artwork_hashes: Mutex<BTreeMap<String, SteamArtworkHashes>>,
    runtime_logs: Mutex<BTreeMap<PathBuf, SteamRuntimeLogCache>>,
}

impl SteamSource {
    pub fn new() -> Result<Self, SteamSourceInitError> {
        let descriptor = SourceDescriptor::new(
            SourceId::new(STEAM_SOURCE_ID)?,
            STEAM_DISPLAY_NAME,
            vec![
                SourceCapability::Launch,
                SourceCapability::LifetimePlaytime,
                SourceCapability::RuntimeObservation,
                SourceCapability::Artwork,
            ],
        )?;

        Ok(Self {
            descriptor,
            roots: default_steam_roots(),
            artwork_hashes: Mutex::new(BTreeMap::new()),
            runtime_logs: Mutex::new(BTreeMap::new()),
        })
    }

    #[cfg(test)]
    fn with_roots(roots: Vec<PathBuf>) -> Result<Self, SteamSourceInitError> {
        let mut source = Self::new()?;
        source.roots = roots;
        Ok(source)
    }

    fn remember_artwork_hashes(&self, app_id: &str, common: Option<&Obj<'_>>) {
        let Some(common) = common else {
            return;
        };
        let hashes = SteamArtworkHashes::from_common(common);
        if hashes.is_empty() {
            return;
        }

        if let Ok(mut cache) = self.artwork_hashes.lock() {
            cache.insert(app_id.to_owned(), hashes);
        }
    }

    fn cached_artwork_hashes(&self, app_id: &str) -> SteamArtworkHashes {
        self.artwork_hashes
            .lock()
            .ok()
            .and_then(|cache| cache.get(app_id).cloned())
            .unwrap_or_default()
    }

    fn gameprocess_log_state(
        &self,
        path: &Path,
        app_id: &str,
    ) -> Result<bool, SteamDiscoveryError> {
        let metadata = fs::metadata(path).map_err(|source| SteamDiscoveryError::Read {
            path: path.to_owned(),
            source,
        })?;
        let modified = metadata.modified().ok();
        let length = metadata.len();

        let mut cache = self
            .runtime_logs
            .lock()
            .map_err(|_| SteamDiscoveryError::RuntimeLogCachePoisoned)?;
        let refresh = cache.get(path).is_none_or(|entry| {
            entry.length != length || entry.modified != modified
        });

        if refresh {
            let content = fs::read_to_string(path).map_err(|source| SteamDiscoveryError::Read {
                path: path.to_owned(),
                source,
            })?;
            cache.insert(
                path.to_owned(),
                SteamRuntimeLogCache {
                    length,
                    modified,
                    states: parse_gameprocess_running_states(&content),
                },
            );
        }

        Ok(cache
            .get(path)
            .and_then(|entry| entry.states.get(app_id))
            .copied()
            .unwrap_or(false))
    }

    fn discover_root(&self, root: &Path) -> Result<SteamRootDiscovery, SteamDiscoveryError> {
        let libraries = read_libraries(root)?;
        let mut installed_ids = BTreeSet::new();
        for library in &libraries {
            installed_ids.extend(library.app_ids.iter().cloned());
        }

        if installed_ids.is_empty() {
            return Ok(SteamRootDiscovery {
                games: vec![],
                present_game_ids: BTreeSet::new(),
            });
        }

        // Manifests are the most direct record of installed titles when the
        // corresponding Steam library is readable. Load them before appinfo so
        // a cache-format/read failure cannot erase otherwise usable games.
        let manifest_names = read_manifest_names(&libraries);

        // appinfo enriches discovery with type filtering and supplies names for
        // libraries whose manifests are outside Horizon's narrow Flatpak
        // permissions. It is useful metadata, but it is not a prerequisite for
        // using valid manifests that are already readable.
        let appinfo_path = root.join("appcache/appinfo.vdf");
        let appinfo_bytes = match fs::read(&appinfo_path) {
            Ok(bytes) => Some(bytes),
            Err(source) => {
                let error = SteamDiscoveryError::Read {
                    path: appinfo_path.clone(),
                    source,
                };
                warn!(root = %root.display(), %error, "Steam appinfo unavailable; using readable manifests as fallback");
                None
            }
        };
        let appinfo = appinfo_bytes.as_deref().and_then(|bytes| match parse_appinfo(bytes) {
            Ok(appinfo) => Some(appinfo),
            Err(error) => {
                let error = SteamDiscoveryError::Parse {
                    path: appinfo_path.clone(),
                    message: error.to_string(),
                };
                warn!(root = %root.display(), %error, "Steam appinfo could not be parsed; using readable manifests as fallback");
                None
            }
        });
        let appinfo_root = appinfo.as_ref().and_then(|appinfo| match appinfo.as_obj() {
            Some(root) => Some(root),
            None => {
                let error = SteamDiscoveryError::InvalidRoot(appinfo_path.clone());
                warn!(root = %root.display(), %error, "Steam appinfo root was unusable; using readable manifests as fallback");
                None
            }
        });

        // Membership and display metadata are separate. An installed ID that
        // lacks a fresh title remains present for reconciliation so Horizon can
        // prune truly uninstalled games without deleting temporarily unresolved
        // installed ones. Explicit non-game appinfo types are excluded.
        let mut present_game_ids = BTreeSet::new();

        debug!(
            root = %root.display(),
            installed = installed_ids.len(),
            manifest_titles = manifest_names.len(),
            appinfo_available = appinfo_root.is_some(),
            "Steam local metadata loaded"
        );

        let mut games = BTreeMap::<String, GameTitle>::new();

        for app_id in installed_ids {
            if app_id.parse::<u32>().is_err() {
                return Err(SteamDiscoveryError::InvalidAppId(app_id));
            }

            let common = appinfo_root
                .and_then(|root| root.get(&app_id))
                .and_then(|value| value.as_obj())
                .and_then(appinfo_common);
            self.remember_artwork_hashes(&app_id, common);
            let app_type = common
                .and_then(|common| common.get("type"))
                .and_then(|value| value.as_str());
            if app_type.is_some_and(|value| !value.eq_ignore_ascii_case("game")) {
                continue;
            }

            let external_id = ExternalGameId::new(app_id.clone())?;
            present_game_ids.insert(external_id.clone());

            let appinfo_name = common
                .and_then(|common| common.get("name"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty());

            let title = if let Some(name) = appinfo_name {
                Some(GameTitle::new(name)?)
            } else {
                manifest_names.get(&app_id).cloned()
            };

            let Some(title) = title else {
                warn!(
                    %app_id,
                    root = %root.display(),
                    "installed Steam app has no usable local title; skipping it"
                );
                continue;
            };

            games.insert(app_id, title);
        }

        let mut games = games
            .into_iter()
            .map(|(app_id, title)| {
                Ok(SourceGame::new(ExternalGameId::new(app_id)?, title))
            })
            .collect::<Result<Vec<_>, SteamDiscoveryError>>()?;
        games.sort_by(|left, right| {
            left.title()
                .as_str()
                .to_lowercase()
                .cmp(&right.title().as_str().to_lowercase())
                .then_with(|| left.external_id().as_str().cmp(right.external_id().as_str()))
        });
        Ok(SteamRootDiscovery {
            games,
            present_game_ids,
        })
    }
}

impl GameSource for SteamSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    fn discover(&self) -> Result<SourceDiscovery, SourceError> {
        let roots = self
            .roots
            .iter()
            .filter(|root| looks_like_steam_root(root))
            .collect::<Vec<_>>();
        if roots.is_empty() {
            return Ok(SourceDiscovery::Unavailable(SourceUnavailableReason::NotInstalled));
        }

        let mut merged = BTreeMap::<String, SourceGame>::new();
        let mut first_error: Option<SteamDiscoveryError> = None;
        let mut successful_roots = 0usize;
        let mut all_roots_succeeded = true;
        let mut present_game_ids = BTreeSet::new();

        for root in roots {
            match self.discover_root(root) {
                Ok(discovery) => {
                    successful_roots += 1;
                    present_game_ids.extend(discovery.present_game_ids);
                    for game in discovery.games {
                        merged
                            .entry(game.external_id().as_str().to_owned())
                            .or_insert(game);
                    }
                }
                Err(error) => {
                    all_roots_succeeded = false;
                    warn!(root = %root.display(), %error, "Steam installation could not be discovered");
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }

        if successful_roots == 0 {
            return Err(SourceError::new(
                first_error.unwrap_or(SteamDiscoveryError::NoUsableInstallation),
            ));
        }

        let mut games = merged.into_values().collect::<Vec<_>>();
        games.sort_by(|left, right| {
            left.title()
                .as_str()
                .to_lowercase()
                .cmp(&right.title().as_str().to_lowercase())
                .then_with(|| left.external_id().as_str().cmp(right.external_id().as_str()))
        });

        let snapshot = if all_roots_succeeded {
            SourceSnapshot::authoritative_with_membership(
                games,
                present_game_ids,
            )
        } else {
            SourceSnapshot::new(games)
        }
        .map_err(SteamDiscoveryError::from)
        .map_err(SourceError::new)?;
        Ok(SourceDiscovery::Available(snapshot))
    }

    /// Steam's own cumulative playtime, read from the active account's local
    /// `localconfig.vdf`. No Steam login or network access is involved. The
    /// value is provider-reported and is never combined with observed sessions.
    fn lifetime_playtime_snapshot(
        &self,
    ) -> Result<Vec<(ExternalGameId, PlaytimeSeconds)>, SourceError> {
        let mut reported = BTreeMap::<String, PlaytimeSeconds>::new();
        let mut seen_paths = BTreeSet::new();
        let mut selected_account: Option<String> = None;
        for root in self.roots.iter().filter(|root| looks_like_steam_root(root)) {
            let Some(path) = active_localconfig_path(root) else {
                continue;
            };
            // Conventional Steam roots often alias one installation through
            // symlinks. Do not read the same account cache more than once.
            let identity = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if !seen_paths.insert(identity) {
                continue;
            }

            let Some(account) = path
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
            else {
                continue;
            };
            // Lifetime playtime is per Steam account. Two distinct clients
            // may have different active accounts; never merge those totals.
            if selected_account.as_deref().is_some_and(|selected| selected != account) {
                warn!(path = %path.display(), account, "Steam root uses a different account; skipping its lifetime playtime");
                continue;
            }
            let content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(error) => {
                    warn!(path = %path.display(), %error, "cannot read Steam lifetime playtime; keeping previous data");
                    continue;
                }
            };
            match parse_localconfig_playtime(&content) {
                Ok(values) => {
                    debug!(path = %path.display(), games = values.len(), "read Steam lifetime playtime");
                    // Do not lock the account to a corrupt or unreadable root.
                    if selected_account.is_none() {
                        selected_account = Some(account.to_owned());
                    }
                    // The same app can appear under native and Flatpak Steam.
                    // First root wins, matching discovery precedence; never sum.
                    for (app_id, seconds) in values {
                        reported.entry(app_id).or_insert(seconds);
                    }
                }
                Err(message) => {
                    warn!(path = %path.display(), %message, "invalid Steam localconfig; keeping previous data");
                }
            }
        }

        reported
            .into_iter()
            .map(|(app_id, seconds)| {
                ExternalGameId::new(app_id)
                    .map(|id| (id, seconds))
                    .map_err(|error| SourceError::new(SteamDiscoveryError::from(error)))
            })
            .collect()
    }

    fn launch_target(
        &self,
        external_id: &ExternalGameId,
    ) -> Result<Option<SourceLaunchTarget>, SourceError> {
        if external_id.as_str().parse::<u32>().is_err() {
            return Err(SourceError::new(SteamDiscoveryError::InvalidAppId(
                external_id.as_str().to_owned(),
            )));
        }

        Ok(Some(SourceLaunchTarget::Uri(format!(
            "steam://rungameid/{}",
            external_id.as_str()
        ))))
    }

    fn runtime_state(
        &self,
        external_id: &ExternalGameId,
    ) -> Result<Option<SourceRuntimeState>, SourceError> {
        let app_id = external_id.as_str();
        if app_id.parse::<u32>().is_err() {
            return Err(SourceError::new(SteamDiscoveryError::InvalidAppId(
                app_id.to_owned(),
            )));
        }

        let mut found_log = false;
        let mut first_error = None;

        for root in self.roots.iter().filter(|root| looks_like_steam_root(root)) {
            let log_path = root.join("logs/gameprocess_log.txt");
            if !log_path.is_file() {
                continue;
            }
            found_log = true;

            match self.gameprocess_log_state(&log_path, app_id) {
                Ok(true) => return Ok(Some(SourceRuntimeState::Running)),
                Ok(false) => {}
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }

        if !found_log {
            return Ok(None);
        }
        if let Some(error) = first_error {
            return Err(SourceError::new(error));
        }

        Ok(Some(SourceRuntimeState::Stopped))
    }

    fn external_artwork_id(&self, external_id: &ExternalGameId) -> Option<super::ExternalArtworkId> {
        external_id.as_str().parse::<u32>().ok()
            .filter(|id| *id > 0)
            .map(super::ExternalArtworkId::SteamAppId)
    }

    fn artwork_candidates(
        &self,
        external_id: &ExternalGameId,
    ) -> Result<Vec<SourceArtworkCandidate>, SourceError> {
        let app_id = external_id.as_str();
        if app_id.parse::<u32>().is_err() {
            return Err(SourceError::new(SteamDiscoveryError::InvalidAppId(
                app_id.to_owned(),
            )));
        }

        let mut candidates = Vec::new();
        let mut seen = BTreeSet::new();
        let hashes = self.cached_artwork_hashes(app_id);
        for root in self.roots.iter().filter(|root| looks_like_steam_root(root)) {
            if let Some(hash) = &hashes.linux_client_icon {
                let archive_path = root.join("steam/games").join(format!("{hash}.zip"));
                if archive_path.is_file() && seen.insert(archive_path.clone())
                    && let Some(candidate) =
                        steam_linux_icon_archive_candidate(&archive_path)
                    {
                        candidates.push(candidate);
                    }
            }

            if let Some(hash) = &hashes.client_icon {
                let ico_path = root.join("steam/games").join(format!("{hash}.ico"));
                if ico_path.is_file() && seen.insert(ico_path.clone())
                    && let Some(candidate) = steam_client_icon_candidate(&ico_path) {
                    candidates.push(candidate);
                }
            }

            for path in steam_artwork_paths(root, app_id, &hashes) {
                if path.is_file() && seen.insert(path.clone()) {
                    candidates.push(SourceArtworkCandidate::local_square_icon(path));
                }
            }
        }

        Ok(candidates)
    }
}

#[derive(Debug, Clone, Default)]
struct SteamArtworkHashes {
    app_icon: Option<String>,
    client_icon: Option<String>,
    linux_client_icon: Option<String>,
}

impl SteamArtworkHashes {
    fn from_common(common: &Obj<'_>) -> Self {
        Self {
            app_icon: artwork_hash(common, "icon"),
            client_icon: artwork_hash(common, "clienticon"),
            linux_client_icon: artwork_hash(common, "linuxclienticon"),
        }
    }

    fn is_empty(&self) -> bool {
        self.app_icon.is_none()
            && self.client_icon.is_none()
            && self.linux_client_icon.is_none()
    }
}

fn artwork_hash(common: &Obj<'_>, key: &str) -> Option<String> {
    common
        .get(key)
        .and_then(|value| value.as_str())
        .filter(|hash| is_sha1_hash(hash))
        .map(ToOwned::to_owned)
}

fn steam_artwork_paths(
    root: &Path,
    app_id: &str,
    hashes: &SteamArtworkHashes,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    let library_cache = root.join("appcache/librarycache");
    let nested = library_cache.join(app_id);
    if let Some(hash) = &hashes.app_icon {
        paths.push(nested.join(format!("{hash}.jpg")));
        paths.push(nested.join(format!("{hash}.png")));
    }

    // Steam has used both the legacy flat filename and newer per-AppID cache
    // layouts. Keep these local-only fallbacks so artwork still works when
    // appinfo is temporarily unavailable or a client version omits the hash.
    paths.push(library_cache.join(format!("{app_id}_icon.jpg")));
    paths.push(library_cache.join(format!("{app_id}_icon.png")));
    paths.push(nested.join("icon.jpg"));
    paths.push(nested.join("icon.png"));

    paths
}

const MAX_STEAM_ICON_ARCHIVE_ENTRY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_STEAM_ICON_DIMENSION: u32 = 4096;

fn steam_linux_icon_archive_candidate(path: &Path) -> Option<SourceArtworkCandidate> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            debug!(
                path = %path.display(),
                %error,
                "optional Steam Linux client icon archive could not be opened"
            );
            return None;
        }
    };
    let mut archive = match ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(error) => {
            debug!(
                path = %path.display(),
                %error,
                "optional Steam Linux client icon archive could not be parsed"
            );
            return None;
        }
    };

    let mut best: Option<(u32, Vec<u8>)> = None;
    for index in 0..archive.len() {
        let mut entry = match archive.by_index(index) {
            Ok(entry) => entry,
            Err(error) => {
                debug!(
                    path = %path.display(),
                    index,
                    %error,
                    "Steam Linux client icon archive entry could not be read"
                );
                continue;
            }
        };

        if !entry.is_file()
            || !entry.name().to_ascii_lowercase().ends_with(".png")
            || entry.size() > MAX_STEAM_ICON_ARCHIVE_ENTRY_BYTES
        {
            continue;
        }

        let mut bytes = Vec::with_capacity(entry.size() as usize);
        if let Err(error) = entry.read_to_end(&mut bytes) {
            debug!(
                path = %path.display(),
                entry = entry.name(),
                %error,
                "Steam Linux client icon PNG could not be read"
            );
            continue;
        }

        let Some(side) = square_png_side(&bytes) else {
            debug!(
                path = %path.display(),
                entry = entry.name(),
                "ignoring Steam Linux client icon entry that is invalid or not 1:1"
            );
            continue;
        };

        if best.as_ref().is_none_or(|(best_side, _)| side > *best_side) {
            best = Some((side, bytes));
        }
    }

    best.map(|(_, bytes)| SourceArtworkCandidate::in_memory_square_icon(bytes))
}

fn steam_client_icon_candidate(path: &Path) -> Option<SourceArtworkCandidate> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            debug!(
                path = %path.display(),
                %error,
                "optional Steam client ICO could not be opened"
            );
            return None;
        }
    };
    let icon_dir = match ico::IconDir::read(file) {
        Ok(icon_dir) => icon_dir,
        Err(error) => {
            debug!(
                path = %path.display(),
                %error,
                "optional Steam client ICO could not be parsed"
            );
            return None;
        }
    };

    // ICO is a container. Do not let a generic decoder implicitly choose a
    // frame. Decode every usable square representation and keep the largest
    // one explicitly. Color depth breaks ties at the same resolution.
    let mut best: Option<(u32, u16, Vec<u8>)> = None;
    for entry in icon_dir.entries() {
        let width = entry.width();
        let height = entry.height();
        if width == 0
            || width != height
            || width > MAX_STEAM_ICON_DIMENSION
        {
            continue;
        }

        let decoded = match entry.decode() {
            Ok(decoded) => decoded,
            Err(error) => {
                debug!(
                    path = %path.display(),
                    width,
                    height,
                    %error,
                    "Steam client ICO frame could not be decoded"
                );
                continue;
            }
        };

        if decoded.width() != decoded.height()
            || decoded.width() == 0
            || decoded.width() > MAX_STEAM_ICON_DIMENSION
        {
            continue;
        }

        let mut png = Vec::new();
        if let Err(error) = decoded.write_png(&mut png) {
            debug!(
                path = %path.display(),
                width = decoded.width(),
                height = decoded.height(),
                %error,
                "Steam client ICO frame could not be converted to PNG"
            );
            continue;
        }

        let rank = (decoded.width(), entry.bits_per_pixel());
        if best
            .as_ref()
            .is_none_or(|(best_side, best_depth, _)| rank > (*best_side, *best_depth))
        {
            best = Some((rank.0, rank.1, png));
        }
    }

    best.map(|(_, _, bytes)| SourceArtworkCandidate::in_memory_square_icon(bytes))
}

fn square_png_side(bytes: &[u8]) -> Option<u32> {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?;
    let width = image.width();
    let height = image.height();
    (width > 0 && width == height && width <= MAX_STEAM_ICON_DIMENSION).then_some(width)
}

fn is_sha1_hash(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn parse_gameprocess_running_states(content: &str) -> BTreeMap<String, bool> {
    let mut states = BTreeMap::new();

    for line in content.lines() {
        if let Some(app_id) = gameprocess_added_app_id(line) {
            states.insert(app_id.to_owned(), true);
            continue;
        }
        if let Some(app_id) = gameprocess_removed_app_id(line) {
            states.insert(app_id.to_owned(), false);
        }
    }

    states
}

fn gameprocess_added_app_id(line: &str) -> Option<&str> {
    let marker = "AppID ";
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let end = rest.find(char::is_whitespace)?;
    let app_id = &rest[..end];
    if !app_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    rest[end..]
        .contains(" adding PID ")
        .then_some(app_id)
}

fn gameprocess_removed_app_id(line: &str) -> Option<&str> {
    let marker = "Remove ";
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let suffix = " from running list";
    let end = rest.find(suffix)?;
    let app_id = rest[..end].trim();
    (!app_id.is_empty() && app_id.bytes().all(|byte| byte.is_ascii_digit())).then_some(app_id)
}

fn appinfo_common<'a, 'text>(app_obj: &'a Obj<'text>) -> Option<&'a Obj<'text>> {
    app_obj
        .get("common")
        .and_then(|value| value.as_obj())
        .or_else(|| {
            app_obj
                .get("appinfo")
                .and_then(|value| value.as_obj())
                .and_then(|appinfo| appinfo.get("common"))
                .and_then(|value| value.as_obj())
        })
        // Keep parser representation details inside the adapter. If a parser
        // version wraps the payload one level differently, accept any direct
        // child object that contains the canonical Steam `common` object.
        .or_else(|| {
            app_obj
                .values()
                .filter_map(|value| value.as_obj())
                .find_map(|nested| nested.get("common").and_then(|value| value.as_obj()))
        })
}

/// Offset between a SteamID64 and the account id used for `userdata/` folders.
const STEAM_ID64_ACCOUNT_BASE: u64 = 76_561_197_960_265_728;

/// Chooses the `userdata/<account>/config/localconfig.vdf` to read. Prefers the
/// account Steam marks `MostRecent` in `loginusers.vdf`; otherwise accepts a
/// lone account. Ambiguity reports nothing rather than guessing.
fn active_localconfig_path(root: &Path) -> Option<PathBuf> {
    let userdata = root.join("userdata");
    let accounts = fs::read_dir(&userdata)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            // Account 0 is Steam's anonymous placeholder folder.
            (name.parse::<u64>().ok()? > 0).then_some(name)
        })
        .collect::<BTreeSet<_>>();

    let recent = fs::read_to_string(root.join("config/loginusers.vdf"))
        .ok()
        .and_then(|content| most_recent_account_id(&content))
        .filter(|account| accounts.contains(account));

    let account = match recent {
        Some(account) => account,
        None if accounts.len() == 1 => accounts.into_iter().next()?,
        None => return None,
    };
    let path = userdata.join(account).join("config/localconfig.vdf");
    path.is_file().then_some(path)
}

fn most_recent_account_id(content: &str) -> Option<String> {
    let vdf = parse_text(content).ok()?;
    let root = vdf.as_obj()?;
    let users = vdf_key_ci(root, "users")
        .and_then(|key| root.get(key.as_str()))
        .and_then(|value| value.as_obj())
        .unwrap_or(root);
    let mut most_recent = None;
    for (steam_id, value) in users.iter() {
        let Some(entry) = value.as_obj() else { continue };
        let Some(key) = vdf_key_ci(entry, "MostRecent") else { continue };
        if entry.get(key.as_str()).and_then(|value| value.as_str()) != Some("1") {
            continue;
        }
        let account = steam_id
            .parse::<u64>()
            .ok()
            .and_then(|id| id.checked_sub(STEAM_ID64_ACCOUNT_BASE))
            .filter(|account| *account != 0);
        let Some(account) = account else { continue };
        // Multiple MostRecent=1 entries are ambiguous. Do not let the VDF's
        // enumeration order pick an arbitrary account.
        if most_recent.replace(account.to_string()).is_some() {
            return None;
        }
    }
    most_recent
}

/// Case-insensitive key lookup; Steam has changed key casing between clients.
fn vdf_key_ci(object: &Obj<'_>, key: &str) -> Option<String> {
    object
        .keys()
        .find(|candidate| candidate.eq_ignore_ascii_case(key))
        .map(|candidate| candidate.to_string())
}

/// Extracts `Playtime` (minutes) per numeric app id from `localconfig.vdf`,
/// normalized to seconds. Zero or malformed entries are skipped.
///
/// `localconfig.vdf` is large and carries unrelated, heavily escaped values, so
/// this uses a small tolerant scanner instead of the strict full-document parser.
fn parse_localconfig_playtime(content: &str) -> Result<Vec<(String, PlaytimeSeconds)>, String> {
    const APPS_PATH: [&str; 5] = ["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"];

    let mut stack: Vec<String> = Vec::new();
    let mut pending_key: Option<String> = None;
    let mut found_apps = false;
    let mut values = BTreeMap::<String, PlaytimeSeconds>::new();

    let mut chars = content.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                let mut text = String::new();
                loop {
                    match chars.next() {
                        Some('\\') => {
                            let escaped = chars.next().ok_or("unterminated escape in Steam localconfig")?;
                            text.push(escaped);
                        }
                        Some('"') => break,
                        None => return Err("unterminated quoted string in Steam localconfig".to_owned()),
                        Some(other) => text.push(other),
                    }
                }
                match pending_key.take() {
                    None => pending_key = Some(text),
                    Some(key) => {
                        let in_app = stack.len() == APPS_PATH.len() + 1
                            && APPS_PATH
                                .iter()
                                .zip(&stack)
                                .all(|(expected, actual)| actual.eq_ignore_ascii_case(expected));
                        if in_app && key.eq_ignore_ascii_case("Playtime") {
                            let app_id = &stack[APPS_PATH.len()];
                            let seconds = text.trim().parse::<i64>().ok()
                                .filter(|minutes| *minutes > 0)
                                .and_then(|minutes| minutes.checked_mul(60))
                                .and_then(|seconds| PlaytimeSeconds::new(seconds).ok());
                            if app_id.parse::<u32>().is_ok()
                                && let Some(seconds) = seconds
                            {
                                values.insert(app_id.clone(), seconds);
                            }
                        }
                    }
                }
            }
            '{' => {
                stack.push(pending_key.take().unwrap_or_default());
                if stack.len() == APPS_PATH.len()
                    && APPS_PATH
                        .iter()
                        .zip(&stack)
                        .all(|(expected, actual)| actual.eq_ignore_ascii_case(expected))
                {
                    found_apps = true;
                }
            }
            '}' => {
                if stack.pop().is_none() {
                    return Err("unexpected closing brace in Steam localconfig".to_owned());
                }
                pending_key = None;
            }
            '/' if chars.peek() == Some(&'/') => {
                for skipped in chars.by_ref() {
                    if skipped == '\n' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    if !found_apps {
        return Err("missing UserLocalConfigStore/Software/Valve/Steam/apps".to_owned());
    }
    // Steam may write the file while Horizon reads it. An incomplete snapshot
    // must not replace previously stored lifetime playtime values.
    if !stack.is_empty() || pending_key.is_some() {
        return Err("incomplete Steam localconfig".to_owned());
    }
    Ok(values.into_iter().collect())
}

fn looks_like_steam_root(root: &Path) -> bool {
    root.join("steamapps").is_dir() || root.join("appcache/appinfo.vdf").is_file()
}

fn default_steam_roots() -> Vec<PathBuf> {
    let Some(home) = env::var_os("HOME").map(PathBuf::from) else {
        return vec![];
    };

    let mut candidates = Vec::new();
    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        let data_home = PathBuf::from(data_home);
        candidates.push(data_home.join("Steam"));
        candidates.push(data_home.join("steam"));
    }
    // Flatpak exposes the host's XDG data directory separately from the
    // sandbox's own XDG_DATA_HOME. Honor it when available so custom host XDG
    // layouts can be discovered without granting broad filesystem access.
    if let Some(data_home) = env::var_os("HOST_XDG_DATA_HOME") {
        let data_home = PathBuf::from(data_home);
        candidates.push(data_home.join("Steam"));
        candidates.push(data_home.join("steam"));
    }

    candidates.extend([
        home.join(".local/share/Steam"),
        home.join(".local/share/steam"),
        home.join(".steam/steam"),
        home.join(".steam/debian-installation"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/steam"),
        home.join(".var/app/com.valvesoftware.Steam/data/Steam"),
        home.join(".var/app/com.valvesoftware.Steam/data/steam"),
    ]);

    let mut seen = BTreeSet::new();
    candidates
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn read_libraries(root: &Path) -> Result<Vec<SteamLibrary>, SteamDiscoveryError> {
    let library_file = root.join("steamapps/libraryfolders.vdf");
    match fs::read_to_string(&library_file) {
        Ok(content) => parse_libraryfolders(root, &library_file, &content),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let app_ids = scan_manifest_ids(&root.join("steamapps"))?;
            Ok(vec![SteamLibrary {
                path: root.to_owned(),
                app_ids,
            }])
        }
        Err(source) => Err(SteamDiscoveryError::Read {
            path: library_file,
            source,
        }),
    }
}

fn parse_libraryfolders(
    root: &Path,
    path: &Path,
    content: &str,
) -> Result<Vec<SteamLibrary>, SteamDiscoveryError> {
    let vdf = parse_text(content).map_err(|error| SteamDiscoveryError::Parse {
        path: path.to_owned(),
        message: error.to_string(),
    })?;
    let object = vdf
        .as_obj()
        .ok_or_else(|| SteamDiscoveryError::InvalidRoot(path.to_owned()))?;

    let mut libraries = Vec::new();
    for (key, value) in object.iter() {
        if key.parse::<u32>().is_err() {
            continue;
        }

        if let Some(library_path) = value.as_str() {
            let library_path = normalize_library_path(root, PathBuf::from(library_path));
            let app_ids = scan_manifest_ids(&library_path.join("steamapps"))?;
            libraries.push(SteamLibrary {
                path: library_path,
                app_ids,
            });
            continue;
        }

        let Some(entry) = value.as_obj() else {
            continue;
        };
        let library_path = entry
            .get("path")
            .and_then(|value| value.as_str())
            .map(PathBuf::from)
            .map(|path| normalize_library_path(root, path))
            .unwrap_or_else(|| root.to_owned());

        let declared_app_ids = entry
            .get("apps")
            .and_then(|value| value.as_obj())
            .map(app_ids_from_object)
            .unwrap_or_default();
        let steamapps = library_path.join("steamapps");
        let app_ids = if steamapps.is_dir() {
            // A manifest exists for each locally installed app. Prefer this
            // direct membership record whenever the library is readable so
            // cached/declared entries cannot make non-installed titles appear.
            scan_manifest_ids(&steamapps)?
        } else {
            // External libraries may be outside Horizon's narrow sandbox.
            // Steam's libraryfolders `apps` map is the installed-membership
            // fallback in that case; appinfo remains metadata only.
            declared_app_ids
        };

        libraries.push(SteamLibrary {
            path: library_path,
            app_ids,
        });
    }

    if !libraries.iter().any(|library| library.path == root) {
        libraries.push(SteamLibrary {
            path: root.to_owned(),
            app_ids: scan_manifest_ids(&root.join("steamapps"))?,
        });
    }

    Ok(libraries)
}

fn normalize_library_path(root: &Path, path: PathBuf) -> PathBuf {
    if is_flatpak_steam_root(root)
        && env::var_os("HOME").is_some_and(|home| {
            let home = PathBuf::from(home);
            path == home.join(".local/share/Steam") || path == home.join(".local/share/steam")
        })
    {
        root.to_owned()
    } else {
        path
    }
}

fn is_flatpak_steam_root(root: &Path) -> bool {
    root.components()
        .any(|component| component.as_os_str() == std::ffi::OsStr::new("com.valvesoftware.Steam"))
}

fn app_ids_from_object(object: &Obj<'_>) -> BTreeSet<String> {
    object
        .keys()
        .filter(|app_id| app_id.parse::<u32>().is_ok())
        .map(ToOwned::to_owned)
        .collect()
}

fn scan_manifest_ids(steamapps: &Path) -> Result<BTreeSet<String>, SteamDiscoveryError> {
    let entries = match fs::read_dir(steamapps) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(source) => {
            return Err(SteamDiscoveryError::Read {
                path: steamapps.to_owned(),
                source,
            });
        }
    };

    let mut ids = BTreeSet::new();
    for entry in entries {
        let entry = entry.map_err(|source| SteamDiscoveryError::Read {
            path: steamapps.to_owned(),
            source,
        })?;
        let Some(name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        let Some(app_id) = name
            .strip_prefix("appmanifest_")
            .and_then(|name| name.strip_suffix(".acf"))
        else {
            continue;
        };
        if app_id.parse::<u32>().is_ok() {
            ids.insert(app_id.to_owned());
        }
    }
    Ok(ids)
}

fn read_manifest_names(libraries: &[SteamLibrary]) -> BTreeMap<String, GameTitle> {
    let mut names = BTreeMap::new();
    for library in libraries {
        for app_id in &library.app_ids {
            if names.contains_key(app_id) {
                continue;
            }
            let path = library
                .path
                .join("steamapps")
                .join(format!("appmanifest_{app_id}.acf"));
            match read_manifest_name(&path) {
                Ok(Some(title)) => {
                    names.insert(app_id.clone(), title);
                }
                Ok(None) => {}
                Err(error) => {
                    debug!(%app_id, path = %path.display(), %error, "Steam app manifest could not be used as title fallback");
                }
            }
        }
    }
    names
}

fn read_manifest_name(path: &Path) -> Result<Option<GameTitle>, SteamDiscoveryError> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
            ) =>
        {
            return Ok(None);
        }
        Err(source) => {
            return Err(SteamDiscoveryError::Read {
                path: path.to_owned(),
                source,
            });
        }
    };

    let vdf = parse_text(&content).map_err(|error| SteamDiscoveryError::Parse {
        path: path.to_owned(),
        message: error.to_string(),
    })?;
    let Some(name) = vdf.get_str(&["name"]) else {
        return Ok(None);
    };
    Ok(Some(GameTitle::new(name)?))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "horizon-steam-{name}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temp directory");
        path
    }

    #[test]
    fn libraryfolders_reads_installed_app_ids_without_opening_external_game_directories() {
        let root = temp_dir("libraryfolders");
        fs::create_dir_all(root.join("steamapps")).expect("steamapps");
        let content = r#""libraryfolders"
{
    "0"
    {
        "path" "/home/test/.local/share/Steam"
        "apps"
        {
            "570" "100"
            "730" "200"
        }
    }
    "1"
    {
        "path" "/mnt/games/SteamLibrary"
        "apps"
        {
            "400" "300"
        }
    }
}
"#;
        let file = root.join("steamapps/libraryfolders.vdf");
        let libraries = parse_libraryfolders(&root, &file, content).expect("libraries");
        let ids = libraries
            .iter()
            .flat_map(|library| library.app_ids.iter().map(String::as_str))
            .collect::<BTreeSet<_>>();
        assert_eq!(ids, BTreeSet::from(["400", "570", "730"]));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn readable_library_uses_manifest_membership_instead_of_declared_cache_entries() {
        let root = temp_dir("installed-membership");
        let steamapps = root.join("steamapps");
        fs::create_dir_all(&steamapps).expect("steamapps");
        fs::write(
            steamapps.join("appmanifest_10.acf"),
            r#""AppState"
{
    "appid" "10"
    "name" "Installed"
}
"#,
        )
        .expect("manifest");

        let content = format!(
            r#""libraryfolders"
{{
    "0"
    {{
        "path" "{}"
        "apps"
        {{
            "10" "100"
            "20" "200"
        }}
    }}
}}
"#,
            root.display()
        );
        let file = steamapps.join("libraryfolders.vdf");
        let libraries = parse_libraryfolders(&root, &file, &content).expect("libraries");
        let ids = libraries
            .iter()
            .find(|library| library.path == root)
            .expect("root library")
            .app_ids
            .clone();

        assert_eq!(ids, BTreeSet::from(["10".to_owned()]));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn manifest_fallback_extracts_title() {
        let root = temp_dir("manifest");
        let manifest = root.join("appmanifest_480.acf");
        fs::write(
            &manifest,
            r#""AppState"
{
    "appid" "480"
    "name" "Spacewar"
    "StateFlags" "4"
}
"#,
        )
        .expect("write manifest");

        assert_eq!(
            read_manifest_name(&manifest)
                .expect("manifest")
                .expect("title")
                .as_str(),
            "Spacewar"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn discovery_uses_readable_manifests_when_appinfo_is_missing() {
        let root = temp_dir("manifest-only-discovery");
        let steamapps = root.join("steamapps");
        fs::create_dir_all(&steamapps).expect("steamapps");
        fs::write(
            steamapps.join("appmanifest_480.acf"),
            r#""AppState"
{
    "appid" "480"
    "name" "Spacewar"
    "StateFlags" "4"
}
"#,
        )
        .expect("write manifest");

        let source = SteamSource::with_roots(vec![root.clone()]).expect("source");
        let discovery = source.discover_root(&root).expect("manifest-only discovery");

        assert_eq!(discovery.games.len(), 1);
        assert_eq!(discovery.games[0].external_id().as_str(), "480");
        assert_eq!(discovery.games[0].title().as_str(), "Spacewar");
        assert!(
            discovery
                .present_game_ids
                .contains(&ExternalGameId::new("480").expect("external id"))
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn steam_advertises_runtime_observation_without_managed_session() {
        let source = SteamSource::with_roots(vec![]).expect("source");
        assert!(source.descriptor().supports(SourceCapability::Launch));
        assert!(
            source
                .descriptor()
                .supports(SourceCapability::RuntimeObservation)
        );
        assert!(source.descriptor().supports(SourceCapability::Artwork));
        assert!(
            !source
                .descriptor()
                .supports(SourceCapability::ManagedSession)
        );
    }

    #[test]
    fn steam_advertises_lifetime_playtime() {
        let source = SteamSource::with_roots(vec![]).expect("source");
        assert!(source.descriptor().supports(SourceCapability::LifetimePlaytime));
    }

    const LOCALCONFIG: &str = r#""UserLocalConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "apps"
                {
                    "570" { "LastPlayed" "1700000000" "Playtime" "120" }
                    "730" { "Playtime" "0" }
                    "400" { "LastPlayed" "1" }
                    "abc" { "Playtime" "5" }
                    "220" { "Playtime" "oops" }
                }
            }
        }
    }
}
"#;

    #[test]
    fn localconfig_playtime_converts_minutes_and_skips_invalid_entries() {
        let values = parse_localconfig_playtime(LOCALCONFIG).expect("playtime");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].0, "570");
        assert_eq!(values[0].1.get(), 120 * 60);
    }

    #[test]
    fn localconfig_without_apps_is_an_error_not_zero_playtime() {
        assert!(parse_localconfig_playtime("\"UserLocalConfigStore\" { }").is_err());
    }

    #[test]
    fn localconfig_rejects_incomplete_snapshots() {
        // Steam rewrites this cache while running. Do not commit a truncated
        // snapshot after successfully reading an earlier game's Playtime.
        let truncated = LOCALCONFIG.trim_end().strip_suffix('}').expect("closing brace");
        assert!(parse_localconfig_playtime(truncated).is_err());
        assert!(parse_localconfig_playtime(&format!("{LOCALCONFIG}}}")).is_err());
        assert!(parse_localconfig_playtime(&format!("{LOCALCONFIG}\"Unclosed")).is_err());
    }

    #[test]
    fn localconfig_skips_overflow_and_tolerates_escaped_unrelated_values() {
        let huge = LOCALCONFIG.replace("\"120\"", "\"9223372036854775807\"");
        assert!(parse_localconfig_playtime(&huge).expect("valid snapshot").is_empty());

        let escaped = LOCALCONFIG.replace(
            "\"570\" {",
            "\"Unrelated\" \"C:\\\\Games\\\\One\" // harmless comment\n                    \"570\" {",
        );
        let values = parse_localconfig_playtime(&escaped).expect("tolerant snapshot");
        assert_eq!(values[0].1.get(), 120 * 60);
    }

    fn write_account(root: &Path, account: &str, minutes: u32) {
        let config = root.join("userdata").join(account).join("config");
        fs::create_dir_all(&config).expect("config dir");
        fs::write(
            config.join("localconfig.vdf"),
            LOCALCONFIG.replace("\"120\"", &format!("\"{minutes}\"")),
        )
        .expect("localconfig");
    }

    #[test]
    fn lifetime_snapshot_uses_most_recent_login_account() {
        let root = temp_dir("lifetime-recent");
        fs::create_dir_all(root.join("steamapps")).expect("steamapps");
        fs::create_dir_all(root.join("config")).expect("config");
        write_account(&root, "1000", 10);
        write_account(&root, "2000", 30);
        let id_2000 = STEAM_ID64_ACCOUNT_BASE + 2000;
        let id_1000 = STEAM_ID64_ACCOUNT_BASE + 1000;
        fs::write(
            root.join("config/loginusers.vdf"),
            format!(
                "\"users\" {{ \"{id_1000}\" {{ \"MostRecent\" \"0\" }} \"{id_2000}\" {{ \"MostRecent\" \"1\" }} }}"
            ),
        )
        .expect("loginusers");

        let source = SteamSource::with_roots(vec![root.clone()]).expect("source");
        let snapshot = source.lifetime_playtime_snapshot().expect("snapshot");
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].0.as_str(), "570");
        assert_eq!(snapshot[0].1.get(), 30 * 60);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn lifetime_snapshot_accepts_single_account_and_rejects_ambiguity() {
        let single = temp_dir("lifetime-single");
        fs::create_dir_all(single.join("steamapps")).expect("steamapps");
        write_account(&single, "1000", 10);
        let source = SteamSource::with_roots(vec![single.clone()]).expect("source");
        assert_eq!(source.lifetime_playtime_snapshot().expect("snapshot").len(), 1);

        let ambiguous = temp_dir("lifetime-ambiguous");
        fs::create_dir_all(ambiguous.join("steamapps")).expect("steamapps");
        write_account(&ambiguous, "1000", 10);
        write_account(&ambiguous, "2000", 30);
        let source = SteamSource::with_roots(vec![ambiguous.clone()]).expect("source");
        assert!(source.lifetime_playtime_snapshot().expect("snapshot").is_empty());

        fs::remove_dir_all(single).expect("cleanup");
        fs::remove_dir_all(ambiguous).expect("cleanup");
    }

    #[test]
    fn multiple_most_recent_markers_are_ambiguous() {
        let id_1000 = STEAM_ID64_ACCOUNT_BASE + 1000;
        let id_2000 = STEAM_ID64_ACCOUNT_BASE + 2000;
        let loginusers = format!(
            "\"users\" {{ \"{id_1000}\" {{ \"MostRecent\" \"1\" }} \"{id_2000}\" {{ \"MostRecent\" \"1\" }} }}"
        );
        assert_eq!(most_recent_account_id(&loginusers), None);
    }

    #[test]
    fn lifetime_snapshot_deduplicates_roots_and_does_not_mix_accounts() {
        let first = temp_dir("lifetime-first-root");
        let second = temp_dir("lifetime-other-account");
        fs::create_dir_all(first.join("steamapps")).expect("steamapps");
        fs::create_dir_all(second.join("steamapps")).expect("steamapps");
        write_account(&first, "1000", 10);
        write_account(&second, "2000", 30);

        let source = SteamSource::with_roots(vec![first.clone(), first.clone(), second.clone()])
            .expect("source");
        let snapshot = source.lifetime_playtime_snapshot().expect("snapshot");
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].1.get(), 10 * 60);

        fs::remove_dir_all(first).expect("cleanup");
        fs::remove_dir_all(second).expect("cleanup");
    }

    #[test]
    fn artwork_candidates_include_local_steam_icon_cache_fallbacks() {
        let root = temp_dir("artwork-cache");
        fs::create_dir_all(root.join("steamapps")).expect("steamapps");
        let cache = root.join("appcache/librarycache");
        fs::create_dir_all(&cache).expect("library cache");
        let icon = cache.join("480_icon.jpg");
        fs::write(&icon, b"not decoded by source adapter").expect("icon marker");

        let source = SteamSource::with_roots(vec![root.clone()]).expect("source");
        let candidates = source
            .artwork_candidates(&ExternalGameId::new("480").expect("appid"))
            .expect("artwork candidates");

        assert!(candidates.iter().any(|candidate| {
            matches!(
                candidate.location(),
                crate::sources::SourceArtworkLocation::File(path) if path == &icon
            )
        }));
        fs::remove_dir_all(root).expect("cleanup");
    }


    #[test]
    fn client_ico_chooses_largest_square_frame() {
        let root = temp_dir("multi-resolution-ico");
        fs::create_dir_all(&root).expect("root");
        let path = root.join("client.ico");

        let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
        for side in [32_u32, 64, 256] {
            let pixels = vec![side as u8; (side * side * 4) as usize];
            let image = ico::IconImage::from_rgba_data(side, side, pixels);
            icon_dir.add_entry(ico::IconDirEntry::encode(&image).expect("encode ICO frame"));
        }
        icon_dir
            .write(fs::File::create(&path).expect("create ICO"))
            .expect("write ICO");

        let candidate = steam_client_icon_candidate(&path).expect("ICO candidate");
        let crate::sources::SourceArtworkLocation::Bytes(bytes) = candidate.location() else {
            panic!("ICO candidate must be materialized in memory");
        };
        let image = image::load_from_memory(bytes).expect("decode selected ICO frame PNG");
        assert_eq!(image.width(), 256);
        assert_eq!(image.height(), 256);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn linux_icon_png_selection_prefers_largest_true_square() {
        fn png(width: u32, height: u32) -> Vec<u8> {
            let image = image::RgbaImage::from_pixel(
                width,
                height,
                image::Rgba([20, 40, 60, 255]),
            );
            let mut bytes = Vec::new();
            image::DynamicImage::ImageRgba8(image)
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Png,
                )
                .expect("encode PNG");
            bytes
        }

        let small = png(64, 64);
        let medium = png(184, 184);
        let large = png(512, 512);
        let non_square = png(1024, 512);

        assert_eq!(square_png_side(&small), Some(64));
        assert_eq!(square_png_side(&medium), Some(184));
        assert_eq!(square_png_side(&large), Some(512));
        assert_eq!(square_png_side(&non_square), None);
    }

    #[test]
    fn launch_target_uses_steam_uri_for_numeric_app_id() {
        let source = SteamSource::with_roots(vec![]).expect("source");
        let target = source
            .launch_target(&ExternalGameId::new("570").expect("appid"))
            .expect("target")
            .expect("launch target");
        assert_eq!(
            target,
            SourceLaunchTarget::Uri("steam://rungameid/570".into())
        );
    }

    #[test]
    fn gameprocess_log_tracks_app_until_remove_from_running_list() {
        let log = "[2026-10-07 10:00:00] AppID 1462040 adding PID 100 as a tracked process
\
                   [2026-10-07 10:00:01] AppID 1462040 no longer tracking PID 100, exit code -1
\
                   [2026-10-07 10:00:02] AppID 480 adding PID 200 as a tracked process
";
        let states = parse_gameprocess_running_states(log);
        assert_eq!(states.get("1462040"), Some(&true));
        assert_eq!(states.get("480"), Some(&true));

        let ended = format!("{log}[2026-10-07 10:03:00] Remove 1462040 from running list
");
        let states = parse_gameprocess_running_states(&ended);
        assert_eq!(states.get("1462040"), Some(&false));
        assert_eq!(states.get("480"), Some(&true));
    }

    #[test]
    fn gameprocess_log_app_id_matching_is_exact() {
        let log = "[x] AppID 14620400 adding PID 1 as a tracked process
\
                   [x] Remove 14620400 from running list
";
        let states = parse_gameprocess_running_states(log);
        assert!(!states.contains_key("1462040"));
        assert_eq!(states.get("14620400"), Some(&false));
    }
}
