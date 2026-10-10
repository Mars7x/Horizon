use std::{cell::RefCell, collections::BTreeMap};

use chrono::Utc;
use tracing::warn;

use crate::{
    domain::{GameId, LibraryGame, PlaytimeSeconds, SourceId, SourceLifetimePlaytime},
    services::activity::{ActivityRepository, ActivityService},
    sources::{SourceCapability, SourceRegistry},
};

/// Imports a provider's independently reported cumulative totals without
/// fabricating sessions. Identity matching is strictly (source, external ID).
/// The snapshot cache avoids duplicate SQLite writes on every poll.
#[derive(Default)]
pub struct SourcePlaytimeSync {
    last_written: RefCell<BTreeMap<(GameId, SourceId), PlaytimeSeconds>>,
}

impl SourcePlaytimeSync {
    pub fn refresh<R: ActivityRepository>(
        &self,
        registry: &SourceRegistry,
        library_games: &[LibraryGame],
        activity: &ActivityService<R>,
    ) -> Result<bool, R::Error> {
        let index: BTreeMap<_, _> = library_games
            .iter()
            .flat_map(|game| {
                game.sources().iter().map(move |reference| {
                    (
                        (
                            reference.source_id().clone(),
                            reference.external_id().clone(),
                        ),
                        game.game().id(),
                    )
                })
            })
            .collect();
        let mut changed = false;
        for source in registry.iter() {
            if !source
                .descriptor()
                .supports(SourceCapability::LifetimePlaytime)
            {
                continue;
            }
            let source_id = source.descriptor().id();
            let reports = match source.lifetime_playtime_snapshot() {
                Ok(reports) => reports,
                Err(error) => {
                    warn!(%error, %source_id, "source lifetime snapshot unavailable; preserving persisted totals");
                    continue;
                }
            };
            for (external_id, duration) in reports {
                let Some(game_id) = index.get(&(source_id.clone(), external_id)) else {
                    continue;
                };
                let key = (*game_id, source_id.clone());
                if self.last_written.borrow().get(&key) == Some(&duration) {
                    continue;
                }
                let report = SourceLifetimePlaytime::new(
                    *game_id,
                    source_id.clone(),
                    duration,
                    Utc::now().timestamp(),
                );
                activity.record_source_lifetime_playtime(&report)?;
                self.last_written.borrow_mut().insert(key, duration);
                changed = true;
            }
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicI64, Ordering},
        },
    };

    use super::*;
    use crate::{
        domain::{ExternalGameId, Game, GameTitle, SourceGameRef},
        persistence::SqliteLibraryRepository,
        services::library::{DiscoveredGame, LibraryRepository},
        sources::{
            GameSource, SourceDescriptor, SourceDiscovery, SourceError, SourceUnavailableReason,
        },
    };

    struct TestSource {
        descriptor: SourceDescriptor,
        minutes: Arc<AtomicI64>,
    }

    impl GameSource for TestSource {
        fn descriptor(&self) -> &SourceDescriptor {
            &self.descriptor
        }
        fn discover(&self) -> Result<SourceDiscovery, SourceError> {
            Ok(SourceDiscovery::Unavailable(
                SourceUnavailableReason::NotInstalled,
            ))
        }
        fn lifetime_playtime_snapshot(
            &self,
        ) -> Result<Vec<(ExternalGameId, PlaytimeSeconds)>, SourceError> {
            let minutes = self.minutes.load(Ordering::Relaxed);
            Ok(vec![
                (
                    ExternalGameId::new("legendary:Known").expect("external"),
                    PlaytimeSeconds::new(minutes * 60).expect("time"),
                ),
                (
                    ExternalGameId::new("legendary:Unknown").expect("external"),
                    PlaytimeSeconds::new(500 * 60).expect("time"),
                ),
            ])
        }
    }

    #[test]
    fn imports_matched_lifetime_without_multiplying_on_refresh() {
        let source_id = SourceId::new("heroic").expect("source");
        let external_id = ExternalGameId::new("legendary:Known").expect("external");
        let repository = Rc::new(RefCell::new(
            SqliteLibraryRepository::open(":memory:").expect("repository"),
        ));
        let game_id = repository
            .borrow_mut()
            .upsert_discovered_game(&DiscoveredGame::new(
                source_id.clone(),
                external_id.clone(),
                GameTitle::new("Known Game").expect("title"),
            ))
            .expect("game id");
        let games = vec![LibraryGame::new(
            Game::new(game_id, GameTitle::new("Known Game").expect("title")),
            vec![SourceGameRef::new(source_id.clone(), external_id)],
        )];
        let minutes = Arc::new(AtomicI64::new(40));
        let mut registry = SourceRegistry::new();
        registry
            .register(TestSource {
                descriptor: SourceDescriptor::new(
                    source_id,
                    "Heroic",
                    vec![SourceCapability::LifetimePlaytime],
                )
                .expect("descriptor"),
                minutes: Arc::clone(&minutes),
            })
            .expect("register");
        let activity = ActivityService::new(Rc::clone(&repository));
        let sync = SourcePlaytimeSync::default();

        assert!(
            sync.refresh(&registry, &games, &activity)
                .expect("first sync")
        );
        assert!(
            !sync
                .refresh(&registry, &games, &activity)
                .expect("unchanged sync")
        );
        let overview = activity.overview(6, 4).expect("overview");
        assert_eq!(overview.reported_playtime().get(), 40 * 60);
        assert_eq!(overview.reported_games().len(), 1);
        assert_eq!(overview.observed_playtime().get(), 0);

        minutes.store(70, Ordering::Relaxed);
        assert!(
            sync.refresh(&registry, &games, &activity)
                .expect("changed sync")
        );
        let overview = activity.overview(6, 4).expect("overview");
        assert_eq!(overview.reported_playtime().get(), 70 * 60);
        assert_eq!(overview.reported_games().len(), 1);
    }
}
