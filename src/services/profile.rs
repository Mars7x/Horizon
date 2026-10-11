//! The profile picture and where it comes from.
//!
//! - **Chosen**: any image the user picks becomes Horizon's own square PNG,
//!   so the original can move or vanish and nothing of it but the pixels (no
//!   metadata) is kept.
//! - **Steam**: the account's avatar, fetched through the Steam Web API and
//!   kept as the same kind of copy so it shows offline.
//! - **Built-in avatars**: Horizon-made pictures compiled into the app
//!   ([`AVATARS`], empty until there is art).
//!
//! [`ProfileSource`] records which one is shown; without one, Home shows a
//! monogram.

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

use image::{DynamicImage, ImageFormat, RgbaImage, imageops::FilterType};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::services::image as decoding;

/// Side of the stored picture: sharp at every size the avatar is drawn.
pub const PICTURE_SIZE: u32 = 256;

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("could not open the image: {0}")]
    Decode(#[from] image::ImageError),
    #[error("could not save the picture: {0}")]
    Save(#[from] io::Error),
}

/// Which picture the profile shows.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "kebab-case")]
pub enum ProfileSource {
    /// The picture chosen in Settings, or the monogram without one.
    #[default]
    Chosen,
    /// The Steam account's avatar. `square`: shown square, as Steam shows
    /// it, with the account's avatar frame around it.
    Steam {
        #[serde(default)]
        square: bool,
    },
    /// A built-in avatar, by its [`Avatar::id`].
    Avatar { id: String },
}

/// A Horizon-made avatar compiled into the app. Its picture is a PNG or
/// JPEG, square or not (it is cropped like any other picture).
pub struct Avatar {
    /// Stable: saved in `profile.json`, so never reuse or rename one.
    pub id: &'static str,
    /// Its accessible name.
    pub name: &'static str,
    pub picture: &'static [u8],
}

/// The built-in avatars, in the order Settings offers them. Empty until there
/// is art; an entry is added as, for example:
/// `Avatar { id: "astronaut", name: "Astronaut",
///   picture: include_bytes!("../../ui/assets/avatars/astronaut.png") }`.
pub const AVATARS: &[Avatar] = &[];

pub fn avatar(id: &str) -> Option<&'static Avatar> {
    AVATARS.iter().find(|avatar| avatar.id == id)
}

/// A built-in avatar's picture, squared like any other.
pub fn avatar_picture(avatar: &Avatar) -> Result<RgbaImage, ProfileError> {
    Ok(square(decoding::decode(avatar.picture, None)?))
}

/// The saved source; anything unreadable is the default.
pub fn load_source(path: &Path) -> ProfileSource {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save_source(path: &Path, source: &ProfileSource) -> Result<(), ProfileError> {
    let json = serde_json::to_vec_pretty(source).map_err(io::Error::other)?;
    let partial = partial_path(path);
    fs::write(&partial, json)?;
    fs::rename(&partial, path)?;
    Ok(())
}

/// Square a picture from bytes (a Steam avatar), under the usual limits.
pub fn prepare_bytes(bytes: &[u8]) -> Result<RgbaImage, ProfileError> {
    Ok(square(decoding::decode(bytes, None)?))
}

/// Side Steam's animated avatars are decoded at: sharp at 2x in the top
/// bar and in the 96 px Settings preview.
pub const ANIMATED_SIDE: u32 = 128;
/// Side avatar frames are decoded at (they surround the picture, 1.22x).
pub const FRAME_SIDE: u32 = 160;
/// Most frames kept of one animation (a minute and more at Steam's pace).
pub const MAX_FRAMES: usize = 600;

/// A small picture that may move: its first frame as a still, and, when it
/// is animated, every frame with its delay.
pub struct Animation {
    pub still: RgbaImage,
    pub frames: Vec<(RgbaImage, Duration)>,
}

/// Decode a Steam profile item (an avatar frame, an animated avatar) from
/// its file's bytes, everything fitted to `side`. A still image is an
/// animation of no frames.
pub fn decode_animation(bytes: &[u8], side: u32) -> Result<Animation, ProfileError> {
    let still = decoding::fit(decoding::decode(bytes, None)?, (side, side));
    let frames =
        decoding::small_animation(bytes, (side, side), MAX_FRAMES, false)?.unwrap_or_default();
    Ok(Animation { still, frames })
}

/// A chosen animated picture (a GIF, an animated PNG) from its file's bytes:
/// every frame cropped to its centre square like the still, at `side`.
/// `None` when it doesn't move.
pub fn decode_chosen_animation(bytes: &[u8], side: u32) -> Option<Animation> {
    let frames = decoding::small_animation(bytes, (side, side), MAX_FRAMES, true)
        .ok()
        .flatten()?;
    let still = frames.first()?.0.clone();
    Some(Animation { still, frames })
}

/// Keep a file's bytes as they are (an animated original), replacing any
/// earlier copy only once the new one is complete.
pub fn save_bytes(path: &Path, bytes: &[u8]) -> Result<(), ProfileError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let partial = partial_path(path);
    let written = fs::write(&partial, bytes).and_then(|()| fs::rename(&partial, path));
    if written.is_err() {
        let _ = fs::remove_file(&partial);
    }
    Ok(written?)
}

/// A kept original, decoded; `None` when absent or unreadable.
pub fn load_animation(path: &Path, side: u32) -> Option<Animation> {
    decode_animation(&fs::read(path).ok()?, side).ok()
}

/// Decode `source` (by content, under the usual limits), crop its centre to a
/// square and scale it to [`PICTURE_SIZE`]. An animated image uses its first
/// frame.
pub fn prepare(source: &Path) -> Result<RgbaImage, ProfileError> {
    let image = decoding::decode_file(source)?;
    Ok(square(image))
}

fn square(image: DynamicImage) -> RgbaImage {
    let side = image.width().min(image.height());
    let x = (image.width() - side) / 2;
    let y = (image.height() - side) / 2;
    let cropped = image.crop_imm(x, y, side, side);
    if side > PICTURE_SIZE {
        cropped
            .resize_exact(PICTURE_SIZE, PICTURE_SIZE, FilterType::Lanczos3)
            .into_rgba8()
    } else {
        cropped.into_rgba8()
    }
}

/// Write the picture to `path`, replacing any earlier one only once the new
/// file is complete.
pub fn save(path: &Path, picture: &RgbaImage) -> Result<(), ProfileError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let partial = partial_path(path);
    let written = picture
        .save_with_format(&partial, ImageFormat::Png)
        .map_err(|error| ProfileError::Save(io::Error::other(error)))
        .and_then(|()| fs::rename(&partial, path).map_err(ProfileError::from));
    if written.is_err() {
        let _ = fs::remove_file(&partial);
    }
    written
}

/// The stored picture, if there is one and it still reads.
pub fn load(path: &Path) -> Option<RgbaImage> {
    path.exists()
        .then(|| decoding::decode_file(path).ok())
        .flatten()
        .map(DynamicImage::into_rgba8)
}

/// Forget the picture. Already gone is fine.
pub fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn partial_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".part");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("horizon-profile-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_wide_image_becomes_its_centre_square() {
        // Left third red, middle green, right third blue: the centre square
        // of a 300x100 image is the green third.
        let mut wide = RgbaImage::new(300, 100);
        for (x, _, pixel) in wide.enumerate_pixels_mut() {
            *pixel = image::Rgba(match x {
                0..100 => [255, 0, 0, 255],
                100..200 => [0, 255, 0, 255],
                _ => [0, 0, 255, 255],
            });
        }
        let picture = square(DynamicImage::ImageRgba8(wide));
        assert_eq!(picture.dimensions(), (100, 100));
        assert_eq!(picture.get_pixel(0, 50).0, [0, 255, 0, 255]);
        assert_eq!(picture.get_pixel(99, 50).0, [0, 255, 0, 255]);
    }

    #[test]
    fn large_pictures_shrink_and_small_ones_keep_their_size() {
        let big = square(DynamicImage::ImageRgba8(RgbaImage::new(1000, 800)));
        assert_eq!(big.dimensions(), (PICTURE_SIZE, PICTURE_SIZE));
        let small = square(DynamicImage::ImageRgba8(RgbaImage::new(64, 90)));
        assert_eq!(small.dimensions(), (64, 64));
    }

    #[test]
    fn saving_loading_and_removing_round_trip() {
        let dir = temp_dir("roundtrip");
        let source = dir.join("photo.jpg");
        // Named .jpg, but a PNG: content decides.
        RgbaImage::from_pixel(40, 30, image::Rgba([10, 20, 30, 255]))
            .save_with_format(&source, ImageFormat::Png)
            .unwrap();
        let stored = dir.join("data/profile.png");
        assert!(load(&stored).is_none());
        let picture = prepare(&source).unwrap();
        save(&stored, &picture).unwrap();
        assert!(!partial_path(&stored).exists());
        assert_eq!(load(&stored).unwrap().dimensions(), (30, 30));
        remove(&stored).unwrap();
        remove(&stored).unwrap();
        assert!(load(&stored).is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_source_round_trips_and_defaults_when_unreadable() {
        let dir = temp_dir("source");
        let path = dir.join("profile.json");
        assert_eq!(load_source(&path), ProfileSource::Chosen);
        for source in [
            ProfileSource::Steam { square: false },
            ProfileSource::Steam { square: true },
            ProfileSource::Avatar {
                id: "astronaut".into(),
            },
            ProfileSource::Chosen,
        ] {
            save_source(&path, &source).unwrap();
            assert_eq!(load_source(&path), source);
        }
        fs::write(&path, b"{ not json").unwrap();
        assert_eq!(load_source(&path), ProfileSource::Chosen);
        fs::write(&path, br#"{"source":"steam"}"#).unwrap();
        assert_eq!(load_source(&path), ProfileSource::Steam { square: false });
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn built_in_avatars_have_unique_ids_and_decode() {
        let mut ids: Vec<_> = AVATARS.iter().map(|avatar| avatar.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), AVATARS.len());
        for avatar in AVATARS {
            assert!(!avatar.id.is_empty() && !avatar.name.is_empty());
            assert!(
                avatar_picture(avatar).is_ok(),
                "{} doesn't decode",
                avatar.id
            );
        }
        assert!(super::avatar("no-such-avatar").is_none());
    }

    #[test]
    fn animations_keep_their_frames_and_stills_have_none() {
        use image::{Delay, Frame, codecs::gif::GifEncoder};
        let mut gif = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut gif);
            for red in [0u8, 100, 200] {
                encoder
                    .encode_frame(Frame::from_parts(
                        RgbaImage::from_pixel(300, 300, image::Rgba([red, 0, 0, 255])),
                        0,
                        0,
                        Delay::from_numer_denom_ms(50, 1),
                    ))
                    .unwrap();
            }
        }
        let animation = decode_animation(&gif, ANIMATED_SIDE).unwrap();
        assert_eq!(animation.still.dimensions(), (ANIMATED_SIDE, ANIMATED_SIDE));
        assert_eq!(animation.frames.len(), 3);
        assert_eq!(animation.frames[2].1, Duration::from_millis(50));

        let mut png = Vec::new();
        RgbaImage::new(40, 40)
            .write_to(&mut io::Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        let still = decode_animation(&png, FRAME_SIDE).unwrap();
        assert_eq!(still.still.dimensions(), (40, 40));
        assert!(still.frames.is_empty());

        let dir = temp_dir("animation");
        let path = dir.join("item");
        save_bytes(&path, &gif).unwrap();
        assert_eq!(
            load_animation(&path, ANIMATED_SIDE).unwrap().frames.len(),
            3
        );
        assert!(load_animation(&dir.join("missing"), ANIMATED_SIDE).is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_chosen_animation_is_squared_and_a_still_is_not_one() {
        use image::{Delay, Frame, codecs::gif::GifEncoder};
        let mut gif = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut gif);
            for red in [0u8, 120] {
                encoder
                    .encode_frame(Frame::from_parts(
                        RgbaImage::from_pixel(300, 150, image::Rgba([red, 0, 0, 255])),
                        0,
                        0,
                        Delay::from_numer_denom_ms(40, 1),
                    ))
                    .unwrap();
            }
        }
        let animation = decode_chosen_animation(&gif, ANIMATED_SIDE).unwrap();
        assert_eq!(animation.frames.len(), 2);
        assert_eq!(
            animation.frames[0].0.dimensions(),
            (ANIMATED_SIDE, ANIMATED_SIDE)
        );
        let mut png = Vec::new();
        RgbaImage::new(40, 40)
            .write_to(&mut io::Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        assert!(decode_chosen_animation(&png, ANIMATED_SIDE).is_none());
    }

    #[test]
    fn an_unreadable_file_is_an_error() {
        let dir = temp_dir("bad");
        let source = dir.join("notes.png");
        fs::write(&source, b"not an image").unwrap();
        assert!(matches!(prepare(&source), Err(ProfileError::Decode(_))));
        fs::remove_dir_all(dir).unwrap();
    }
}
