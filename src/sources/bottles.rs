use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use thiserror::Error;
use tracing::warn;

use crate::domain::{DomainValidationError, ExternalGameId, GameTitle, SourceId};

use super::{
    GameSource, SourceCapability, SourceDescriptor, SourceDiscovery, SourceError, SourceGame,
    SourceInitializationError, SourceLaunchTarget, SourceManagedLaunchTarget, SourceSnapshot,
    SourceSnapshotError, SourceUnavailableReason,
    support::{dedupe_paths, home_dir, host_data_home, percent_encode_component},
};

const BOTTLES_SOURCE_ID: &str = "bottles";
const BOTTLES_DISPLAY_NAME: &str = "Bottles";

#[derive(Debug, Deserialize)]
struct BottleConfig {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "External_Programs", default)]
    external_programs: Option<BTreeMap<String, BottleProgram>>,
}

#[derive(Debug, Deserialize)]
struct BottleProgram {
    id: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Error)]
enum BottlesDiscoveryError {
    #[error("failed to read Bottles metadata at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse Bottles YAML at {path}: {source}")]
    Yaml {
        path: PathBuf,
        #[source]
        source: yaml_serde::Error,
    },
    #[error("Bottles was detected but no bottle metadata could be discovered successfully")]
    NoUsableInstallation,
    #[error("Bottles external id is invalid: {0}")]
    InvalidExternalId(String),
    #[error("Bottles program {program_id} no longer exists in bottle {bottle_dir}")]
    ProgramMissing {
        bottle_dir: String,
        program_id: String,
    },
    #[error(transparent)]
    Domain(#[from] DomainValidationError),
    #[error(transparent)]
    Snapshot(#[from] SourceSnapshotError),
}

#[derive(Debug)]
struct BottlesRootDiscovery {
    games: Vec<SourceGame>,
    membership: BTreeSet<ExternalGameId>,
    complete: bool,
}

pub struct BottlesSource {
    descriptor: SourceDescriptor,
    roots: Vec<PathBuf>,
}

impl BottlesSource {
    pub fn new() -> Result<Self, SourceInitializationError> {
        let descriptor = SourceDescriptor::new(
            SourceId::new(BOTTLES_SOURCE_ID)?,
            BOTTLES_DISPLAY_NAME,
            vec![SourceCapability::Launch, SourceCapability::ManagedSession],
        )?;

        Ok(Self {
            descriptor,
            roots: default_bottles_roots(),
        })
    }

    #[cfg(test)]
    fn with_roots(roots: Vec<PathBuf>) -> Result<Self, SourceInitializationError> {
        let mut source = Self::new()?;
        source.roots = roots;
        Ok(source)
    }

    fn discover_root(&self, root: &Path) -> Result<BottlesRootDiscovery, BottlesDiscoveryError> {
        let entries = fs::read_dir(root).map_err(|source| BottlesDiscoveryError::Read {
            path: root.to_owned(),
            source,
        })?;
        let mut games = Vec::new();
        let mut membership = BTreeSet::new();
        let mut complete = true;

        for entry in entries {
            let entry = entry.map_err(|source| BottlesDiscoveryError::Read {
                path: root.to_owned(),
                source,
            })?;
            let file_type = entry.file_type().map_err(|source| BottlesDiscoveryError::Read {
                path: entry.path(),
                source,
            })?;
            if !file_type.is_dir() {
                continue;
            }

            let bottle_dir = entry.file_name().to_string_lossy().into_owned();
            let scope = bottles_scope(root);
            let bottle_path = entry.path();
            let config_path = bottle_path.join("bottle.yml");
            if !config_path.is_file() {
                // Bottles may use placeholder.yml when a bottle lives outside
                // its standard root. Horizon deliberately does not expand
                // filesystem permissions to chase arbitrary external paths.
                if bottle_path.join("placeholder.yml").is_file() {
                    complete = false;
                }
                continue;
            }

            let config = read_bottle_config(&config_path)?;
            for (program_key, program) in config.external_programs.unwrap_or_default() {
                let program_id = program
                    .id
                    .as_deref()
                    .filter(|id| !id.trim().is_empty())
                    .unwrap_or(program_key.as_str());
                let Some(name) = program
                    .name
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
                else {
                    continue;
                };

                let external_id = bottles_external_id(scope, &bottle_dir, program_id)?;
                membership.insert(external_id.clone());
                games.push(SourceGame::new(external_id, GameTitle::new(name)?));
            }
        }

        Ok(BottlesRootDiscovery {
            games,
            membership,
            complete,
        })
    }

    fn resolve_launch_names(
        &self,
        scope: &str,
        bottle_dir: &str,
        program_id: &str,
    ) -> Result<(String, String), BottlesDiscoveryError> {
        for root in self
            .roots
            .iter()
            .filter(|root| root.is_dir() && bottles_scope(root) == scope)
        {
            let config_path = root.join(bottle_dir).join("bottle.yml");
            if !config_path.is_file() {
                continue;
            }
            let config = read_bottle_config(&config_path)?;
            for (program_key, program) in config.external_programs.unwrap_or_default() {
                let current_id = program
                    .id
                    .as_deref()
                    .filter(|id| !id.trim().is_empty())
                    .unwrap_or(program_key.as_str());
                if current_id != program_id {
                    continue;
                }
                let Some(program_name) = program.name.filter(|name| !name.trim().is_empty()) else {
                    break;
                };
                return Ok((config.name, program_name));
            }
        }

        Err(BottlesDiscoveryError::ProgramMissing {
            bottle_dir: bottle_dir.to_owned(),
            program_id: program_id.to_owned(),
        })
    }
}

impl GameSource for BottlesSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    fn discover(&self) -> Result<SourceDiscovery, SourceError> {
        let roots = self
            .roots
            .iter()
            .filter(|root| root.is_dir())
            .collect::<Vec<_>>();
        if roots.is_empty() {
            return Ok(SourceDiscovery::Unavailable(SourceUnavailableReason::NotInstalled));
        }

        let mut merged = BTreeMap::<String, SourceGame>::new();
        let mut membership = BTreeSet::new();
        let mut successful = 0usize;
        let mut complete = true;
        let mut first_error = None;

        for root in roots {
            match self.discover_root(root) {
                Ok(discovery) => {
                    successful += 1;
                    complete &= discovery.complete;
                    membership.extend(discovery.membership);
                    for game in discovery.games {
                        merged
                            .entry(game.external_id().as_str().to_owned())
                            .or_insert(game);
                    }
                }
                Err(error) => {
                    complete = false;
                    warn!(
                        root = %root.display(),
                        %error,
                        "Bottles metadata could not be discovered"
                    );
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }

        if successful == 0 {
            return Err(SourceError::new(
                first_error.unwrap_or(BottlesDiscoveryError::NoUsableInstallation),
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

        let snapshot = if complete {
            SourceSnapshot::authoritative_with_membership(games, membership)
        } else {
            SourceSnapshot::new(games)
        }
        .map_err(BottlesDiscoveryError::from)
        .map_err(SourceError::new)?;

        Ok(SourceDiscovery::Available(snapshot))
    }

    fn launch_target(
        &self,
        external_id: &ExternalGameId,
    ) -> Result<Option<SourceLaunchTarget>, SourceError> {
        let (scope, bottle_dir, program_id) =
            parse_bottles_external_id(external_id).map_err(SourceError::new)?;
        let (bottle_name, program_name) = self
            .resolve_launch_names(scope, bottle_dir, program_id)
            .map_err(SourceError::new)?;
        Ok(Some(SourceLaunchTarget::Uri(format!(
            "bottles:run/{}/{}",
            percent_encode_component(&bottle_name),
            percent_encode_component(&program_name)
        ))))
    }

    fn managed_launch_target(
        &self,
        external_id: &ExternalGameId,
    ) -> Result<Option<SourceManagedLaunchTarget>, SourceError> {
        let (scope, bottle_dir, program_id) =
            parse_bottles_external_id(external_id).map_err(SourceError::new)?;
        let (bottle_name, program_name) = self
            .resolve_launch_names(scope, bottle_dir, program_id)
            .map_err(SourceError::new)?;

        let target = match scope {
            "native" => SourceManagedLaunchTarget::new(
                "bottles-cli",
                vec![
                    "run".to_owned(),
                    "-b".to_owned(),
                    bottle_name,
                    "-p".to_owned(),
                    program_name,
                ],
            ),
            "flatpak" => SourceManagedLaunchTarget::new(
                "flatpak",
                vec![
                    "run".to_owned(),
                    "--command=bottles-cli".to_owned(),
                    "com.usebottles.bottles".to_owned(),
                    "run".to_owned(),
                    "-b".to_owned(),
                    bottle_name,
                    "-p".to_owned(),
                    program_name,
                ],
            ),
            _ => unreachable!("validated Bottles scope"),
        }
        .map_err(SourceError::new)?;

        Ok(Some(target))
    }
}

fn parse_bottles_external_id(
    external_id: &ExternalGameId,
) -> Result<(&str, &str, &str), BottlesDiscoveryError> {
    let Some((scope, remainder)) = external_id.as_str().split_once(':') else {
        return Err(BottlesDiscoveryError::InvalidExternalId(
            external_id.as_str().to_owned(),
        ));
    };
    let Some((bottle_dir, program_id)) = remainder.split_once('/') else {
        return Err(BottlesDiscoveryError::InvalidExternalId(
            external_id.as_str().to_owned(),
        ));
    };
    if !matches!(scope, "native" | "flatpak")
        || bottle_dir.is_empty()
        || program_id.is_empty()
    {
        return Err(BottlesDiscoveryError::InvalidExternalId(
            external_id.as_str().to_owned(),
        ));
    }

    Ok((scope, bottle_dir, program_id))
}

fn read_bottle_config(path: &Path) -> Result<BottleConfig, BottlesDiscoveryError> {
    let text = fs::read_to_string(path).map_err(|source| BottlesDiscoveryError::Read {
        path: path.to_owned(),
        source,
    })?;
    yaml_serde::from_str(&text).map_err(|source| BottlesDiscoveryError::Yaml {
        path: path.to_owned(),
        source,
    })
}

fn bottles_external_id(
    scope: &str,
    bottle_dir: &str,
    program_id: &str,
) -> Result<ExternalGameId, DomainValidationError> {
    ExternalGameId::new(format!("{scope}:{bottle_dir}/{program_id}"))
}

fn bottles_scope(path: &Path) -> &'static str {
    if path.components().any(|component| {
        component.as_os_str() == std::ffi::OsStr::new("com.usebottles.bottles")
    }) {
        "flatpak"
    } else {
        "native"
    }
}

fn default_bottles_roots() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(data_home) = host_data_home() {
        candidates.push(data_home.join("bottles/bottles"));
    }
    if let Some(home) = home_dir() {
        candidates.push(home.join(".var/app/com.usebottles.bottles/data/bottles/bottles"));
    }
    dedupe_paths(candidates)
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
            "horizon-bottles-{name}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("temp directory");
        root
    }

    #[test]
    fn external_programs_are_discovered_and_launchable() {
        let root = temp_root("programs");
        let bottle = root.join("gaming");
        fs::create_dir_all(&bottle).expect("bottle directory");
        fs::write(
            bottle.join("bottle.yml"),
            r#"Name: My Bottle
External_Programs:
  89c90f7a:
    executable: Game.exe
    id: 89c90f7a
    name: My Game
    path: /games/Game.exe
"#,
        )
        .expect("bottle fixture");

        let source = BottlesSource::with_roots(vec![root]).expect("source");
        let SourceDiscovery::Available(snapshot) = source.discover().expect("discovery") else {
            panic!("Bottles should be available");
        };
        assert!(snapshot.is_authoritative());
        assert_eq!(snapshot.games().len(), 1);
        assert_eq!(snapshot.games()[0].title().as_str(), "My Game");
        assert_eq!(
            snapshot.games()[0].external_id().as_str(),
            "native:gaming/89c90f7a"
        );

        let target = source
            .launch_target(snapshot.games()[0].external_id())
            .expect("launch target");
        assert_eq!(
            target,
            Some(SourceLaunchTarget::Uri(
                "bottles:run/My%20Bottle/My%20Game".to_owned()
            ))
        );

        let managed = source
            .managed_launch_target(snapshot.games()[0].external_id())
            .expect("managed target")
            .expect("managed target");
        assert_eq!(managed.program(), "bottles-cli");
        assert_eq!(
            managed.args(),
            &["run", "-b", "My Bottle", "-p", "My Game"]
        );
    }

    #[test]
    fn unresolved_external_bottle_makes_snapshot_non_authoritative() {
        let root = temp_root("placeholder");
        let bottle = root.join("external");
        fs::create_dir_all(&bottle).expect("bottle directory");
        fs::write(bottle.join("placeholder.yml"), "Path: /outside/bottle\n")
            .expect("placeholder fixture");

        let source = BottlesSource::with_roots(vec![root]).expect("source");
        let SourceDiscovery::Available(snapshot) = source.discover().expect("discovery") else {
            panic!("Bottles should be available");
        };
        assert!(!snapshot.is_authoritative());
        assert!(snapshot.games().is_empty());
    }
}
