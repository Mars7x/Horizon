use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io,
    path::{Path, PathBuf},
};

use serde_json::Value;
use thiserror::Error;
use tracing::{debug, warn};

use crate::domain::{DomainValidationError, ExternalGameId, GameTitle, SourceId};

use super::{
    GameSource, SourceCapability, SourceDescriptor, SourceDiscovery, SourceError, SourceGame,
    SourceInitializationError, SourceLaunchTarget, SourceSnapshot, SourceSnapshotError,
    SourceUnavailableReason,
    support::{dedupe_paths, home_dir, host_config_home, percent_encode_component},
};

const HEROIC_SOURCE_ID: &str = "heroic";
const HEROIC_DISPLAY_NAME: &str = "Heroic";
const LEGENDARY_RUNNER: &str = "legendary";

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
}

impl HeroicSource {
    pub fn new() -> Result<Self, SourceInitializationError> {
        let descriptor = SourceDescriptor::new(
            SourceId::new(HEROIC_SOURCE_ID)?,
            HEROIC_DISPLAY_NAME,
            vec![SourceCapability::Launch],
        )?;

        Ok(Self {
            descriptor,
            legendary_roots: default_legendary_roots(),
        })
    }

    #[cfg(test)]
    fn with_roots(roots: Vec<PathBuf>) -> Result<Self, SourceInitializationError> {
        let mut source = Self::new()?;
        source.legendary_roots = roots;
        Ok(source)
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

        Ok(Some(SourceLaunchTarget::Uri(format!(
            "heroic://launch?appName={}&runner={}",
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
    fn launch_uses_current_heroic_protocol_shape() {
        let source = HeroicSource::with_roots(vec![]).expect("source");
        let target = source
            .launch_target(&ExternalGameId::new("legendary:Game Name").expect("id"))
            .expect("launch target");
        assert_eq!(
            target,
            Some(SourceLaunchTarget::Uri(
                "heroic://launch?appName=Game%20Name&runner=legendary".to_owned()
            ))
        );
    }
}
