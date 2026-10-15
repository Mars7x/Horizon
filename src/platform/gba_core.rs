//! Narrow mGBA libretro host. All core calls and callbacks stay on one worker.
use libloading::Library;
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
}
pub type LatestFrame = Arc<Mutex<Option<Frame>>>;
pub type AudioBuffer = Arc<Mutex<std::collections::VecDeque<f32>>>;
const MAX_AUDIO: usize = 32768 * 2 / 4;

struct Callbacks {
    frame: LatestFrame,
    audio: AudioBuffer,
    buttons: Arc<AtomicU16>,
    pixel_format: u32,
    frames: Arc<AtomicU64>,
}
thread_local! { static CALLBACKS: RefCell<Option<Callbacks>> = const { RefCell::new(None) }; }

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
    run: unsafe extern "C" fn(),
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
        frame: LatestFrame,
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
            let mut core = Self {
                run: symbol(&library, b"retro_run\0")?,
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
                    frame,
                    audio,
                    buttons,
                    pixel_format: 0,
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
            Ok(core)
        }
    }
    pub fn run_frame(&mut self) {
        unsafe { (self.run)() }
    }
    pub fn save_ram(&self) -> Result<Vec<u8>, String> {
        // SAFETY: memory belongs to the loaded core; copied before its next call.
        unsafe {
            let size = (self.memory_size)(0);
            if size == 0 {
                return Ok(Vec::new());
            }
            let data = (self.memory)(0);
            if size > 128 * 1024 || data.is_null() {
                return Err("mGBA returned invalid save memory".into());
            }
            Ok(std::slice::from_raw_parts(data.cast::<u8>(), size).to_vec())
        }
    }
    pub fn restore_save(&mut self, bytes: &[u8]) -> Result<(), String> {
        unsafe {
            let capacity = (self.memory_size)(0);
            let data = (self.memory)(0);
            if bytes.len() > capacity || capacity > 128 * 1024 || data.is_null() {
                return Err(
                    "The existing save does not fit this game. It has been preserved.".into(),
                );
            }
            // SAFETY: checked the core's allocation above; input is a distinct Vec.
            ptr::copy_nonoverlapping(bytes.as_ptr(), data.cast::<u8>(), bytes.len());
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
                variable.value = ptr::null();
                false
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
        let Ok(state) = state.try_borrow() else {
            return;
        };
        let Some(state) = state.as_ref() else {
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
        let mut rgba = vec![0; width as usize * height as usize * 4];
        for y in 0..height as usize {
            for x in 0..width as usize {
                let offset = y * pitch + x * bpp;
                let color = if bpp == 4 {
                    let value = u32::from_ne_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]);
                    [(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]
                } else {
                    let value = u16::from_ne_bytes([data[offset], data[offset + 1]]);
                    let green = if state.pixel_format == 2 {
                        ((value >> 5) & 63) * 255 / 63
                    } else {
                        ((value >> 5) & 31) * 255 / 31
                    };
                    let red = if state.pixel_format == 2 {
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
                rgba[(y * width as usize + x) * 4..(y * width as usize + x + 1) * 4]
                    .copy_from_slice(&color);
            }
        }
        if let Ok(mut frame) = state.frame.lock() {
            *frame = Some(Frame {
                width,
                height,
                rgba,
            });
            state.frames.fetch_add(1, Ordering::Release);
        }
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
        let Ok(state) = state.try_borrow() else {
            return;
        };
        let Some(state) = state.as_ref() else {
            return;
        };
        if let Ok(mut audio) = state.audio.lock() {
            let samples = unsafe { std::slice::from_raw_parts(data, frames * 2) };
            if audio.len() + samples.len() <= MAX_AUDIO {
                audio.extend(samples.iter().map(|sample| f32::from(*sample) / 32768.0));
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires the built mGBA core (HORIZON_GBA_CORE)"]
    fn real_core_draws_responds_to_held_input_and_restores_battery_memory() {
        let path = std::env::var_os("HORIZON_GBA_CORE").expect("set HORIZON_GBA_CORE");
        let rom = test_rom();
        let frame = Arc::new(Mutex::new(None));
        let audio = Arc::new(Mutex::new(std::collections::VecDeque::with_capacity(32768)));
        let buttons = Arc::new(AtomicU16::new(0));
        let mut core = Core::load(
            Path::new(&path),
            &rom,
            Arc::clone(&frame),
            Arc::clone(&audio),
            Arc::clone(&buttons),
        )
        .unwrap();
        for _ in 0..10 {
            core.run_frame();
        }
        assert_eq!((core.av.base_width, core.av.base_height), (240, 160));
        let image = frame.lock().unwrap().clone().unwrap();
        assert!(image.rgba[1] > 200);
        assert!(image.rgba[0] < 30);
        buttons.store(1 << 8, Ordering::Release);
        for _ in 0..3 {
            core.run_frame();
        }
        let image = frame.lock().unwrap().clone().unwrap();
        assert!(image.rgba[0] > 200);
        assert!(image.rgba[1] < 30);
        let saved = core.save_ram().unwrap();
        assert_eq!(saved[0], 31);
        buttons.store(0, Ordering::Release);
        for _ in 0..3 {
            core.run_frame();
        }
        assert!(frame.lock().unwrap().as_ref().unwrap().rgba[1] > 200);
        assert!(!audio.lock().unwrap().is_empty());
        assert!(
            audio
                .lock()
                .unwrap()
                .iter()
                .any(|sample| sample.abs() > 0.01)
        );
        drop(core);
        let mut reopened = Core::load(Path::new(&path), &rom, frame, audio, buttons).unwrap();
        reopened.restore_save(&saved).unwrap();
        assert_eq!(reopened.save_ram().unwrap()[0], 31);
    }
}
