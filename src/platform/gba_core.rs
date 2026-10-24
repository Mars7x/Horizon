//! Narrow mGBA libretro host for GB, GBC and GBA games. All core calls and
//! callbacks stay on one worker.
use crate::domain::core_settings::PlaybackSpeed;
use crate::domain::core_settings::{GameBoyPalette, MgbaPreferences};
use libloading::Library;
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::{
    cell::RefCell,
    ffi::{CStr, c_char, c_void},
    path::Path,
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU16, AtomicU64, Ordering},
    },
};

#[derive(Clone, Default)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub smooth: bool,
}
/// A filtered frame ready to show: Slint's own buffer, so the UI hands it to
/// an `Image` without copying it again.
pub struct Presented {
    pub pixels: SharedPixelBuffer<Rgba8Pixel>,
    pub smooth: bool,
    /// Show at exactly this size in physical pixels, pixelated: the
    /// buffer divides it evenly. None: fit to the window.
    pub exact: Option<(u32, u32)>,
    /// A DS frame: the top screen's (width, height) at the buffer's top,
    /// then the bottom screen's below it, each filtered at its own size.
    pub screens: Option<[(u32, u32); 2]>,
}

/// The DS screen arrangement shared with the filter thread: the layout's
/// index, with bit 4 set while the screens are swapped.
pub fn pack_ds_view(layout: crate::domain::core_settings::DsLayout, swapped: bool) -> u8 {
    layout.index() | (u8::from(swapped) << 4)
}
pub fn unpack_ds_view(value: u8) -> (crate::domain::core_settings::DsLayout, bool) {
    (
        crate::domain::core_settings::DsLayout::from_index(value & 0x0F),
        value & 0x10 != 0,
    )
}
/// The player's size in physical pixels, `width << 32 | height`; 0 unknown.
pub type DisplaySize = Arc<AtomicU64>;
pub type LatestFrame = Arc<Mutex<Option<Presented>>>;
pub type AudioBuffer = Arc<super::gba_audio::GameAudio>;

/// A packed snapshot keeps the pointer's coordinates and press coherent across threads.
#[derive(Default)]
pub struct DsInput {
    pub pointer: AtomicU64,
    /// Circle Pad X/Y, then C-stick X/Y, as packed signed libretro axes.
    pub analog: AtomicU64,
    /// The screen arrangement (see `pack_ds_view`), for the filter thread.
    pub view: std::sync::atomic::AtomicU8,
}

struct Callbacks {
    audio: AudioBuffer,
    buttons: Arc<AtomicU16>,
    pixel_format: u32,
    preferences: MgbaPreferences,
    ds: bool,
    three_ds: bool,
    vulkan: Option<super::azahar_vulkan::Backend>,
    error: Option<String>,
    extra: Arc<DsInput>,
    directory: Option<std::ffi::CString>,
    options: std::collections::BTreeMap<Vec<u8>, std::ffi::CString>,
    /// The core's newest frame, unfiltered (screenshots and states use it).
    raw_frame: Option<Frame>,
    /// `raw_frame` has not been handed to the filter stage yet.
    fresh: bool,
    audio_speed: super::gba_audio::SpeedAudio,
    frames: Arc<AtomicU64>,
    /// melonDS's upscaled DS resolution (1 is native, software rendering).
    scale: u8,
    /// The GPU context an upscaling core draws with, when it has one.
    hardware: Option<Hardware>,
    /// Buttons pressed for the next frame only, beside the player's.
    pulse: u16,
}
thread_local! { static CALLBACKS: RefCell<Option<Callbacks>> = const { RefCell::new(None) }; }

/// A core drawing with OpenGL (libretro's hardware rendering).
struct Hardware {
    gl: super::gl_context::GlContext,
    version: (u32, u32),
    /// The core's own context callbacks, once it asked for the context.
    reset: Option<extern "C" fn()>,
    destroy: Option<extern "C" fn()>,
    stencil: bool,
    /// The size of a frame the core drew and Horizon hasn't read back yet.
    pending: Option<(u32, u32)>,
    presentation: Frame,
    viewport_failed: bool,
}
// What the core calls during `retro_run`, while CALLBACKS may be in use.
thread_local! {
    static HW_FRAMEBUFFER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static HW_PROC: std::cell::Cell<Option<super::gl_context::ProcAddress>> =
        const { std::cell::Cell::new(None) };
}
extern "C" fn hw_framebuffer() -> usize {
    HW_FRAMEBUFFER.with(std::cell::Cell::get)
}
extern "C" fn hw_proc_address(name: *const c_char) -> *const c_void {
    match HW_PROC.with(std::cell::Cell::get) {
        // SAFETY: the driver's eglGetProcAddress, with the core's symbol name.
        Some(get) if !name.is_null() => unsafe { get(name) },
        _ => std::ptr::null(),
    }
}
/// The top and bottom screens of a 3DS frame of `size`, as Azahar draws them
/// by default (the 320-wide bottom screen centred under the 400-wide top).
fn three_ds_regions((width, height): (u32, u32)) -> Option<[super::azahar_vulkan::Region; 2]> {
    let scale = width / 400;
    (scale > 0 && width == 400 * scale && height == 480 * scale).then(|| {
        let half = 240 * scale;
        [(0, 0, width, half), (40 * scale, half, 320 * scale, half)]
    })
}
/// Every screen fits its region and at least one is smaller: worth shrinking.
fn shrinks(regions: [super::azahar_vulkan::Region; 2], targets: [(u32, u32); 2]) -> bool {
    let pairs = || targets.iter().zip(&regions);
    pairs().all(|(&(tw, th), &(_, _, w, h))| tw > 0 && th > 0 && tw <= w && th <= h)
        && pairs().any(|(&(tw, th), &(_, _, w, h))| tw < w || th < h)
}
/// libretro: the frame is in the framebuffer, not in memory.
const HW_FRAME: usize = usize::MAX;
impl Callbacks {
    /// Read the core's drawn frame back into `raw_frame` if one is waiting.
    fn read_back(&mut self) {
        if let Some(backend) = self.vulkan.as_mut() {
            let Some(host) = backend.host.as_mut().filter(|_| backend.unread) else {
                return;
            };
            backend.unread = false;
            let raw = self.raw_frame.get_or_insert_with(|| Frame {
                width: 0,
                height: 0,
                rgba: Vec::new(),
                smooth: false,
            });
            match host.read_frame(&mut raw.rgba) {
                Ok((width, height)) => {
                    raw.width = width;
                    raw.height = height;
                }
                Err(error) => self.error = Some(error),
            }
            return;
        }
        let Some(hardware) = self.hardware.as_mut() else {
            return;
        };
        let Some(size) = hardware.pending.take() else {
            return;
        };
        let raw = self.raw_frame.get_or_insert_with(|| Frame {
            width: 0,
            height: 0,
            rgba: Vec::new(),
            smooth: false,
        });
        hardware.gl.read(size, &mut raw.rgba);
        raw.width = size.0;
        raw.height = size.1;
    }
}

/// libretro memory ids.
const SAVE_RAM: u32 = 0;
const RTC: u32 = 1;
/// Battery memory and any cartridge clock together; the save files match.
const MAX_SAVE: usize = 128 * 1024;

#[repr(C)]
struct GameInfo {
    path: *const c_char,
    data: *const c_void,
    size: usize,
    meta: *const c_char,
}
#[repr(C)]
#[derive(Default)]
pub struct AvInfo {
    pub base_width: u32,
    pub base_height: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub aspect_ratio: f32,
    pub fps: f64,
    pub sample_rate: f64,
}

pub struct Core {
    // Function pointers cannot outlive this library. Drop deinitializes first.
    _library: Library,
    serialize_size: unsafe extern "C" fn() -> usize,
    serialize: unsafe extern "C" fn(*mut c_void, usize) -> bool,
    unserialize: unsafe extern "C" fn(*const c_void, usize) -> bool,
    pub state_identity: String,
    ds: bool,
    three_ds: bool,
    run: unsafe extern "C" fn(),
    reset: unsafe extern "C" fn(),
    unload: unsafe extern "C" fn(),
    deinit: unsafe extern "C" fn(),
    memory: unsafe extern "C" fn(u32) -> *mut c_void,
    memory_size: unsafe extern "C" fn(u32) -> usize,
    pub av: AvInfo,
    pub frames: Arc<AtomicU64>,
}

impl Core {
    #[cfg(test)]
    pub fn load(
        path: &Path,
        rom: &[u8],
        preferences: MgbaPreferences,
        audio: AudioBuffer,
        buttons: Arc<AtomicU16>,
    ) -> Result<Self, String> {
        Self::load_system(path, rom, preferences, audio, buttons, None)
    }

    pub fn load_system(
        path: &Path,
        rom: &[u8],
        preferences: MgbaPreferences,
        audio: AudioBuffer,
        buttons: Arc<AtomicU16>,
        extra: Option<(Arc<DsInput>, &Path, u8)>,
    ) -> Result<Self, String> {
        Self::load_content(path, rom, preferences, audio, buttons, extra, None)
    }
    pub fn load_3ds(
        path: &Path,
        content: &Path,
        directory: &Path,
        audio: AudioBuffer,
        buttons: Arc<AtomicU16>,
        input: Arc<DsInput>,
        resolution: u8,
    ) -> Result<Self, String> {
        Self::load_content(
            path,
            &[],
            MgbaPreferences::default(),
            audio,
            buttons,
            Some((input, directory, resolution)),
            Some(content),
        )
    }
    fn load_content(
        path: &Path,
        rom: &[u8],
        preferences: MgbaPreferences,
        audio: AudioBuffer,
        buttons: Arc<AtomicU16>,
        extra: Option<(Arc<DsInput>, &Path, u8)>,
        content: Option<&Path>,
    ) -> Result<Self, String> {
        let three_ds = content.is_some();
        let ds = extra.is_some() && !three_ds;
        let full_path = content
            .map(|path| {
                use std::os::unix::ffi::OsStrExt;
                std::ffi::CString::new(path.as_os_str().as_bytes())
                    .map_err(|_| "Invalid 3DS game path".to_owned())
            })
            .transpose()?;
        let (extra, directory, scale) = if let Some((input, path, scale)) = extra {
            use std::os::unix::ffi::OsStrExt;
            (
                input,
                Some(
                    std::ffi::CString::new(path.as_os_str().as_bytes())
                        .map_err(|_| "Invalid core data directory")?,
                ),
                scale.max(1),
            )
        } else {
            (Arc::default(), None, 1)
        };
        // Upscaling draws on the graphics card. Without a usable context the
        // game still plays, at native resolution.
        let gl_version = (3, 2);
        let hardware = (scale > 1 && !three_ds)
            .then(|| match super::gl_context::GlContext::new(gl_version.0, gl_version.1) {
                Ok(gl) => Some(Hardware {
                    gl,
                    version: (gl_version.0 as u32, gl_version.1 as u32),
                    reset: None,
                    destroy: None,
                    stencil: false,
                    pending: None,
                    presentation: Frame { width: 0, height: 0, rgba: Vec::new(), smooth: false },
                    viewport_failed: false,
                }),
                Err(error) => {
                    tracing::warn!(%error, "DS upscaling unavailable; playing at native resolution");
                    None
                }
            })
            .flatten();
        // DS upscales only with its GL context; Azahar draws with Vulkan at
        // any of its resolutions.
        let scale = if hardware.is_some() || three_ds {
            scale
        } else {
            1
        };
        HW_PROC.with(|cell| cell.set(hardware.as_ref().map(|h| h.gl.proc_address())));
        // SAFETY: only the application-packaged, pinned mGBA core is loaded;
        // the explicit development override is selected by data_paths.
        let library = unsafe { Library::new(path) }.map_err(|_| {
            "The bundled emulator core is unavailable. Rebuild Horizon with its core.".to_owned()
        })?;
        // SAFETY: symbols use the libretro v1 C ABI. The library is retained.
        unsafe {
            fn symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T, String> {
                // SAFETY: callers provide the exact exported ABI/signature.
                unsafe { library.get::<T>(name).map(|s| *s) }
                    .map_err(|_| "The bundled mGBA core has an incompatible interface".into())
            }
            let version: unsafe extern "C" fn() -> u32 = symbol(&library, b"retro_api_version\0")?;
            if version() != 1 {
                return Err("Unsupported mGBA core interface".into());
            }
            let init: unsafe extern "C" fn() = symbol(&library, b"retro_init\0")?;
            let load: unsafe extern "C" fn(*const GameInfo) -> bool =
                symbol(&library, b"retro_load_game\0")?;
            let get_av: unsafe extern "C" fn(*mut AvInfo) =
                symbol(&library, b"retro_get_system_av_info\0")?;
            let set_environment: unsafe extern "C" fn(extern "C" fn(u32, *mut c_void) -> bool) =
                symbol(&library, b"retro_set_environment\0")?;
            let set_video: unsafe extern "C" fn(extern "C" fn(*const c_void, u32, u32, usize)) =
                symbol(&library, b"retro_set_video_refresh\0")?;
            let set_audio: unsafe extern "C" fn(extern "C" fn(i16, i16)) =
                symbol(&library, b"retro_set_audio_sample\0")?;
            let set_batch: unsafe extern "C" fn(extern "C" fn(*const i16, usize) -> usize) =
                symbol(&library, b"retro_set_audio_sample_batch\0")?;
            let set_poll: unsafe extern "C" fn(extern "C" fn()) =
                symbol(&library, b"retro_set_input_poll\0")?;
            let set_input: unsafe extern "C" fn(extern "C" fn(u32, u32, u32, u32) -> i16) =
                symbol(&library, b"retro_set_input_state\0")?;
            #[repr(C)]
            struct SystemInfo {
                name: *const std::ffi::c_char,
                version: *const std::ffi::c_char,
                extensions: *const std::ffi::c_char,
                need_fullpath: bool,
                block_extract: bool,
            }
            let get_info: unsafe extern "C" fn(*mut SystemInfo) =
                symbol(&library, b"retro_get_system_info\0")?;
            let mut info = SystemInfo {
                name: std::ptr::null(),
                version: std::ptr::null(),
                extensions: std::ptr::null(),
                need_fullpath: false,
                block_extract: false,
            };
            get_info(&mut info);
            if info.name.is_null() || info.version.is_null() {
                return Err("Core version is unavailable".into());
            }
            // SAFETY: libretro guarantees NUL-terminated strings valid for the library lifetime.
            let core_version = format!(
                "{}@{}",
                CStr::from_ptr(info.name).to_string_lossy(),
                CStr::from_ptr(info.version).to_string_lossy()
            );
            let mut core = Self {
                serialize_size: symbol(&library, b"retro_serialize_size\0")?,
                serialize: symbol(&library, b"retro_serialize\0")?,
                unserialize: symbol(&library, b"retro_unserialize\0")?,
                state_identity: {
                    use std::io::Read;
                    let mut bytes = Vec::new();
                    std::fs::File::open(path)
                        .map_err(|_| "Could not inspect the core build")?
                        .take(64 * 1024 * 1024 + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|_| "Could not inspect the core build")?;
                    if bytes.len() > 64 * 1024 * 1024 {
                        return Err("Core build is too large".into());
                    }
                    let digest: String = ring::digest::digest(&ring::digest::SHA256, &bytes)
                        .as_ref()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect();
                    format!("{core_version}:{digest}")
                },
                run: symbol(&library, b"retro_run\0")?,
                reset: symbol(&library, b"retro_reset\0")?,
                unload: symbol(&library, b"retro_unload_game\0")?,
                deinit: symbol(&library, b"retro_deinit\0")?,
                memory: symbol(&library, b"retro_get_memory_data\0")?,
                memory_size: symbol(&library, b"retro_get_memory_size\0")?,
                ds,
                three_ds,
                _library: library,
                av: AvInfo::default(),
                frames: Arc::new(AtomicU64::new(0)),
            };
            CALLBACKS.with(|state| {
                *state.borrow_mut() = Some(Callbacks {
                    audio,
                    buttons,
                    pixel_format: 0,
                    preferences,
                    ds,
                    three_ds,
                    vulkan: three_ds.then(super::azahar_vulkan::Backend::default),
                    error: None,
                    extra,
                    directory,
                    options: Default::default(),
                    raw_frame: None,
                    fresh: false,
                    audio_speed: super::gba_audio::SpeedAudio::default(),
                    frames: Arc::clone(&core.frames),
                    scale,
                    hardware,
                    pulse: 0,
                })
            });
            set_environment(environment);
            set_video(video);
            set_audio(audio_sample);
            set_batch(audio_batch);
            set_poll(input_poll);
            set_input(input_state);
            init();
            let info = GameInfo {
                path: if let Some(path) = &full_path {
                    path.as_ptr()
                } else if ds {
                    c"game.nds".as_ptr()
                } else {
                    ptr::null()
                },
                data: rom.as_ptr().cast(),
                size: rom.len(),
                meta: ptr::null(),
            };
            let loading = std::time::Instant::now();
            if !load(&info) {
                return Err("The core could not load this game".into());
            }
            let loaded = loading.elapsed();
            get_av(&mut core.av);
            // The core drew nothing yet: give it its framebuffer and context.
            let reset = if three_ds {
                CALLBACKS.with(|state| -> Result<_, String> {
                    let mut state = state.borrow_mut();
                    let backend = state
                        .as_mut()
                        .and_then(|s| s.vulkan.as_mut())
                        .ok_or("3DS Vulkan backend is unavailable")?;
                    let negotiation = backend
                        .negotiation
                        .ok_or("Azahar did not negotiate Vulkan")?;
                    backend.host = Some(super::azahar_vulkan::Host::new(negotiation)?);
                    Ok(backend.reset)
                })?
            } else {
                CALLBACKS.with(|state| {
                    let mut state = state.borrow_mut();
                    let hardware = state.as_mut()?.hardware.as_mut()?;
                    let size = (
                        core.av.max_width.clamp(256, 4096),
                        core.av.max_height.clamp(384, 4096),
                    );
                    match hardware.gl.framebuffer(size, hardware.stencil) {
                        Ok(name) => HW_FRAMEBUFFER.with(|cell| cell.set(name as usize)),
                        Err(error) => tracing::warn!(%error, "DS upscaling framebuffer"),
                    }
                    hardware.reset
                })
            };
            let resetting = std::time::Instant::now();
            if let Some(reset) = reset {
                reset();
            }
            tracing::debug!(
                load_ms = loaded.as_millis(),
                context_ms = resetting.elapsed().as_millis(),
                "core started its game"
            );
            if !core.av.fps.is_finite()
                || !(30.0..=120.0).contains(&core.av.fps)
                || !core.av.sample_rate.is_finite()
                || !(8000.0..=96000.0).contains(&core.av.sample_rate)
            {
                return Err("The core returned invalid playback timing".into());
            }
            CALLBACKS.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    state.audio.configure(core.av.sample_rate.round() as u32);
                }
            });
            Ok(core)
        }
    }
    pub fn take_error(&self) -> Option<String> {
        CALLBACKS.with(|state| state.borrow_mut().as_mut()?.error.take())
    }
    pub fn supports_states(&self) -> bool {
        // Azahar's size query creates a complete state (about a second), so
        // it is never asked: Azahar always has states.
        if self.three_ds {
            return true;
        }
        // SAFETY: retained core on its worker.
        let size = unsafe { (self.serialize_size)() };
        size > 0 && size <= self.max_state_size()
    }
    /// States cheap enough to take every half-second (rewind) and in the
    /// background (timed autosaves). A 3DS state takes about a second: only
    /// manual slots, quick save and the resume point on exit.
    pub fn supports_frequent_states(&self) -> bool {
        !self.three_ds && self.supports_states()
    }
    fn max_state_size(&self) -> usize {
        if self.three_ds {
            128 * 1024 * 1024
        } else if self.ds {
            64 * 1024 * 1024
        } else {
            16 * 1024 * 1024
        }
    }
    pub fn max_save_size(&self) -> usize {
        if self.ds { 8 * 1024 * 1024 } else { MAX_SAVE }
    }
    pub fn serialize_state(&self) -> Result<Vec<u8>, String> {
        // SAFETY: libretro v1 symbols from the retained core, on its worker.
        let size = unsafe { (self.serialize_size)() };
        if size == 0 || size > self.max_state_size() {
            return Err("The core returned an invalid state size".into());
        }
        let mut bytes = vec![0; size];
        if !unsafe { (self.serialize)(bytes.as_mut_ptr().cast(), bytes.len()) } {
            return Err("The core could not save this state".into());
        }
        Ok(bytes)
    }
    pub fn restore_state(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() || bytes.len() > self.max_state_size() {
            return Err("Invalid saved state size".into());
        }
        // A 3DS rollback would be a second full state (about a second).
        // Store bundles are checksummed and Azahar checks the game and build
        // before reading one, and a load that fails still ends the session.
        if self.three_ds {
            return self.unserialize_with(bytes, None);
        }
        let rollback = self.serialize_state()?;
        self.unserialize_with(bytes, Some(&rollback))
    }
    /// Restore a state into a game that has only just started: nothing to
    /// roll back to (a failure ends the launch), so no state is taken first.
    pub fn restore_launch_state(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() || bytes.len() > self.max_state_size() {
            return Err("Invalid saved state size".into());
        }
        self.unserialize_with(bytes, None)
    }
    fn unserialize_with(&mut self, bytes: &[u8], rollback: Option<&[u8]>) -> Result<(), String> {
        let Some(rollback) = rollback else {
            // SAFETY: validated bounded buffer, retained core, same worker.
            return if unsafe { (self.unserialize)(bytes.as_ptr().cast(), bytes.len()) } {
                Ok(())
            } else {
                Err("The core rejected the saved state".into())
            };
        };
        // SAFETY: validated bounded buffers, retained core, same worker.
        if !unsafe { (self.unserialize)(bytes.as_ptr().cast(), bytes.len()) } {
            let restored = unsafe { (self.unserialize)(rollback.as_ptr().cast(), rollback.len()) };
            return Err(if restored {
                "The core rejected this state; the previous session was restored"
            } else {
                "The core rejected this state and could not restore its previous session"
            }
            .into());
        }
        Ok(())
    }
    pub fn restart(&mut self) {
        // SAFETY: same worker and retained library as every other core call.
        unsafe { (self.reset)() }
    }
    pub fn set_speed(&mut self, speed: PlaybackSpeed) {
        CALLBACKS.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.audio_speed.set_speed(speed);
            }
        });
    }
    pub fn capture_frame(&self) -> Result<Frame, String> {
        CALLBACKS
            .with(|state| {
                let mut state = state.borrow_mut();
                let state = state.as_mut()?;
                state.read_back();
                state.raw_frame.clone()
            })
            .ok_or_else(|| "No game frame is available yet".into())
    }
    /// A bounded timeline picture, without transferring an upscaled DS frame
    /// or disturbing the pending full-resolution screenshot/state capture.
    pub fn capture_rewind_preview(&self) -> Result<Frame, String> {
        let preview = CALLBACKS.with(|state| {
            let mut state = state.borrow_mut();
            let state = state.as_mut()?;
            let raw_size = state.raw_frame.as_ref().map(|raw| (raw.width, raw.height));
            let hardware = state.hardware.as_mut()?;
            let source = hardware.pending.or(raw_size)?;
            if hardware.viewport_failed || source.0 <= 256 || source.1 <= 384 {
                return None;
            }
            let mut rgba = Vec::new();
            match hardware.gl.read_viewport(source, [(256, 192); 2], &mut rgba) {
                Ok(()) => Some(Frame { width: 256, height: 384, rgba, smooth: false }),
                Err(error) => {
                    hardware.viewport_failed = true;
                    tracing::warn!(%error, "DS rewind preview readback unavailable; retaining full-resolution capture");
                    None
                }
            }
        });
        if let Some(preview) = preview {
            return Ok(preview);
        }
        self.capture_frame()
    }
    pub fn run_frame(&mut self) {
        unsafe { (self.run)() }
        CALLBACKS.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.pulse = 0;
            }
        });
    }
    /// Press `mask` (libretro joypad bits) for the next frame only: the DS
    /// lid toggles on melonDS's L2 + Y.
    pub fn pulse_buttons(&mut self, mask: u16) {
        CALLBACKS.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.pulse = mask;
            }
        });
    }
    /// Hands the newest frame to the filter stage, once. False when the core
    /// has not drawn a new one since.
    pub fn hand_off(&self, stage: &super::gba_present::FrameStage, display: &DisplaySize) -> bool {
        CALLBACKS.with(|state| {
            let Ok(mut state) = state.try_borrow_mut() else {
                return false;
            };
            let Some(state) = state.as_mut() else {
                return false;
            };
            if !state.fresh {
                return false;
            }
            // A 3DS frame is shrunk on the GPU to the size each screen is
            // shown at, then read: at 6× a full frame is 27 MB.
            if state.vulkan.is_some() {
                if !stage.wanted() {
                    return false;
                }
                let size = display.load(Ordering::Acquire);
                let (layout, swapped) = unpack_ds_view(state.extra.view.load(Ordering::Acquire));
                let targets = layout
                    .place(
                        swapped,
                        ((size >> 32) as u32 as f64, size as u32 as f64),
                        crate::domain::core_settings::DsLayout::THREE_DS_SCREENS,
                    )
                    .map(|rect| (rect.width.round() as u32, rect.height.round() as u32));
                if let Some(backend) = state.vulkan.as_mut()
                    && !backend.shrink_failed
                    && let Some(host) = backend.host.as_mut()
                    && let Some(regions) = host.current_size().and_then(three_ds_regions)
                    && shrinks(regions, targets)
                {
                    match host.read_shrunk(regions, targets, &mut backend.presentation.rgba) {
                        Ok((width, height)) => {
                            backend.presentation.width = width;
                            backend.presentation.height = height;
                            stage.submit_viewport(&backend.presentation, targets);
                            state.fresh = false;
                            // The full frame stays readable for screenshots.
                            return true;
                        }
                        Err(error) => {
                            backend.shrink_failed = true;
                            tracing::warn!(%error, "3DS frames can't be shrunk on the GPU; reading them at full size");
                        }
                    }
                }
                state.read_back();
            } else if state.hardware.is_some() {
                if !stage.wanted() {
                    return false;
                }
                if state.ds && state.preferences.video_filter == crate::domain::core_settings::VideoFilter::None {
                    let size = display.load(Ordering::Acquire);
                    let (layout, swapped) = unpack_ds_view(state.extra.view.load(Ordering::Acquire));
                    let targets = layout.screens(swapped, ((size >> 32) as u32 as f64, size as u32 as f64))
                        .map(|rect| (rect.width.round() as u32, rect.height.round() as u32));
                    let raw_size = state.raw_frame.as_ref().map(|raw| (raw.width, raw.height));
                    if let Some(hardware) = state.hardware.as_mut()
                        && !hardware.viewport_failed
                        && let Some(source) = hardware.pending.or(raw_size)
                        && let Some(screens) = super::gba_video::ds_viewport_sizes(source, targets)
                    {
                        match hardware.gl.read_viewport(source, screens, &mut hardware.presentation.rgba) {
                            Ok(()) => {
                                hardware.presentation.width = screens[0].0.max(screens[1].0);
                                hardware.presentation.height = screens[0].1 + screens[1].1;
                                stage.submit_viewport(&hardware.presentation, screens);
                                state.fresh = false;
                                // Keep the full GPU frame pending for captures/rewind.
                                return true;
                            }
                            Err(error) => {
                                hardware.viewport_failed = true;
                                tracing::warn!(%error, "DS viewport readback unavailable; retaining full-resolution presentation");
                            }
                        }
                    }
                }
                state.read_back();
            }
            let Some(raw) = state.raw_frame.as_ref() else {
                return false;
            };
            stage.submit(raw);
            state.fresh = false;
            true
        })
    }
    /// Battery memory, then a Game Boy cartridge clock when the game has
    /// one: the same .sav layout standalone mGBA writes.
    fn save_regions(&self) -> Result<[(*mut u8, usize); 2], String> {
        // Azahar owns its SDMC/NAND files, not libretro battery-memory slots.
        if self.three_ds {
            return Ok([(ptr::null_mut(), 0); 2]);
        }
        // SAFETY: libretro v1 symbols from the retained core, on its worker.
        let regions = [SAVE_RAM, RTC]
            .map(|id| unsafe { ((self.memory)(id).cast::<u8>(), (self.memory_size)(id)) });
        if regions
            .iter()
            .any(|&(data, size)| size > 0 && data.is_null())
            || regions[0].1 + regions[1].1 > self.max_save_size()
        {
            return Err("mGBA returned invalid save memory".into());
        }
        Ok(regions)
    }
    pub fn save_ram(&self) -> Result<Vec<u8>, String> {
        let regions = self.save_regions()?;
        let mut bytes = Vec::with_capacity(regions[0].1 + regions[1].1);
        for (data, size) in regions.into_iter().filter(|&(_, size)| size > 0) {
            // SAFETY: checked above; copied before the core's next call.
            bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(data, size) });
        }
        Ok(bytes)
    }
    /// Must run before the first frame: mGBA reads a cartridge clock then.
    pub fn restore_save(&mut self, bytes: &[u8]) -> Result<(), String> {
        let [(save, capacity), (clock, clock_size)] = self
            .save_regions()
            .ok()
            .filter(|_| bytes.len() <= self.max_save_size())
            .ok_or("The existing save does not fit this game. It has been preserved.")?;
        let (battery, rest) = bytes.split_at(bytes.len().min(capacity));
        if rest.len() > clock_size {
            return Err("The existing save does not fit this game. It has been preserved.".into());
        }
        // SAFETY: lengths checked against the core's allocations above; the
        // input is a distinct Vec. A save without a clock leaves it as is.
        unsafe {
            ptr::copy_nonoverlapping(battery.as_ptr(), save, battery.len());
            if !rest.is_empty() {
                ptr::copy_nonoverlapping(rest.as_ptr(), clock, rest.len());
            }
        }
        Ok(())
    }
}
impl Drop for Core {
    fn drop(&mut self) {
        // A GPU-drawing core lets go of its context first, while it's current.
        let destroy = CALLBACKS.with(|state| {
            state.borrow().as_ref().and_then(|state| {
                state
                    .vulkan
                    .as_ref()
                    .and_then(|v| v.destroy)
                    .or_else(|| state.hardware.as_ref().and_then(|h| h.destroy))
            })
        });
        if let Some(destroy) = destroy {
            destroy();
        }
        // SAFETY: lifecycle stays on the same worker; library is still alive.
        unsafe {
            (self.unload)();
            (self.deinit)();
        }
        CALLBACKS.with(|state| *state.borrow_mut() = None);
        HW_FRAMEBUFFER.with(|cell| cell.set(0));
        HW_PROC.with(|cell| cell.set(None));
    }
}

unsafe extern "C" {
    fn hz_retro_log_set_sink(sink: extern "C" fn(level: u32, message: *const c_char));
    /// Variadic printf-style; defined in `csrc/horizon_retro_log.c`.
    fn hz_retro_log(level: u32, format: *const c_char, ...);
}

/// libretro's language for the system locale (LC_ALL, LC_MESSAGES, LANG);
/// English when unknown.
fn system_language() -> u32 {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        .unwrap_or_default();
    language_for_locale(&locale)
}

fn language_for_locale(locale: &str) -> u32 {
    let locale = locale.to_ascii_lowercase();
    let (language, region) = locale
        .split(['.', '@'])
        .next()
        .unwrap_or("")
        .split_once(['_', '-'])
        .unwrap_or((locale.split(['.', '@']).next().unwrap_or(""), ""));
    match (language, region) {
        ("ja", _) => 1,
        ("fr", _) => 2,
        ("es", _) => 3,
        ("de", _) => 4,
        ("it", _) => 5,
        ("nl", _) => 6,
        ("pt", "br") => 7,
        ("pt", _) => 8,
        ("ru", _) => 9,
        ("ko", _) => 10,
        ("zh", "tw" | "hk" | "mo") => 11,
        ("zh", _) => 12,
        _ => 0,
    }
}

/// Core log lines join Horizon's log. Cores are chatty at debug and info,
/// so those stay at debug and trace; warnings and errors stay visible.
extern "C" fn core_log(level: u32, message: *const c_char) {
    if message.is_null() {
        return;
    }
    // SAFETY: the shim passes a NUL-terminated buffer valid for this call.
    let message = unsafe { std::ffi::CStr::from_ptr(message) }.to_string_lossy();
    // Notes about features Horizon doesn't offer (Wi-Fi, a host microphone,
    // newer option formats, a VFS, cheat memory maps) are expected on every
    // launch: they stay in debug logs, so real problems stand out.
    const EXPECTED: [&str; 7] = [
        "PCap: init failed",
        "Failed to enumerate Wi-fi adapters",
        "Microphone interface not available",
        "core options not supported",
        "Could not get VFS interface",
        "Frontend refused the memory map",
        "Failed to get language from frontend",
    ];
    if level >= 2 && EXPECTED.iter().any(|expected| message.contains(expected)) {
        tracing::debug!(target: "horizon::core", "{message}");
        return;
    }
    // Azahar tags each line with its own level ("… Service.FS <Error> …").
    // Its warnings and errors are mostly routine for an emulator (stubbed
    // services, files a fresh console lacks): debug, except its renderer's
    // warnings and anything Critical, which stay visible.
    if level >= 2 && (message.contains(" <Warning> ") || message.contains(" <Error> ")) {
        if message.contains("Render.Vulkan") && message.contains(" <Error> ") {
            tracing::warn!(target: "horizon::core", "{message}");
        } else {
            tracing::debug!(target: "horizon::core", "{message}");
        }
        return;
    }
    match level {
        0 => tracing::trace!(target: "horizon::core", "{message}"),
        1 => tracing::debug!(target: "horizon::core", "{message}"),
        2 => tracing::warn!(target: "horizon::core", "{message}"),
        _ => tracing::error!(target: "horizon::core", "{message}"),
    }
}

extern "C" fn environment(command: u32, data: *mut c_void) -> bool {
    if data.is_null() {
        return false;
    }
    match command {
        9 | 31 => CALLBACKS.with(|state| {
            let state = state.borrow();
            let Some(directory) = state.as_ref().and_then(|s| s.directory.as_ref()) else {
                return false;
            };
            // SAFETY: GET_SYSTEM_DIRECTORY / GET_SAVE_DIRECTORY provide a char**.
            unsafe {
                *data.cast::<*const c_char>() = directory.as_ptr();
            }
            true
        }),
        16 => CALLBACKS.with(|state| {
            #[repr(C)]
            struct Variable {
                key: *const c_char,
                value: *const c_char,
            }
            let mut state = state.borrow_mut();
            let Some(state) = state.as_mut() else {
                return false;
            };
            // SAFETY: the pinned core supplies a null-terminated variable array.
            // Bound traversal, and copy values before returning to the core.
            for index in 0..256 {
                let entry = unsafe { &*data.cast::<Variable>().add(index) };
                if entry.key.is_null() {
                    return true;
                }
                if entry.value.is_null() {
                    return false;
                }
                let key = unsafe { CStr::from_ptr(entry.key) }.to_bytes().to_vec();
                let value = unsafe { CStr::from_ptr(entry.value) }.to_string_lossy();
                if let Some((_, choices)) = value.split_once("; ")
                    && let Some(first) = choices.split('|').next()
                    && let Ok(value) = std::ffi::CString::new(first)
                {
                    state.options.insert(key, value);
                }
            }
            false
        }),
        // SET_PIXEL_FORMAT: accept software 0RGB1555, XRGB8888 and RGB565.
        10 => CALLBACKS.with(|state| {
            let Ok(mut state) = state.try_borrow_mut() else {
                return false;
            };
            let Some(state) = state.as_mut() else {
                return false;
            };
            let format = unsafe { *data.cast::<u32>() };
            if format > 2 {
                return false;
            }
            state.pixel_format = format;
            true
        }),
        // GET_VARIABLE: explicitly use mGBA's built-in BIOS implementation.
        15 => unsafe {
            #[repr(C)]
            struct Variable {
                key: *const c_char,
                value: *const c_char,
            }
            let variable = &mut *data.cast::<Variable>();
            if variable.key.is_null() {
                return false;
            }
            let key = CStr::from_ptr(variable.key).to_bytes();
            let registered = CALLBACKS.with(|state| {
                state
                    .borrow()
                    .as_ref()
                    .and_then(|s| s.options.get(key))
                    .map(|v| v.as_ptr())
            });
            if CStr::from_ptr(variable.key).to_bytes() == b"mgba_use_bios" {
                variable.value = c"OFF".as_ptr();
                true
            } else if CStr::from_ptr(variable.key).to_bytes() == b"mgba_skip_bios" {
                variable.value = c"ON".as_ptr();
                true
            } else {
                let value = CALLBACKS.with(|state| {
                    let state = state.borrow();
                    let preferences = state.as_ref()?.preferences;
                    let key = CStr::from_ptr(variable.key).to_bytes();
                    if state.as_ref()?.three_ds {
                        return match key {
                            b"citra_graphics_api" => Some(c"Vulkan"),
                            b"citra_use_libretro_save_path" => Some(c"LibRetro Default"),
                            b"citra_layout_option" => Some(c"default"),
                            b"citra_resolution_factor" => [c"1", c"2", c"3", c"4", c"5", c"6"]
                                .get(usize::from(state.as_ref()?.scale.clamp(1, 6) - 1))
                                .copied(),
                            // The right stick is the C-stick only: its
                            // default also drags a touchscreen cursor, and
                            // R3 would touch. Touch is the mouse/pointer's.
                            b"citra_analog_function" => Some(c"c_stick"),
                            // One pointer path: the "mouse" option reads the
                            // same pointer again.
                            b"citra_enable_mouse_touchscreen" => Some(c"disabled"),
                            b"citra_enable_touch_touchscreen" => Some(c"enabled"),
                            _ => None,
                        };
                    }
                    if state.as_ref()?.ds {
                        #[cfg(test)]
                        if let Some(value) = test_ds_option(key) {
                            return Some(&*Box::leak(value.into_boxed_c_str()));
                        }
                        return match key {
                            b"melonds_console_mode" => Some(c"ds"),
                            b"melonds_sysfile_mode" => Some(c"builtin"),
                            b"melonds_boot_mode" => Some(c"direct"),
                            b"melonds_render_mode" => Some(if state.as_ref()?.scale > 1 {
                                c"opengl"
                            } else {
                                c"software"
                            }),
                            b"melonds_opengl_resolution" => {
                                [c"1", c"2", c"3", c"4", c"5", c"6", c"7", c"8"]
                                    .get(usize::from(state.as_ref()?.scale.clamp(1, 8) - 1))
                                    .copied()
                            }
                            b"melonds_screen_layout1" => Some(c"top-bottom"),
                            b"melonds_screen_gap" => Some(c"0"),
                            b"melonds_number_of_screen_layouts" => Some(c"1"),
                            // Only the mouse touches; the controller has no
                            // cursor, and the core draws none.
                            b"melonds_touch_mode" => Some(c"touch"),
                            b"melonds_show_cursor" => Some(c"disabled"),
                            // Horizon's Blow control holds the core's mic
                            // button, which plays its built-in blowing sound.
                            b"melonds_mic_input" => Some(c"blow"),
                            b"melonds_mic_input_active" => Some(c"hold"),
                            b"melonds_network_mode"
                            | b"melonds_homebrew_sdcard"
                            | b"melonds_dsi_sdcard" => Some(c"disabled"),
                            _ => None,
                        };
                    }
                    match key {
                        b"mgba_audio_low_pass_filter" => Some(c"disabled"),
                        // The border turns the 160x144 picture into 256x224,
                        // for Super Game Boy games only; off unless chosen.
                        b"mgba_sgb_borders" => Some(if preferences.sgb_border {
                            c"ON"
                        } else {
                            c"OFF"
                        }),
                        b"mgba_gb_colors" => Some(match preferences.game_boy_palette {
                            GameBoyPalette::Grey | GameBoyPalette::Color => c"Grayscale",
                            GameBoyPalette::Green => c"DMG Green",
                            GameBoyPalette::Pocket => c"GB Pocket",
                            GameBoyPalette::Light => c"GB Light",
                        }),
                        // 1: the Game Boy Color's colours for games it has
                        // them for; others fall back to grey, as on hardware.
                        b"mgba_gb_colors_preset" => {
                            Some(if preferences.game_boy_palette == GameBoyPalette::Color {
                                c"1"
                            } else {
                                c"0"
                            })
                        }
                        b"mgba_solar_sensor_level" => Some(
                            [
                                c"0", c"1", c"2", c"3", c"4", c"5", c"6", c"7", c"8", c"9", c"10",
                            ][usize::from(preferences.sunlight.level())],
                        ),
                        _ => None,
                    }
                });
                variable.value = value
                    .map(CStr::as_ptr)
                    .or(registered)
                    .unwrap_or(ptr::null());
                !variable.value.is_null()
            }
        },
        17 => {
            unsafe {
                *data.cast::<bool>() = false;
            }
            true
        } // GET_VARIABLE_UPDATE
        3 => {
            unsafe {
                *data.cast::<bool>() = true;
            }
            true
        } // GET_CAN_DUPE
        14 => CALLBACKS.with(|state| {
            // SET_HW_RENDER: struct retro_hw_render_callback.
            #[repr(C)]
            struct HwRender {
                context_type: u32,
                context_reset: Option<extern "C" fn()>,
                get_current_framebuffer: Option<extern "C" fn() -> usize>,
                get_proc_address: Option<extern "C" fn(*const c_char) -> *const c_void>,
                depth: bool,
                stencil: bool,
                bottom_left_origin: bool,
                version_major: u32,
                version_minor: u32,
                cache_context: bool,
                context_destroy: Option<extern "C" fn()>,
                debug_context: bool,
            }
            let Ok(mut state) = state.try_borrow_mut() else {
                return false;
            };
            // SAFETY: the core passes its retro_hw_render_callback.
            let request = unsafe { &mut *data.cast::<HwRender>() };
            if let Some(backend) = state.as_mut().and_then(|s| s.vulkan.as_mut()) {
                if request.context_type != 6 {
                    return false;
                }
                backend.reset = request.context_reset;
                backend.destroy = request.context_destroy;
                return true;
            }
            let Some(hardware) = state.as_mut().and_then(|s| s.hardware.as_mut()) else {
                return false;
            };
            match request.context_type {
                // NONE: the core gave up on hardware rendering.
                0 => {
                    hardware.reset = None;
                    hardware.destroy = None;
                    true
                }
                // OPENGL or OPENGL_CORE up to the context Horizon made (3.2).
                1 | 3
                    if (request.version_major, request.version_minor) <= hardware.version
                        && request.bottom_left_origin =>
                {
                    request.get_current_framebuffer = Some(hw_framebuffer);
                    request.get_proc_address = Some(hw_proc_address);
                    hardware.reset = request.context_reset;
                    hardware.destroy = request.context_destroy;
                    hardware.stencil = request.stencil;
                    true
                }
                _ => false,
            }
        }),
        // Experimental libretro Vulkan interfaces used by pinned Azahar.
        65577 => CALLBACKS.with(|state| {
            let state = state.borrow();
            let Some(host) = state
                .as_ref()
                .and_then(|s| s.vulkan.as_ref()?.host.as_ref())
            else {
                return false;
            };
            // SAFETY: GET_HW_RENDER_INTERFACE supplies an interface pointer slot.
            unsafe {
                *data.cast::<*const c_void>() = host.interface();
            }
            true
        }),
        65579 => CALLBACKS.with(|state| {
            let mut state = state.borrow_mut();
            let Some(backend) = state.as_mut().and_then(|s| s.vulkan.as_mut()) else {
                return false;
            };
            // SAFETY: pinned Azahar supplies the complete v2 negotiation struct.
            let header = unsafe { std::slice::from_raw_parts(data.cast::<u32>(), 2) };
            if header != [0, 2] {
                return false;
            }
            let negotiation = unsafe { *data.cast::<super::azahar_vulkan::Negotiation>() };
            if !negotiation.supported() {
                return false;
            }
            backend.negotiation = Some(negotiation);
            true
        }),
        56 => CALLBACKS.with(|state| {
            if !state.borrow().as_ref().is_some_and(|s| s.three_ds) {
                return false;
            }
            // SAFETY: GET_PREFERRED_HW_RENDER supplies an enum slot.
            unsafe {
                *data.cast::<u32>() = 6;
            }
            true
        }),
        27 => {
            // GET_LOG_INTERFACE: struct retro_log_callback { retro_log_printf_t log; }.
            // SAFETY: the shim only formats and forwards to `core_log`.
            unsafe {
                hz_retro_log_set_sink(core_log);
                *data.cast::<unsafe extern "C" fn(u32, *const c_char, ...)>() = hz_retro_log;
            }
            true
        }
        39 => {
            // GET_LANGUAGE: the system's, so multi-language DS cartridges
            // start in it (the DS firmware's language).
            unsafe {
                *data.cast::<u32>() = system_language();
            }
            true
        }
        37 => true, // SET_GEOMETRY: software frames carry their actual dimensions.
        8 => true,  // SET_PERFORMANCE_LEVEL (advisory)
        18 | 11 | 35 => true, // options, input descriptors, no-game, controllers
        _ => false, // No disk/network/VFS/graphics/BIOS permissions supplied.
    }
}
extern "C" fn input_poll() {}
extern "C" fn input_state(port: u32, device: u32, index: u32, id: u32) -> i16 {
    if port == 0 && device == 6 && index == 0 && id < 3 {
        return CALLBACKS.with(|state| {
            let state = state.borrow();
            let Some(s) = state.as_ref().filter(|s| s.ds || s.three_ds) else {
                return 0;
            };
            let packed = s.extra.pointer.load(Ordering::Acquire);
            if id == 2 {
                ((packed >> 32) & 1) as i16
            } else {
                (packed >> (id * 16)) as u16 as i16
            }
        });
    }
    if port == 0 && device == 5 && index < 2 && id < 2 {
        return CALLBACKS.with(|state| {
            state
                .borrow()
                .as_ref()
                .filter(|s| s.three_ds)
                .map_or(0, |s| {
                    (s.extra.analog.load(Ordering::Acquire) >> ((index * 2 + id) * 16)) as u16
                        as i16
                })
        });
    }
    if port != 0 || device != 1 || index != 0 || id >= 16 {
        return 0;
    }
    CALLBACKS.with(|state| {
        state
            .try_borrow()
            .ok()
            .and_then(|state| {
                state.as_ref().map(|s| {
                    i16::from((s.buttons.load(Ordering::Acquire) | s.pulse) & (1 << id) != 0)
                })
            })
            .unwrap_or(0)
    })
}
extern "C" fn video(data: *const c_void, width: u32, height: u32, pitch: usize) {
    if data as usize == HW_FRAME {
        // Drawn on the GPU: read back only when needed (see `hand_off`).
        CALLBACKS.with(|state| {
            if let Ok(mut state) = state.try_borrow_mut()
                && let Some(state) = state.as_mut()
                && (1..=4096).contains(&width)
                && (1..=4096).contains(&height)
            {
                if let Some(backend) = state.vulkan.as_mut()
                    && let Some(host) = backend.host.as_mut()
                {
                    // Read later, at the size it's shown (see `hand_off`).
                    if let Err(error) = host.accept_frame(width, height) {
                        state.error = Some(error);
                        return;
                    }
                    backend.unread = true;
                } else if let Some(hardware) = state.hardware.as_mut() {
                    hardware.pending = Some((width, height));
                } else {
                    return;
                }
                state.fresh = true;
                state.frames.fetch_add(1, Ordering::Release);
            }
        });
        return;
    }
    if data.is_null() || width == 0 || height == 0 || width > 1024 || height > 1024 {
        return;
    }
    CALLBACKS.with(|state| {
        let Ok(mut state) = state.try_borrow_mut() else {
            return;
        };
        let Some(state) = state.as_mut() else {
            return;
        };
        let bpp = if state.pixel_format == 1 { 4 } else { 2 };
        let Some(bytes) = pitch
            .checked_mul(height as usize)
            .filter(|n| *n <= 8 * 1024 * 1024)
        else {
            return;
        };
        if pitch < width as usize * bpp {
            return;
        }
        // SAFETY: libretro provides a frame with `height` rows of `pitch` bytes.
        let data = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), bytes) };
        let pixel_format = state.pixel_format;
        let (width, height) = (width as usize, height as usize);
        // Reuse the buffer: frames keep their size, so this rarely allocates.
        let raw = state.raw_frame.get_or_insert_with(|| Frame {
            width: 0,
            height: 0,
            rgba: Vec::new(),
            smooth: false,
        });
        raw.width = width as u32;
        raw.height = height as u32;
        raw.rgba.resize(width * height * 4, 0);
        // One tight loop per pixel format: this runs for every frame, up to
        // hundreds a second when fast-forwarding.
        let rows = raw.rgba.chunks_exact_mut(width * 4).zip(data.chunks(pitch));
        match pixel_format {
            1 => {
                for (row, line) in rows {
                    for (pixel, source) in row
                        .as_chunks_mut::<4>()
                        .0
                        .iter_mut()
                        .zip(line[..width * 4].as_chunks::<4>().0)
                    {
                        let value = u32::from_ne_bytes(*source);
                        *pixel = [(value >> 16) as u8, (value >> 8) as u8, value as u8, 255];
                    }
                }
            }
            format => {
                // RGB565 (2) or 0RGB1555 (0), widened to 8 bits a channel.
                let (green_bits, red_shift) = if format == 2 { (63, 11) } else { (31, 10) };
                for (row, line) in rows {
                    for (pixel, source) in row
                        .as_chunks_mut::<4>()
                        .0
                        .iter_mut()
                        .zip(line[..width * 2].as_chunks::<2>().0)
                    {
                        let value = u16::from_ne_bytes(*source);
                        *pixel = [
                            (((value >> red_shift) & 31) * 255 / 31) as u8,
                            (((value >> 5) & green_bits) * 255 / green_bits) as u8,
                            ((value & 31) * 255 / 31) as u8,
                            255,
                        ];
                    }
                }
            }
        }
        state.fresh = true;
        state.frames.fetch_add(1, Ordering::Release);
    });
}
extern "C" fn audio_sample(left: i16, right: i16) {
    let values = [left, right];
    audio_batch(values.as_ptr(), 1);
}
extern "C" fn audio_batch(data: *const i16, frames: usize) -> usize {
    if data.is_null() || frames > 8192 {
        return 0;
    }
    CALLBACKS.with(|state| {
        let Ok(mut state) = state.try_borrow_mut() else {
            return;
        };
        let Some(state) = state.as_mut() else {
            return;
        };
        let samples = unsafe { std::slice::from_raw_parts(data, frames * 2) };
        for stereo in samples.as_chunks::<2>().0 {
            if let Some(output) = state.audio_speed.process([
                f32::from(stereo[0]) / 32768.0,
                f32::from(stereo[1]) / 32768.0,
            ]) {
                state.audio.push(output);
            }
        }
    });
    frames
}

/// Measurements can try other melonDS options: `HORIZON_DS_OPTIONS` holds
/// `key=value` pairs separated by commas.
#[cfg(test)]
fn test_ds_option(key: &[u8]) -> Option<std::ffi::CString> {
    let options = std::env::var("HORIZON_DS_OPTIONS").ok()?;
    options.split(',').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name.as_bytes() == key).then(|| std::ffi::CString::new(value).ok())?
    })
}

#[cfg(test)]
pub(crate) fn test_rom() -> Vec<u8> {
    // Original ARM7 homebrew: mode 4's palette is green, or red while A is
    // held; its low byte is written to cartridge SRAM. Channel 2 plays a tone.
    // No BIOS/logo/game data is included.
    let mut rom = vec![0; 1024];
    rom[..4].copy_from_slice(&0xEA00002Eu32.to_le_bytes());
    rom[0xA0..0xAC].copy_from_slice(b"HORIZON TEST");
    rom[0xB2] = 0x96;
    let literals = [
        0x04000000u32,
        0x0404,
        0x05000000,
        0x04000130,
        0x0E000000,
        0x001F,
        0x03E0,
        0x0080,
        0x2277,
        0x0002,
        0xF080,
        0x86D6,
    ];
    let ldr = |position: usize, register: u32, literal: usize, condition: u32| {
        condition << 28
            | 0x059F0000
            | register << 12
            | ((23 + literal) * 4 - position * 4 - 8) as u32
    };
    let words = [
        ldr(0, 0, 0, 14),
        ldr(1, 1, 1, 14),
        0xE1C010B0,
        ldr(3, 2, 2, 14),
        ldr(4, 3, 3, 14),
        ldr(5, 7, 4, 14),
        ldr(6, 1, 7, 14),
        0xE1C018B4,
        ldr(8, 1, 8, 14),
        0xE1C018B0,
        ldr(10, 1, 9, 14),
        0xE1C018B2,
        ldr(12, 1, 10, 14),
        0xE1C016B8,
        ldr(14, 1, 11, 14),
        0xE1C016BC,
        0xE1D340B0,
        0xE3140001,
        ldr(18, 5, 5, 0),
        ldr(19, 5, 6, 1),
        0xE1C250B0,
        0xE5C75000,
        0xEAFFFFF8,
    ];
    for (index, word) in words.iter().chain(literals.iter()).enumerate() {
        rom[192 + index * 4..196 + index * 4].copy_from_slice(&word.to_le_bytes());
    }
    rom
}

/// Original Game Boy homebrew that idles with the screen on. Only the
/// 4-byte header signature mGBA checks is included, no logo or game data.
/// `clock` makes it an MBC3 cartridge with a clock and 32 KiB battery RAM;
/// `sgb` marks it Super Game Boy enhanced.
#[cfg(test)]
pub(crate) fn test_gb_rom(color: bool, clock: bool, sgb: bool) -> Vec<u8> {
    let mut rom = vec![0; 32 * 1024];
    // nop; jp 0x150 / at 0x150: jr -2
    rom[0x100..0x104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
    rom[0x104..0x108].copy_from_slice(&[0xCE, 0xED, 0x66, 0x66]);
    rom[0x134..0x140].copy_from_slice(b"HORIZON TEST");
    rom[0x143] = if color { 0xC0 } else { 0 };
    if sgb {
        rom[0x146] = 0x03;
        rom[0x14B] = 0x33;
    }
    if clock {
        rom[0x147] = 0x10;
        rom[0x149] = 0x03;
    }
    rom[0x150..0x152].copy_from_slice(&[0x18, 0xFE]);
    rom
}

#[cfg(test)]
/// Game Boy homebrew that switches its screen off and on every few
/// frames, as games do while loading.
pub(crate) fn test_gb_toggling_rom() -> Vec<u8> {
    let mut rom = test_gb_rom(false, false, false);
    let program = [
        0x3E, 0x11, 0xE0, 0x40, // ld a,0x11; ldh (LCDC),a: screen off
        0x01, 0x00, 0x20, // ld bc,0x2000
        0x0B, 0x78, 0xB1, 0x20, 0xFB, // delay: dec bc; ld a,b; or c; jr nz
        0x3E, 0x91, 0xE0, 0x40, // ld a,0x91; ldh (LCDC),a: screen on
        0x01, 0x00, 0x20, // ld bc,0x2000
        0x0B, 0x78, 0xB1, 0x20, 0xFB, // delay
        0x18, 0xE6, // jr back to the start
    ];
    rom[0x150..0x150 + program.len()].copy_from_slice(&program);
    rom
}

/// Original, logo-free DS test program: two colored backdrops and idle ARM7.
#[cfg(test)]
pub(crate) fn test_ds_rom() -> Vec<u8> {
    let mut rom = vec![0; 0x10000];
    rom[..12].copy_from_slice(b"HORIZON TEST");
    rom[12..16].copy_from_slice(b"####");
    let header = [
        (0x20, 0x200u32),
        (0x24, 0x02000000),
        (0x28, 0x02000000),
        (0x2c, 0x100),
        (0x30, 0x400),
        (0x34, 0x03800000),
        (0x38, 0x03800000),
        (0x3c, 4),
        (0x80, 0x10000),
        (0x84, 0x200),
    ];
    for (offset, value) in header {
        rom[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    // Write power control, both display controls, then distinct palette colors.
    let writes = [
        (0x04000304u32, 0x820fu32),
        (0x04000000, 0x10000),
        (0x04001000, 0x10000),
        (0x05000000, 0x001f),
        (0x05000400, 0x7c00),
    ];
    let mut words = Vec::new();
    for (i, _) in writes.iter().enumerate() {
        let pc = i * 12;
        let literal = 64 + i * 8;
        words.push(0xE59F0000u32 | (literal - pc - 8) as u32);
        words.push(0xE59F1000u32 | (literal - pc - 8) as u32);
        words.push(0xE5801000); // str r1,[r0]
    }
    words.push(0xEAFFFFFE); // b .
    for (address, value) in writes {
        words.extend([address, value]);
    }
    for (i, word) in words.into_iter().enumerate() {
        rom[0x200 + i * 4..0x204 + i * 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x400..0x404].copy_from_slice(&0xEAFFFFFEu32.to_le_bytes());
    rom
}

#[cfg(test)]
mod tests {
    #[test]
    fn three_ds_frames_shrink_only_when_both_screens_fit() {
        use super::{shrinks, three_ds_regions};
        assert_eq!(
            three_ds_regions((800, 960)),
            Some([(0, 0, 800, 480), (80, 480, 640, 480)])
        );
        assert_eq!(three_ds_regions((256, 384)), None);
        assert_eq!(three_ds_regions((400, 481)), None);
        let regions = three_ds_regions((2400, 2880)).unwrap();
        assert!(shrinks(regions, [(900, 540), (720, 540)]));
        // Larger than drawn, unknown, or already the drawn size: read whole.
        assert!(!shrinks(regions, [(2401, 1440), (720, 540)]));
        assert!(!shrinks(regions, [(0, 0), (0, 0)]));
        assert!(!shrinks(regions, [(2400, 1440), (1920, 1440)]));
    }

    #[test]
    #[ignore = "requires Azahar, a decrypted HORIZON_3DS_ROM and fresh HORIZON_3DS_TEST_DIR"]
    fn real_3ds_core_boots_without_polling_save_states() {
        use super::*;
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();
        let directory = std::path::PathBuf::from(std::env::var_os("HORIZON_3DS_TEST_DIR").unwrap());
        std::fs::create_dir_all(&directory).unwrap();
        let audio = Arc::new(super::super::gba_audio::GameAudio::default());
        let mut core = Core::load_3ds(
            &std::path::PathBuf::from(std::env::var_os("HORIZON_3DS_CORE").unwrap()),
            &std::path::PathBuf::from(std::env::var_os("HORIZON_3DS_ROM").unwrap()),
            &directory,
            Arc::clone(&audio),
            Arc::new(AtomicU16::new(0)),
            Arc::new(DsInput::default()),
            std::env::var("HORIZON_3DS_SCALE")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1),
        )
        .unwrap();
        // States, but never the frequent ones (rewind, timed autosaves).
        assert!(core.supports_states() && !core.supports_frequent_states());
        for _ in 0..600 {
            let started = std::time::Instant::now();
            core.run_frame();
            assert_eq!(core.take_error(), None);
            audio.clear();
            std::thread::sleep(
                std::time::Duration::from_secs_f64(1.0 / core.av.fps)
                    .saturating_sub(started.elapsed()),
            );
        }
        let frame = core.capture_frame().unwrap();
        assert!(frame.width >= 400 && frame.height >= 240);
        assert!(core.frames.load(Ordering::Acquire) >= 600);
        // Shrunk on the GPU, each screen matches the CPU's average of the
        // full frame: at half size (one linear blit) and at a quarter (a
        // halving first).
        let regions = three_ds_regions((frame.width, frame.height)).unwrap();
        for divisor in [2, 4] {
            let targets = regions.map(|(_, _, w, h)| (w / divisor, h / divisor));
            let mut shrunk = Vec::new();
            let size = CALLBACKS
                .with(|state| {
                    let mut state = state.borrow_mut();
                    let host = state.as_mut()?.vulkan.as_mut()?.host.as_mut()?;
                    Some(host.read_shrunk(regions, targets, &mut shrunk))
                })
                .unwrap()
                .unwrap();
            assert_eq!(size, (targets[0].0, targets[0].1 + targets[1].1));
            let mut worst = 0;
            let mut below = 0;
            for (&(x0, y0, _, _), &(tw, th)) in regions.iter().zip(&targets) {
                for ty in 0..th {
                    for tx in 0..tw {
                        for channel in 0..3 {
                            let mut sum = 0u32;
                            for dy in 0..divisor {
                                for dx in 0..divisor {
                                    let (x, y) = (x0 + tx * divisor + dx, y0 + ty * divisor + dy);
                                    sum += u32::from(
                                        frame.rgba[((y * frame.width + x) * 4 + channel) as usize],
                                    );
                                }
                            }
                            let expected = sum / (divisor * divisor);
                            let actual = u32::from(
                                shrunk[(((below + ty) * size.0 + tx) * 4 + channel) as usize],
                            );
                            worst = worst.max(expected.abs_diff(actual));
                        }
                    }
                }
                below += th;
            }
            assert!(worst <= 3, "1/{divisor} differs by up to {worst}");
        }
        image::save_buffer(
            directory.join("boot.png"),
            &frame.rgba,
            frame.width,
            frame.height,
            image::ColorType::Rgba8,
        )
        .unwrap();
        assert!(
            frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[..3].iter().any(|&channel| channel > 16)),
            "game must produce a visible picture"
        );
        // A real state round trip: saved, played past, restored.
        let state = core.serialize_state().unwrap();
        assert!(state.len() > 1024 * 1024);
        for _ in 0..30 {
            core.run_frame();
            audio.clear();
        }
        core.restore_state(&state).unwrap();
        core.run_frame();
        assert_eq!(core.take_error(), None);
        assert!(core.restore_state(&state[..state.len() / 2]).is_err());
        core.run_frame();
        assert_eq!(
            core.take_error(),
            None,
            "a rejected state leaves the game running"
        );
    }
    use super::*;

    #[test]
    #[ignore = "requires the pinned melonDS DS shared library"]
    fn real_ds_core_frames_states_and_restart() {
        let path = std::env::var_os("HORIZON_DS_CORE").unwrap();
        let rom = test_ds_rom();
        let directory =
            std::env::temp_dir().join(format!("horizon-ds-core-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let audio = Arc::new(super::super::gba_audio::GameAudio::default());
        let mut core = Core::load_system(
            Path::new(&path),
            &rom,
            MgbaPreferences::default(),
            Arc::clone(&audio),
            Arc::new(AtomicU16::new(0)),
            Some((Arc::new(DsInput::default()), &directory, 1)),
        )
        .unwrap();
        for _ in 0..120 {
            core.run_frame();
            audio.clear();
        }
        let frame = core.capture_frame().unwrap();
        assert_eq!((frame.width, frame.height), (256, 384));
        assert!(frame.rgba[0] >= 248 && frame.rgba[1] == 0 && frame.rgba[2] == 0);
        assert_eq!(&frame.rgba[256 * 192 * 4..256 * 192 * 4 + 2], &[0, 0]);
        assert!(frame.rgba[256 * 192 * 4 + 2] >= 248);
        assert!(core.frames.load(Ordering::Acquire) >= 120);
        let state = core.serialize_state().unwrap();
        core.run_frame();
        core.restore_state(&state).unwrap();
        core.restart();
        for _ in 0..5 {
            core.run_frame();
        }
        assert_eq!(core.capture_frame().unwrap().height, 384);
        drop(core);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires pinned melonDS DS and packaged EGL/OpenGL driver"]
    fn real_ds_gpu_viewport_keeps_full_resolution_captures() {
        let path = std::env::var_os("HORIZON_DS_CORE").unwrap();
        let directory =
            std::env::temp_dir().join(format!("horizon-ds-viewport-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let audio = Arc::new(super::super::gba_audio::GameAudio::default());
        let mut core = Core::load_system(
            Path::new(&path),
            &test_ds_rom(),
            MgbaPreferences {
                video_filter: crate::domain::core_settings::VideoFilter::None,
                ..Default::default()
            },
            Arc::clone(&audio),
            Arc::new(AtomicU16::new(0)),
            Some((Arc::new(DsInput::default()), &directory, 8)),
        )
        .unwrap();
        for _ in 0..120 {
            core.run_frame();
            audio.clear();
        }
        let stage = super::super::gba_present::FrameStage::new();
        let display = Arc::new(AtomicU64::new((1280u64 << 32) | 720));
        assert!(core.hand_off(&stage, &display));
        CALLBACKS.with(|state| {
            let state = state.borrow();
            let hardware = state.as_ref().unwrap().hardware.as_ref().unwrap();
            assert!(!hardware.viewport_failed);
            assert_eq!(
                (hardware.presentation.width, hardware.presentation.height),
                (640, 960)
            );
            assert_eq!(hardware.pending, Some((2048, 3072)));
        });
        let preview = core.capture_rewind_preview().unwrap();
        assert_eq!((preview.width, preview.height), (256, 384));
        assert!(preview.rgba[0] >= 248 && preview.rgba[1] == 0 && preview.rgba[2] == 0);
        assert!(preview.rgba[256 * 192 * 4 + 2] >= 248);
        CALLBACKS.with(|state| {
            let state = state.borrow();
            let state = state.as_ref().unwrap();
            assert!(
                state.raw_frame.is_none(),
                "preview must not force a full readback"
            );
            assert_eq!(state.hardware.as_ref().unwrap().pending, Some((2048, 3072)));
        });
        let frame = core.capture_frame().unwrap();
        assert_eq!((frame.width, frame.height), (2048, 3072));
        assert!(frame.rgba[0] >= 248 && frame.rgba[1] == 0 && frame.rgba[2] == 0);
        assert!(frame.rgba[2048 * 1536 * 4 + 2] >= 248);
        assert_eq!(core.capture_rewind_preview().unwrap().rgba, preview.rgba);
        let saved = core.serialize_state().unwrap();
        core.run_frame();
        core.restore_state(&saved).unwrap();
        core.run_frame();
        stage.want();
        assert!(core.hand_off(&stage, &display));
        assert_eq!(core.capture_frame().unwrap().width, 2048);
        // The upstream core can fall back to software while retaining its GL
        // context. A native software frame must never read that old GPU image.
        CALLBACKS.with(|state| {
            let mut state = state.borrow_mut();
            let state = state.as_mut().unwrap();
            state.hardware.as_mut().unwrap().pending = None;
            state.raw_frame = Some(Frame {
                width: 256,
                height: 384,
                rgba: [0, 255, 0, 255].repeat(256 * 384),
                smooth: false,
            });
        });
        assert_eq!(
            &core.capture_rewind_preview().unwrap().rgba[..4],
            &[0, 255, 0, 255]
        );
        drop(core);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn locales_map_to_libretro_languages() {
        assert_eq!(language_for_locale("en_US.UTF-8"), 0);
        assert_eq!(language_for_locale("de_DE.UTF-8"), 4);
        assert_eq!(language_for_locale("fr"), 2);
        assert_eq!(language_for_locale("pt_BR.UTF-8"), 7);
        assert_eq!(language_for_locale("zh_TW.UTF-8"), 11);
        assert_eq!(language_for_locale("zh_CN.UTF-8"), 12);
        assert_eq!(language_for_locale("C.UTF-8"), 0);
        assert_eq!(language_for_locale(""), 0);
    }

    #[test]
    #[ignore = "measurement; requires HORIZON_DS_CORE and a real cartridge in HORIZON_DS_ROM"]
    fn real_ds_core_speed_report() {
        let path = std::env::var_os("HORIZON_DS_CORE").unwrap();
        let rom = std::fs::read(std::env::var_os("HORIZON_DS_ROM").unwrap()).unwrap();
        let directory =
            std::env::temp_dir().join(format!("horizon-ds-speed-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let audio = Arc::new(super::super::gba_audio::GameAudio::default());
        let mut core = Core::load_system(
            Path::new(&path),
            &rom,
            MgbaPreferences::default(),
            Arc::clone(&audio),
            Arc::new(AtomicU16::new(0)),
            Some((
                Arc::new(DsInput::default()),
                &directory,
                std::env::var("HORIZON_DS_SCALE")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1),
            )),
        )
        .unwrap();
        for _ in 0..600 {
            core.run_frame();
            audio.clear();
        }
        for round in 0..3 {
            let started = std::time::Instant::now();
            for _ in 0..600 {
                core.run_frame();
                audio.clear();
            }
            let rate = 600.0 / started.elapsed().as_secs_f64();
            eprintln!(
                "DS speed round {round}: {rate:.0} frames/s ({:.2}x)",
                rate / 59.83
            );
        }
        let frame = core.capture_frame().unwrap();
        eprintln!("DS speed frame {}x{}", frame.width, frame.height);
        if let Some(path) = std::env::var_os("HORIZON_DS_SHOT") {
            image::save_buffer(
                path,
                &frame.rgba,
                frame.width,
                frame.height,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
        drop(core);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "measurement; requires the built pinned mGBA core with Game Boy support"]
    fn real_core_game_boy_audio_per_frame_report() {
        let path = std::env::var_os("HORIZON_GBA_CORE").unwrap();
        for (name, rom) in [
            ("steady", test_gb_rom(false, false, false)),
            ("screen toggling", test_gb_toggling_rom()),
        ] {
            let audio = Arc::new(super::super::gba_audio::GameAudio::default());
            let mut core = Core::load(
                Path::new(&path),
                &rom,
                MgbaPreferences::default(),
                Arc::clone(&audio),
                Arc::new(AtomicU16::new(0)),
            )
            .unwrap();
            let mut counts = Vec::new();
            for _ in 0..300 {
                audio.clear();
                core.run_frame();
                counts.push(audio.len());
            }
            let (min, max) = (counts.iter().min().unwrap(), counts.iter().max().unwrap());
            let unusual = counts.iter().filter(|c| c.abs_diff(549) > 10).count();
            eprintln!(
                "AUDIO {name}: per frame min {min} max {max}, {unusual} of 300 frames off by more than 10"
            );
            eprintln!("AUDIO {name}: first 40 {:?}", &counts[..40]);
        }
    }

    #[test]
    #[ignore = "requires the built pinned mGBA core with Game Boy support"]
    fn real_core_plays_game_boy_games_at_their_own_size_and_keeps_their_clock() {
        let path = std::env::var_os("HORIZON_GBA_CORE").unwrap();
        for (color, sgb) in [(false, false), (true, false), (false, true)] {
            let mut core = Core::load(
                Path::new(&path),
                &test_gb_rom(color, true, sgb),
                MgbaPreferences::default(),
                Arc::new(super::super::gba_audio::GameAudio::default()),
                Arc::new(AtomicU16::new(0)),
            )
            .unwrap();
            assert_eq!((core.av.base_width, core.av.base_height), (160, 144));
            assert!((core.av.fps - 59.73).abs() < 0.01, "{}", core.av.fps);
            // Battery RAM, then the clock, as one save.
            let saved = core.save_ram().unwrap();
            assert_eq!(saved.len(), 32 * 1024 + 48);
            core.restore_save(&saved).unwrap();
            core.restore_save(&saved[..32 * 1024]).unwrap();
            assert!(core.restore_save(&[0; 32 * 1024 + 49]).is_err());
            for _ in 0..10 {
                core.run_frame();
            }
            // No Super Game Boy border: every game keeps 160x144.
            assert_eq!(core.capture_frame().unwrap().width, 160);
            assert_eq!(core.capture_frame().unwrap().height, 144);
            assert!(core.frames.load(Ordering::Acquire) >= 10);
            assert!(core.serialize_state().is_ok());
        }
        // A cartridge without a clock saves its battery RAM only.
        let core = Core::load(
            Path::new(&path),
            &{
                let mut rom = test_gb_rom(false, false, false);
                rom[0x147] = 0x03;
                rom[0x149] = 0x02;
                rom
            },
            MgbaPreferences::default(),
            Arc::new(super::super::gba_audio::GameAudio::default()),
            Arc::new(AtomicU16::new(0)),
        )
        .unwrap();
        assert_eq!(core.save_ram().unwrap().len(), 8 * 1024);
    }
    #[test]
    #[ignore = "requires the built pinned mGBA core with Game Boy support"]
    fn real_core_applies_game_boy_palette_and_super_game_boy_border() {
        let path = std::env::var_os("HORIZON_GBA_CORE").unwrap();
        let picture = |rom: Vec<u8>, preferences| {
            let mut core = Core::load(
                Path::new(&path),
                &rom,
                preferences,
                Arc::new(super::super::gba_audio::GameAudio::default()),
                Arc::new(AtomicU16::new(0)),
            )
            .unwrap();
            for _ in 0..10 {
                core.run_frame();
            }
            core.capture_frame().unwrap()
        };
        let grey = picture(test_gb_rom(false, false, false), MgbaPreferences::default());
        let green = picture(
            test_gb_rom(false, false, false),
            MgbaPreferences {
                game_boy_palette: GameBoyPalette::Green,
                ..MgbaPreferences::default()
            },
        );
        let centre = ((72 * 160 + 80) * 4) as usize;
        let (grey, green) = (
            &grey.rgba[centre..centre + 3],
            &green.rgba[centre..centre + 3],
        );
        // RGB565 output: neutral within one green step.
        assert!(
            grey[0].abs_diff(grey[1]) <= 6 && grey[0] == grey[2],
            "{grey:?}"
        );
        assert!(green[1] > green[0] && green[1] > green[2], "{green:?}");
        let bordered = picture(
            test_gb_rom(false, false, true),
            MgbaPreferences {
                sgb_border: true,
                ..MgbaPreferences::default()
            },
        );
        assert_eq!((bordered.width, bordered.height), (256, 224));
    }

    #[test]
    #[ignore = "requires the built pinned mGBA core"]
    fn real_core_state_restores_machine_and_rejects_damage_without_changing_it() {
        let path = std::env::var_os("HORIZON_GBA_CORE").unwrap();
        let mut core = Core::load(
            Path::new(&path),
            &test_rom(),
            MgbaPreferences::default(),
            Arc::new(super::super::gba_audio::GameAudio::default()),
            Arc::new(AtomicU16::new(0)),
        )
        .unwrap();
        for _ in 0..10 {
            core.run_frame();
        }
        let first = core.serialize_state().unwrap();
        for _ in 0..30 {
            core.run_frame();
        }
        let later = core.serialize_state().unwrap();
        // Pinned mGBA format: frame counter at 0x1fc, CPU registers at
        // 0x20..0x60, palette/VRAM/working RAM at 0x800..0x61000.
        // Audio bookkeeping is normalized during restore, so compare machine
        // progress and memory rather than requiring byte-identical containers.
        let counter = |bytes: &[u8]| u32::from_le_bytes(bytes[0x1fc..0x200].try_into().unwrap());
        assert!(counter(&later) > counter(&first));
        core.restore_state(&first).unwrap();
        let restored = core.serialize_state().unwrap();
        assert_eq!(counter(&restored), counter(&first));
        assert!(
            restored[0x20..0x60] == first[0x20..0x60],
            "CPU registers differ"
        );
        assert!(
            restored[0x800..0x61000] == first[0x800..0x61000],
            "Game memory differs"
        );
        assert!(core.restore_state(b"damaged").is_err());
        let recovered = core.serialize_state().unwrap();
        assert_eq!(counter(&recovered), counter(&first));
        assert!(
            recovered[0x20..0x60] == first[0x20..0x60],
            "Rollback CPU differs"
        );
        assert!(
            recovered[0x800..0x61000] == first[0x800..0x61000],
            "Rollback memory differs"
        );
        assert!(core.state_identity.starts_with("mGBA@"));
    }

    #[test]
    #[ignore = "requires the built pinned mGBA core"]
    fn real_core_applies_sunlight_and_keeps_unfiltered_audio() {
        let path = std::env::var_os("HORIZON_GBA_CORE").unwrap();
        let capture = |preferences: MgbaPreferences| {
            let audio = Arc::new(super::super::gba_audio::GameAudio::default());
            let mut core = Core::load(
                Path::new(&path),
                &test_rom(),
                preferences,
                Arc::clone(&audio),
                Arc::new(AtomicU16::new(0)),
            )
            .unwrap();
            #[repr(C)]
            struct Variable {
                key: *const c_char,
                value: *const c_char,
            }
            let mut variable = Variable {
                key: c"mgba_solar_sensor_level".as_ptr(),
                value: ptr::null(),
            };
            assert!(environment(15, (&mut variable as *mut Variable).cast()));
            assert_eq!(
                unsafe { CStr::from_ptr(variable.value) }.to_str().unwrap(),
                preferences.sunlight.level().to_string()
            );
            for _ in 0..10 {
                core.run_frame();
            }
            let samples: Vec<f32> = audio.drain();
            samples
        };
        let unfiltered = capture(MgbaPreferences::default());
        let filtered = capture(MgbaPreferences {
            sunlight: 7.try_into().unwrap(),
            ..MgbaPreferences::default()
        });
        assert_eq!(unfiltered.len(), filtered.len());
        assert!(unfiltered.iter().any(|sample| sample.abs() > 0.001));
        assert_eq!(unfiltered, filtered);
    }

    /// The newest frame as the screen would show it, read back as a plain
    /// frame (window size unknown, so xBRZ stays at 6×).
    fn shown(core: &Core, preferences: MgbaPreferences) -> Frame {
        let frame = super::super::gba_video::present(
            &core.capture_frame().unwrap(),
            preferences.color_correction,
            preferences.video_filter,
            (0, 0),
            &mut super::super::gba_video::Scratch::default(),
        );
        Frame {
            width: frame.pixels.width(),
            height: frame.pixels.height(),
            rgba: frame.pixels.as_bytes().to_vec(),
            smooth: frame.smooth,
        }
    }

    #[test]
    #[ignore = "requires the built pinned mGBA core"]
    fn real_core_combines_colour_and_pixel_art_without_changing_capture() {
        let path = std::env::var_os("HORIZON_GBA_CORE").unwrap();
        let preferences = MgbaPreferences {
            color_correction: true,
            video_filter: crate::domain::core_settings::VideoFilter::Xbrz,
            ..MgbaPreferences::default()
        };
        let mut core = Core::load(
            Path::new(&path),
            &test_rom(),
            preferences,
            Arc::new(super::super::gba_audio::GameAudio::default()),
            Arc::new(AtomicU16::new(0)),
        )
        .unwrap();
        for _ in 0..6 {
            core.run_frame();
        }
        let display = shown(&core, preferences);
        let raw = core.capture_frame().unwrap();
        assert_eq!((display.width, display.height), (1440, 960));
        assert_eq!((raw.width, raw.height), (240, 160));
        assert!(raw.rgba[1] > 240 && raw.rgba[0] < 20);
        let centre = ((480 * 1440 + 720) * 4) as usize;
        assert!(display.rgba[centre] > 50);
        assert!(display.rgba[centre + 1] < raw.rgba[1]);
    }

    #[test]
    #[ignore = "requires the built mGBA core (HORIZON_GBA_CORE)"]
    fn real_core_draws_responds_to_held_input_and_restores_battery_memory() {
        let path = std::env::var_os("HORIZON_GBA_CORE").expect("set HORIZON_GBA_CORE");
        let rom = test_rom();
        let audio = Arc::new(super::super::gba_audio::GameAudio::default());
        let buttons = Arc::new(AtomicU16::new(0));
        let mut core = Core::load(
            Path::new(&path),
            &rom,
            MgbaPreferences::default(),
            Arc::clone(&audio),
            Arc::clone(&buttons),
        )
        .unwrap();
        for _ in 0..10 {
            core.run_frame();
        }
        assert_eq!((core.av.base_width, core.av.base_height), (240, 160));
        let image = shown(&core, MgbaPreferences::default());
        assert!(image.rgba[1] > 200);
        assert!(image.rgba[0] < 30);
        buttons.store(1 << 8, Ordering::Release);
        for _ in 0..3 {
            core.run_frame();
        }
        let image = shown(&core, MgbaPreferences::default());
        assert!(image.rgba[0] > 200);
        assert!(image.rgba[1] < 30);
        let saved = core.save_ram().unwrap();
        assert_eq!(saved[0], 31);
        buttons.store(0, Ordering::Release);
        for _ in 0..3 {
            core.run_frame();
        }
        assert!(shown(&core, MgbaPreferences::default()).rgba[1] > 200);
        assert!(!audio.is_empty());
        assert!(audio.drain().iter().any(|sample| sample.abs() > 0.01));
        audio.clear();
        core.set_speed(PlaybackSpeed::Double);
        for _ in 0..3 {
            core.run_frame();
        }
        assert!(audio.drain().iter().any(|sample| sample.abs() > 0.01));
        let capture = core.capture_frame().unwrap();
        assert_eq!((capture.width, capture.height), (240, 160));
        assert!(capture.rgba[1] > 200);
        core.restart();
        core.restore_save(&saved).unwrap();
        assert_eq!(core.save_ram().unwrap()[0], 31);
        core.set_speed(PlaybackSpeed::Normal);
        for _ in 0..3 {
            core.run_frame();
        }
        assert!(!audio.is_empty());
        drop(core);
        let mut reopened = Core::load(
            Path::new(&path),
            &rom,
            MgbaPreferences::default(),
            audio,
            buttons,
        )
        .unwrap();
        reopened.restore_save(&saved).unwrap();
        assert_eq!(reopened.save_ram().unwrap()[0], 31);
    }
}
