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

/// Original input imports are bounded independently of decoded image limits.
pub fn read_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(std::io::Error::other("image input exceeds its size limit"));
    }
    Ok(bytes)
}

/// A worker-side lossless write; callers own temporary paths/publication.
pub fn write_jxl(path: &Path, image: &RgbaImage) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    crate::platform::jpeg_xl::write_rgba(&file, image.as_raw(), image.width(), image.height())
}

pub fn is_jxl(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xff, 0x0a]) || bytes.starts_with(b"\0\0\0\x0cJXL \r\n\x87\n")
}

/// Bounded frame-by-frame JPEG XL reader. Rendered frames include composition,
/// orientation and alpha; the low-level API handles animation unavailable in
/// the image-rs integration. Used by profiles and Album media workers.
pub struct JxlAnimation {
    image: jxl_oxide::JxlImage,
    index: usize,
    tick_numerator: u32,
    tick_denominator: u32,
    pub loops: u32,
}

fn jxl_error(error: impl std::fmt::Display) -> image::ImageError {
    image::ImageError::IoError(std::io::Error::other(error.to_string()))
}

impl JxlAnimation {
    pub fn frame_count(&self) -> usize {
        self.image.num_loaded_keyframes()
    }
    pub fn from_bytes(bytes: &[u8]) -> ImageResult<Option<Self>> {
        if !is_jxl(bytes) {
            return Ok(None);
        }
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(jxl_error("JXL input too large"));
        }
        let mut image = jxl_oxide::JxlImage::builder()
            .alloc_tracker(jxl_oxide::AllocTracker::with_limit(MAX_ALLOC as usize))
            .read(bytes)
            .map_err(jxl_error)?;
        if image.width() > MAX_SIDE
            || image.height() > MAX_SIDE
            || u64::from(image.width()) * u64::from(image.height()) * 4 > 64 * 1024 * 1024
            || image.num_loaded_keyframes() > 600
            || !image.is_loading_done()
        {
            return Err(jxl_error("JXL animation exceeds limits or is incomplete"));
        }
        let Some(header) = image.image_header().metadata.animation.as_ref() else {
            return Ok(None);
        };
        if image.num_loaded_keyframes() < 2 {
            return Ok(None);
        }
        let (tick_numerator, tick_denominator, loops) = (
            header.tps_numerator,
            header.tps_denominator,
            header.num_loops,
        );
        if tick_numerator == 0 || tick_denominator == 0 {
            return Err(jxl_error("invalid JXL animation timing"));
        }
        image.request_color_encoding(jxl_oxide::EnumColourEncoding::srgb(
            jxl_oxide::RenderingIntent::Relative,
        ));
        Ok(Some(Self {
            image,
            index: 0,
            tick_numerator,
            tick_denominator,
            loops,
        }))
    }

    pub fn rewind(&mut self) {
        self.index = 0;
    }

    pub fn next_frame(&mut self) -> ImageResult<Option<(RgbaImage, Duration)>> {
        if self.index >= self.image.num_loaded_keyframes() {
            return Ok(None);
        }
        let render = self.image.render_frame(self.index).map_err(jxl_error)?;
        self.index += 1;
        let nanos =
            (u128::from(render.duration()) * u128::from(self.tick_denominator) * 1_000_000_000
                / u128::from(self.tick_numerator))
            .max(1_000_000);
        let delay = Duration::from_nanos(u64::try_from(nanos).map_err(jxl_error)?);
        let mut stream = render.stream();
        let (width, height, channels) = (stream.width(), stream.height(), stream.channels());
        if !(1..=4).contains(&channels) {
            return Err(jxl_error("unsupported JXL channels"));
        }
        let mut samples = vec![0u8; width as usize * height as usize * channels as usize];
        if stream.write_to_buffer(&mut samples) != samples.len() {
            return Err(jxl_error("incomplete JXL frame"));
        }
        let mut rgba = RgbaImage::new(width, height);
        for (pixel, source) in rgba
            .pixels_mut()
            .zip(samples.chunks_exact(channels as usize))
        {
            pixel.0 = match channels {
                1 => [source[0], source[0], source[0], 255],
                2 => [source[0], source[0], source[0], source[1]],
                3 => [source[0], source[1], source[2], 255],
                _ => [source[0], source[1], source[2], source[3]],
            };
        }
        Ok(Some((rgba, delay)))
    }
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
    if is_jxl(bytes) {
        let Some(mut animation) = JxlAnimation::from_bytes(bytes)? else {
            return Ok(None);
        };
        if animation.frame_count() > max_frames {
            return Err(jxl_error("animation has too many frames"));
        }
        let mut frames = Vec::new();
        while frames.len() < max_frames {
            let Some((image, delay)) = animation.next_frame()? else {
                break;
            };
            frames.push((
                fit_animation_frame(DynamicImage::ImageRgba8(image), max, square),
                delay,
            ));
            let total: usize = frames.iter().map(|(image, _)| image.as_raw().len()).sum();
            if total > 128 * 1024 * 1024 {
                return Err(jxl_error("animation frames exceed memory limit"));
            }
        }
        return Ok((frames.len() > 1).then_some(frames));
    }
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
    let mut decoded = Vec::new();
    let mut total = 0usize;
    for frame in frames {
        if decoded.len() == max_frames {
            return Err(jxl_error("animation has too many frames"));
        }
        let frame = frame?;
        let delay = frame_delay(frame.delay().into());
        let image = fit_animation_frame(DynamicImage::ImageRgba8(frame.into_buffer()), max, square);
        total = total.saturating_add(image.as_raw().len());
        if total > 128 * 1024 * 1024 {
            return Err(jxl_error("animation exceeds memory limit"));
        }
        decoded.push((image, delay));
    }

    Ok((decoded.len() > 1).then_some(decoded))
}

fn fit_animation_frame(mut image: DynamicImage, max: (u32, u32), square: bool) -> RgbaImage {
    if square {
        let side = image.width().min(image.height());
        image = image.crop_imm(
            (image.width() - side) / 2,
            (image.height() - side) / 2,
            side,
            side,
        );
    }
    fit(image, max)
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
        assert!(small_animation(&gif, (3, 3), 2, false).is_err());
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
