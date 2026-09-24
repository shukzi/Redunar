//! Bounded SDL2/OpenGL presentation target that resolves swaps through dlsym.

use std::ffi::{c_char, c_int, c_void};
use std::mem;
use std::thread;
use std::time::Duration;

const RTLD_NOW: c_int = 2;
const RTLD_LOCAL: c_int = 0;
const SDL_INIT_VIDEO: u32 = 0x0000_0020;
const SDL_WINDOWPOS_UNDEFINED: c_int = 0x1fff_0000;
const SDL_WINDOW_OPENGL: u32 = 0x0000_0002;
const SDL_WINDOW_HIDDEN: u32 = 0x0000_0008;
const DEFAULT_WIDTH: u32 = 640;
const DEFAULT_HEIGHT: u32 = 240;
const DEFAULT_FRAME_COUNT: u32 = 180;
const GL_COLOR_BUFFER_BIT: u32 = 0x0000_4000;
const GL_SCISSOR_TEST: u32 = 0x0c11;
const GL_RGBA: u32 = 0x1908;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_READ_BUFFER: u32 = 0x0c02;
const GL_DOUBLEBUFFER: u32 = 0x0c32;

type SdlInit = unsafe extern "C" fn(u32) -> c_int;
type SdlCreateWindow =
    unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, c_int, u32) -> *mut c_void;
type SdlGlCreateContext = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type SdlGlMakeCurrent = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
type SdlGlGetProcAddress = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type SdlGlSwapWindow = unsafe extern "C" fn(*mut c_void);
type SdlGlDeleteContext = unsafe extern "C" fn(*mut c_void);
type SdlDestroyWindow = unsafe extern "C" fn(*mut c_void);
type SdlQuit = unsafe extern "C" fn();
type Dlsym = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
type GlClearColor = unsafe extern "C" fn(f32, f32, f32, f32);
type GlClear = unsafe extern "C" fn(u32);
type GlEnable = unsafe extern "C" fn(u32);
type GlDisable = unsafe extern "C" fn(u32);
type GlScissor = unsafe extern "C" fn(c_int, c_int, c_int, c_int);
type GlReadPixels = unsafe extern "C" fn(c_int, c_int, c_int, c_int, u32, u32, *mut c_void);
type GlGetInteger = unsafe extern "C" fn(u32, *mut c_int);

struct GlPainter {
    clear_color: GlClearColor,
    clear: GlClear,
    enable: GlEnable,
    disable: GlDisable,
    scissor: GlScissor,
    read_pixels: GlReadPixels,
    get_integer: GlGetInteger,
}

impl GlPainter {
    unsafe fn load(get_proc_address: SdlGlGetProcAddress) -> Option<Self> {
        // SAFETY: SDL owns a current GL context, and these are standard GL
        // entry points with the exact signatures declared above.
        unsafe {
            let functions = [
                get_proc_address(c"glClearColor".as_ptr()),
                get_proc_address(c"glClear".as_ptr()),
                get_proc_address(c"glEnable".as_ptr()),
                get_proc_address(c"glDisable".as_ptr()),
                get_proc_address(c"glScissor".as_ptr()),
                get_proc_address(c"glReadPixels".as_ptr()),
                get_proc_address(c"glGetIntegerv".as_ptr()),
            ];
            if functions.iter().any(|address| address.is_null()) {
                return None;
            }
            Some(Self {
                clear_color: mem::transmute_copy(&functions[0]),
                clear: mem::transmute_copy(&functions[1]),
                enable: mem::transmute_copy(&functions[2]),
                disable: mem::transmute_copy(&functions[3]),
                scissor: mem::transmute_copy(&functions[4]),
                read_pixels: mem::transmute_copy(&functions[5]),
                get_integer: mem::transmute_copy(&functions[6]),
            })
        }
    }

    unsafe fn draw(&self, frame: u32, width: c_int, height: c_int, orientation_pattern: bool) {
        // SAFETY: SDL owns a current GL context for every invocation.
        unsafe {
            let phase = f32::from(u8::try_from(frame % 256).unwrap_or(0)) / 255.0;
            (self.clear_color)(0.06, phase, 0.18, 1.0);
            (self.clear)(GL_COLOR_BUFFER_BIT);
            if orientation_pattern {
                let quarter = height / 4;
                (self.enable)(GL_SCISSOR_TEST);
                (self.scissor)(0, 0, width, quarter);
                (self.clear_color)(0.05, 0.1, 0.9, 1.0);
                (self.clear)(GL_COLOR_BUFFER_BIT);
                (self.scissor)(0, height - quarter, width, quarter);
                (self.clear_color)(0.9, 0.1, 0.05, 1.0);
                (self.clear)(GL_COLOR_BUFFER_BIT);
                (self.disable)(GL_SCISSOR_TEST);
                if frame == 0 {
                    let mut bottom = [0_u8; 4];
                    let mut top = [0_u8; 4];
                    (self.read_pixels)(
                        width / 2,
                        height / 8,
                        1,
                        1,
                        GL_RGBA,
                        GL_UNSIGNED_BYTE,
                        bottom.as_mut_ptr().cast(),
                    );
                    (self.read_pixels)(
                        width / 2,
                        height * 7 / 8,
                        1,
                        1,
                        GL_RGBA,
                        GL_UNSIGNED_BYTE,
                        top.as_mut_ptr().cast(),
                    );
                    let mut read_buffer = 0;
                    let mut double_buffer = 0;
                    (self.get_integer)(GL_READ_BUFFER, &raw mut read_buffer);
                    (self.get_integer)(GL_DOUBLEBUFFER, &raw mut double_buffer);
                    eprintln!(
                        "SDL GL draw samples bottom={bottom:?} top={top:?} read_buffer={read_buffer:#x} double_buffer={double_buffer}"
                    );
                }
            }
        }
    }
}

unsafe extern "C" {
    fn dlopen(file: *const c_char, mode: c_int) -> *mut c_void;
    fn dlvsym(handle: *mut c_void, symbol: *const c_char, version: *const c_char) -> *mut c_void;
}

fn main() -> Result<(), &'static str> {
    let width = bounded_env_u32("REDUNAR_CAPTURE_PROBE_WIDTH", DEFAULT_WIDTH, 1, 3840)?;
    let height = bounded_env_u32("REDUNAR_CAPTURE_PROBE_HEIGHT", DEFAULT_HEIGHT, 1, 2160)?;
    let frame_count = bounded_env_u32(
        "REDUNAR_CAPTURE_PROBE_FRAMES",
        DEFAULT_FRAME_COUNT,
        2,
        1_000_000,
    )?;
    let orientation_pattern = std::env::var("REDUNAR_CAPTURE_PROBE_ORIENTATION_PATTERN")
        .ok()
        .as_deref()
        == Some("1");
    let width_i32 = c_int::try_from(width).map_err(|_| "SDL probe width is too large")?;
    let height_i32 = c_int::try_from(height).map_err(|_| "SDL probe height is too large")?;
    // SAFETY: every SDL handle and function result is checked before use and
    // released in reverse ownership order.
    unsafe {
        let library = dlopen(c"libSDL2-2.0.so.0".as_ptr(), RTLD_NOW | RTLD_LOCAL);
        if library.is_null() {
            return Err("SDL probe could not load SDL2");
        }
        let real_dlsym: Dlsym = mem::transmute(dlvsym(
            (-1_isize) as *mut c_void,
            c"dlsym".as_ptr(),
            c"GLIBC_2.2.5".as_ptr(),
        ));
        let init: SdlInit = symbol(real_dlsym, library, c"SDL_Init".as_ptr())?;
        let create_window: SdlCreateWindow =
            symbol(real_dlsym, library, c"SDL_CreateWindow".as_ptr())?;
        let create_context: SdlGlCreateContext =
            symbol(real_dlsym, library, c"SDL_GL_CreateContext".as_ptr())?;
        let make_current: SdlGlMakeCurrent =
            symbol(real_dlsym, library, c"SDL_GL_MakeCurrent".as_ptr())?;
        let get_proc_address: SdlGlGetProcAddress =
            symbol(real_dlsym, library, c"SDL_GL_GetProcAddress".as_ptr())?;
        let swap_window: SdlGlSwapWindow =
            symbol(real_dlsym, library, c"SDL_GL_SwapWindow".as_ptr())?;
        let delete_context: SdlGlDeleteContext =
            symbol(real_dlsym, library, c"SDL_GL_DeleteContext".as_ptr())?;
        let destroy_window: SdlDestroyWindow =
            symbol(real_dlsym, library, c"SDL_DestroyWindow".as_ptr())?;
        let quit: SdlQuit = symbol(real_dlsym, library, c"SDL_Quit".as_ptr())?;

        if init(SDL_INIT_VIDEO) != 0 {
            return Err("SDL probe could not initialize video");
        }
        let window = create_window(
            c"Redunar SDL OpenGL probe".as_ptr(),
            SDL_WINDOWPOS_UNDEFINED,
            SDL_WINDOWPOS_UNDEFINED,
            width_i32,
            height_i32,
            SDL_WINDOW_OPENGL | SDL_WINDOW_HIDDEN,
        );
        if window.is_null() {
            quit();
            return Err("SDL probe could not create a window");
        }
        let context = create_context(window);
        if context.is_null() || make_current(window, context) != 0 {
            if !context.is_null() {
                delete_context(context);
            }
            destroy_window(window);
            quit();
            return Err("SDL probe could not create an OpenGL context");
        }
        let Some(painter) = GlPainter::load(get_proc_address) else {
            delete_context(context);
            destroy_window(window);
            quit();
            return Err("SDL probe could not resolve OpenGL drawing functions");
        };
        for frame in 0..frame_count {
            painter.draw(frame, width_i32, height_i32, orientation_pattern);
            swap_window(window);
            thread::sleep(Duration::from_millis(8));
        }
        delete_context(context);
        destroy_window(window);
        quit();
    }
    Ok(())
}

fn bounded_env_u32(
    name: &str,
    default: u32,
    minimum: u32,
    maximum: u32,
) -> Result<u32, &'static str> {
    let Some(raw) = std::env::var_os(name) else {
        return Ok(default);
    };
    let value = raw
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or("SDL probe received an invalid numeric setting")?;
    if !(minimum..=maximum).contains(&value) {
        return Err("SDL probe received an out-of-range setting");
    }
    Ok(value)
}

unsafe fn symbol<T: Copy>(
    real_dlsym: Dlsym,
    handle: *mut c_void,
    name: *const c_char,
) -> Result<T, &'static str> {
    // SAFETY: the caller supplies a live dlopen handle and static C string.
    let address = unsafe { real_dlsym(handle, name) };
    if address.is_null() {
        Err("SDL probe could not resolve a required symbol")
    } else {
        // SAFETY: each caller chooses the function type matching `name`.
        Ok(unsafe { mem::transmute_copy(&address) })
    }
}
