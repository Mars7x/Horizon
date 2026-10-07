use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OpenFlags};
use thiserror::Error;
use tracing::warn;

use crate::domain::{DomainValidationError, ExternalGameId, GameTitle, SourceId};

use super::{
    GameSource, SourceCapability, SourceDescriptor, SourceDiscovery, SourceError, SourceGame,
    SourceInitializationError, SourceLaunchTarget, SourceSnapshot, SourceSnapshotError,
    SourceUnavailableReason,
    support::{dedupe_paths, home_dir, host_data_home},
};

const LUTRIS_SOURCE_ID: &str = "lutris";
const LUTRIS_DISPLAY_NAME: &str = "Lutris";

#[derive(Debug, Error)]
enum LutrisDiscoveryError {
    #[error("failed to open Lutris database at {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("failed to read installed games from Lutris database at {path}: {source}")]
    Query {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("Lutris was detected but no database could be discovered successfully")]
    NoUsableInstallation,
    #[error("Lutris game id is invalid: {0}")]
    InvalidGameId(String),
    #[error(transparent)]
    Domain(#[from] DomainValidationError),
    #[error(transparent)]
    Snapshot(#[from] SourceSnapshotError),
}

#[derive(Debug)]
struct LutrisDatabaseDiscovery {
    games: Vec<SourceGame>,
    membership: BTreeSet<ExternalGameId>,
}

pub struct LutrisSource {
    descriptor: SourceDescriptor,
    databases: Vec<PathBuf>,
}

impl LutrisSource {
    pub fn new() -> Result<Self, SourceInitializationError> {
        let descriptor = SourceDescriptor::new(
            SourceId::new(LUTRIS_SOURCE_ID)?,
            LUTRIS_DISPLAY_NAME,
            vec![SourceCapability::Launch],
        )?;

        Ok(Self {
            descriptor,
            databases: default_lutris_databases(),
        })
    }

    #[cfg(test)]
    fn with_databases(databases: Vec<PathBuf>) -> Result<Self, SourceInitializationError> {
        let mut source = Self::new()?;
        source.databases = databases;
        Ok(source)
    }

    fn discover_database(
        &self,
        path: &Path,
        scope: &str,
    ) -> Result<LutrisDatabaseDiscovery, LutrisDiscoveryError> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|source| LutrisDiscoveryError::Open {
            path: path.to_owned(),
            source,
        })?;

        let mut statement = connection
            .prepare(
                "SELECT id, name FROM games \
                 WHERE installed = 1 \
                   AND configpath IS NOT NULL \
                   AND trim(configpath) <> '' \
                 ORDER BY id",
            )
            .map_err(|source| LutrisDiscoveryError::Query {
                path: path.to_owned(),
                source,
            })?;

        let rows = statement
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let name: String = row.get(1)?;
                Ok((id, name))
            })
            .map_err(|source| LutrisDiscoveryError::Query {
                path: path.to_owned(),
                source,
            })?;

        let mut games = Vec::new();
        let mut membership = BTreeSet::new();
        for row in rows {
            let (id, name) = row.map_err(|source| LutrisDiscoveryError::Query {
                path: path.to_owned(),
                source,
            })?;
            if id <= 0 {
                return Err(LutrisDiscoveryError::InvalidGameId(id.to_string()));
            }

            let external_id = ExternalGameId::new(format!("{scope}:{id}"))?;
            membership.insert(external_id.clone());
            games.push(SourceGame::new(external_id, GameTitle::new(name)?));
        }

        Ok(LutrisDatabaseDiscovery { games, membership })
    }
}

impl GameSource for LutrisSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    fn discover(&self) -> Result<SourceDiscovery, SourceError> {
        let detected = self
            .databases
            .iter()
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        if detected.is_empty() {
            return Ok(SourceDiscovery::Unavailable(SourceUnavailableReason::NotInstalled));
        }

        let mut games = BTreeMap::<String, SourceGame>::new();
        let mut membership = BTreeSet::new();
        let mut successful = 0usize;
        let mut all_succeeded = true;
        let mut first_error = None;

        for path in detected {
            let scope = lutris_scope(path);
            match self.discover_database(path, scope) {
                Ok(discovery) => {
                    successful += 1;
                    membership.extend(discovery.membership);
                    for game in discovery.games {
                        games.insert(game.external_id().as_str().to_owned(), game);
                    }
                }
                Err(error) => {
                    all_succeeded = false;
                    warn!(
                        path = %path.display(),
                        %error,
                        "Lutris database could not be discovered"
                    );
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }

        if successful == 0 {
            return Err(SourceError::new(
                first_error.unwrap_or(LutrisDiscoveryError::NoUsableInstallation),
            ));
        }

        let mut games = games.into_values().collect::<Vec<_>>();
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
        .map_err(LutrisDiscoveryError::from)
        .map_err(SourceError::new)?;

        Ok(SourceDiscovery::Available(snapshot))
    }

    fn launch_target(
        &self,
        external_id: &ExternalGameId,
    ) -> Result<Option<SourceLaunchTarget>, SourceError> {
        let Some((_, raw_id)) = external_id.as_str().rsplit_once(':') else {
            return Err(SourceError::new(LutrisDiscoveryError::InvalidGameId(
                external_id.as_str().to_owned(),
            )));
        };
        if raw_id.parse::<u64>().is_err() {
            return Err(SourceError::new(LutrisDiscoveryError::InvalidGameId(
                external_id.as_str().to_owned(),
            )));
        }

        Ok(Some(SourceLaunchTarget::Uri(format!(
            "lutris:rungameid/{raw_id}"
        ))))
    }
}

fn lutris_scope(path: &Path) -> &'static str {
    if path.components().any(|component| {
        component.as_os_str() == std::ffi::OsStr::new("net.lutris.Lutris")
    }) {
        "flatpak"
    } else {
        "native"
    }
}

fn default_lutris_databases() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(data_home) = host_data_home() {
        candidates.push(data_home.join("lutris/pga.db"));
    }
    if let Some(home) = home_dir() {
        candidates.push(home.join(".var/app/net.lutris.Lutris/data/lutris/pga.db"));
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

    fn temp_db(name: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "horizon-lutris-{name}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp directory");
        dir.join("pga.db")
    }

    #[test]
    fn discovery_only_returns_installed_configured_games() {
        let path = temp_db("installed");
        let connection = Connection::open(&path).expect("database");
        connection
            .execute_batch(
                "CREATE TABLE games (id INTEGER PRIMARY KEY, name TEXT, installed INTEGER, configpath TEXT);\n\
                 INSERT INTO games VALUES (1, 'Installed', 1, 'installed-1');\n\
                 INSERT INTO games VALUES (2, 'Not Installed', 0, 'not-installed');\n\
                 INSERT INTO games VALUES (3, 'Missing Config', 1, '');",
            )
            .expect("fixture schema");
        drop(connection);

        let source = LutrisSource::with_databases(vec![path]).expect("source");
        let SourceDiscovery::Available(snapshot) = source.discover().expect("discovery") else {
            panic!("Lutris should be available");
        };
        assert!(snapshot.is_authoritative());
        assert_eq!(snapshot.games().len(), 1);
        assert_eq!(snapshot.games()[0].title().as_str(), "Installed");
        assert_eq!(snapshot.games()[0].external_id().as_str(), "native:1");
    }

    #[test]
    fn launch_uses_lutris_game_id_protocol() {
        let source = LutrisSource::with_databases(vec![]).expect("source");
        let target = source
            .launch_target(&ExternalGameId::new("flatpak:42").expect("id"))
            .expect("launch target");
        assert_eq!(
            target,
            Some(SourceLaunchTarget::Uri("lutris:rungameid/42".to_owned()))
        );
    }
}
