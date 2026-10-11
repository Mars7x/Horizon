//! Decoding through FFmpeg, via Horizon's C shim (`csrc/horizon_media.c`).
//!
//! The runtime ships FFmpeg (the full codec set comes with the Freedesktop
//! runtime's codecs-extra extension); nothing is bundled. The shim keeps
//! FFmpeg's struct layouts in C, compiled against the SDK's headers, and
//! exposes a few flat functions: open, seek, and "next decoded unit".
use std::{
    ffi::{CString, c_char, c_int},
    os::unix::ffi::OsStrExt,
    path::Path,
    ptr::NonNull,
    time::Duration,
};

#[repr(C)]
struct RawInfo {
    duration_us: i64,
    has_video: c_int,
    has_audio: c_int,
    hardware: c_int,
}

#[repr(C)]
struct RawUnit {
    pixels: *const u8,
    width: c_int,
    height: c_int,
    samples: *const f32,
    frames: c_int,
    pts_us: i64,
    duration_us: i64,
}

enum HzMedia {}

const HZ_ERROR: c_int = -1;
const HZ_END: c_int = 0;
const HZ_VIDEO: c_int = 1;
const HZ_AUDIO: c_int = 2;

unsafe extern "C" {
    fn hz_media_open(
        path: *const c_char,
        box_width: c_int,
        box_height: c_int,
        hardware: c_int,
        want_audio: c_int,
        out_rate: c_int,
        out_channels: c_int,
        error: *mut c_char,
        error_length: usize,
    ) -> *mut HzMedia;
    fn hz_media_info_get(media: *const HzMedia, info: *mut RawInfo);
    fn hz_media_seek(media: *mut HzMedia, position_us: i64, accurate: c_int) -> c_int;
    fn hz_media_next(media: *mut HzMedia, unit: *mut RawUnit) -> c_int;
    fn hz_media_close(media: *mut HzMedia);
}

/// What a file holds.
#[derive(Debug, Clone, Copy)]
pub struct MediaInfo {
    pub duration: Option<Duration>,
    pub has_audio: bool,
    /// The video decodes on the GPU (VA-API).
    pub hardware: bool,
}

/// One decoded unit, borrowed until the next call on its `Media`.
pub enum Unit<'a> {
    /// Packed RGBA, `width * 4` bytes per row.
    Video {
        pixels: &'a [u8],
        width: u32,
        height: u32,
        position: Option<Duration>,
        duration: Option<Duration>,
    },
    /// Interleaved float samples at the requested rate and channels.
    Audio {
        samples: &'a [f32],
        position: Option<Duration>,
    },
    End,
}

/// An open video file. Used by one thread at a time.
pub struct Media {
    raw: NonNull<HzMedia>,
    channels: usize,
}

// The shim's state is only touched through `&mut self` (or `&self` for the
// read-only info), from whichever single thread owns the Media.
unsafe impl Send for Media {}

impl Media {
    /// Opens `path`. Frames are scaled down (never up) to fit `max`. With
    /// `hardware`, the video may decode on the GPU (for playback; one
    /// poster frame isn't worth starting it). With `audio`, sound is decoded
    /// and resampled to that (rate, channels).
    pub fn open(
        path: &Path,
        max: (u32, u32),
        hardware: bool,
        audio: Option<(u32, u32)>,
    ) -> Result<Self, String> {
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| "the path contains a NUL byte".to_owned())?;
        let (rate, channels) = audio.unwrap_or((0, 0));
        let mut error = [0 as c_char; 256];
        // SAFETY: valid C string and error buffer for the call's duration.
        let raw = unsafe {
            hz_media_open(
                path.as_ptr(),
                clamp(max.0),
                clamp(max.1),
                c_int::from(hardware),
                c_int::from(audio.is_some()),
                clamp(rate),
                clamp(channels),
                error.as_mut_ptr(),
                error.len(),
            )
        };
        NonNull::new(raw)
            .map(|raw| Self {
                raw,
                channels: channels as usize,
            })
            .ok_or_else(|| {
                // SAFETY: the shim writes a NUL-terminated message (or leaves zeros).
                unsafe { std::ffi::CStr::from_ptr(error.as_ptr()) }
                    .to_string_lossy()
                    .into_owned()
            })
    }

    pub fn info(&self) -> MediaInfo {
        let mut info = RawInfo {
            duration_us: -1,
            has_video: 0,
            has_audio: 0,
            hardware: 0,
        };
        // SAFETY: `raw` is open; `info` is a valid out-pointer.
        unsafe { hz_media_info_get(self.raw.as_ptr(), &mut info) };
        MediaInfo {
            duration: micros(info.duration_us),
            has_audio: info.has_audio != 0,
            hardware: info.hardware != 0,
        }
    }

    /// Seeks to the keyframe at or before `position`. With `accurate`, units
    /// before `position` are decoded and skipped.
    pub fn seek(&mut self, position: Duration, accurate: bool) -> Result<(), String> {
        let micros = i64::try_from(position.as_micros()).unwrap_or(i64::MAX);
        // SAFETY: `raw` is open and used by this thread only.
        match unsafe { hz_media_seek(self.raw.as_ptr(), micros, c_int::from(accurate)) } {
            HZ_ERROR => Err("seek failed".into()),
            _ => Ok(()),
        }
    }

    /// Decodes the next unit (video or audio, in file order) or the end.
    pub fn decode_next(&mut self) -> Result<Unit<'_>, String> {
        let mut unit = RawUnit {
            pixels: std::ptr::null(),
            width: 0,
            height: 0,
            samples: std::ptr::null(),
            frames: 0,
            pts_us: -1,
            duration_us: -1,
        };
        // SAFETY: `raw` is open; the unit's buffers live until the next call,
        // which the returned borrow of `self` enforces.
        match unsafe { hz_media_next(self.raw.as_ptr(), &mut unit) } {
            HZ_END => Ok(Unit::End),
            HZ_VIDEO => {
                let (width, height) = (unit.width.max(0) as u32, unit.height.max(0) as u32);
                let length = width as usize * height as usize * 4;
                Ok(Unit::Video {
                    // SAFETY: the shim filled `length` bytes of packed RGBA.
                    pixels: unsafe { std::slice::from_raw_parts(unit.pixels, length) },
                    width,
                    height,
                    position: micros(unit.pts_us),
                    duration: micros(unit.duration_us),
                })
            }
            HZ_AUDIO => Ok(Unit::Audio {
                // SAFETY: the shim filled `frames * channels` floats, at the
                // channel count this Media was opened with.
                samples: unsafe {
                    std::slice::from_raw_parts(
                        unit.samples,
                        unit.frames.max(0) as usize * self.channels,
                    )
                },
                position: micros(unit.pts_us),
            }),
            _ => Err("the file could not be decoded".into()),
        }
    }
}

impl Drop for Media {
    fn drop(&mut self) {
        // SAFETY: `raw` came from hz_media_open and is closed once.
        unsafe { hz_media_close(self.raw.as_ptr()) };
    }
}

fn clamp(value: u32) -> c_int {
    c_int::try_from(value).unwrap_or(c_int::MAX)
}

fn micros(value: i64) -> Option<Duration> {
    u64::try_from(value).ok().map(Duration::from_micros)
}
