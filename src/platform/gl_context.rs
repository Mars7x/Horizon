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

pub struct GlContext {
    egl: Egl,
    gl: Gl,
    display: Display,
    context: Context,
    framebuffer: u32,
    texture: u32,
    depth: u32,
    size: (u32, u32),
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
        // GL's first row is the bottom one.
        for top in 0..height / 2 {
            let bottom = height - 1 - top;
            let (upper, lower) = rgba.split_at_mut(bottom * row);
            upper[top * row..][..row].swap_with_slice(&mut lower[..row]);
        }
        for alpha in rgba.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
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

impl Drop for GlContext {
    fn drop(&mut self) {
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
