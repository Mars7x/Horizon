//! Lossless screenshot and Album thumbnail encoding through runtime libjxl.
use std::{fs::File, io, os::fd::AsRawFd, time::Duration};

#[repr(C)]
struct NativeFrame {
    rgba: *const u8,
    length: usize,
    duration_ms: u32,
}

unsafe extern "C" {
    fn horizon_jxl_write(
        fd: std::ffi::c_int,
        frames: *const NativeFrame,
        count: usize,
        width: u32,
        height: u32,
        effort: std::ffi::c_int,
    ) -> std::ffi::c_int;
}

/// libjxl's effort: 7 for files kept long (profiles, artwork, thumbnails),
/// where size matters; 2 for game screenshots and state previews, which must
/// be quick even for a 4× DS frame. Both are lossless.
const EFFORT: std::ffi::c_int = 7;
const QUICK_EFFORT: std::ffi::c_int = 2;

/// Write a single lossless sRGB RGBA8 frame on a media worker. The caller
/// owns atomic publication and cleanup; this function never closes the file.
pub(crate) fn write_rgba(file: &File, rgba: &[u8], width: u32, height: u32) -> io::Result<()> {
    encode(file, &[(rgba, Duration::ZERO)], width, height, EFFORT)
}

/// `write_rgba`, encoding quickly: for game screenshots and state previews.
pub(crate) fn write_rgba_quickly(
    file: &File,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> io::Result<()> {
    encode(file, &[(rgba, Duration::ZERO)], width, height, QUICK_EFFORT)
}

/// Lossless full-frame animation with millisecond timing and looping, matching
/// Horizon's profile/Album playback policy. Stills contain one untimed frame.
pub(crate) fn write_frames(
    file: &File,
    frames: &[(&[u8], Duration)],
    width: u32,
    height: u32,
) -> io::Result<()> {
    encode(file, frames, width, height, EFFORT)
}

fn encode(
    file: &File,
    frames: &[(&[u8], Duration)],
    width: u32,
    height: u32,
    effort: std::ffi::c_int,
) -> io::Result<()> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4));
    let total = expected.and_then(|n| n.checked_mul(frames.len()));
    if width == 0
        || height == 0
        || frames.is_empty()
        || frames.len() > 600
        || total.is_none_or(|n| n > 128 * 1024 * 1024)
        || frames.iter().any(|(pixels, delay)| {
            expected != Some(pixels.len())
                || (frames.len() > 1
                    && (delay.as_millis() == 0 || delay.as_millis() > u32::MAX as u128))
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid JXL frames",
        ));
    }
    let native: Vec<_> = frames
        .iter()
        .map(|(pixels, delay)| NativeFrame {
            rgba: pixels.as_ptr(),
            length: pixels.len(),
            duration_ms: delay.as_millis() as u32,
        })
        .collect();
    // SAFETY: the file, frame descriptors and validated pixel slices remain
    // alive for this synchronous call. C borrows them and keeps no pointers.
    let error = unsafe {
        horizon_jxl_write(
            file.as_raw_fd(),
            native.as_ptr(),
            native.len(),
            width,
            height,
            effort,
        )
    };
    if error == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_round_trip_preserves_frames_alpha_and_timing() {
        let path =
            std::env::temp_dir().join(format!("horizon-jxl-animation-{}.jxl", std::process::id()));
        let images: Vec<_> = [[255, 0, 0, 255], [0, 255, 0, 64], [90, 80, 70, 0]]
            .into_iter()
            .map(|pixel| image::RgbaImage::from_pixel(8, 4, image::Rgba(pixel)))
            .collect();
        let delays = [20, 50, 90].map(Duration::from_millis);
        let frames: Vec<_> = images
            .iter()
            .zip(delays)
            .map(|(image, delay)| (image.as_raw().as_slice(), delay))
            .collect();
        let file = File::create(&path).unwrap();
        write_frames(&file, &frames, 8, 4).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let decoded = crate::services::image::small_animation(&bytes, (8, 4), 600, false)
            .unwrap()
            .unwrap();
        assert_eq!(decoded.len(), 3);
        for ((actual, delay), (expected, original_delay)) in
            decoded.iter().zip(images.iter().zip(delays))
        {
            assert_eq!(actual, expected);
            assert_eq!(*delay, original_delay);
        }
        let truncated = &bytes[..bytes.len() / 2];
        assert!(crate::services::image::JxlAnimation::from_bytes(truncated).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn lossless_round_trip_preserves_rgba_including_invisible_colours() {
        let path =
            std::env::temp_dir().join(format!("horizon-jxl-encode-{}.jxl", std::process::id()));
        let original = image::RgbaImage::from_fn(240, 160, |x, y| {
            image::Rgba([x as u8, y as u8, (x ^ y) as u8, (x + y) as u8])
        });
        let file = File::create(&path).unwrap();
        write_rgba(&file, original.as_raw(), 240, 160).unwrap();
        let decoded = crate::services::image::decode_file(&path)
            .unwrap()
            .into_rgba8();
        assert_eq!(decoded, original);
        assert!(write_rgba(&file, &[0; 3], 1, 1).is_err());
        assert!(write_rgba(&file, &[], 0, 0).is_err());
        let read_only = File::open(&path).unwrap();
        assert!(write_rgba(&read_only, &[0, 0, 0, 255], 1, 1).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
