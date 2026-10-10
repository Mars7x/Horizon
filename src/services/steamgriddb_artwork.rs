//! Asynchronous SteamGridDB artwork resolution and isolated provenance cache.
//!
//! Network + disk operations run on a worker; Slint/UI state is never sent to
//! that worker. Local source art is owned by `ArtworkService` and untouched.
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::Sender,
    },
    thread,
    time::{Duration, SystemTime},
};

use image::{ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::{
    artwork::{SquareArtwork, normalize_remote_square, square_artwork_from_rgba},
    steamgriddb::{
        MatchMethod, MatchOutcome, SteamGridDbClient, SteamGridDbError, SteamGridDbLookup,
        SteamGridDbMatchService,
    },
};

const CACHE_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
// The v5 miss marker invalidates v2/v3/v4 results generated before the API URL fix.
// Cache entries are written only for authoritative no-match or no-metadata
// outcomes; CDN/network errors must never create negative-cache entries.
const NEGATIVE_CACHE_AGE: Duration = Duration::from_secs(5 * 60);
// Highest scores are ranked first; try a bounded number of candidates if a
// top-voted image fails validation or is unavailable.
const MAX_CANDIDATES: usize = 8;
const CACHE_SCHEMA: u8 = 2;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Contains only owned data; safe to transfer from the presentation thread.
#[derive(Debug, Clone)]
pub struct ArtworkLookupJob {
    pub index: usize,
    pub identity: String,
    pub lookup: SteamGridDbLookup,
}

pub struct ArtworkReady {
    pub generation: u64,
    pub index: usize,
    pub artwork: SquareArtwork,
}

pub enum ArtworkWorkerEvent {
    Artwork(ArtworkReady),
    Status {
        generation: u64,
        message: String,
    },
    // Typed progress is separate from diagnostic text. A completed game can
    // succeed, fail validation, or be missing; all count toward the denominator.
    RefreshProgress {
        generation: u64,
        completed: usize,
        total: usize,
        finished: bool,
        error: Option<String>,
    },
}

// RAII ensures that network/authentication early returns always conclude the
// modal. Generation checks prevent stale/cancelled workers from touching UI.
struct RefreshProgress {
    sender: Sender<ArtworkWorkerEvent>,
    active_generation: Arc<AtomicU64>,
    generation: u64,
    enabled: bool,
    completed: usize,
    total: usize,
    failure: Option<String>,
}

impl RefreshProgress {
    fn new(
        sender: Sender<ArtworkWorkerEvent>,
        active_generation: Arc<AtomicU64>,
        generation: u64,
        enabled: bool,
        total: usize,
    ) -> Self {
        let progress = Self {
            sender,
            active_generation,
            generation,
            enabled,
            completed: 0,
            total,
            failure: None,
        };
        progress.report(false);
        progress
    }
    fn report(&self, finished: bool) {
        if self.enabled && self.active_generation.load(Ordering::Acquire) == self.generation {
            let _ = self.sender.send(ArtworkWorkerEvent::RefreshProgress {
                generation: self.generation,
                completed: self.completed,
                total: self.total,
                finished,
                error: self.failure.clone(),
            });
        }
    }
    fn completed(&mut self, completed: usize) {
        self.completed = completed;
        self.report(false);
    }
    fn fail(&mut self, message: &str) {
        self.failure = Some(message.into());
    }
}
impl Drop for RefreshProgress {
    fn drop(&mut self) {
        self.report(true);
    }
}

fn error_status(error: &SteamGridDbError) -> &'static str {
    match error {
        SteamGridDbError::Unauthorized => "SteamGridDB rejected the API key.",
        SteamGridDbError::RateLimited => {
            "SteamGridDB rate limit reached. Existing artwork retained."
        }
        SteamGridDbError::Network(_) => {
            "SteamGridDB unavailable or offline. Cached artwork retained."
        }
        _ => "SteamGridDB could not load some artwork. Existing artwork retained.",
    }
}

/// Cancellation is generation-based: a changed API key or precedence must never
/// allow old in-flight replies to alter a newly selected artwork policy.
pub fn start_artwork_worker(
    generation: u64,
    active_generation: Arc<AtomicU64>,
    sender: Sender<ArtworkWorkerEvent>,
    api_key: String,
    cache_root: Option<PathBuf>,
    jobs: Vec<ArtworkLookupJob>,
    force_refresh: bool,
) {
    let fallback_sender = sender.clone();
    let total_jobs = jobs.len();
    let spawn = thread::Builder::new()
        .name("horizon-steamgriddb".into())
        .spawn(move || {
            let mut progress = RefreshProgress::new(sender.clone(),
                Arc::clone(&active_generation), generation, force_refresh, jobs.len());
            let client = match SteamGridDbClient::new(&api_key) {
                Ok(client) => client,
                Err(error) => {
                    warn!(%error, "SteamGridDB client unavailable");
                    progress.fail(error_status(&error));
                    let _ = sender.send(ArtworkWorkerEvent::Status { generation,
                        message: error_status(&error).into() });
                    return;
                }
            };
            let matcher = SteamGridDbMatchService::new(client);
            let total = jobs.len();
            if total == 0 {
                let _ = sender.send(ArtworkWorkerEvent::Status { generation,
                    message: "All games already have source artwork. Enable preference to replace it.".into() });
                return;
            }
            let mut loaded = 0usize;
            let mut cached = 0usize;
            let mut matched_count = 0usize;
            let mut missing = 0usize;
            let mut no_square = 0usize;
            let mut rejected_downloads = 0usize;
            let mut cache_write_failures = 0usize;
            let mut negative_cached = 0usize;
            let mut first_failure: Option<String> = None;
            for (job_number, job) in jobs.into_iter().enumerate() {
                if active_generation.load(Ordering::Acquire) != generation { break; }
                progress.completed(job_number); // Previous jobs are fully checked.
                let _ = sender.send(ArtworkWorkerEvent::Status { generation,
                    message: format!("SteamGridDB: checking game {}/{}; {} covers ready.",
                        job_number + 1, total, loaded + cached) });
                // Retain a single representative failure so the Settings status
                // can identify the stage without exposing API credentials.
                let diagnostic_name = job.lookup.title().chars().take(48).collect::<String>();
                let id_source = if job.lookup.has_platform_identity() { "platform ID" } else { "title only" };
                let cache = cache_root.as_ref().map(|root| root.join(&job.identity));
                // Manual refresh bypasses BOTH cache types, but never removes
                // existing files or their displayed images before success.
                if !force_refresh {
                    let cached_artwork = cache.as_ref().and_then(|path| read_cache(path));
                    if let Some((artwork, fresh)) = cached_artwork {
                        if sender.send(ArtworkWorkerEvent::Artwork(ArtworkReady { generation, index: job.index, artwork })).is_err() {
                            return;
                        }
                        if fresh { cached += 1; continue; }
                    }
                    if cache.as_ref().is_some_and(|path| recent_miss(path)) {
                        negative_cached += 1;
                        continue;
                    }
                }
                // Token is never logged. Authentication, network and rate-limit
                // errors abort this pass rather than hammering each game.
                let matched = match matcher.resolve(&job.lookup) {
                    Ok(MatchOutcome::Matched(matched)) => matched,
                    Ok(MatchOutcome::NotFound | MatchOutcome::Ambiguous) => {
                        missing += 1;
                        if first_failure.is_none() {
                            first_failure = Some(format!("not matched: {diagnostic_name} ({id_source})"));
                        }
                        info!(game = %diagnostic_name, id_source, "SteamGridDB lookup returned no unambiguous match");
                        if let Some(path) = &cache { write_miss(path); }
                        continue;
                    }
                    Err(error) => {
                        warn!(%error, "SteamGridDB matching stopped; existing artwork kept");
                        progress.fail(error_status(&error));
                        let _ = sender.send(ArtworkWorkerEvent::Status { generation,
                            message: format!("SteamGridDB match request failed for {diagnostic_name} ({id_source}): {}", error_status(&error)) });
                        return;
                    }
                };
                matched_count += 1;
                let grids = match matcher.square_grids(&matched) {
                    Ok(grids) => grids,
                    Err(SteamGridDbError::Http(404)) => Vec::new(),
                    Err(error) => {
                        warn!(%error, "SteamGridDB grid lookup stopped; existing artwork kept");
                        progress.fail(error_status(&error));
                        let _ = sender.send(ArtworkWorkerEvent::Status { generation,
                            message: format!("SteamGridDB grid metadata failed for {diagnostic_name}: {}", error_status(&error)) });
                        return;
                    }
                };
                // Prefer eligible square grids by SteamGridDB score. The first
                // successfully decoded image is the highest-ranked valid one;
                // native resolution is NOT a reason to skip a higher vote score.
                let had_grids = !grids.is_empty();
                let mut best: Option<(RgbaImage, super::steamgriddb::SteamGridDbSquareGrid)> = None;
                let mut network_failed = false;
                let mut last_rejection: Option<String> = None;
                for grid in grids.into_iter().take(MAX_CANDIDATES) {
                    if active_generation.load(Ordering::Acquire) != generation { return; }
                    match matcher.download_square_grid(&grid) {
                        Ok(image) => { best = Some((image, grid)); break; }
                        Err(SteamGridDbError::Network(error)) => {
                            warn!(%error, "SteamGridDB grid network failure; keeping available artwork");
                            let _ = sender.send(ArtworkWorkerEvent::Status { generation,
                                message: format!("SteamGridDB CDN/network failed for {diagnostic_name}; no negative cache written.") });
                            progress.fail("SteamGridDB CDN/network unavailable. Existing artwork retained.");
                            network_failed = true;
                            break;
                        }
                        Err(error) => {
                            last_rejection = Some(error.to_string());
                            debug!(%error, grid_id = grid.id, "SteamGridDB square grid candidate rejected");
                        }
                    }
                }
                if network_failed { return; }
                // Only if the grid tier produced no usable art should we try
                // square icons. These too are sorted by score in metadata.
                let had_icons = if best.is_none() {
                    let icons = match matcher.square_icons(&matched) {
                        Ok(icons) => icons,
                        Err(SteamGridDbError::Http(404)) => Vec::new(),
                        Err(error) => {
                            warn!(%error, "SteamGridDB icon fallback failed; existing artwork kept");
                            progress.fail(error_status(&error));
                            let _ = sender.send(ArtworkWorkerEvent::Status { generation,
                                message: format!("SteamGridDB icon metadata failed for {diagnostic_name}: {}", error_status(&error)) });
                            return;
                        }
                    };
                    let has_icons = !icons.is_empty();
                    for icon in icons.into_iter().take(MAX_CANDIDATES) {
                        if active_generation.load(Ordering::Acquire) != generation { return; }
                        match matcher.download_square_grid(&icon) {
                            Ok(image) => { best = Some((image, icon)); break; }
                            Err(SteamGridDbError::Network(error)) => {
                                warn!(%error, "SteamGridDB icon network failure; keeping available artwork");
                                let _ = sender.send(ArtworkWorkerEvent::Status { generation,
                                    message: format!("SteamGridDB CDN/network failed for {diagnostic_name}; no negative cache written.") });
                                progress.fail("SteamGridDB CDN/network unavailable. Existing artwork retained.");
                                network_failed = true;
                                break;
                            }
                            Err(error) => {
                                last_rejection = Some(error.to_string());
                                debug!(%error, icon_id = icon.id, "SteamGridDB square icon candidate rejected");
                            }
                        }
                    }
                    has_icons
                } else { false };
                if network_failed { return; }
                if !had_grids && !had_icons && best.is_none() {
                    no_square += 1;
                    if first_failure.is_none() {
                        first_failure = Some(format!("no square metadata: {diagnostic_name} (SteamGridDB ID {})", matched.game.id));
                    }
                    info!(game = %diagnostic_name, steamgriddb_id = matched.game.id,
                        "SteamGridDB game matched but no eligible square metadata returned");
                    if let Some(path) = &cache { write_miss(path); }
                    continue;
                }
                if active_generation.load(Ordering::Acquire) != generation { return; }
                if let Some((image, grid)) = best {
                    // Reuse the existing native-square/pixel-art pipeline.
                    let artwork = normalize_remote_square(image);
                    if let Some(path) = &cache {
                        let provenance = ArtworkProvenance {
                            schema: CACHE_SCHEMA,
                            game_id: matched.game.id,
                            grid_id: grid.id,
                            score: grid.score,
                            match_method: match matched.method {
                                MatchMethod::ExactPlatformId => "exact-platform-id".into(),
                                MatchMethod::UniqueExactTitle => "unique-exact-title".into(),
                            },
                            author: grid.author,
                            image_url: grid.image_url.to_string(),
                            pixelated: artwork.pixelated(),
                            rgba_hash: hash_bytes(artwork.rgba()),
                        };
                        if let Err(error) = write_cache(path, &artwork, &provenance) {
                            cache_write_failures += 1;
                            if first_failure.is_none() {
                                first_failure = Some(format!("cache write failed for {diagnostic_name}: {error}"));
                            }
                            warn!(%error, "SteamGridDB cache write failed; using image for this session");
                        }
                    }
                    loaded += 1;
                    let _ = sender.send(ArtworkWorkerEvent::Artwork(ArtworkReady {
                        generation, index: job.index, artwork,
                    }));
                } else if !network_failed {
                    rejected_downloads += 1;
                    if first_failure.is_none() {
                        first_failure = Some(format!("download rejected: {diagnostic_name} ({})",
                            last_rejection.as_deref().unwrap_or("no compatible downloaded image")));
                    }
                    info!(game = %diagnostic_name, steamgriddb_id = matched.game.id,
                        rejection = ?last_rejection, "SteamGridDB square metadata matched, but image downloads failed validation");
                }
                if network_failed { return; }
            }
            if active_generation.load(Ordering::Acquire) == generation {
                progress.completed(total);
                let _ = sender.send(ArtworkWorkerEvent::Status {
                    generation,
                    message: format!(
                        "SteamGridDB: {}/{}; First: {}. Matched {}, unmatched {}, no square {}, CDN rejected {}, cache errors {}, cached {}, skipped {}.",
                        loaded + cached, total,
                        first_failure.as_deref().unwrap_or("none"),
                        matched_count, missing, no_square, rejected_downloads,
                        cache_write_failures, cached, negative_cached
                    ),
                });
            }
        });
    if let Err(error) = spawn {
        warn!(%error, "SteamGridDB artwork worker could not start");
        if force_refresh {
            let _ = fallback_sender.send(ArtworkWorkerEvent::RefreshProgress {
                generation,
                completed: 0,
                total: total_jobs,
                finished: true,
                error: Some("Unable to start the artwork refresh worker".into()),
            });
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ArtworkProvenance {
    schema: u8,
    game_id: u64,
    grid_id: u64,
    #[serde(default)]
    score: Option<i64>,
    match_method: String,
    author: Option<String>,
    image_url: String,
    pixelated: bool,
    rgba_hash: u64,
}

fn read_cache(root: &Path) -> Option<(SquareArtwork, bool)> {
    if fs::metadata(root.with_extension("json")).ok()?.len() > 16 * 1024 {
        return None;
    }
    let metadata = fs::read(root.with_extension("json")).ok()?;
    let provenance: ArtworkProvenance = serde_json::from_slice(&metadata).ok()?;
    if provenance.schema != CACHE_SCHEMA
        || provenance.grid_id == 0
        || provenance.game_id == 0
        || !(provenance.image_url.starts_with("https://")
            && provenance.image_url.contains(".steamgriddb.com/"))
    {
        return None;
    }
    let file = root.with_extension("png");
    if fs::metadata(&file).ok()?.len() > 8 * 1024 * 1024 {
        return None;
    }
    let reader = image::ImageReader::open(&file).ok()?;
    if reader.into_dimensions().ok()? != (512, 512) {
        return None;
    }
    let image = image::open(&file).ok()?.into_rgba8();
    if image.dimensions() != (512, 512) || hash_bytes(image.as_raw()) != provenance.rgba_hash {
        return None;
    }
    let fresh = fs::metadata(root.with_extension("json"))
        .ok()?
        .modified()
        .ok()
        .and_then(|time| SystemTime::now().duration_since(time).ok())
        .is_some_and(|age| age <= CACHE_MAX_AGE);
    Some((square_artwork_from_rgba(image, provenance.pixelated), fresh))
}

fn recent_miss(root: &Path) -> bool {
    // v6 retries misses after adopting highest-score-first grid selection.
    fs::metadata(root.with_extension("miss-v6"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age <= NEGATIVE_CACHE_AGE)
}

fn write_miss(root: &Path) {
    if let Some(parent) = root.parent()
        && fs::create_dir_all(parent).is_ok()
    {
        let _ = fs::write(root.with_extension("miss-v6"), b"");
    }
}

fn write_cache(
    root: &Path,
    artwork: &SquareArtwork,
    provenance: &ArtworkProvenance,
) -> io::Result<()> {
    let parent = root
        .parent()
        .ok_or(io::Error::from(io::ErrorKind::InvalidInput))?;
    fs::create_dir_all(parent)?;
    let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let png = root.with_extension("png");
    let json = root.with_extension("json");
    let tmp_png = root.with_extension(format!("{}.{}.png.tmp", std::process::id(), id));
    let tmp_json = root.with_extension(format!("{}.{}.json.tmp", std::process::id(), id));
    let result = (|| {
        let rgba = RgbaImage::from_raw(artwork.size(), artwork.size(), artwork.rgba().to_vec())
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
        rgba.save_with_format(&tmp_png, ImageFormat::Png)
            .map_err(io::Error::other)?;
        fs::write(
            &tmp_json,
            serde_json::to_vec_pretty(provenance).map_err(io::Error::other)?,
        )?;
        fs::rename(&tmp_png, &png)?;
        // Metadata is the final commit marker; a mismatch checksum rejects any
        // interrupted write that left mixed PNG/JSON versions.
        fs::rename(&tmp_json, &json)?;
        let _ = fs::remove_file(root.with_extension("miss-v6"));
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp_png);
        let _ = fs::remove_file(tmp_json);
    }
    result
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |hash, b| {
        (hash ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}

/// Stable, non-identifying cache identity: matches the provider-local hash
/// policy but lives in its own isolated subdirectory.
pub fn cache_identity(game: &crate::domain::LibraryGame) -> String {
    let mut bytes = game.game().title().as_str().as_bytes().to_vec();
    let mut ids = game
        .sources()
        .iter()
        .map(|s| format!("{}:{}", s.source_id().as_str(), s.external_id().as_str()))
        .collect::<Vec<_>>();
    ids.sort();
    for id in ids {
        bytes.extend_from_slice(b"\0");
        bytes.extend_from_slice(id.as_bytes());
    }
    format!("{}-{:016x}", game.game().id().get(), hash_bytes(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manual_refresh_reports_progress_and_always_finishes() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let epoch = Arc::new(AtomicU64::new(3));
        {
            let mut progress = RefreshProgress::new(sender, epoch, 3, true, 5);
            progress.completed(2);
            progress.fail("Request interrupted");
        }
        assert!(matches!(
            receiver.recv().unwrap(),
            ArtworkWorkerEvent::RefreshProgress {
                completed: 0,
                total: 5,
                finished: false,
                ..
            }
        ));
        assert!(matches!(
            receiver.recv().unwrap(),
            ArtworkWorkerEvent::RefreshProgress {
                completed: 2,
                total: 5,
                finished: false,
                ..
            }
        ));
        assert!(matches!(
            receiver.recv().unwrap(),
            ArtworkWorkerEvent::RefreshProgress {
                completed: 2,
                total: 5,
                finished: true,
                error: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn previous_miss_markers_do_not_block_new_lookup_policy() {
        let root = std::env::temp_dir()
            .join(format!(
                "horizon-sgdb-miss-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ))
            .join("game");
        fs::create_dir_all(root.parent().unwrap()).unwrap();
        fs::write(root.with_extension("miss-v2"), b"").unwrap();
        fs::write(root.with_extension("miss-v3"), b"").unwrap();
        fs::write(root.with_extension("miss-v4"), b"").unwrap();
        fs::write(root.with_extension("miss-v5"), b"").unwrap();
        assert!(!recent_miss(&root));
        write_miss(&root);
        assert!(recent_miss(&root));
        fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }

    #[test]
    fn cache_keeps_provenance_and_pixel_art_and_rejects_corruption() {
        let root = std::env::temp_dir()
            .join(format!(
                "horizon-sgdb-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ))
            .join("game");
        let image = RgbaImage::from_pixel(512, 512, image::Rgba([80, 130, 255, 255]));
        let artwork = normalize_remote_square(image);
        let meta = ArtworkProvenance {
            schema: CACHE_SCHEMA,
            game_id: 5,
            grid_id: 9,
            score: Some(13),
            match_method: "exact-platform-id".into(),
            author: Some("Creator".into()),
            image_url: "https://cdn2.steamgriddb.com/grid/example.png".into(),
            pixelated: artwork.pixelated(),
            rgba_hash: hash_bytes(artwork.rgba()),
        };
        write_cache(&root, &artwork, &meta).unwrap();
        assert_eq!(read_cache(&root).unwrap().0, artwork);
        let json_path = root.with_extension("json");
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&fs::read(&json_path).unwrap()).unwrap();
        legacy["schema"] = serde_json::json!(1);
        fs::write(&json_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert!(
            read_cache(&root).is_none(),
            "pre-score cache must be ranked again"
        );
        fs::write(root.with_extension("png"), b"bad").unwrap();
        assert!(read_cache(&root).is_none());
        fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
}
