use std::{path::Path, rc::Rc};

use image::{RgbaImage, imageops};
use tracing::{debug, warn};

use crate::{
    domain::LibraryGame,
    sources::{SourceArtworkKind, SourceArtworkLocation, SourceCapability, SourceRegistry},
};

const MAX_SQUARE_ARTWORK_PX: u32 = 512;

/// Source-neutral decoded artwork ready for presentation.
///
/// The pixel buffer is always square RGBA8. Smaller authoritative sources are
/// preserved at their native resolution; larger sources are downscaled to the
/// maximum presentation cache size. Horizon never upscales a small source just
/// to claim a higher-resolution asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SquareArtwork {
    size: u32,
    rgba: Vec<u8>,
}

impl SquareArtwork {
    pub const fn size(&self) -> u32 {
        self.size
    }

    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

/// Resolves provider-owned artwork through the generic source capability
/// boundary and normalizes presentation art to a 1:1 pixel buffer.
pub struct ArtworkService {
    registry: Rc<SourceRegistry>,
}

impl ArtworkService {
    pub fn new(registry: Rc<SourceRegistry>) -> Self {
        Self { registry }
    }

    pub fn square_artwork(&self, game: &LibraryGame) -> Option<SquareArtwork> {
        let mut best: Option<(u64, RgbaImage)> = None;

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
                if candidate.kind() != SourceArtworkKind::SquareIcon {
                    continue;
                }
                let decoded = match candidate.location() {
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

                let Some(image) = decoded else {
                    continue;
                };
                let score = u64::from(image.width()) * u64::from(image.height());
                if best.as_ref().is_none_or(|(best_score, _)| score > *best_score) {
                    best = Some((score, image));
                }
            }
        }

        best.map(|(_, image)| normalize_square(image))
    }
}

fn decode_artwork_file(path: &Path) -> Result<RgbaImage, image::ImageError> {
    image::open(path).map(image::DynamicImage::into_rgba8)
}

fn decode_artwork_bytes(bytes: &[u8]) -> Result<RgbaImage, image::ImageError> {
    image::load_from_memory(bytes).map(image::DynamicImage::into_rgba8)
}

fn normalize_square(image: RgbaImage) -> SquareArtwork {
    let width = image.width();
    let height = image.height();
    let side = width.max(height).max(1);

    let mut square = if width == height {
        image
    } else {
        // Preserve the complete source instead of cropping provider artwork.
        // Transparent padding makes every output pixel buffer exactly 1:1.
        let mut canvas = RgbaImage::new(side, side);
        let x = i64::from((side - width) / 2);
        let y = i64::from((side - height) / 2);
        imageops::overlay(&mut canvas, &image, x, y);
        canvas
    };

    if side > MAX_SQUARE_ARTWORK_PX {
        square = imageops::resize(
            &square,
            MAX_SQUARE_ARTWORK_PX,
            MAX_SQUARE_ARTWORK_PX,
            imageops::FilterType::Lanczos3,
        );
    }

    SquareArtwork {
        size: square.width(),
        rgba: square.into_raw(),
    }
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::*;

    #[test]
    fn normalization_pads_non_square_art_without_cropping() {
        let mut input = RgbaImage::new(4, 2);
        for pixel in input.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }

        let normalized = normalize_square(input);
        assert_eq!(normalized.size(), 4);
        assert_eq!(normalized.rgba().len(), 4 * 4 * 4);

        // The top-left padded pixel stays transparent while the centered image
        // remains opaque, proving that normalization does not stretch/crop it.
        assert_eq!(&normalized.rgba()[0..4], &[0, 0, 0, 0]);
        let centered_pixel = ((1 * 4 + 0) * 4) as usize;
        assert_eq!(
            &normalized.rgba()[centered_pixel..centered_pixel + 4],
            &[255, 255, 255, 255]
        );
    }

    #[test]
    fn normalization_never_upscales_small_square_sources() {
        let input = RgbaImage::new(184, 184);
        assert_eq!(normalize_square(input).size(), 184);
    }

    #[test]
    fn normalization_caps_very_large_sources() {
        let input = RgbaImage::new(1024, 1024);
        assert_eq!(normalize_square(input).size(), MAX_SQUARE_ARTWORK_PX);
    }
}
