//! The one way Horizon reads images: covers, icons, badges, Album photos and
//! thumbnails.
//!
//! - **Safe decoders.** `image-rs` decodes PNG, JPEG, GIF and ICO, and
//!   `jxl-oxide` JPEG XL, all in memory-safe Rust (the same decoders GNOME's
//!   Glycin uses for these formats, without its per-image sandbox process).
//! - **Format from content.** The bytes decide the format, never a file
//!   name, so a mislabelled or hostile file is read as what it is, or
//!   refused.
//! - **Bounded.** Every read is capped at [`MAX_SIDE`] pixels per side and
//!   [`MAX_ALLOC`] bytes of decoder memory, so a tiny file that claims to be
//!   enormous (a decompression bomb) fails instead of exhausting memory.
//! - **Headers first.** [`dimensions`] and [`file_dimensions`] read only the
//!   header, for checks that don't need pixels.
use std::{
    io::{BufRead, Cursor, Seek},
    path::Path,
    sync::Once,
    time::Duration,
};

use image::{
    AnimationDecoder, DynamicImage, Frames, ImageDecoder, ImageFormat, ImageReader, ImageResult,
    Limits, RgbaImage,
    codecs::{gif::GifDecoder, png::PngDecoder},
};

/// Largest width or height Horizon will decode (16K).
pub const MAX_SIDE: u32 = 16_384;
/// Most memory one decode may allocate.
pub const MAX_ALLOC: u64 = 256 * 1024 * 1024;

fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    limits
}

/// Teach `image` JPEG XL (through `jxl-oxide`), by content like the rest.
fn register_formats() {
    static FORMATS: Once = Once::new();
    FORMATS.call_once(|| {
        jxl_oxide::integration::register_image_decoding_hook();
    });
}

fn reader<R: BufRead + Seek>(
    mut reader: ImageReader<R>,
    format: Option<ImageFormat>,
) -> ImageResult<ImageReader<R>> {
    register_formats();
    reader = match format {
        Some(format) => {
            reader.set_format(format);
            reader
        }
        None => reader.with_guessed_format()?,
    };
    reader.limits(limits());
    Ok(reader)
}

/// Decodes a file, whatever its name says.
pub fn decode_file(path: &Path) -> ImageResult<DynamicImage> {
    reader(ImageReader::open(path)?, None)?.decode()
}

/// Decodes bytes. `format` insists on one format; `None` detects it.
pub fn decode(bytes: &[u8], format: Option<ImageFormat>) -> ImageResult<DynamicImage> {
    reader(ImageReader::new(Cursor::new(bytes)), format)?.decode()
}

/// A GIF's frames, decoded one at a time as they are taken (each a whole
/// composited picture), under the same limits. `None` when the file isn't
/// a GIF, whatever its name says.
pub fn gif_frames(path: &Path) -> ImageResult<Option<Frames<'static>>> {
    let reader = reader(ImageReader::open(path)?, None)?;
    if reader.format() != Some(ImageFormat::Gif) {
        return Ok(None);
    }
    let mut decoder = GifDecoder::new(reader.into_inner())?;
    decoder.set_limits(limits())?;
    Ok(Some(decoder.into_frames()))
}

/// Frame delays below this play at [`UNSET_FRAME_DELAY`], as browsers do
/// (0 and 10 ms usually mean "unset").
pub const MIN_FRAME_DELAY: Duration = Duration::from_millis(20);
pub const UNSET_FRAME_DELAY: Duration = Duration::from_millis(100);

pub fn frame_delay(delay: Duration) -> Duration {
    if delay < MIN_FRAME_DELAY {
        UNSET_FRAME_DELAY
    } else {
        delay
    }
}

/// A small animation decoded whole: an animated GIF or PNG, every frame
/// scaled to fit `max` with its delay (`square`: each cropped to its centre
/// square first). For little pictures (a profile's), at most `max_frames`
/// frames; `None` for a still image or another format. A frame that fails
/// ends the animation where it is.
pub fn small_animation(
    bytes: &[u8],
    max: (u32, u32),
    max_frames: usize,
    square: bool,
) -> ImageResult<Option<Vec<(RgbaImage, Duration)>>> {
    let format = image::guess_format(bytes)?;
    let frames: Frames<'_> = match format {
        ImageFormat::Gif => {
            let mut decoder = GifDecoder::new(Cursor::new(bytes))?;
            decoder.set_limits(limits())?;
            decoder.into_frames()
        }
        ImageFormat::Png => {
            let decoder = PngDecoder::with_limits(Cursor::new(bytes), limits())?;
            if !decoder.is_apng()? {
                return Ok(None);
            }
            decoder.apng()?.into_frames()
        }
        _ => return Ok(None),
    };
    let decoded: Vec<_> = frames
        .take(max_frames)
        .map_while(Result::ok)
        .map(|frame| {
            let delay = frame_delay(frame.delay().into());
            let mut image = DynamicImage::ImageRgba8(frame.into_buffer());
            if square {
                let side = image.width().min(image.height());
                image = image.crop_imm(
                    (image.width() - side) / 2,
                    (image.height() - side) / 2,
                    side,
                    side,
                );
            }
            (fit(image, max), delay)
        })
        .collect();
    Ok((decoded.len() > 1).then_some(decoded))
}

/// Width and height from the header alone.
pub fn dimensions(bytes: &[u8], format: Option<ImageFormat>) -> ImageResult<(u32, u32)> {
    reader(ImageReader::new(Cursor::new(bytes)), format)?.into_dimensions()
}

/// Width and height of a file from its header alone.
pub fn file_dimensions(path: &Path) -> ImageResult<(u32, u32)> {
    reader(ImageReader::open(path)?, None)?.into_dimensions()
}

/// The image as RGBA, scaled down (never up) to fit `max`, keeping its
/// aspect ratio.
pub fn fit(image: DynamicImage, (width, height): (u32, u32)) -> RgbaImage {
    if image.width() > width || image.height() > height {
        image.thumbnail(width, height).into_rgba8()
    } else {
        image.into_rgba8()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        RgbaImage::new(width, height)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn the_content_decides_the_format_not_the_name() {
        let dir = std::env::temp_dir().join(format!("horizon-image-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mislabelled = dir.join("cover.jpg");
        std::fs::write(&mislabelled, png(8, 4)).unwrap();
        assert_eq!(file_dimensions(&mislabelled).unwrap(), (8, 4));
        assert_eq!(decode_file(&mislabelled).unwrap().width(), 8);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn animated_gif(frames: usize) -> Vec<u8> {
        use image::{Delay, Frame, codecs::gif::GifEncoder};
        let mut bytes = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut bytes);
            for i in 0..frames {
                let pixel = image::Rgba([(i * 80) as u8, 0, 0, 255]);
                encoder
                    .encode_frame(Frame::from_parts(
                        RgbaImage::from_pixel(6, 4, pixel),
                        0,
                        0,
                        Delay::from_numer_denom_ms(30, 1),
                    ))
                    .unwrap();
            }
        }
        bytes
    }

    #[test]
    fn small_animations_decode_whole_and_stills_do_not() {
        let gif = animated_gif(3);
        let frames = small_animation(&gif, (3, 3), 10, false).unwrap().unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].0.dimensions(), (3, 2));
        assert_eq!(frames[0].1, Duration::from_millis(30));
        let squared = small_animation(&gif, (3, 3), 10, true).unwrap().unwrap();
        assert_eq!(squared[0].0.dimensions(), (3, 3));
        assert_eq!(
            small_animation(&gif, (3, 3), 2, false)
                .unwrap()
                .unwrap()
                .len(),
            2
        );
        assert!(
            small_animation(&animated_gif(1), (3, 3), 10, false)
                .unwrap()
                .is_none()
        );
        assert!(
            small_animation(&png(4, 4), (3, 3), 10, false)
                .unwrap()
                .is_none()
        );
        assert_eq!(frame_delay(Duration::ZERO), UNSET_FRAME_DELAY);
        assert_eq!(
            frame_delay(Duration::from_millis(40)),
            Duration::from_millis(40)
        );
    }

    #[test]
    fn jpeg_xl_decodes_by_content() {
        let dir = std::env::temp_dir().join(format!("horizon-jxl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gradient-32x24.jxl");
        // Named as something else: the content still decides.
        let renamed = dir.join("capture.png");
        std::fs::copy(&fixture, &renamed).unwrap();
        for path in [fixture.as_path(), renamed.as_path()] {
            assert_eq!(file_dimensions(path).unwrap(), (32, 24));
            let image = decode_file(path).unwrap().into_rgba8();
            assert_eq!(image.get_pixel(31, 0).0, [248, 0, 200, 255]);
            assert_eq!(image.get_pixel(0, 23).0, [0, 230, 200, 255]);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn gif_frames_come_one_by_one_and_only_from_gifs() {
        let dir = std::env::temp_dir().join(format!("horizon-gif-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let gif = dir.join("clip.gif");
        std::fs::write(&gif, animated_gif(3)).unwrap();
        let frames = gif_frames(&gif).unwrap().unwrap();
        let reds: Vec<u8> = frames
            .map(|frame| frame.unwrap().buffer().get_pixel(0, 0).0[0])
            .collect();
        assert_eq!(reds, [0, 80, 160]);
        // The first frame is the photo.
        assert_eq!(decode_file(&gif).unwrap().width(), 6);
        let not_gif = dir.join("fake.gif");
        std::fs::write(&not_gif, png(2, 2)).unwrap();
        assert!(gif_frames(&not_gif).unwrap().is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn headers_are_read_without_decoding() {
        assert_eq!(dimensions(&png(3, 5), None).unwrap(), (3, 5));
        assert!(dimensions(&png(3, 5), Some(ImageFormat::Jpeg)).is_err());
    }

    #[test]
    fn an_oversized_image_is_refused() {
        // A valid PNG header claiming more than MAX_SIDE pixels wide.
        let mut bytes = png(1, 1);
        bytes[16..20].copy_from_slice(&(MAX_SIDE + 1).to_be_bytes());
        assert!(decode(&bytes, None).is_err());
    }

    #[test]
    fn fit_only_shrinks() {
        let small = DynamicImage::ImageRgba8(RgbaImage::new(10, 10));
        assert_eq!(fit(small, (100, 100)).dimensions(), (10, 10));
        let wide = DynamicImage::ImageRgba8(RgbaImage::new(400, 100));
        assert_eq!(fit(wide, (200, 200)).dimensions(), (200, 50));
    }
}
