//! Sound output straight to PipeWire, via Horizon's C shim
//! (`csrc/horizon_audio.c`). Used for video sound and UI sounds.
//!
//! Each stream runs on its own PipeWire thread, which calls the stream's
//! fill function for every buffer: no queues between Horizon and the
//! graph, and the latency asked of the graph is what you hear. Opening and
//! closing never wait for the server, so a stalled server can't freeze
//! Horizon.
use std::{
    ffi::{CString, c_char, c_int, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    time::Duration,
};

enum HzAudio {}

type RawFill = extern "C" fn(user: *mut c_void, samples: *mut f32, count: u32, delay_ns: i64);

unsafe extern "C" {
    #[allow(clippy::too_many_arguments)]
    fn hz_audio_open(
        name: *const c_char,
        role: *const c_char,
        rate: u32,
        channels: u32,
        latency_frames: u32,
        fill: RawFill,
        user: *mut c_void,
        error: *mut c_char,
        error_length: usize,
    ) -> *mut HzAudio;
    fn hz_audio_set_active(audio: *mut HzAudio, active: c_int);
    fn hz_audio_close(audio: *mut HzAudio);
}

/// Called on PipeWire's real-time thread with the interleaved samples to
/// write and the output latency (how long until the first is heard). It
/// must not block or allocate.
pub type Fill = Box<dyn FnMut(&mut [f32], Duration) + Send>;

/// What a stream is for, so the session manager can route it.
#[derive(Debug, Clone, Copy)]
pub enum Role {
    /// UI feedback sounds.
    Game,
    /// A video's sound.
    Movie,
}

/// A playback stream of float samples.
pub struct AudioStream {
    raw: NonNull<HzAudio>,
    fill: *mut Fill,
}

// The shim locks its own thread loop for every call that touches the stream.
unsafe impl Send for AudioStream {}
unsafe impl Sync for AudioStream {}

extern "C" fn fill_trampoline(user: *mut c_void, samples: *mut f32, count: u32, delay_ns: i64) {
    // SAFETY: `user` is the `Fill` this stream was opened with, alive until
    // the stream is closed; PipeWire hands `count` writable floats.
    let fill = unsafe { &mut *user.cast::<Fill>() };
    let out = unsafe { std::slice::from_raw_parts_mut(samples, count as usize) };
    // Never unwind into C: a panicking fill writes silence.
    let delay = Duration::from_nanos(u64::try_from(delay_ns).unwrap_or(0));
    if catch_unwind(AssertUnwindSafe(|| fill(out, delay))).is_err() {
        out.fill(0.0);
    }
}

impl AudioStream {
    /// Opens a stream of `channels` x float at `rate` Hz. `latency` is how
    /// many frames the graph asks for per cycle (smaller is sooner). The
    /// stream starts active.
    pub fn open(
        name: &str,
        role: Role,
        rate: u32,
        channels: u32,
        latency: u32,
        fill: Fill,
    ) -> Result<Self, String> {
        let name = CString::new(name).map_err(|_| "bad stream name".to_owned())?;
        let role = CString::new(match role {
            Role::Game => "Game",
            Role::Movie => "Movie",
        })
        .map_err(|_| "bad role".to_owned())?;
        let fill = Box::into_raw(Box::new(fill));
        let mut error = [0 as c_char; 256];
        // SAFETY: valid strings, callback and buffer; `fill` outlives the
        // stream (freed in Drop after closing).
        let raw = unsafe {
            hz_audio_open(
                name.as_ptr(),
                role.as_ptr(),
                rate,
                channels,
                latency,
                fill_trampoline,
                fill.cast(),
                error.as_mut_ptr(),
                error.len(),
            )
        };
        match NonNull::new(raw) {
            Some(raw) => Ok(Self { raw, fill }),
            None => {
                // SAFETY: the shim never stored `fill` on failure.
                drop(unsafe { Box::from_raw(fill) });
                // SAFETY: NUL-terminated message (or zeros).
                Err(unsafe { std::ffi::CStr::from_ptr(error.as_ptr()) }
                    .to_string_lossy()
                    .into_owned())
            }
        }
    }

    /// An inactive stream isn't asked for samples, and lets the device sleep.
    pub fn set_active(&self, active: bool) {
        // SAFETY: `raw` is open.
        unsafe { hz_audio_set_active(self.raw.as_ptr(), c_int::from(active)) };
    }
}

impl Drop for AudioStream {
    fn drop(&mut self) {
        // SAFETY: closing stops PipeWire's thread, so `fill` is no longer
        // called when it is freed.
        unsafe {
            hz_audio_close(self.raw.as_ptr());
            drop(Box::from_raw(self.fill));
        }
    }
}
