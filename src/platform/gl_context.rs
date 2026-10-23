//! A hidden OpenGL context for a core that renders with the GPU (melonDS's
//! upscaling renderer). EGL and OpenGL come from the system's graphics driver
//! at run time, so nothing is linked; the core draws into a framebuffer here,
//! and its frames are read back into the usual software picture pipeline.
//!
//! Everything here runs on the emulation worker, the context's only thread.
use libloading::Library;
use std::ffi::{CStr, c_char, c_void};

type Display = *mut c_void;
type Config = *mut c_void;
type Context = *mut c_void;

const EGL_NONE: i32 = 0x3038;
const EGL_SURFACE_TYPE: i32 = 0x3033;
const EGL_RENDERABLE_TYPE: i32 = 0x3040;
const EGL_OPENGL_BIT: i32 = 0x0008;
const EGL_OPENGL_API: u32 = 0x30A2;
const EGL_CONTEXT_MAJOR_VERSION: i32 = 0x3098;
const EGL_CONTEXT_MINOR_VERSION: i32 = 0x30FB;
const EGL_CONTEXT_OPENGL_PROFILE_MASK: i32 = 0x30FD;
const EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT: i32 = 1;
const EGL_PLATFORM_SURFACELESS_MESA: u32 = 0x31DD;

const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_READ_FRAMEBUFFER: u32 = 0x8CA8;
const GL_DRAW_FRAMEBUFFER: u32 = 0x8CA9;
const GL_READ_FRAMEBUFFER_BINDING: u32 = 0x8CAA;
const GL_DRAW_FRAMEBUFFER_BINDING: u32 = 0x8CA6;
const GL_TEXTURE_BINDING_2D: u32 = 0x8069;
const GL_SCISSOR_TEST: u32 = 0x0C11;
const GL_COLOR_BUFFER_BIT: u32 = 0x4000;
const GL_RENDERBUFFER: u32 = 0x8D41;
const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_RGBA8: i32 = 0x8058;
const GL_RGBA: u32 = 0x1908;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
const GL_DEPTH_ATTACHMENT: u32 = 0x8D00;
const GL_DEPTH_STENCIL_ATTACHMENT: u32 = 0x821A;
const GL_DEPTH_COMPONENT24: u32 = 0x81A6;
const GL_DEPTH24_STENCIL8: u32 = 0x88F0;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
const GL_PACK_ALIGNMENT: u32 = 0x0D05;
const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
const GL_NEAREST: i32 = 0x2600;

pub type ProcAddress = unsafe extern "C" fn(*const c_char) -> *const c_void;

struct Egl {
    get_proc_address: ProcAddress,
    make_current: unsafe extern "C" fn(Display, *mut c_void, *mut c_void, Context) -> u32,
    destroy_context: unsafe extern "C" fn(Display, Context) -> u32,
    terminate: unsafe extern "C" fn(Display) -> u32,
}

struct Gl {
    get_integerv: unsafe extern "C" fn(u32, *mut i32),
    is_enabled: unsafe extern "C" fn(u32) -> u8,
    enable: unsafe extern "C" fn(u32),
    disable: unsafe extern "C" fn(u32),
    blit_framebuffer: unsafe extern "C" fn(i32, i32, i32, i32, i32, i32, i32, i32, u32, u32),
    gen_framebuffers: unsafe extern "C" fn(i32, *mut u32),
    delete_framebuffers: unsafe extern "C" fn(i32, *const u32),
    bind_framebuffer: unsafe extern "C" fn(u32, u32),
    gen_textures: unsafe extern "C" fn(i32, *mut u32),
    delete_textures: unsafe extern "C" fn(i32, *const u32),
    bind_texture: unsafe extern "C" fn(u32, u32),
    tex_image_2d: unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void),
    tex_parameteri: unsafe extern "C" fn(u32, u32, i32),
    framebuffer_texture_2d: unsafe extern "C" fn(u32, u32, u32, u32, i32),
    gen_renderbuffers: unsafe extern "C" fn(i32, *mut u32),
    delete_renderbuffers: unsafe extern "C" fn(i32, *const u32),
    bind_renderbuffer: unsafe extern "C" fn(u32, u32),
    renderbuffer_storage: unsafe extern "C" fn(u32, u32, i32, i32),
    framebuffer_renderbuffer: unsafe extern "C" fn(u32, u32, u32, u32),
    check_framebuffer_status: unsafe extern "C" fn(u32) -> u32,
    pixel_storei: unsafe extern "C" fn(u32, i32),
    read_pixels: unsafe extern "C" fn(i32, i32, i32, i32, u32, u32, *mut c_void),
}

#[derive(Default)]
struct ReadbackTarget {
    framebuffer: u32,
    texture: u32,
    size: (u32, u32),
}

pub struct GlContext {
    egl: Egl,
    gl: Gl,
    display: Display,
    context: Context,
    framebuffer: u32,
    texture: u32,
    depth: u32,
    size: (u32, u32),
    viewport: ReadbackTarget,
    // Last: the driver's code must outlive every pointer above.
    _library: Library,
}

impl GlContext {
    /// An OpenGL `major.minor` core-profile context, current on this thread,
    /// with no window: Mesa's surfaceless platform, else the default display.
    pub fn new(major: i32, minor: i32) -> Result<Self, String> {
        // SAFETY: the system EGL library, called with its documented C ABI.
        unsafe {
            let library = Library::new("libEGL.so.1")
                .map_err(|_| "The graphics driver's EGL library is unavailable")?;
            fn symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T, String> {
                // SAFETY: callers name an EGL 1.4/1.5 entry point with its signature.
                unsafe { library.get::<T>(name).map(|s| *s) }
                    .map_err(|_| format!("EGL lacks {}", String::from_utf8_lossy(name)))
            }
            let get_proc_address: ProcAddress = symbol(&library, b"eglGetProcAddress\0")?;
            let get_display: unsafe extern "C" fn(*mut c_void) -> Display =
                symbol(&library, b"eglGetDisplay\0")?;
            let initialize: unsafe extern "C" fn(Display, *mut i32, *mut i32) -> u32 =
                symbol(&library, b"eglInitialize\0")?;
            let bind_api: unsafe extern "C" fn(u32) -> u32 = symbol(&library, b"eglBindAPI\0")?;
            let choose_config: unsafe extern "C" fn(
                Display,
                *const i32,
                *mut Config,
                i32,
                *mut i32,
            ) -> u32 = symbol(&library, b"eglChooseConfig\0")?;
            let create_context: unsafe extern "C" fn(
                Display,
                Config,
                Context,
                *const i32,
            ) -> Context = symbol(&library, b"eglCreateContext\0")?;
            let egl = Egl {
                get_proc_address,
                make_current: symbol(&library, b"eglMakeCurrent\0")?,
                destroy_context: symbol(&library, b"eglDestroyContext\0")?,
                terminate: symbol(&library, b"eglTerminate\0")?,
            };
            let platform = get_proc_address(c"eglGetPlatformDisplayEXT".as_ptr());
            let mut display = std::ptr::null_mut();
            if !platform.is_null() {
                let get_platform: unsafe extern "C" fn(u32, *mut c_void, *const i32) -> Display =
                    std::mem::transmute(platform);
                display = get_platform(
                    EGL_PLATFORM_SURFACELESS_MESA,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                );
            }
            if display.is_null()
                || initialize(display, std::ptr::null_mut(), std::ptr::null_mut()) == 0
            {
                display = get_display(std::ptr::null_mut());
                if display.is_null()
                    || initialize(display, std::ptr::null_mut(), std::ptr::null_mut()) == 0
                {
                    return Err("No EGL display is available".into());
                }
            }
            let fail = |message: &str| {
                (egl.terminate)(display);
                Err::<Self, String>(message.into())
            };
            if bind_api(EGL_OPENGL_API) == 0 {
                return fail("The graphics driver has no desktop OpenGL");
            }
            // No surface: the core draws into a framebuffer of ours.
            let attributes = [
                EGL_SURFACE_TYPE,
                0,
                EGL_RENDERABLE_TYPE,
                EGL_OPENGL_BIT,
                EGL_NONE,
            ];
            let (mut config, mut count) = (std::ptr::null_mut(), 0);
            if choose_config(display, attributes.as_ptr(), &mut config, 1, &mut count) == 0
                || count < 1
            {
                return fail("No OpenGL configuration is available");
            }
            let version = [
                EGL_CONTEXT_MAJOR_VERSION,
                major,
                EGL_CONTEXT_MINOR_VERSION,
                minor,
                EGL_CONTEXT_OPENGL_PROFILE_MASK,
                EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT,
                EGL_NONE,
            ];
            let context = create_context(display, config, std::ptr::null_mut(), version.as_ptr());
            if context.is_null() {
                return fail("The graphics driver could not create an OpenGL context");
            }
            if (egl.make_current)(display, std::ptr::null_mut(), std::ptr::null_mut(), context) == 0
            {
                (egl.destroy_context)(display, context);
                return fail("The OpenGL context could not be made current");
            }
            let gl = match load_gl(get_proc_address) {
                Ok(gl) => gl,
                Err(error) => {
                    (egl.make_current)(
                        display,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    );
                    (egl.destroy_context)(display, context);
                    (egl.terminate)(display);
                    return Err(error);
                }
            };
            Ok(Self {
                egl,
                gl,
                display,
                context,
                framebuffer: 0,
                texture: 0,
                depth: 0,
                size: (0, 0),
                viewport: ReadbackTarget::default(),
                _library: library,
            })
        }
    }

    pub fn proc_address(&self) -> ProcAddress {
        self.egl.get_proc_address
    }

    /// A framebuffer of at least `width`×`height` (the core's largest frame),
    /// with depth (and stencil) when the core asks. Returns its name.
    pub fn framebuffer(
        &mut self,
        (width, height): (u32, u32),
        stencil: bool,
    ) -> Result<u32, String> {
        if self.framebuffer != 0 && self.size.0 >= width && self.size.1 >= height {
            return Ok(self.framebuffer);
        }
        self.release_framebuffer();
        let gl = &self.gl;
        let (w, h) = (width.max(1) as i32, height.max(1) as i32);
        // SAFETY: GL 3.2 core calls on this thread's current context.
        unsafe {
            (gl.gen_textures)(1, &mut self.texture);
            (gl.bind_texture)(GL_TEXTURE_2D, self.texture);
            (gl.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
            (gl.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
            (gl.tex_image_2d)(
                GL_TEXTURE_2D,
                0,
                GL_RGBA8,
                w,
                h,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                std::ptr::null(),
            );
            (gl.bind_texture)(GL_TEXTURE_2D, 0);
            (gl.gen_renderbuffers)(1, &mut self.depth);
            (gl.bind_renderbuffer)(GL_RENDERBUFFER, self.depth);
            (gl.renderbuffer_storage)(
                GL_RENDERBUFFER,
                if stencil {
                    GL_DEPTH24_STENCIL8
                } else {
                    GL_DEPTH_COMPONENT24
                },
                w,
                h,
            );
            (gl.bind_renderbuffer)(GL_RENDERBUFFER, 0);
            (gl.gen_framebuffers)(1, &mut self.framebuffer);
            (gl.bind_framebuffer)(GL_FRAMEBUFFER, self.framebuffer);
            (gl.framebuffer_texture_2d)(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                self.texture,
                0,
            );
            (gl.framebuffer_renderbuffer)(
                GL_FRAMEBUFFER,
                if stencil {
                    GL_DEPTH_STENCIL_ATTACHMENT
                } else {
                    GL_DEPTH_ATTACHMENT
                },
                GL_RENDERBUFFER,
                self.depth,
            );
            let complete = (gl.check_framebuffer_status)(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE;
            (gl.bind_framebuffer)(GL_FRAMEBUFFER, 0);
            if !complete {
                self.release_framebuffer();
                return Err("The OpenGL framebuffer is incomplete".into());
            }
        }
        self.size = (width, height);
        Ok(self.framebuffer)
    }

    /// The bottom-left-origin `width`×`height` frame the core drew, as
    /// top-down RGBA rows in `rgba`.
    pub fn read(&self, (width, height): (u32, u32), rgba: &mut Vec<u8>) {
        let (width, height) = (
            width.min(self.size.0) as usize,
            height.min(self.size.1) as usize,
        );
        let row = width * 4;
        rgba.resize(row * height, 0);
        // SAFETY: the buffer holds exactly width×height RGBA pixels.
        unsafe {
            (self.gl.bind_framebuffer)(GL_READ_FRAMEBUFFER, self.framebuffer);
            (self.gl.pixel_storei)(GL_PACK_ALIGNMENT, 1);
            (self.gl.read_pixels)(
                0,
                0,
                width as i32,
                height as i32,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                rgba.as_mut_ptr().cast(),
            );
            (self.gl.bind_framebuffer)(GL_READ_FRAMEBUFFER, 0);
        }
        normalize_readback(rgba, width, height);
    }

    /// Sample each DS screen on the GPU at its actual presentation size.
    /// The core's framebuffer is untouched; captures can still read it in full.
    pub fn read_viewport(
        &mut self,
        source: (u32, u32),
        screens: [(u32, u32); 2],
        rgba: &mut Vec<u8>,
    ) -> Result<(), String> {
        let size = (screens[0].0.max(screens[1].0), screens[0].1 + screens[1].1);
        if source.0 == 0
            || source.1 == 0
            || source.0 > self.size.0
            || source.1 > self.size.1
            || size.0 == 0
            || size.1 == 0
            || size.0 > source.0
            || size.1 > source.1
            || screens
                .iter()
                .any(|&(w, h)| w == 0 || h == 0 || h > source.1 / 2)
        {
            return Err("Invalid DS presentation dimensions".into());
        }
        // The core owns its GL state. Restore every binding/state we touch,
        // including on incomplete-target failures or window/layout changes.
        // SAFETY: this context is current on its worker; GL 3.2 entry points
        // and buffer sizes were checked when loaded/above.
        unsafe {
            let (mut read, mut draw, mut texture, mut pack) = (0, 0, 0, 0);
            (self.gl.get_integerv)(GL_READ_FRAMEBUFFER_BINDING, &mut read);
            (self.gl.get_integerv)(GL_DRAW_FRAMEBUFFER_BINDING, &mut draw);
            (self.gl.get_integerv)(GL_TEXTURE_BINDING_2D, &mut texture);
            (self.gl.get_integerv)(GL_PACK_ALIGNMENT, &mut pack);
            let scissor = (self.gl.is_enabled)(GL_SCISSOR_TEST) != 0;
            (self.gl.disable)(GL_SCISSOR_TEST);
            let result = (|| {
                self.prepare_viewport(size)?;
                (self.gl.bind_framebuffer)(GL_READ_FRAMEBUFFER, self.framebuffer);
                (self.gl.bind_framebuffer)(GL_DRAW_FRAMEBUFFER, self.viewport.framebuffer);
                let (w, h) = (size.0 as i32, size.1 as i32);
                // Initialize the entire target, including the unused right
                // margin beside a smaller screen. Equal screens need one blit.
                (self.gl.blit_framebuffer)(
                    0,
                    0,
                    source.0 as i32,
                    source.1 as i32,
                    0,
                    0,
                    w,
                    h,
                    GL_COLOR_BUFFER_BIT,
                    GL_NEAREST as u32,
                );
                if screens[0] != screens[1] {
                    for (screen, (width, height)) in screens.into_iter().enumerate() {
                        let source_y = if screen == 0 { source.1 / 2 } else { 0 };
                        let target_y = if screen == 0 { screens[1].1 } else { 0 };
                        (self.gl.blit_framebuffer)(
                            0,
                            source_y as i32,
                            source.0 as i32,
                            (source_y + source.1 / 2) as i32,
                            0,
                            target_y as i32,
                            width as i32,
                            (target_y + height) as i32,
                            GL_COLOR_BUFFER_BIT,
                            GL_NEAREST as u32,
                        );
                    }
                }
                rgba.resize(size.0 as usize * size.1 as usize * 4, 0);
                (self.gl.bind_framebuffer)(GL_READ_FRAMEBUFFER, self.viewport.framebuffer);
                (self.gl.pixel_storei)(GL_PACK_ALIGNMENT, 1);
                (self.gl.read_pixels)(
                    0,
                    0,
                    w,
                    h,
                    GL_RGBA,
                    GL_UNSIGNED_BYTE,
                    rgba.as_mut_ptr().cast(),
                );
                normalize_readback(rgba, size.0 as usize, size.1 as usize);
                Ok(())
            })();
            (self.gl.bind_texture)(GL_TEXTURE_2D, texture as u32);
            (self.gl.pixel_storei)(GL_PACK_ALIGNMENT, pack);
            (self.gl.bind_framebuffer)(GL_READ_FRAMEBUFFER, read as u32);
            (self.gl.bind_framebuffer)(GL_DRAW_FRAMEBUFFER, draw as u32);
            if scissor {
                (self.gl.enable)(GL_SCISSOR_TEST);
            }
            result
        }
    }

    fn prepare_viewport(&mut self, size: (u32, u32)) -> Result<(), String> {
        if self.viewport.size == size {
            return Ok(());
        }
        self.release_viewport();
        // SAFETY: current worker context, bounded nonzero dimensions.
        unsafe {
            let gl = &self.gl;
            (gl.gen_textures)(1, &mut self.viewport.texture);
            (gl.bind_texture)(GL_TEXTURE_2D, self.viewport.texture);
            (gl.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
            (gl.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
            (gl.tex_image_2d)(
                GL_TEXTURE_2D,
                0,
                GL_RGBA8,
                size.0 as i32,
                size.1 as i32,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                std::ptr::null(),
            );
            (gl.gen_framebuffers)(1, &mut self.viewport.framebuffer);
            (gl.bind_framebuffer)(GL_DRAW_FRAMEBUFFER, self.viewport.framebuffer);
            (gl.framebuffer_texture_2d)(
                GL_DRAW_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                self.viewport.texture,
                0,
            );
            if (gl.check_framebuffer_status)(GL_DRAW_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE {
                self.release_viewport();
                return Err("The DS presentation framebuffer is incomplete".into());
            }
        }
        self.viewport.size = size;
        Ok(())
    }

    fn release_viewport(&mut self) {
        // SAFETY: names created by this worker's current context; zero is ignored.
        unsafe {
            (self.gl.delete_framebuffers)(1, &self.viewport.framebuffer);
            (self.gl.delete_textures)(1, &self.viewport.texture);
        }
        self.viewport = ReadbackTarget::default();
    }

    fn release_framebuffer(&mut self) {
        // SAFETY: deleting names this context created (0 is ignored).
        unsafe {
            if self.framebuffer != 0 {
                (self.gl.delete_framebuffers)(1, &self.framebuffer);
            }
            if self.texture != 0 {
                (self.gl.delete_textures)(1, &self.texture);
            }
            if self.depth != 0 {
                (self.gl.delete_renderbuffers)(1, &self.depth);
            }
        }
        (self.framebuffer, self.texture, self.depth) = (0, 0, 0);
        self.size = (0, 0);
    }
}

/// GL returns bottom-up rows; game pictures are top-down and opaque.
fn normalize_readback(rgba: &mut [u8], width: usize, height: usize) {
    let row = width * 4;
    for top in 0..height / 2 {
        let bottom = height - 1 - top;
        let (upper, lower) = rgba.split_at_mut(bottom * row);
        upper[top * row..][..row].swap_with_slice(&mut lower[..row]);
    }
    // Whole-pixel operations let LLVM vectorize this pass. A strided byte
    // write costs significantly more on the 24 MiB frames produced at 8×.
    // Native-endian conversion keeps the alpha mask correct on either endian.
    let opaque = u32::from_ne_bytes([0, 0, 0, 255]);
    for pixel in rgba.as_chunks_mut::<4>().0 {
        *pixel = (u32::from_ne_bytes(*pixel) | opaque).to_ne_bytes();
    }
}

impl Drop for GlContext {
    fn drop(&mut self) {
        self.release_viewport();
        self.release_framebuffer();
        // SAFETY: the context and display this value created, on its thread.
        unsafe {
            (self.egl.make_current)(
                self.display,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            (self.egl.destroy_context)(self.display, self.context);
            (self.egl.terminate)(self.display);
        }
    }
}

/// The GL entry points Horizon itself uses, from the driver.
unsafe fn load_gl(get_proc_address: ProcAddress) -> Result<Gl, String> {
    unsafe fn get<T: Copy>(get_proc_address: ProcAddress, name: &CStr) -> Result<T, String> {
        // SAFETY: the driver returns this entry point or null.
        let pointer = unsafe { get_proc_address(name.as_ptr()) };
        if pointer.is_null() {
            return Err(format!("OpenGL lacks {}", name.to_string_lossy()));
        }
        // SAFETY: T is the entry point's documented function signature.
        Ok(unsafe { std::mem::transmute_copy::<*const c_void, T>(&pointer) })
    }
    // SAFETY: each name is paired with its GL 3.2 core signature.
    unsafe {
        Ok(Gl {
            get_integerv: get(get_proc_address, c"glGetIntegerv")?,
            is_enabled: get(get_proc_address, c"glIsEnabled")?,
            enable: get(get_proc_address, c"glEnable")?,
            disable: get(get_proc_address, c"glDisable")?,
            blit_framebuffer: get(get_proc_address, c"glBlitFramebuffer")?,
            gen_framebuffers: get(get_proc_address, c"glGenFramebuffers")?,
            delete_framebuffers: get(get_proc_address, c"glDeleteFramebuffers")?,
            bind_framebuffer: get(get_proc_address, c"glBindFramebuffer")?,
            gen_textures: get(get_proc_address, c"glGenTextures")?,
            delete_textures: get(get_proc_address, c"glDeleteTextures")?,
            bind_texture: get(get_proc_address, c"glBindTexture")?,
            tex_image_2d: get(get_proc_address, c"glTexImage2D")?,
            tex_parameteri: get(get_proc_address, c"glTexParameteri")?,
            framebuffer_texture_2d: get(get_proc_address, c"glFramebufferTexture2D")?,
            gen_renderbuffers: get(get_proc_address, c"glGenRenderbuffers")?,
            delete_renderbuffers: get(get_proc_address, c"glDeleteRenderbuffers")?,
            bind_renderbuffer: get(get_proc_address, c"glBindRenderbuffer")?,
            renderbuffer_storage: get(get_proc_address, c"glRenderbufferStorage")?,
            framebuffer_renderbuffer: get(get_proc_address, c"glFramebufferRenderbuffer")?,
            check_framebuffer_status: get(get_proc_address, c"glCheckFramebufferStatus")?,
            pixel_storei: get(get_proc_address, c"glPixelStorei")?,
            read_pixels: get(get_proc_address, c"glReadPixels")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_readback;

    #[test]
    fn readback_preserves_rgb_flips_rows_and_makes_every_pixel_opaque() {
        // Odd heights exercise the middle row; narrow/odd widths exercise
        // vector tails. Transparent source alpha must never leak into the UI.
        for width in [1, 3, 16, 257] {
            for height in [1, 2, 3, 4, 385] {
                let mut rgba = Vec::new();
                for y in 0..height {
                    for x in 0..width {
                        rgba.extend_from_slice(&[x as u8, y as u8, (x ^ y) as u8, 0]);
                    }
                }
                normalize_readback(&mut rgba, width, height);
                for y in 0..height {
                    for x in 0..width {
                        let source_y = height - y - 1;
                        assert_eq!(
                            &rgba[(y * width + x) * 4..][..4],
                            &[x as u8, source_y as u8, (x ^ source_y) as u8, 255],
                        );
                    }
                }
            }
        }
    }
    #[test]
    #[ignore = "requires the packaged EGL/OpenGL driver"]
    fn gpu_viewport_matches_cpu_sampling_and_preserves_full_frame_and_gl_state() {
        use crate::domain::core_settings::VideoFilter;
        let mut gl = super::GlContext::new(3, 2).unwrap();
        let source = (2048, 3072);
        let fbo = gl.framebuffer(source, false).unwrap();
        let input: Vec<u8> = (0..source.0 * source.1)
            .flat_map(|i| {
                let (x, y) = (i % source.0, i / source.0);
                [
                    (x % 251) as u8,
                    (y % 241) as u8,
                    (((x / 7) ^ (y / 11)) % 239) as u8,
                    0,
                ]
            })
            .collect();
        // SAFETY: this test's current context and exact source-sized RGBA buffer.
        unsafe {
            (gl.gl.bind_texture)(super::GL_TEXTURE_2D, gl.texture);
            (gl.gl.tex_image_2d)(
                super::GL_TEXTURE_2D,
                0,
                super::GL_RGBA8,
                source.0 as i32,
                source.1 as i32,
                0,
                super::GL_RGBA,
                super::GL_UNSIGNED_BYTE,
                input.as_ptr().cast(),
            );
        }
        let mut full = Vec::new();
        gl.read(source, &mut full);
        let frame = crate::platform::gba_core::Frame {
            width: source.0,
            height: source.1,
            rgba: full.clone(),
            smooth: false,
        };
        for targets in [
            [(640, 480); 2],
            [(1024, 768), (341, 256)],
            [(513, 385), (257, 193)],
            [(800, 600); 2],
        ] {
            let screens = crate::platform::gba_video::ds_viewport_sizes(source, targets).unwrap();
            let expected =
                crate::platform::gba_video::present_ds(&frame, VideoFilter::None, targets);
            let mut actual = Vec::new();
            // Exercise scissor restoration and distinct prior framebuffer bindings.
            unsafe {
                (gl.gl.bind_framebuffer)(super::GL_READ_FRAMEBUFFER, fbo);
                (gl.gl.bind_framebuffer)(super::GL_DRAW_FRAMEBUFFER, 0);
                (gl.gl.enable)(super::GL_SCISSOR_TEST);
                (gl.gl.pixel_storei)(super::GL_PACK_ALIGNMENT, 8);
            }
            gl.read_viewport(source, screens, &mut actual).unwrap();
            let stride = screens[0].0.max(screens[1].0) as usize * 4;
            let mut first = 0;
            for (screen, (w, h)) in screens.into_iter().enumerate() {
                for y in 0..h as usize {
                    for x in 0..w as usize {
                        let start = (first + y) * stride + x * 4;
                        let pixel = &actual[start..start + 4];
                        let reference = &expected.pixels.as_bytes()[start..start + 4];
                        if pixel == reference {
                            continue;
                        }
                        // GL nearest may choose either adjacent texel at an
                        // exact texel boundary. Verify only those legal ties,
                        // never permit arbitrary one-pixel coordinate errors.
                        let x_num = (x * 2 + 1) * source.0 as usize;
                        let y_num = (y * 2 + 1) * (source.1 / 2) as usize;
                        let (sx, sy) = (x_num / (w as usize * 2), y_num / (h as usize * 2));
                        let xs = if x_num.is_multiple_of(w as usize * 2) {
                            vec![sx - 1, sx]
                        } else {
                            vec![sx]
                        };
                        let ys = if y_num.is_multiple_of(h as usize * 2) {
                            vec![sy - 1, sy]
                        } else {
                            vec![sy]
                        };
                        assert!(
                            ys.iter().any(|&yy| xs.iter().any(|&xx| {
                                let at = ((screen * (source.1 / 2) as usize + yy)
                                    * source.0 as usize
                                    + xx)
                                    * 4;
                                pixel == &frame.rgba[at..at + 4]
                            })),
                            "GPU sampling differs outside a legal nearest tie at {targets:?}, screen {screen}, ({x},{y}): {pixel:?} vs {reference:?}"
                        );
                    }
                }
                first += h as usize;
            }
            unsafe {
                let mut binding = 0;
                (gl.gl.get_integerv)(super::GL_READ_FRAMEBUFFER_BINDING, &mut binding);
                assert_eq!(binding as u32, fbo);
                (gl.gl.get_integerv)(super::GL_DRAW_FRAMEBUFFER_BINDING, &mut binding);
                assert_eq!(binding, 0);
                (gl.gl.get_integerv)(super::GL_PACK_ALIGNMENT, &mut binding);
                assert_eq!(binding, 8);
                assert_ne!((gl.gl.is_enabled)(super::GL_SCISSOR_TEST), 0);
                (gl.gl.disable)(super::GL_SCISSOR_TEST);
            }
            let mut retained = Vec::new();
            gl.read(source, &mut retained);
            assert_eq!(
                retained, full,
                "presentation must not change capture pixels"
            );
        }
        let targets = [(640, 480); 2];
        for round in 0..3 {
            let mut raw = frame.clone();
            let at = std::time::Instant::now();
            for _ in 0..30 {
                gl.read(source, &mut raw.rgba);
                std::hint::black_box(crate::platform::gba_video::present_ds(
                    &raw,
                    VideoFilter::None,
                    targets,
                ));
            }
            let before = at.elapsed().as_secs_f64() * 1000.0 / 30.0;
            let mut rgba = Vec::new();
            let at = std::time::Instant::now();
            for _ in 0..30 {
                gl.read_viewport(source, targets, &mut rgba).unwrap();
                std::hint::black_box(
                    slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                        &rgba, 640, 960,
                    ),
                );
            }
            let after = at.elapsed().as_secs_f64() * 1000.0 / 30.0;
            eprintln!(
                "DS viewport round {round}: full readback + CPU preparation {before:.3} ms; GPU viewport readback + preparation {after:.3} ms (24 MiB → {:.3} MiB)",
                rgba.len() as f64 / 1_048_576.0
            );
        }
    }
}
