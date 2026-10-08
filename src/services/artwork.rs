use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, SystemTime},
};

use image::{ImageFormat, RgbaImage, imageops};
use tracing::{debug, warn};

use crate::{
    domain::LibraryGame,
    sources::{SourceArtworkLocation, SourceCapability, SourceRegistry},
};

const NORMALIZED_SQUARE_ARTWORK_PX: u32 = 512;
const ARTWORK_CACHE_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const ARTWORK_CACHE_VERSION: &str = "square-v6";

/// Source-neutral decoded artwork ready for presentation.
///
/// Every successful result is a canonical 512×512 RGBA8 buffer. Candidate
/// quality is decided before normalization so upscaling never lets a tiny icon
/// outrank a genuinely higher-resolution provider asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SquareArtwork {
    size: u32,
    rgba: Vec<u8>,
    pixelated: bool,
}

impl SquareArtwork {
    pub const fn size(&self) -> u32 {
        self.size
    }

    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// True when the provider artwork is pixel-art-like and must use nearest
    /// neighbour scaling both while normalizing and while Slint presents it.
    pub const fn pixelated(&self) -> bool {
        self.pixelated
    }
}

#[derive(Debug)]
struct DecodedCandidate {
    image: RgbaImage,
}

#[derive(Debug)]
struct CachedArtwork {
    artwork: SquareArtwork,
    fresh: bool,
}

/// Resolves provider-owned artwork through the generic source capability
/// boundary and normalizes presentation art to a cached 1:1 pixel buffer.
pub struct ArtworkService {
    registry: Rc<SourceRegistry>,
    cache_root: Option<PathBuf>,
}

impl ArtworkService {
    pub fn new(registry: Rc<SourceRegistry>) -> Self {
        Self {
            registry,
            cache_root: default_artwork_cache_root(),
        }
    }

    #[cfg(test)]
    fn with_cache_root(registry: Rc<SourceRegistry>, cache_root: PathBuf) -> Self {
        Self {
            registry,
            cache_root: Some(cache_root),
        }
    }

    pub fn steamgriddb_cache_root(&self) -> Option<PathBuf> {
        self.cache_root.as_ref().map(|root| root.join("steamgriddb"))
    }

    pub fn square_artwork(&self, game: &LibraryGame) -> Option<SquareArtwork> {
        let cached = self.load_cached_artwork(game);
        if cached.as_ref().is_some_and(|entry| entry.fresh) {
            return cached.map(|entry| entry.artwork);
        }

        let resolved = self.resolve_square_artwork(game);
        if let Some(artwork) = resolved {
            self.store_cached_artwork(game, &artwork);
            return Some(artwork);
        }

        // A stale cache is still preferable to dropping back to procedural art
        // because Steam temporarily moved/evicted one of its own cache files.
        cached.map(|entry| entry.artwork)
    }

    fn resolve_square_artwork(&self, game: &LibraryGame) -> Option<SquareArtwork> {
        let mut decoded = Vec::new();

        for source_ref in game.sources() {
            let Some(source) = self.registry.get(source_ref.source_id()) else {
                continue;
            };
            if !source.descriptor().supports(SourceCapability::Artwork) {
                continue;
            }

            let candidates = match source.artwork_candidates(source_ref.external_id()) {
                Ok(candidates) => candidates,
                Err(error) => {
                    warn!(
                        source = %source_ref.source_id(),
                        external_id = %source_ref.external_id(),
                        %error,
                        "source artwork candidates could not be resolved"
                    );
                    continue;
                }
            };

            for candidate in candidates {
                let image = match candidate.location() {
                    SourceArtworkLocation::File(path) => match decode_artwork_file(path) {
                        Ok(image) => Some(image),
                        Err(error) => {
                            debug!(
                                path = %path.display(),
                                %error,
                                "optional source artwork file could not be decoded"
                            );
                            None
                        }
                    },
                    SourceArtworkLocation::Bytes(bytes) => match decode_artwork_bytes(bytes) {
                        Ok(image) => Some(image),
                        Err(error) => {
                            debug!(
                                %error,
                                "optional in-memory source artwork could not be decoded"
                            );
                            None
                        }
                    },
                };

                if let Some(image) = image {
                    if image.width() != image.height() {
                        debug!(
                            width = image.width(),
                            height = image.height(),
                            "rejecting provider artwork because Horizon only accepts native 1:1 sources"
                        );
                        continue;
                    }

                    decoded.push(DecodedCandidate { image });
                }
            }
        }

        select_best_candidate(decoded).map(normalize_candidate)
    }

    fn cache_path(&self, game: &LibraryGame) -> Option<PathBuf> {
        self.cache_root.as_ref().map(|root| {
            root.join(format!(
                "{}-{:016x}.png",
                game.game().id().get(),
                artwork_cache_identity(game)
            ))
        })
    }

    fn load_cached_artwork(&self, game: &LibraryGame) -> Option<CachedArtwork> {
        let path = self.cache_path(game)?;
        let metadata = fs::metadata(&path).ok()?;
        let image = match decode_artwork_file(&path) {
            Ok(image)
                if image.width() == NORMALIZED_SQUARE_ARTWORK_PX
                    && image.height() == NORMALIZED_SQUARE_ARTWORK_PX =>
            {
                image
            }
            Ok(_) => {
                debug!(path = %path.display(), "ignoring artwork cache entry with obsolete dimensions");
                return None;
            }
            Err(error) => {
                debug!(path = %path.display(), %error, "artwork cache entry could not be decoded");
                return None;
            }
        };

        let fresh = metadata
            .modified()
            .ok()
            .map(cache_timestamp_is_fresh)
            .unwrap_or(false);

        let pixelated = looks_like_pixel_art(&image);
        Some(CachedArtwork {
            artwork: square_artwork_from_rgba(image, pixelated),
            fresh,
        })
    }

    fn store_cached_artwork(&self, game: &LibraryGame, artwork: &SquareArtwork) {
        let Some(path) = self.cache_path(game) else {
            return;
        };
        let Some(parent) = path.parent() else {
            return;
        };
        if let Err(error) = fs::create_dir_all(parent) {
            debug!(path = %parent.display(), %error, "artwork cache directory could not be created");
            return;
        }

        let temporary = path.with_extension("tmp.png");
        let write_result = image::save_buffer_with_format(
            &temporary,
            artwork.rgba(),
            artwork.size(),
            artwork.size(),
            image::ColorType::Rgba8,
            ImageFormat::Png,
        );
        if let Err(error) = write_result {
            debug!(path = %temporary.display(), %error, "normalized artwork cache entry could not be encoded");
            let _ = fs::remove_file(&temporary);
            return;
        }

        if let Err(error) = fs::rename(&temporary, &path) {
            debug!(path = %path.display(), %error, "normalized artwork cache entry could not be committed");
            let _ = fs::remove_file(&temporary);
        }
    }
}


fn artwork_cache_identity(game: &LibraryGame) -> u64 {
    // Stable FNV-1a is sufficient for a disposable cache key and avoids tying
    // cache filenames to provider IDs/titles directly. Include title/source
    // identity so a recreated database cannot accidentally reuse unrelated art
    // merely because it assigned the same numeric GameId.
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;

    let mut hash = OFFSET;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(PRIME);
    };

    feed(game.game().title().as_str().as_bytes());
    let mut sources = game
        .sources()
        .iter()
        .map(|source| {
            format!(
                "{}\0{}",
                source.source_id().as_str(),
                source.external_id().as_str()
            )
        })
        .collect::<Vec<_>>();
    sources.sort();
    for source in sources {
        feed(source.as_bytes());
    }

    hash
}

fn default_artwork_cache_root() -> Option<PathBuf> {
    if let Some(cache_home) = env::var_os("XDG_CACHE_HOME") {
        return Some(
            PathBuf::from(cache_home)
                .join("horizon/artwork")
                .join(ARTWORK_CACHE_VERSION),
        );
    }

    env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".cache/horizon/artwork")
            .join(ARTWORK_CACHE_VERSION)
    })
}

fn cache_timestamp_is_fresh(modified: SystemTime) -> bool {
    SystemTime::now()
        .duration_since(modified)
        .map_or(true, |age| age <= ARTWORK_CACHE_MAX_AGE)
}

fn decode_artwork_file(path: &Path) -> Result<RgbaImage, image::ImageError> {
    image::open(path).map(image::DynamicImage::into_rgba8)
}

fn decode_artwork_bytes(bytes: &[u8]) -> Result<RgbaImage, image::ImageError> {
    image::load_from_memory(bytes).map(image::DynamicImage::into_rgba8)
}

fn select_best_candidate(candidates: Vec<DecodedCandidate>) -> Option<DecodedCandidate> {
    candidates
        .into_iter()
        .max_by_key(|candidate| candidate.image.width())
}

fn normalize_candidate(candidate: DecodedCandidate) -> SquareArtwork {
    debug_assert_eq!(candidate.image.width(), candidate.image.height());

    let pixelated = looks_like_pixel_art(&candidate.image);
    let filter = if pixelated {
        // Pixel art is discrete raster artwork. Linear/Lanczos interpolation
        // invents blended colors between authored pixels, so never use it.
        imageops::FilterType::Nearest
    } else {
        imageops::FilterType::Lanczos3
    };

    let normalized = if candidate.image.width() == NORMALIZED_SQUARE_ARTWORK_PX {
        candidate.image
    } else {
        imageops::resize(
            &candidate.image,
            NORMALIZED_SQUARE_ARTWORK_PX,
            NORMALIZED_SQUARE_ARTWORK_PX,
            filter,
        )
    };

    square_artwork_from_rgba(normalized, pixelated)
}

/// Remote candidates use the identical native-square and pixel-art normalization
/// as provider-local icons. The caller must validate decoded dimensions first.
pub(crate) fn normalize_remote_square(image: RgbaImage) -> SquareArtwork {
    normalize_candidate(DecodedCandidate { image })
}

fn looks_like_pixel_art(image: &RgbaImage) -> bool {
    let width = image.width();
    let height = image.height();
    if width == 0 || width != height {
        return false;
    }

    // Very small icon sources are treated as discrete raster art. At these
    // sizes smoothing is more destructive than nearest-neighbour scaling and
    // this preserves the pre-9.5.38 behavior for tiny Steam icons.
    if width <= 128 {
        return true;
    }

    // Pixel art is characterized less by "few colors" alone than by discrete
    // transitions: long runs of identical/near-identical pixels interrupted by
    // hard color changes, with comparatively few soft gradient transitions.
    // Quantized color count keeps flat vector/photo-like sources from being
    // classified solely from one edge statistic.
    let sample_step = if width > 512 { 2 } else { 1 };
    let mut quantized_colors = HashSet::new();
    let mut flat_edges = 0_u64;
    let mut soft_edges = 0_u64;
    let mut sharp_edges = 0_u64;

    let mut y = 0;
    while y < height {
        let mut x = 0;
        while x < width {
            let pixel = image.get_pixel(x, y).0;
            if pixel[3] >= 16 {
                quantized_colors.insert(quantized_rgb(pixel));
            }

            if x + sample_step < width {
                classify_edge(
                    pixel,
                    image.get_pixel(x + sample_step, y).0,
                    &mut flat_edges,
                    &mut soft_edges,
                    &mut sharp_edges,
                );
            }
            if y + sample_step < height {
                classify_edge(
                    pixel,
                    image.get_pixel(x, y + sample_step).0,
                    &mut flat_edges,
                    &mut soft_edges,
                    &mut sharp_edges,
                );
            }

            x += sample_step;
        }
        y += sample_step;
    }

    let total_edges = flat_edges + soft_edges + sharp_edges;
    if total_edges == 0 {
        return false;
    }

    let flat_ratio = flat_edges as f64 / total_edges as f64;
    let soft_ratio = soft_edges as f64 / total_edges as f64;
    let sharp_ratio = sharp_edges as f64 / total_edges as f64;
    let color_count = quantized_colors.len();

    if width <= 256 {
        color_count <= 2048
            && soft_ratio <= 0.14
            && (flat_ratio >= 0.42 || sharp_ratio >= 0.28)
    } else {
        color_count <= 4096
            && soft_ratio <= 0.10
            && (flat_ratio >= 0.52 || sharp_ratio >= 0.34)
    }
}

fn quantized_rgb(pixel: [u8; 4]) -> u16 {
    (u16::from(pixel[0] >> 3) << 10)
        | (u16::from(pixel[1] >> 3) << 5)
        | u16::from(pixel[2] >> 3)
}

fn classify_edge(
    first: [u8; 4],
    second: [u8; 4],
    flat_edges: &mut u64,
    soft_edges: &mut u64,
    sharp_edges: &mut u64,
) {
    if first[3] < 16 && second[3] < 16 {
        return;
    }

    let delta = first
        .into_iter()
        .zip(second)
        .map(|(left, right)| left.abs_diff(right))
        .max()
        .unwrap_or(0);

    if delta <= 2 {
        *flat_edges += 1;
    } else if delta <= 24 {
        *soft_edges += 1;
    } else {
        *sharp_edges += 1;
    }
}

pub(crate) fn square_artwork_from_rgba(image: RgbaImage, pixelated: bool) -> SquareArtwork {
    SquareArtwork {
        size: image.width(),
        rgba: image.into_raw(),
        pixelated,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::RgbaImage;

    use super::*;

    fn square_candidate(side: u32) -> DecodedCandidate {
        DecodedCandidate {
            image: RgbaImage::new(side, side),
        }
    }

    #[test]
    fn highest_resolution_native_square_wins() {
        let selected = select_best_candidate(vec![
            square_candidate(184),
            square_candidate(512),
            square_candidate(256),
        ])
        .expect("selected artwork");

        assert_eq!(selected.image.dimensions(), (512, 512));
    }

    #[test]
    fn pixel_art_detector_accepts_hard_discrete_edges() {
        let mut image = RgbaImage::new(184, 184);
        for y in 0..184 {
            for x in 0..184 {
                let block = ((x / 8) + (y / 8)) % 4;
                let color = match block {
                    0 => image::Rgba([20, 30, 40, 255]),
                    1 => image::Rgba([220, 80, 40, 255]),
                    2 => image::Rgba([40, 180, 100, 255]),
                    _ => image::Rgba([240, 220, 120, 255]),
                };
                image.put_pixel(x, y, color);
            }
        }

        assert!(looks_like_pixel_art(&image));
        let normalized = normalize_candidate(DecodedCandidate { image });
        assert!(normalized.pixelated());
    }

    #[test]
    fn pixel_art_detector_rejects_smooth_gradient() {
        let mut image = RgbaImage::new(184, 184);
        for y in 0..184 {
            for x in 0..184 {
                image.put_pixel(
                    x,
                    y,
                    image::Rgba([
                        x as u8,
                        y as u8,
                        ((x + y) / 2) as u8,
                        255,
                    ]),
                );
            }
        }

        assert!(!looks_like_pixel_art(&image));
        let normalized = normalize_candidate(DecodedCandidate { image });
        assert!(!normalized.pixelated());
    }

    #[test]
    fn every_normalized_result_is_canonical_512_square() {
        let normalized = normalize_candidate(square_candidate(184));
        assert_eq!(normalized.size(), NORMALIZED_SQUARE_ARTWORK_PX);
        assert_eq!(
            normalized.rgba().len(),
            (NORMALIZED_SQUARE_ARTWORK_PX * NORMALIZED_SQUARE_ARTWORK_PX * 4) as usize
        );
    }

    #[test]
    fn cache_round_trip_reuses_canonical_artwork() {
        let root = env::temp_dir().join(format!(
            "horizon-artwork-cache-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("cache root");

        let registry = Rc::new(SourceRegistry::new());
        let service = ArtworkService::with_cache_root(registry, root.clone());
        let game = crate::domain::LibraryGame::new(
            crate::domain::Game::new(
                crate::domain::GameId::new(1).expect("id"),
                crate::domain::GameTitle::new("Cached Game").expect("title"),
            ),
            vec![],
        );
        let artwork = normalize_candidate(square_candidate(64));
        assert!(artwork.pixelated());
        service.store_cached_artwork(&game, &artwork);

        let cached = service
            .load_cached_artwork(&game)
            .expect("cached artwork should load");
        assert!(cached.fresh);
        assert_eq!(cached.artwork, artwork);

        fs::remove_dir_all(root).expect("cleanup");
    }
}
