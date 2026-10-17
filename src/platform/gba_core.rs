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

#[derive(Clone)]
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
}
/// The player's size in physical pixels, `width << 32 | height`; 0 unknown.
pub type DisplaySize = Arc<AtomicU64>;
pub type LatestFrame = Arc<Mutex<Option<Presented>>>;
pub type AudioBuffer = Arc<super::gba_audio::GameAudio>;

struct Callbacks {
    audio: AudioBuffer,
    buttons: Arc<AtomicU16>,
    pixel_format: u32,
    preferences: MgbaPreferences,
    /// The core's newest frame, unfiltered (screenshots and states use it).
    raw_frame: Option<Frame>,
    /// `raw_frame` has not been handed to the filter stage yet.
    fresh: bool,
    audio_speed: super::gba_audio::SpeedAudio,
    frames: Arc<AtomicU64>,
}
thread_local! { static CALLBACKS: RefCell<Option<Callbacks>> = const { RefCell::new(None) }; }

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
    pub fn load(
        path: &Path,
        rom: &[u8],
        preferences: MgbaPreferences,
        audio: AudioBuffer,
        buttons: Arc<AtomicU16>,
    ) -> Result<Self, String> {
        // SAFETY: only the application-packaged, pinned mGBA core is loaded;
        // the explicit development override is selected by data_paths.
        let library = unsafe { Library::new(path) }.map_err(|_| {
            "The bundled mGBA core is unavailable. Rebuild Horizon with its core.".to_owned()
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
                    raw_frame: None,
                    fresh: false,
                    audio_speed: super::gba_audio::SpeedAudio::default(),
                    frames: Arc::clone(&core.frames),
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
                path: ptr::null(),
                data: rom.as_ptr().cast(),
                size: rom.len(),
                meta: ptr::null(),
            };
            if !load(&info) {
                return Err("mGBA could not load this game".into());
            }
            get_av(&mut core.av);
            if !core.av.fps.is_finite()
                || !(30.0..=120.0).contains(&core.av.fps)
                || !core.av.sample_rate.is_finite()
                || !(8000.0..=96000.0).contains(&core.av.sample_rate)
            {
                return Err("mGBA returned invalid playback timing".into());
            }
            CALLBACKS.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    state.audio.configure(core.av.sample_rate.round() as u32);
                }
            });
            Ok(core)
        }
    }
    pub fn serialize_state(&self) -> Result<Vec<u8>, String> {
        // SAFETY: libretro v1 symbols from the retained core, on its worker.
        let size = unsafe { (self.serialize_size)() };
        if size == 0 || size > 16 * 1024 * 1024 {
            return Err("The core returned an invalid state size".into());
        }
        let mut bytes = vec![0; size];
        if !unsafe { (self.serialize)(bytes.as_mut_ptr().cast(), bytes.len()) } {
            return Err("The core could not save this state".into());
        }
        Ok(bytes)
    }
    pub fn restore_state(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
            return Err("Invalid saved state size".into());
        }
        let rollback = self.serialize_state()?;
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
                state
                    .borrow()
                    .as_ref()
                    .and_then(|state| state.raw_frame.clone())
            })
            .ok_or_else(|| "No game frame is available yet".into())
    }
    pub fn run_frame(&mut self) {
        unsafe { (self.run)() }
    }
    /// Hands the newest frame to the filter stage, once. False when the core
    /// has not drawn a new one since.
    pub fn hand_off(&self, stage: &super::gba_present::FrameStage) -> bool {
        CALLBACKS.with(|state| {
            let Ok(mut state) = state.try_borrow_mut() else {
                return false;
            };
            let Some(state) = state.as_mut() else {
                return false;
            };
            let Some(raw) = state.raw_frame.as_ref().filter(|_| state.fresh) else {
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
        // SAFETY: libretro v1 symbols from the retained core, on its worker.
        let regions = [SAVE_RAM, RTC]
            .map(|id| unsafe { ((self.memory)(id).cast::<u8>(), (self.memory_size)(id)) });
        if regions
            .iter()
            .any(|&(data, size)| size > 0 && data.is_null())
            || regions[0].1 + regions[1].1 > MAX_SAVE
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
            .filter(|_| bytes.len() <= MAX_SAVE)
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
        // SAFETY: lifecycle stays on the same worker; library is still alive.
        unsafe {
            (self.unload)();
            (self.deinit)();
        }
        CALLBACKS.with(|state| *state.borrow_mut() = None);
    }
}

extern "C" fn environment(command: u32, data: *mut c_void) -> bool {
    if data.is_null() {
        return false;
    }
    match command {
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
                    match CStr::from_ptr(variable.key).to_bytes() {
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
                variable.value = value.map_or(ptr::null(), CStr::as_ptr);
                value.is_some()
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
        8 => true,                 // SET_PERFORMANCE_LEVEL (advisory)
        16 | 18 | 11 | 35 => true, // options, input descriptors, no-game, controllers
        _ => false,                // No disk/network/VFS/graphics/BIOS permissions supplied.
    }
}
extern "C" fn input_poll() {}
extern "C" fn input_state(port: u32, device: u32, index: u32, id: u32) -> i16 {
    if port != 0 || device != 1 || index != 0 || id >= 16 {
        return 0;
    }
    CALLBACKS.with(|state| {
        state
            .try_borrow()
            .ok()
            .and_then(|state| {
                state
                    .as_ref()
                    .map(|s| i16::from(s.buttons.load(Ordering::Acquire) & (1 << id) != 0))
            })
            .unwrap_or(0)
    })
}
extern "C" fn video(data: *const c_void, width: u32, height: u32, pitch: usize) {
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
        for (y, row) in raw.rgba.chunks_exact_mut(width * 4).enumerate() {
            let line = &data[y * pitch..][..width * bpp];
            for (pixel, source) in row
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(line.chunks_exact(bpp))
            {
                *pixel = if bpp == 4 {
                    let value = u32::from_ne_bytes([source[0], source[1], source[2], source[3]]);
                    [(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]
                } else {
                    let value = u16::from_ne_bytes([source[0], source[1]]);
                    let green = if pixel_format == 2 {
                        ((value >> 5) & 63) * 255 / 63
                    } else {
                        ((value >> 5) & 31) * 255 / 31
                    };
                    let red = if pixel_format == 2 {
                        (value >> 11) & 31
                    } else {
                        (value >> 10) & 31
                    };
                    [
                        (red * 255 / 31) as u8,
                        green as u8,
                        ((value & 31) * 255 / 31) as u8,
                        255,
                    ]
                };
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
mod tests {
    use super::*;

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
