//! Bounded SDL2 renderer presentation target that resolves through dlsym.

use std::ffi::{c_char, c_int, c_void};
use std::mem;
use std::thread;
use std::time::Duration;

const RTLD_NOW: c_int = 2;
const RTLD_LOCAL: c_int = 0;
const SDL_INIT_VIDEO: u32 = 0x0000_0020;
const SDL_WINDOWPOS_UNDEFINED: c_int = 0x1fff_0000;
const SDL_WINDOW_HIDDEN: u32 = 0x0000_0008;
const SDL_RENDERER_ACCELERATED: u32 = 0x0000_0002;

type SdlInit = unsafe extern "C" fn(u32) -> c_int;
type SdlSetHint = unsafe extern "C" fn(*const c_char, *const c_char) -> u32;
type SdlCreateWindow =
    unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, c_int, u32) -> *mut c_void;
type SdlCreateRenderer = unsafe extern "C" fn(*mut c_void, c_int, u32) -> *mut c_void;
type SdlRenderPresent = unsafe extern "C" fn(*mut c_void);
type SdlDestroyRenderer = unsafe extern "C" fn(*mut c_void);
type SdlDestroyWindow = unsafe extern "C" fn(*mut c_void);
type SdlQuit = unsafe extern "C" fn();
type Dlsym = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;

unsafe extern "C" {
    fn dlopen(file: *const c_char, mode: c_int) -> *mut c_void;
    fn dlvsym(handle: *mut c_void, symbol: *const c_char, version: *const c_char) -> *mut c_void;
}

fn main() -> Result<(), &'static str> {
    // SAFETY: every SDL handle and function result is checked before use and
    // released in reverse ownership order.
    unsafe {
        let library = dlopen(c"libSDL2-2.0.so.0".as_ptr(), RTLD_NOW | RTLD_LOCAL);
        if library.is_null() {
            return Err("SDL renderer probe could not load SDL2");
        }
        let real_dlsym: Dlsym = mem::transmute(dlvsym(
            (-1_isize) as *mut c_void,
            c"dlsym".as_ptr(),
            c"GLIBC_2.2.5".as_ptr(),
        ));
        let init: SdlInit = symbol(real_dlsym, library, c"SDL_Init".as_ptr())?;
        let set_hint: SdlSetHint = symbol(real_dlsym, library, c"SDL_SetHint".as_ptr())?;
        let create_window: SdlCreateWindow =
            symbol(real_dlsym, library, c"SDL_CreateWindow".as_ptr())?;
        let create_renderer: SdlCreateRenderer =
            symbol(real_dlsym, library, c"SDL_CreateRenderer".as_ptr())?;
        let present: SdlRenderPresent = symbol(real_dlsym, library, c"SDL_RenderPresent".as_ptr())?;
        let destroy_renderer: SdlDestroyRenderer =
            symbol(real_dlsym, library, c"SDL_DestroyRenderer".as_ptr())?;
        let destroy_window: SdlDestroyWindow =
            symbol(real_dlsym, library, c"SDL_DestroyWindow".as_ptr())?;
        let quit: SdlQuit = symbol(real_dlsym, library, c"SDL_Quit".as_ptr())?;

        set_hint(c"SDL_RENDER_DRIVER".as_ptr(), c"opengl".as_ptr());
        if init(SDL_INIT_VIDEO) != 0 {
            return Err("SDL renderer probe could not initialize video");
        }
        let window = create_window(
            c"Redunar SDL renderer probe".as_ptr(),
            SDL_WINDOWPOS_UNDEFINED,
            SDL_WINDOWPOS_UNDEFINED,
            640,
            240,
            SDL_WINDOW_HIDDEN,
        );
        if window.is_null() {
            quit();
            return Err("SDL renderer probe could not create a window");
        }
        let renderer = create_renderer(window, -1, SDL_RENDERER_ACCELERATED);
        if renderer.is_null() {
            destroy_window(window);
            quit();
            return Err("SDL renderer probe could not create an OpenGL renderer");
        }
        for _ in 0..180 {
            present(renderer);
            thread::sleep(Duration::from_millis(8));
        }
        destroy_renderer(renderer);
        destroy_window(window);
        quit();
    }
    Ok(())
}

unsafe fn symbol<T: Copy>(
    real_dlsym: Dlsym,
    handle: *mut c_void,
    name: *const c_char,
) -> Result<T, &'static str> {
    // SAFETY: the caller supplies a live dlopen handle and static C string.
    let address = unsafe { real_dlsym(handle, name) };
    if address.is_null() {
        Err("SDL renderer probe could not resolve a required symbol")
    } else {
        // SAFETY: each caller chooses the function type matching `name`.
        Ok(unsafe { mem::transmute_copy(&address) })
    }
}
