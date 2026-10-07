use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs,
    io,
    path::{Path, PathBuf},
};

use steam_vdf_parser::{Obj, parse_appinfo, parse_text};
use thiserror::Error;
use tracing::{debug, warn};

use crate::domain::{DomainValidationError, ExternalGameId, GameTitle, SourceId};

use super::{
    GameSource, SourceCapability, SourceDescriptor, SourceDescriptorError, SourceDiscovery,
    SourceError, SourceGame, SourceLaunchTarget, SourceSnapshot, SourceSnapshotError,
    SourceUnavailableReason,
};

const STEAM_SOURCE_ID: &str = "steam";
const STEAM_DISPLAY_NAME: &str = "Steam";

#[derive(Debug, Error)]
pub enum SteamSourceInitError {
    #[error(transparent)]
    Domain(#[from] DomainValidationError),
    #[error(transparent)]
    Descriptor(#[from] SourceDescriptorError),
}

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

pub struct SteamSource {
    descriptor: SourceDescriptor,
    roots: Vec<PathBuf>,
}

impl SteamSource {
    pub fn new() -> Result<Self, SteamSourceInitError> {
        let descriptor = SourceDescriptor::new(
            SourceId::new(STEAM_SOURCE_ID)?,
            STEAM_DISPLAY_NAME,
            vec![SourceCapability::Launch],
        )?;

        Ok(Self {
            descriptor,
            roots: default_steam_roots(),
        })
    }

    #[cfg(test)]
    fn with_roots(roots: Vec<PathBuf>) -> Result<Self, SteamSourceInitError> {
        let mut source = Self::new()?;
        source.roots = roots;
        Ok(source)
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
                present_game_ids.into_iter(),
            )
        } else {
            SourceSnapshot::new(games)
        }
        .map_err(SteamDiscoveryError::from)
        .map_err(SourceError::new)?;
        Ok(SourceDiscovery::Available(snapshot))
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
}
