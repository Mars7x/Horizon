use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::DateTime;
use serde_json::Value;
use thiserror::Error;
use tracing::{debug, warn};

use crate::domain::{DomainValidationError, ExternalGameId, GameTitle, PlaytimeSeconds, SourceId};

use super::{
    GameSource, SourceCapability, SourceDescriptor, SourceDiscovery, SourceError, SourceGame,
    SourceInitializationError, SourceLaunchTarget, SourceRuntimeState, SourceSnapshot, SourceSnapshotError,
    SourceUnavailableReason,
    support::{dedupe_paths, home_dir, host_config_home, host_state_home, percent_encode_component},
};

const HEROIC_SOURCE_ID: &str = "heroic";
const HEROIC_DISPLAY_NAME: &str = "Heroic";
const LEGENDARY_RUNNER: &str = "legendary";
// Heroic's own game launch logs terminate with "End of log" when logging is
// enabled, and its lastPlayed timestamp is written after the launch command
// resolves even when game logging is disabled. This is best-effort session
// observation, NOT a guaranteed game-process lifecycle API.
const HEROIC_LOG_END_MARKER: &str = "============= End of log =============";
const HEROIC_LOG_TAIL_BYTES: u64 = 16 * 1024;
// A killed Heroic process can leave an unclosed log. Bound that failure so a
// stale session never permanently claims the card is Playing. This also means
// an exceptionally long-running session may stop being observable.
const HEROIC_LOG_MAX_IDLE_MS: i64 = 18 * 60 * 60 * 1000;
const HEROIC_LOG_CLOCK_SKEW_MS: i64 = 2_000;
// OpenURI returns asynchronously. Allow a small pre-arm gap for Heroic to
// create its log before the D-Bus runtime observer is armed. Do not interpret
// older orphaned logs as a new launch.
const HEROIC_LOG_ARM_GRACE_MS: i64 = 45_000;

#[derive(Clone, Debug)]
struct HeroicRuntimeStore {
    game_logs: PathBuf,
    timestamp: PathBuf,
}

#[derive(Debug, Error)]
enum HeroicDiscoveryError {
    #[error("failed to read Heroic metadata at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse Heroic JSON at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("Heroic installed metadata at {0} was not a JSON object")]
    InvalidInstalledRoot(PathBuf),
    #[error("Heroic was detected but no installation metadata could be discovered successfully")]
    NoUsableInstallation,
    #[error("Heroic external id has an unsupported runner namespace: {0}")]
    InvalidExternalId(String),
    #[error(transparent)]
    Domain(#[from] DomainValidationError),
    #[error(transparent)]
    Snapshot(#[from] SourceSnapshotError),
}

#[derive(Debug)]
struct HeroicRootDiscovery {
    games: Vec<SourceGame>,
    membership: BTreeSet<ExternalGameId>,
}

pub struct HeroicSource {
    descriptor: SourceDescriptor,
    legendary_roots: Vec<PathBuf>,
    timestamp_paths: Vec<PathBuf>,
    runtime_stores: Vec<HeroicRuntimeStore>,
}

impl HeroicSource {
    pub fn new() -> Result<Self, SourceInitializationError> {
        let descriptor = SourceDescriptor::new(
            SourceId::new(HEROIC_SOURCE_ID)?,
            HEROIC_DISPLAY_NAME,
            vec![
                SourceCapability::Launch,
                SourceCapability::LifetimePlaytime,
                SourceCapability::RuntimeObservation,
            ],
        )?;

        Ok(Self {
            descriptor,
            legendary_roots: default_legendary_roots(),
            timestamp_paths: default_timestamp_paths(),
            runtime_stores: default_runtime_stores(),
        })
    }

    #[cfg(test)]
    fn with_roots(roots: Vec<PathBuf>) -> Result<Self, SourceInitializationError> {
        let mut source = Self::new()?;
        source.legendary_roots = roots;
        source.timestamp_paths = vec![];
        source.runtime_stores = vec![];
        Ok(source)
    }

    fn runtime_state_inner(
        &self,
        external_id: &ExternalGameId,
        armed_ms: Option<i64>,
    ) -> Result<Option<SourceRuntimeState>, SourceError> {
        let Some((runner, app_name)) = external_id.as_str().split_once(':') else {
            return Ok(None);
        };
        if runner != LEGENDARY_RUNNER || !valid_log_identity(app_name) {
            return Ok(None);
        }

        // Choose the newest generation across native and Flatpak stores.
        // An old unfinished native log must not override a newer completed
        // Flatpak launch of the same Epic identity.
        let mut latest: Option<HeroicLogObservation> = None;
        for store in &self.runtime_stores {
            if let Some(observation) = heroic_log_observation(store, app_name)
                .map_err(SourceError::new)?
                && latest.as_ref().is_none_or(|previous| {
                    observation.modified_ms > previous.modified_ms
                        || (observation.modified_ms == previous.modified_ms
                            && !observation.active && previous.active)
                }) {
                    latest = Some(observation);
                }
        }
        let running = latest.is_some_and(|observation| {
            observation.active && armed_ms.is_none_or(|armed| {
                observation.modified_ms >= armed.saturating_sub(HEROIC_LOG_ARM_GRACE_MS)
            })
        });
        Ok(self.legendary_roots.iter().any(|root| root.is_dir())
            .then_some(if running { SourceRuntimeState::Running } else { SourceRuntimeState::Stopped }))
    }

    fn discover_legendary_root(
        &self,
        root: &Path,
    ) -> Result<HeroicRootDiscovery, HeroicDiscoveryError> {
        let installed_path = root.join("installed.json");
        let installed_text = match fs::read_to_string(&installed_path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(HeroicRootDiscovery {
                    games: vec![],
                    membership: BTreeSet::new(),
                });
            }
            Err(source) => {
                return Err(HeroicDiscoveryError::Read {
                    path: installed_path,
                    source,
                });
            }
        };
        let installed: Value = serde_json::from_str(&installed_text).map_err(|source| {
            HeroicDiscoveryError::Json {
                path: installed_path.clone(),
                source,
            }
        })?;
        let installed = installed
            .as_object()
            .ok_or_else(|| HeroicDiscoveryError::InvalidInstalledRoot(installed_path.clone()))?;

        let mut games = Vec::new();
        let mut membership = BTreeSet::new();
        for (app_name, installed_entry) in installed {
            let external_id = heroic_external_id(LEGENDARY_RUNNER, app_name)?;
            membership.insert(external_id.clone());

            let title = title_from_installed_entry(installed_entry)
                .and_then(|title| GameTitle::new(title).ok());
            let title = match title {
                Some(title) => Some(title),
                None => match read_legendary_title(root, app_name) {
                    Ok(title) => title,
                    Err(error) => {
                        warn!(
                            %app_name,
                            root = %root.display(),
                            %error,
                            "Heroic title metadata could not be used; preserving installed membership"
                        );
                        None
                    }
                },
            };

            if let Some(title) = title {
                games.push(SourceGame::new(external_id, title));
            } else {
                debug!(
                    %app_name,
                    root = %root.display(),
                    "Heroic installed Epic game has no usable local title metadata; preserving membership only"
                );
            }
        }

        Ok(HeroicRootDiscovery { games, membership })
    }
}

impl GameSource for HeroicSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    fn discover(&self) -> Result<SourceDiscovery, SourceError> {
        let roots = self
            .legendary_roots
            .iter()
            .filter(|root| root.is_dir())
            .collect::<Vec<_>>();
        if roots.is_empty() {
            return Ok(SourceDiscovery::Unavailable(SourceUnavailableReason::NotInstalled));
        }

        let mut merged = BTreeMap::<String, SourceGame>::new();
        let mut membership = BTreeSet::new();
        let mut successful = 0usize;
        let mut all_succeeded = true;
        let mut first_error = None;

        for root in roots {
            match self.discover_legendary_root(root) {
                Ok(discovery) => {
                    successful += 1;
                    membership.extend(discovery.membership);
                    for game in discovery.games {
                        merged
                            .entry(game.external_id().as_str().to_owned())
                            .or_insert(game);
                    }
                }
                Err(error) => {
                    all_succeeded = false;
                    warn!(
                        root = %root.display(),
                        %error,
                        "Heroic Epic metadata could not be discovered"
                    );
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }

        if successful == 0 {
            return Err(SourceError::new(
                first_error.unwrap_or(HeroicDiscoveryError::NoUsableInstallation),
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

        let snapshot = if all_succeeded {
            SourceSnapshot::authoritative_with_membership(games, membership)
        } else {
            SourceSnapshot::new(games)
        }
        .map_err(HeroicDiscoveryError::from)
        .map_err(SourceError::new)?;

        Ok(SourceDiscovery::Available(snapshot))
    }

    fn runtime_state(&self, external_id: &ExternalGameId) -> Result<Option<SourceRuntimeState>, SourceError> {
        self.runtime_state_inner(external_id, None)
    }

    fn runtime_state_for_observation(
        &self,
        external_id: &ExternalGameId,
        armed_at: SystemTime,
    ) -> Result<Option<SourceRuntimeState>, SourceError> {
        let armed_ms = armed_at
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|time| i64::try_from(time.as_millis()).ok());
        self.runtime_state_inner(external_id, armed_ms)
    }

    fn lifetime_playtime_snapshot(
        &self,
    ) -> Result<Vec<(ExternalGameId, PlaytimeSeconds)>, SourceError> {
        let mut reported = BTreeMap::new();
        for path in &self.timestamp_paths {
            let text = match fs::read_to_string(path) {
                Ok(text) => text,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    warn!(path = %path.display(), %error, "cannot read Heroic lifetime playtime");
                    continue;
                }
            };
            let values = match serde_json::from_str::<Value>(&text) {
                Ok(values) => values,
                Err(error) => {
                    warn!(path = %path.display(), %error, "invalid Heroic lifetime JSON; keeping previous data");
                    continue;
                }
            };
            if !values.is_object() {
                warn!(path = %path.display(), "Heroic lifetime JSON is not an object");
                continue;
            }
            for (app_name, seconds) in parse_timestamp_values(&values) {
                // The same Epic app can occur in both native and Flatpak Heroic
                // stores. Match discovery's native-first precedence; never sum.
                let id = heroic_external_id(LEGENDARY_RUNNER, &app_name)
                    .map_err(SourceError::new)?;
                reported.entry(id).or_insert(seconds);
            }
        }
        Ok(reported.into_iter().collect())
    }

    fn launch_target(
        &self,
        external_id: &ExternalGameId,
    ) -> Result<Option<SourceLaunchTarget>, SourceError> {
        let Some((runner, app_name)) = external_id.as_str().split_once(':') else {
            return Err(SourceError::new(HeroicDiscoveryError::InvalidExternalId(
                external_id.as_str().to_owned(),
            )));
        };
        if runner != LEGENDARY_RUNNER || app_name.is_empty() {
            return Err(SourceError::new(HeroicDiscoveryError::InvalidExternalId(
                external_id.as_str().to_owned(),
            )));
        }

        // Heroic 2.22.0+ supports the protocol-level gui=false flag, including
        // when its Electron process is already running. Keep portal OpenURI
        // dispatch so no broad Flatpak host-spawn permission is needed.
        Ok(Some(SourceLaunchTarget::Uri(format!(
            "heroic://launch?appName={}&runner={}&gui=false",
            percent_encode_component(app_name),
            percent_encode_component(runner)
        ))))
    }
}

fn heroic_external_id(
    runner: &str,
    app_name: &str,
) -> Result<ExternalGameId, DomainValidationError> {
    ExternalGameId::new(format!("{runner}:{app_name}"))
}

fn title_from_installed_entry(value: &Value) -> Option<&str> {
    value
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty())
}

fn read_legendary_title(
    root: &Path,
    app_name: &str,
) -> Result<Option<GameTitle>, HeroicDiscoveryError> {
    let path = root.join("metadata").join(format!("{app_name}.json"));
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(HeroicDiscoveryError::Read { path, source });
        }
    };
    let value: Value = serde_json::from_str(&text).map_err(|source| HeroicDiscoveryError::Json {
        path: path.clone(),
        source,
    })?;
    let title = value
        .pointer("/metadata/title")
        .and_then(Value::as_str)
        .or_else(|| value.get("title").and_then(Value::as_str));

    match title {
        Some(title) if !title.trim().is_empty() => Ok(Some(GameTitle::new(title)?)),
        _ => Ok(None),
    }
}

/// Heroic uses electron-store dot paths (appName.totalPlayed), which
/// serialize as nested JSON objects. A literal dotted-key variant is accepted
/// for older or manually exported data. Missing/invalid values stay unknown.
fn parse_timestamp_values(value: &Value) -> Vec<(String, PlaytimeSeconds)> {
    fn collect(value: &Value, prefix: &str, result: &mut BTreeMap<String, PlaytimeSeconds>) {
        let Some(object) = value.as_object() else { return };
        // electron-store dot keys can nest a dotted app ID at arbitrary depth.
        if !prefix.is_empty()
            && let Some(duration) = object.get("totalPlayed").and_then(valid_minutes) {
            result.entry(prefix.to_owned()).or_insert(duration);
        }
        for (key, child) in object {
            if key == "totalPlayed" || key == "firstPlayed" || key == "lastPlayed" {
                continue;
            }
            if let Some(name) = key.strip_suffix(".totalPlayed") {
                let full_name = if prefix.is_empty() { name.to_owned() } else { format!("{prefix}.{name}") };
                if !full_name.is_empty()
                    && let Some(duration) = valid_minutes(child) {
                    result.entry(full_name).or_insert(duration);
                }
            } else if child.is_object() {
                let full_name = if prefix.is_empty() { key.to_owned() } else { format!("{prefix}.{key}") };
                collect(child, &full_name, result);
            }
        }
    }
    let mut result = BTreeMap::new();
    collect(value, "", &mut result);
    result.into_iter().collect()
}

fn valid_minutes(value: &Value) -> Option<PlaytimeSeconds> {
    // Heroic writes Math.floor(totalMinutes). Never coerce strings, negative
    // values, fractional numbers or imprecise numbers into invented time.
    let minutes = value.as_u64()?;
    let seconds = i64::try_from(minutes).ok()?.checked_mul(60)?;
    PlaytimeSeconds::new(seconds).ok()
}

fn default_runtime_stores() -> Vec<HeroicRuntimeStore> {
    let mut stores = Vec::new();
    if let (Some(state), Some(config)) = (host_state_home(), host_config_home()) {
        stores.push(HeroicRuntimeStore {
            game_logs: state.join("Heroic/logs"),
            timestamp: config.join("heroic/store/timestamp.json"),
        });
    }
    if let Some(home) = home_dir() {
        let root = home.join(".var/app/com.heroicgameslauncher.hgl");
        let timestamp = root.join("config/heroic/store/timestamp.json");
        // Flatpak's XDG_STATE_HOME is normally .local/state; older Heroic
        // installations can also have a state/ tree. Both remain confined to
        // Heroic's already-granted read-only application data subtree.
        for relative in [".local/state", "state"] {
            stores.push(HeroicRuntimeStore {
                game_logs: root.join(relative).join("Heroic/logs"),
                timestamp: timestamp.clone(),
            });
        }
    }
    stores
}

fn valid_log_identity(app_name: &str) -> bool {
    !app_name.is_empty()
        && app_name != "."
        && app_name != ".."
        && !app_name.chars().any(|character| matches!(character, '/' | '\\' | '\0'))
}

fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

fn log_session_appears_active(
    tail: &str,
    log_modified_ms: i64,
    last_played_ms: Option<i64>,
    now_ms: i64,
) -> bool {
    // We cannot prove the game process is alive from the log alone. Only
    // accept recent, unclosed sessions, and cross-check the completion record.
    if tail.is_empty()
        || tail.contains(HEROIC_LOG_END_MARKER)
        || log_modified_ms > now_ms.saturating_add(HEROIC_LOG_CLOCK_SKEW_MS)
        || now_ms.saturating_sub(log_modified_ms) > HEROIC_LOG_MAX_IDLE_MS
    {
        return false;
    }
    // Heroic writes lastPlayed after awaiting game.launch(), even when logs
    // are disabled and closing the LogWriter emits no end-of-log marker.
    !last_played_ms.is_some_and(|ended| ended >= log_modified_ms)
}

fn last_played_millis(path: &Path, app_name: &str) -> io::Result<Option<i64>> {
    let json = match fs::read_to_string(path) {
        Ok(json) => json,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let Ok(root) = serde_json::from_str::<Value>(&json) else {
        return Ok(None); // A corrupt optional timestamp store is not a start signal.
    };
    let nested = app_name.split('.').try_fold(&root, |node, key| node.get(key));
    let value = nested
        .and_then(|game| game.get("lastPlayed"))
        .or_else(|| root.get(format!("{app_name}.lastPlayed").as_str()));
    Ok(value
        .and_then(Value::as_str)
        .and_then(|date| DateTime::parse_from_rfc3339(date).ok())
        .map(|date| date.timestamp_millis()))
}

#[derive(Clone, Copy, Debug)]
struct HeroicLogObservation {
    modified_ms: i64,
    active: bool,
}

fn heroic_log_observation(
    store: &HeroicRuntimeStore,
    app_name: &str,
) -> io::Result<Option<HeroicLogObservation>> {
    let path = store.game_logs.join("games")
        .join(format!("{app_name}_{LEGENDARY_RUNNER}"))
        .join("launch.log");
    let mut file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Ok(None);
    }
    let modified_ms: i64 = match metadata.modified()?.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis().try_into().unwrap_or(i64::MAX),
        Err(_) => return Ok(None),
    };
    let bytes_to_read = metadata.len().min(HEROIC_LOG_TAIL_BYTES);
    file.seek(SeekFrom::End(-(bytes_to_read as i64)))?;
    let mut tail = Vec::with_capacity(bytes_to_read as usize);
    file.take(bytes_to_read).read_to_end(&mut tail)?;
    let last_played = last_played_millis(&store.timestamp, app_name)?;
    Ok(Some(HeroicLogObservation {
        modified_ms,
        active: log_session_appears_active(
            &String::from_utf8_lossy(&tail),
            modified_ms,
            last_played,
            unix_millis(),
        ),
    }))
}

fn default_timestamp_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(config) = host_config_home() {
        paths.push(config.join("heroic/store/timestamp.json"));
    }
    if let Some(home) = home_dir() {
        paths.push(home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic/store/timestamp.json"));
    }
    dedupe_paths(paths)
}

fn default_legendary_roots() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(config_home) = host_config_home() {
        push_preferred_legendary_root(
            &mut candidates,
            config_home.join("heroic/legendaryConfig/legendary"),
            config_home.join("legendary"),
        );
    }
    if let Some(home) = home_dir() {
        let flatpak_config = home.join(".var/app/com.heroicgameslauncher.hgl/config");
        push_preferred_legendary_root(
            &mut candidates,
            flatpak_config.join("heroic/legendaryConfig/legendary"),
            flatpak_config.join("legendary"),
        );
    }
    dedupe_paths(candidates)
}

fn push_preferred_legendary_root(
    candidates: &mut Vec<PathBuf>,
    current: PathBuf,
    legacy: PathBuf,
) {
    if current.is_dir() {
        candidates.push(current);
    } else if legacy.is_dir() {
        candidates.push(legacy);
    } else {
        // Preserve both candidates so Source construction is deterministic even
        // before either optional installation exists. Discovery filters them by
        // directory existence at runtime.
        candidates.push(current);
        candidates.push(legacy);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "horizon-heroic-{name}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("metadata")).expect("metadata directory");
        root
    }

    #[test]
    fn installed_json_is_authoritative_and_metadata_supplies_title() {
        let root = temp_root("installed");
        fs::write(
            root.join("installed.json"),
            r#"{"CrabEA":{"version":"1.0"},"NoMetadata":{"version":"1.0"}}"#,
        )
        .expect("installed fixture");
        fs::write(
            root.join("metadata/CrabEA.json"),
            r#"{"metadata":{"title":"Satisfactory"}}"#,
        )
        .expect("metadata fixture");

        let source = HeroicSource::with_roots(vec![root]).expect("source");
        let SourceDiscovery::Available(snapshot) = source.discover().expect("discovery") else {
            panic!("Heroic should be available");
        };
        assert!(snapshot.is_authoritative());
        assert_eq!(snapshot.games().len(), 1);
        assert_eq!(snapshot.games()[0].title().as_str(), "Satisfactory");
        assert_eq!(snapshot.games()[0].external_id().as_str(), "legendary:CrabEA");
        assert_eq!(
            snapshot
                .authoritative_membership()
                .expect("membership")
                .len(),
            2
        );
    }

    #[test]
    fn missing_installed_json_is_an_authoritative_empty_library() {
        let root = temp_root("empty");
        let source = HeroicSource::with_roots(vec![root]).expect("source");
        let SourceDiscovery::Available(snapshot) = source.discover().expect("discovery") else {
            panic!("Heroic should be available");
        };

        assert!(snapshot.is_authoritative());
        assert!(snapshot.games().is_empty());
        assert!(snapshot
            .authoritative_membership()
            .expect("membership")
            .is_empty());
    }

    #[test]
    fn launch_requests_hidden_heroic_window_through_the_protocol() {
        let source = HeroicSource::with_roots(vec![]).expect("source");
        let target = source
            .launch_target(&ExternalGameId::new("legendary:Game Name").expect("id"))
            .expect("launch target");
        assert_eq!(
            target,
            Some(SourceLaunchTarget::Uri(
                "heroic://launch?appName=Game%20Name&runner=legendary&gui=false".to_owned()
            ))
        );
    }
    #[test]
    fn heroic_headless_launch_keeps_encoded_game_identity_intact() {
        let source = HeroicSource::with_roots(vec![]).expect("source");
        let target = source
            .launch_target(&ExternalGameId::new("legendary:Game & Friends?#").expect("id"))
            .expect("launch target");
        assert_eq!(
            target,
            Some(SourceLaunchTarget::Uri(
                "heroic://launch?appName=Game%20%26%20Friends%3F%23&runner=legendary&gui=false".to_owned()
            ))
        );
    }

    #[test]
    fn timestamp_nested_entries_are_normalized_to_seconds() {
        let json: Value = serde_json::from_str(r#"{
            "CrabEA": {"totalPlayed": 73, "lastPlayed": "2026-10-08"},
            "Zero": {"totalPlayed": 0},
            "Negative": {"totalPlayed": -1},
            "String": {"totalPlayed": "123"},
            "Fractional": {"totalPlayed": 1.5},
            "Huge": {"totalPlayed": 18446744073709551615},
            "Partial": {"firstPlayed": "2026-10-08"},
            "Dotted": {"App": {"totalPlayed": 5}}
        }"#).expect("fixture");
        let reports = parse_timestamp_values(&json);
        assert_eq!(reports.len(), 3);
        assert_eq!(reports[0].0, "CrabEA");
        assert_eq!(reports[0].1.get(), 73 * 60);
        assert_eq!(reports[1].0, "Dotted.App");
        assert_eq!(reports[1].1.get(), 5 * 60);
        assert_eq!(reports[2].0, "Zero");
        assert_eq!(reports[2].1.get(), 0);
    }

    #[test]
    fn dotted_entries_and_duplicate_roots_do_not_double_count() {
        let path_a = temp_root("timestamp-native").join("timestamp.json");
        let path_b = temp_root("timestamp-flatpak").join("timestamp.json");
        fs::write(&path_a, r#"{"CrabEA.totalPlayed": 60}"#).expect("native");
        fs::write(&path_b, r#"{"CrabEA": {"totalPlayed": 80}, "Another": {"totalPlayed": 2}}"#)
            .expect("flatpak");
        let mut source = HeroicSource::with_roots(vec![]).expect("source");
        source.timestamp_paths = vec![path_a, path_b];
        let reports = source.lifetime_playtime_snapshot().expect("snapshot");
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].0.as_str(), "legendary:Another");
        assert_eq!(reports[0].1.get(), 120);
        assert_eq!(reports[1].0.as_str(), "legendary:CrabEA");
        assert_eq!(reports[1].1.get(), 3_600);
    }

    #[test]
    fn corrupt_optional_timestamp_store_does_not_erase_valid_sources() {
        let invalid = temp_root("timestamp-invalid").join("timestamp.json");
        let valid = temp_root("timestamp-valid").join("timestamp.json");
        fs::write(&invalid, "{partial").expect("invalid");
        fs::write(&valid, r#"{"CrabEA": {"totalPlayed": 12}}"#).expect("valid");
        let mut source = HeroicSource::with_roots(vec![]).expect("source");
        source.timestamp_paths = vec![invalid, valid];
        let reports = source.lifetime_playtime_snapshot().expect("snapshot");
        assert_eq!(reports[0].1.get(), 720);
    }

}

#[cfg(test)]
mod automatic_runtime_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn fixture() -> (PathBuf, HeroicSource) {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "horizon-heroic-auto-{}-{}",
            std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let logs = root.join("state/Heroic/logs");
        let config = root.join("config/heroic/store/timestamp.json");
        fs::create_dir_all(logs.join("games/GameId_legendary")).unwrap();
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        let mut source = HeroicSource::with_roots(vec![root.join("installed")]).unwrap();
        fs::create_dir_all(&source.legendary_roots[0]).unwrap();
        source.runtime_stores = vec![HeroicRuntimeStore { game_logs: logs, timestamp: config }];
        (root, source)
    }

    #[test]
    fn unconfigured_game_watches_native_log_without_any_wrapper() {
        let (root, source) = fixture();
        let id = ExternalGameId::new("legendary:GameId").unwrap();
        assert_eq!(source.runtime_state(&id).unwrap(), Some(SourceRuntimeState::Stopped));
        let log = source.runtime_stores[0].game_logs.join("games/GameId_legendary/launch.log");
        fs::write(&log, "IMPORTANT: Logs are disabled\n").unwrap();
        assert_eq!(source.runtime_state(&id).unwrap(), Some(SourceRuntimeState::Running));
        fs::write(&log, "IMPORTANT: Logs are disabled\n============= End of log =============\n").unwrap();
        assert_eq!(source.runtime_state(&id).unwrap(), Some(SourceRuntimeState::Stopped));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn timestamp_completion_terminates_log_without_end_marker() {
        let (root, source) = fixture();
        let id = ExternalGameId::new("legendary:GameId").unwrap();
        let log = source.runtime_stores[0].game_logs.join("games/GameId_legendary/launch.log");
        fs::write(&log, "IMPORTANT: Logs are disabled\n").unwrap();
        assert_eq!(source.runtime_state(&id).unwrap(), Some(SourceRuntimeState::Running));
        let timestamp = source.runtime_stores[0].timestamp.clone();
        // Guaranteed to be after the log file's last modified time.
        let future = chrono::Utc::now() + chrono::Duration::seconds(10);
        fs::write(timestamp, format!(r#"{{"GameId":{{"lastPlayed":"{}"}}}}"#, future.to_rfc3339())).unwrap();
        assert_eq!(source.runtime_state(&id).unwrap(), Some(SourceRuntimeState::Stopped));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn incomplete_old_log_and_future_timestamps_are_rejected() {
        let now = unix_millis();
        assert!(!log_session_appears_active("live", now - HEROIC_LOG_MAX_IDLE_MS - 1, None, now));
        assert!(!log_session_appears_active("live", now + HEROIC_LOG_CLOCK_SKEW_MS + 1, None, now));
        assert!(!log_session_appears_active("live", now, Some(now), now));
        assert!(!log_session_appears_active("", now, None, now));
        assert!(log_session_appears_active("live", now, Some(now - 5_000), now));
    }

    #[test]
    fn game_log_identity_does_not_traverse_directories() {
        for rejected in ["", ".", "..", "../other", "a/b", "a\\b", "\0"] {
            assert!(!valid_log_identity(rejected));
        }
        assert!(valid_log_identity("Dotted.GameId"));
        assert!(valid_log_identity("Example Game β"));
    }

    #[test]
    fn orphaned_open_log_does_not_start_new_observation() {
        let (root, source) = fixture();
        let id = ExternalGameId::new("legendary:GameId").unwrap();
        let log = source.runtime_stores[0].game_logs.join("games/GameId_legendary/launch.log");
        fs::write(&log, "IMPORTANT: Logs are disabled\n").unwrap();
        // The unscoped check remains useful for the helper's capability probe.
        assert_eq!(source.runtime_state(&id).unwrap(), Some(SourceRuntimeState::Running));
        let future = SystemTime::now() + std::time::Duration::from_secs(90);
        assert_eq!(source.runtime_state_for_observation(&id, future).unwrap(),
            Some(SourceRuntimeState::Stopped));
        assert_eq!(source.runtime_state_for_observation(&id, SystemTime::now()).unwrap(),
            Some(SourceRuntimeState::Running));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn newest_completed_store_beats_older_open_store() {
        let (root, mut source) = fixture();
        let id = ExternalGameId::new("legendary:GameId").unwrap();
        let old_log = source.runtime_stores[0].game_logs.join("games/GameId_legendary/launch.log");
        fs::write(&old_log, "IMPORTANT: Logs are disabled\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        let second = root.join("flatpak/logs");
        let new_log = second.join("games/GameId_legendary/launch.log");
        fs::create_dir_all(new_log.parent().unwrap()).unwrap();
        fs::write(&new_log, "============= End of log =============\n").unwrap();
        source.runtime_stores.push(HeroicRuntimeStore {
            game_logs: second,
            timestamp: root.join("other_timestamp.json"),
        });
        assert_eq!(source.runtime_state(&id).unwrap(), Some(SourceRuntimeState::Stopped));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn newest_open_store_beats_older_closed_store() {
        let (root, mut source) = fixture();
        let id = ExternalGameId::new("legendary:GameId").unwrap();
        let old_log = source.runtime_stores[0].game_logs.join("games/GameId_legendary/launch.log");
        fs::write(&old_log, "============= End of log =============\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        let second = root.join("flatpak/logs");
        let new_log = second.join("games/GameId_legendary/launch.log");
        fs::create_dir_all(new_log.parent().unwrap()).unwrap();
        fs::write(&new_log, "IMPORTANT: Logs are disabled\n").unwrap();
        source.runtime_stores.push(HeroicRuntimeStore {
            game_logs: second,
            timestamp: root.join("other_timestamp.json"),
        });
        assert_eq!(source.runtime_state(&id).unwrap(), Some(SourceRuntimeState::Running));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn separate_games_cannot_trigger_each_others_playing_state() {
        let (root, source) = fixture();
        fs::write(
            source.runtime_stores[0].game_logs.join("games/GameId_legendary/launch.log"),
            "launching\n"
        ).unwrap();
        assert_eq!(source.runtime_state(&ExternalGameId::new("legendary:AnotherGame").unwrap()).unwrap(),
            Some(SourceRuntimeState::Stopped));
        assert_eq!(source.runtime_state(&ExternalGameId::new("legendary:GameId").unwrap()).unwrap(),
            Some(SourceRuntimeState::Running));
        assert_eq!(source.runtime_state(&ExternalGameId::new("gog:GameId").unwrap()).unwrap(), None);
        fs::remove_dir_all(root).unwrap();
    }
}
