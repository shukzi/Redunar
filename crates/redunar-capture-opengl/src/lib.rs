//! Launch-scoped GLX/EGL presentation telemetry for Redunar.
//!
//! The library is loaded only into a Redunar-owned direct child through
//! `LD_PRELOAD`. It observes completed buffer swaps, sends bounded frame-time
//! batches to the daemon, draws a bounded FPS overlay when requested, and
//! delegates every graphics call to the next implementation. Production
//! Replay frames leave the game process through the GBM/GL DMA export in
//! `replay_export`; the diagnostic readback path in `replay_readback` remains
//! metadata-only.

mod fd_transport;
mod overlay;
mod replay_export;
mod replay_readback;

use redunar_capture::{
    CaptureApi, CaptureMessage, CaptureSessionId, GoodbyeReason, MAX_MESSAGE_BYTES,
    OverlayFailureReason, OverlayRuntimeStatus, PROTOCOL_VERSION, ReplaySourceCandidate,
    ReplaySourceRejection, encode_frame_batch, encode_message,
};
use std::collections::VecDeque;
use std::env;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::mem;
use std::os::fd::FromRawFd;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::UnixDatagram;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock};

const BATCH_INTERVALS: usize = 64;
const MAX_BATCH_LATENCY_NS: u64 = 250_000_000;
const CLOCK_MONOTONIC: c_int = 1;
const RTLD_NOW: c_int = 2;
const RTLD_NOLOAD: c_int = 4;

type GlxSwapBuffers = unsafe extern "C" fn(*mut c_void, usize);
type GlxDestroyContext = unsafe extern "C" fn(*mut c_void, *mut c_void);
type EglSwapBuffers = unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32;
type EglDestroyContext = unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32;
type SdlInit = unsafe extern "C" fn(u32) -> c_int;
type SdlDynapiEntry = unsafe extern "C" fn(u32, *mut c_void, u32) -> c_int;
type SdlGlSwapWindow = unsafe extern "C" fn(*mut c_void);
type SdlGlDeleteContext = unsafe extern "C" fn(*mut c_void);
type SdlGlGetProcAddress = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type SdlRenderPresent = unsafe extern "C" fn(*mut c_void);
type Dlsym = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
type Dlopen = unsafe extern "C" fn(*const c_char, c_int) -> *mut c_void;
type GetCurrentContext = unsafe extern "C" fn() -> *mut c_void;
type GetProcAddress = unsafe extern "C" fn(*const u8) -> *mut c_void;
type GetInteger = unsafe extern "C" fn(u32, *mut c_int);

const GL_VIEWPORT: u32 = 0x0BA2;

#[repr(C)]
struct Timespec {
    seconds: i64,
    nanoseconds: i64,
}

unsafe extern "C" {
    fn clock_gettime(clock_id: c_int, time: *mut Timespec) -> c_int;
    fn dlvsym(handle: *mut c_void, symbol: *const c_char, version: *const c_char) -> *mut c_void;
    fn dlerror() -> *mut c_char;
    fn atexit(callback: unsafe extern "C" fn()) -> c_int;
    #[cfg(feature = "launch-diagnostics")]
    fn write(fd: c_int, buffer: *const c_void, count: usize) -> isize;
    #[cfg(feature = "launch-diagnostics")]
    fn getpid() -> c_int;
}

#[cfg(feature = "launch-diagnostics")]
mod launch_diagnostics {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static REPORTED: AtomicUsize = AtomicUsize::new(0);

    pub(super) const SIDECAR_LOADED: usize = 1 << 0;
    pub(super) const GLX_HOOK: usize = 1 << 1;
    pub(super) const GLX_SELECTED: usize = 1 << 2;
    pub(super) const GLX_REJECTED: usize = 1 << 3;
    pub(super) const EGL_HOOK: usize = 1 << 4;
    pub(super) const EGL_SELECTED: usize = 1 << 5;
    pub(super) const EGL_REJECTED: usize = 1 << 6;
    pub(super) const SDL_HOOK: usize = 1 << 7;
    pub(super) const SDL_SELECTED: usize = 1 << 8;
    pub(super) const SDL_REJECTED: usize = 1 << 9;
    pub(super) const PRODUCER_HELLO_SENT: usize = 1 << 10;
    pub(super) const PRODUCER_ENV_MISSING: usize = 1 << 11;
    pub(super) const PRODUCER_SOCKET_FAILED: usize = 1 << 12;
    pub(super) const SDL_DYNSYM_REQUEST: usize = 1 << 13;
    pub(super) const SDL_DYNSYM_FOUND: usize = 1 << 14;
    pub(super) const SDL_DYNAPI_ENTERED: usize = 1 << 15;
    pub(super) const SDL_INIT_HOOK: usize = 1 << 16;
    pub(super) const SDL_HOOKS_INSTALLED: usize = 1 << 17;
    pub(super) const GLX_SWAP_RESOLVER: usize = 1 << 18;
    pub(super) const SDL_SWAP_SYMBOL: usize = 1 << 19;
    pub(super) const SDL_LIBRARY_FOUND: usize = 1 << 20;
    pub(super) const SDL_HOOK_INSTALL_FAILED: usize = 1 << 21;
    pub(super) const GLX_SWAP_SYMBOL: usize = 1 << 22;
    pub(super) const EGL_SWAP_SYMBOL: usize = 1 << 23;
    pub(super) const SDL_SELF_WRAPPER: usize = 1 << 24;
    pub(super) const SDL_FOREIGN_UNPATCHABLE: usize = 1 << 25;
    pub(super) const SDL_DLOPEN_REQUEST: usize = 1 << 26;
    pub(super) const SDL_DLOPEN_SUCCEEDED: usize = 1 << 27;
    pub(super) const SDL_DLOPEN_FAILED: usize = 1 << 28;
    pub(super) const NULL_DLOPEN_REQUEST: usize = 1 << 29;
    pub(super) const DLOPEN_DYNSYM_REQUEST: usize = 1 << 30;
    pub(super) const DLOPEN_DYNSYM_EXTERNAL: usize = 1 << 31;
    pub(super) const DLOPEN_DYNSYM_ALREADY_WRAPPED: usize = 1 << 32;
    pub(super) const DLSYM_DYNSYM_REQUEST: usize = 1 << 33;
    pub(super) const DLSYM_DYNSYM_EXTERNAL: usize = 1 << 34;
    pub(super) const DLSYM_DYNSYM_ALREADY_WRAPPED: usize = 1 << 35;

    pub(super) fn report(bit: usize, message: &'static [u8]) {
        if REPORTED.fetch_or(bit, Ordering::Relaxed) & bit != 0 {
            return;
        }
        let mut line = [0_u8; 128];
        let prefix = b"redunar-opengl-diag: pid=";
        line[..prefix.len()].copy_from_slice(prefix);
        let mut pid = unsafe { super::getpid() }.max(0) as u32;
        let mut digits = [0_u8; 10];
        let mut digit_count = 0;
        loop {
            digits[digit_count] = b'0' + (pid % 10) as u8;
            digit_count += 1;
            pid /= 10;
            if pid == 0 {
                break;
            }
        }
        let mut used = prefix.len();
        for digit in digits[..digit_count].iter().rev() {
            line[used] = *digit;
            used += 1;
        }
        line[used] = b' ';
        used += 1;
        let available = line.len() - used;
        let copied = message.len().min(available);
        line[used..used + copied].copy_from_slice(&message[..copied]);
        used += copied;
        // SAFETY: `line` is initialized through `used`; fd 2 is the process
        // diagnostic stream. Each short marker is emitted once per PID.
        unsafe {
            super::write(2, line.as_ptr().cast(), used);
        }
    }
}

#[cfg(feature = "launch-diagnostics")]
macro_rules! launch_diag {
    ($event:ident, $message:literal) => {
        launch_diagnostics::report(
            launch_diagnostics::$event,
            concat!($message, "\n").as_bytes(),
        )
    };
}

#[cfg(not(feature = "launch-diagnostics"))]
macro_rules! launch_diag {
    ($event:ident, $message:literal) => {};
}

struct Producer {
    initialized: bool,
    socket: Option<UnixDatagram>,
    session_id: Option<CaptureSessionId>,
    intervals: [u64; BATCH_INTERVALS],
    interval_count: usize,
    first_sequence: u64,
    previous_sequence: Option<u64>,
    last_flush_ns: u64,
    overlay_status: Option<OverlayRuntimeStatus>,
    reply_path: Option<std::path::PathBuf>,
}

#[derive(Default)]
struct PresentationTarget {
    context: usize,
    pixels: u64,
}

impl PresentationTarget {
    fn select(&mut self, context: usize, pixels: u64) -> bool {
        if context == 0 || pixels == 0 {
            return false;
        }
        if self.context == context {
            self.pixels = pixels;
            return true;
        }
        if self.context == 0 || pixels > self.pixels {
            self.context = context;
            self.pixels = pixels;
            return true;
        }
        false
    }

    fn remove(&mut self, context: usize) {
        if self.context == context {
            *self = Self::default();
        }
    }
}

impl Default for Producer {
    fn default() -> Self {
        Self {
            initialized: false,
            socket: None,
            session_id: None,
            intervals: [0; BATCH_INTERVALS],
            interval_count: 0,
            first_sequence: 0,
            previous_sequence: None,
            last_flush_ns: 0,
            overlay_status: None,
            reply_path: None,
        }
    }
}

static PRODUCER: LazyLock<Mutex<Producer>> = LazyLock::new(|| Mutex::new(Producer::default()));
// Keep consumed daemon acknowledgements until both capture paths can apply
// them. A render-thread lock collision must never permanently occupy a slot.
static PENDING_REPLAY_RELEASES: LazyLock<Mutex<VecDeque<u64>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(8)));
static PRESENTATION_TARGET: LazyLock<Mutex<PresentationTarget>> =
    LazyLock::new(|| Mutex::new(PresentationTarget::default()));
static LAST_PRESENT_NS: AtomicU64 = AtomicU64::new(0);
static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static NEXT_REPLAY_COPY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static NEXT_GLX_SWAP_BUFFERS: OnceLock<Option<GlxSwapBuffers>> = OnceLock::new();
static NEXT_GLX_DESTROY_CONTEXT: OnceLock<Option<GlxDestroyContext>> = OnceLock::new();
static NEXT_EGL_SWAP_BUFFERS: OnceLock<Option<EglSwapBuffers>> = OnceLock::new();
static NEXT_EGL_DESTROY_CONTEXT: OnceLock<Option<EglDestroyContext>> = OnceLock::new();
static NEXT_SDL_INIT: AtomicUsize = AtomicUsize::new(0);
static NEXT_SDL_GL_SWAP_WINDOW: AtomicUsize = AtomicUsize::new(0);
static NEXT_SDL_GL_DELETE_CONTEXT: AtomicUsize = AtomicUsize::new(0);
static NEXT_SDL_RENDER_PRESENT: AtomicUsize = AtomicUsize::new(0);
static SDL_HANDLE: AtomicUsize = AtomicUsize::new(0);
static NEXT_SDL_DYNAPI_ENTRY: AtomicUsize = AtomicUsize::new(0);
static REAL_DLSYM: OnceLock<Option<Dlsym>> = OnceLock::new();
static REAL_DLOPEN: OnceLock<Option<Dlopen>> = OnceLock::new();
static NEXT_GLX_GET_PROC_ADDRESS: OnceLock<Option<GetProcAddress>> = OnceLock::new();
static NEXT_GLX_GET_PROC_ADDRESS_ARB: OnceLock<Option<GetProcAddress>> = OnceLock::new();
static NEXT_EGL_GET_PROC_ADDRESS: OnceLock<Option<GetProcAddress>> = OnceLock::new();
static GLX_GET_CURRENT_CONTEXT: OnceLock<Option<GetCurrentContext>> = OnceLock::new();
static EGL_GET_CURRENT_CONTEXT: OnceLock<Option<GetCurrentContext>> = OnceLock::new();
static GL_GET_INTEGER: OnceLock<Option<GetInteger>> = OnceLock::new();

#[used]
#[unsafe(link_section = ".init_array")]
static INSTALL_LOADED_SDL_HOOKS: unsafe extern "C" fn() = install_loaded_sdl_hooks;

unsafe extern "C" fn install_loaded_sdl_hooks() {
    launch_diag!(SIDECAR_LOADED, "sidecar-loaded");
    for name in [
        c"libSDL2-2.0.so.0",
        c"libSDL2.so.0",
        c"libSDL2.so",
        c"libSDL3.so.0",
    ] {
        let Some(real) = real_dlopen() else {
            clear_loader_error();
            return;
        };
        // SAFETY: RTLD_NOLOAD only obtains a handle to an already-loaded
        // object. The static names are NUL-terminated.
        let handle = unsafe { real(name.as_ptr(), RTLD_NOW | RTLD_NOLOAD) };
        if !handle.is_null() {
            install_sdl_dynapi_hook(handle);
        }
        // RTLD_NOLOAD and the optional SDL symbol probes above can set the
        // thread-local loader error even though this constructor is internal.
        clear_loader_error();
    }
}

#[unsafe(export_name = "dlopen")]
unsafe extern "C" fn interposed_dlopen(file: *const c_char, mode: c_int) -> *mut c_void {
    let sdl_request = if file.is_null() {
        launch_diag!(NULL_DLOPEN_REQUEST, "dlopen-null-filename");
        false
    } else {
        // SAFETY: dlopen requires a NUL-terminated filename when non-null.
        let name = unsafe { CStr::from_ptr(file) }.to_bytes();
        let is_sdl = name.windows(3).any(|part| part == b"SDL" || part == b"sdl");
        if is_sdl {
            launch_diag!(SDL_DLOPEN_REQUEST, "sdl-dlopen-requested");
        }
        is_sdl
    };
    let Some(real) = real_dlopen() else {
        return std::ptr::null_mut();
    };
    // SAFETY: this forwards the caller-owned filename and mode to glibc's
    // versioned dlopen implementation.
    let handle = unsafe { real(file, mode) };
    if !handle.is_null() {
        if sdl_request {
            launch_diag!(SDL_DLOPEN_SUCCEEDED, "sdl-dlopen-succeeded");
        }
        // Discover SDL by exported symbols, including late loads under
        // game-specific filenames. These optional lookups can set dlerror on
        // unrelated libraries, so consume that internal diagnostic before
        // returning a successful handle to the original caller.
        install_sdl_dynapi_hook(handle);
        clear_loader_error();
    } else if sdl_request {
        launch_diag!(SDL_DLOPEN_FAILED, "sdl-dlopen-failed");
    }
    handle
}

fn clear_loader_error() {
    // SAFETY: dlerror returns a thread-local pointer or null; the value is
    // intentionally discarded because only errors from Redunar's internal
    // loader probes are cleared here.
    unsafe { dlerror() };
}

#[unsafe(export_name = "dlsym")]
unsafe extern "C" fn interposed_dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void {
    let Some(real) = real_dlsym() else {
        return std::ptr::null_mut();
    };
    // SAFETY: this forwards the caller-owned handle and symbol to glibc's
    // versioned dlsym implementation.
    let resolved = unsafe { real(handle, symbol) };
    if !symbol.is_null() {
        // Log lookup attempts even when a handle-specific lookup returns null.
        // SAFETY: dlsym requires a valid NUL-terminated symbol name.
        match unsafe { CStr::from_ptr(symbol) }.to_bytes() {
            b"dlopen" => {
                launch_diag!(DLOPEN_DYNSYM_REQUEST, "dlopen-function-symbol-requested");
            }
            b"dlsym" => {
                launch_diag!(DLSYM_DYNSYM_REQUEST, "dlsym-function-symbol-requested");
            }
            b"SDL_DYNAPI_entry" => {
                launch_diag!(SDL_DYNSYM_REQUEST, "sdl-dynapi-symbol-requested");
            }
            b"SDL_GL_SwapWindow" => {
                launch_diag!(SDL_SWAP_SYMBOL, "sdl-swap-symbol-requested");
            }
            b"glXSwapBuffers" => {
                launch_diag!(GLX_SWAP_SYMBOL, "glx-swap-symbol-requested");
            }
            b"eglSwapBuffers" => {
                launch_diag!(EGL_SWAP_SYMBOL, "egl-swap-symbol-requested");
            }
            _ => {}
        }
    }
    if resolved.is_null() || symbol.is_null() {
        return resolved;
    }
    // .NET P/Invoke resolves SDL entry points against a specific dlopen
    // handle, bypassing ordinary LD_PRELOAD symbol interposition. Preserve the
    // handle-specific target before returning our launch-scoped wrapper. Some
    // loaders first resolve the dlopen/dlsym functions themselves this way;
    // return our forwarding wrappers for those two function pointers so later
    // library and SDL-symbol lookups remain observable and interceptable.
    // SAFETY: dlsym requires a valid NUL-terminated symbol name.
    match unsafe { CStr::from_ptr(symbol) }.to_bytes() {
        b"dlopen" => {
            if resolved as usize == interposed_dlopen as *const () as usize {
                launch_diag!(
                    DLOPEN_DYNSYM_ALREADY_WRAPPED,
                    "dlopen-function-already-resolves-to-redunar"
                );
            } else {
                launch_diag!(DLOPEN_DYNSYM_EXTERNAL, "dlopen-function-resolves-to-loader");
            }
            // Route function pointers resolved by runtimes such as MonoGame
            // through the same launch-scoped loader hook as ordinary PLT calls.
            interposed_dlopen as *const () as *mut c_void
        }
        b"dlsym" => {
            if resolved as usize == interposed_dlsym as *const () as usize {
                launch_diag!(
                    DLSYM_DYNSYM_ALREADY_WRAPPED,
                    "dlsym-function-already-resolves-to-redunar"
                );
            } else {
                launch_diag!(DLSYM_DYNSYM_EXTERNAL, "dlsym-function-resolves-to-loader");
            }
            interposed_dlsym as *const () as *mut c_void
        }
        b"SDL_DYNAPI_entry" => {
            let address = resolved as usize;
            if address != sdl_dynapi_entry_wrapper_address() {
                NEXT_SDL_DYNAPI_ENTRY.store(address, Ordering::Release);
                launch_diag!(SDL_DYNSYM_FOUND, "sdl-dynapi-symbol-found");
            }
            sdl_dynapi_entry as *const () as *mut c_void
        }
        b"SDL_Init" => {
            remember_sdl_target(resolved, &NEXT_SDL_INIT, sdl_init_wrapper_address());
            sdl_init as *const () as *mut c_void
        }
        b"SDL_GL_SwapWindow" => {
            remember_sdl_target(
                resolved,
                &NEXT_SDL_GL_SWAP_WINDOW,
                sdl_swap_wrapper_address(),
            );
            sdl_gl_swap_window as *const () as *mut c_void
        }
        b"SDL_GL_DeleteContext" => {
            remember_sdl_target(
                resolved,
                &NEXT_SDL_GL_DELETE_CONTEXT,
                sdl_delete_context_wrapper_address(),
            );
            sdl_gl_delete_context as *const () as *mut c_void
        }
        b"SDL_RenderPresent" => {
            remember_sdl_target(
                resolved,
                &NEXT_SDL_RENDER_PRESENT,
                sdl_render_wrapper_address(),
            );
            sdl_render_present as *const () as *mut c_void
        }
        b"glXSwapBuffers" => glx_swap_buffers as *const () as *mut c_void,
        b"eglSwapBuffers" => egl_swap_buffers as *const () as *mut c_void,
        b"glXDestroyContext" => glx_destroy_context as *const () as *mut c_void,
        b"eglDestroyContext" => egl_destroy_context as *const () as *mut c_void,
        _ => resolved,
    }
}

#[unsafe(export_name = "SDL_Init")]
unsafe extern "C" fn sdl_init(flags: u32) -> c_int {
    let mut address = NEXT_SDL_INIT.load(Ordering::Acquire);
    if address == 0 || address == sdl_init_wrapper_address() {
        // A handle-specific lookup may reach this exported wrapper before the
        // SDL dynapi table was discovered. Recover the next implementation so
        // the observer never turns a loader-order race into failed game audio.
        if let Some(next) = resolve_next::<SdlInit>(b"SDL_Init\0") {
            let resolved = next as *const () as usize;
            if resolved != 0 && resolved != sdl_init_wrapper_address() {
                NEXT_SDL_INIT.store(resolved, Ordering::Release);
                address = resolved;
            }
        }
    }
    if address == 0 || address == sdl_init_wrapper_address() {
        return -1;
    }
    // SAFETY: the address was saved from SDL_Init's exact dynapi slot.
    let next: SdlInit = unsafe { mem::transmute(address) };
    // SAFETY: the original caller-owned flags are forwarded unchanged.
    let result = unsafe { next(flags) };
    launch_diag!(SDL_INIT_HOOK, "sdl-init-hook-reached");
    let handle = SDL_HANDLE.load(Ordering::Acquire) as *mut c_void;
    if !handle.is_null() {
        install_sdl_presentation_hooks(handle);
    }
    result
}

#[unsafe(export_name = "SDL_DYNAPI_entry")]
unsafe extern "C" fn sdl_dynapi_entry(version: u32, table: *mut c_void, table_size: u32) -> c_int {
    let address = NEXT_SDL_DYNAPI_ENTRY.load(Ordering::Acquire);
    if address == 0 || address == sdl_dynapi_entry_wrapper_address() {
        return -1;
    }
    // SAFETY: the address was resolved from SDL's exact DYNAPI entry symbol.
    let next: SdlDynapiEntry = unsafe { mem::transmute(address) };
    launch_diag!(SDL_DYNAPI_ENTERED, "sdl-dynapi-entry-reached");
    // SAFETY: SDL supplies its own versioned jump-table buffer unchanged.
    let result = unsafe { next(version, table, table_size) };
    if result == 0 {
        let handle = SDL_HANDLE.load(Ordering::Acquire) as *mut c_void;
        if !handle.is_null() {
            install_sdl_presentation_hooks(handle);
        }
    }
    result
}

#[unsafe(export_name = "SDL_GL_SwapWindow")]
unsafe extern "C" fn sdl_gl_swap_window(window: *mut c_void) {
    let mut address = NEXT_SDL_GL_SWAP_WINDOW.load(Ordering::Acquire);
    if address == 0
        && let Some(resolved) = resolve_next::<SdlGlSwapWindow>(b"SDL_GL_SwapWindow\0")
    {
        let resolved = resolved as *const () as *mut c_void;
        remember_sdl_target(
            resolved,
            &NEXT_SDL_GL_SWAP_WINDOW,
            sdl_swap_wrapper_address(),
        );
        address = NEXT_SDL_GL_SWAP_WINDOW.load(Ordering::Acquire);
    }
    if address == 0 || address == sdl_swap_wrapper_address() {
        return;
    }
    launch_diag!(SDL_HOOK, "sdl-present-hook-reached");
    // SAFETY: the address was read from SDL_GL_SwapWindow's dynapi slot or an
    // exact successful symbol lookup.
    let next: SdlGlSwapWindow = unsafe { mem::transmute(address) };
    let context_key = desktop_context_key(window as usize);
    let selected = select_presentation(context_key);
    if selected {
        launch_diag!(SDL_SELECTED, "sdl-presentation-selected");
    } else {
        launch_diag!(SDL_REJECTED, "sdl-presentation-not-selected");
    }
    poll_replay_releases();
    if selected {
        overlay::render(context_key, overlay::ApiFlavor::Desktop);
        replay_readback::capture(context_key, overlay::ApiFlavor::Desktop);
        // SAFETY: the window's GL context is current for this presentation.
        unsafe { replay_export::capture(context_key, overlay::ApiFlavor::Desktop) };
    }
    // SAFETY: the function was either saved from SDL's handle-specific dlsym
    // result or resolved from the next ELF object for this exact symbol.
    unsafe { next(window) };
    if selected {
        record_present();
    }
}

#[unsafe(export_name = "SDL_GL_DeleteContext")]
unsafe extern "C" fn sdl_gl_delete_context(context: *mut c_void) {
    let mut address = NEXT_SDL_GL_DELETE_CONTEXT.load(Ordering::Acquire);
    if address == 0
        && let Some(resolved) = resolve_next::<SdlGlDeleteContext>(b"SDL_GL_DeleteContext\0")
    {
        let resolved = resolved as *const () as *mut c_void;
        remember_sdl_target(
            resolved,
            &NEXT_SDL_GL_DELETE_CONTEXT,
            sdl_delete_context_wrapper_address(),
        );
        address = NEXT_SDL_GL_DELETE_CONTEXT.load(Ordering::Acquire);
    }
    if address == 0 || address == sdl_delete_context_wrapper_address() {
        return;
    }
    destroy_context(context as usize);
    // SAFETY: the address was resolved from SDL_GL_DeleteContext's exact ABI.
    let next: SdlGlDeleteContext = unsafe { mem::transmute(address) };
    unsafe { next(context) };
}

#[unsafe(export_name = "SDL_RenderPresent")]
unsafe extern "C" fn sdl_render_present(renderer: *mut c_void) {
    let mut address = NEXT_SDL_RENDER_PRESENT.load(Ordering::Acquire);
    if address == 0
        && let Some(resolved) = resolve_next::<SdlRenderPresent>(b"SDL_RenderPresent\0")
    {
        let resolved = resolved as *const () as *mut c_void;
        remember_sdl_target(
            resolved,
            &NEXT_SDL_RENDER_PRESENT,
            sdl_render_wrapper_address(),
        );
        address = NEXT_SDL_RENDER_PRESENT.load(Ordering::Acquire);
    }
    if address == 0 || address == sdl_render_wrapper_address() {
        return;
    }
    // SAFETY: the address was read from SDL_RenderPresent's dynapi slot or an
    // exact successful symbol lookup.
    let next: SdlRenderPresent = unsafe { mem::transmute(address) };
    let context_key = desktop_context_key(renderer as usize);
    let selected = select_presentation(context_key);
    poll_replay_releases();
    if selected {
        overlay::render(context_key, overlay::ApiFlavor::Desktop);
        replay_readback::capture(context_key, overlay::ApiFlavor::Desktop);
        // SAFETY: the renderer's target GL context is current for this present.
        unsafe { replay_export::capture(context_key, overlay::ApiFlavor::Desktop) };
    }
    // SAFETY: the function was saved from the exact SDL presentation symbol.
    unsafe { next(renderer) };
    if selected {
        record_present();
    }
}

fn remember_sdl_target(resolved: *mut c_void, next: &AtomicUsize, wrapper: usize) {
    let target = sdl_dynapi_slot(resolved).map_or(resolved as usize, |slot| {
        // SAFETY: `sdl_dynapi_slot` validates the x86_64 stub and alignment.
        unsafe { (*slot).load(Ordering::Acquire) }
    });
    if target != 0 && target != wrapper {
        next.store(target, Ordering::Release);
    }
}

fn install_sdl_dynapi_hook(handle: *mut c_void) -> bool {
    let Some(real) = real_dlsym() else {
        return false;
    };
    let init_hooked = install_sdl_symbol_hook(
        real,
        handle,
        c"SDL_Init",
        &NEXT_SDL_INIT,
        sdl_init_wrapper_address(),
    );
    // These exports identify SDL without recording paths or enumerating
    // unrelated libraries from the game's loader.
    let has_sdl_swap = unsafe { real(handle, c"SDL_GL_SwapWindow".as_ptr()) };
    let own_sdl_swap = has_sdl_swap as usize == sdl_swap_wrapper_address();
    if own_sdl_swap {
        launch_diag!(SDL_SELF_WRAPPER, "sdl-probe-resolved-to-redunar-wrapper");
    } else if !has_sdl_swap.is_null() {
        launch_diag!(SDL_LIBRARY_FOUND, "sdl-library-found");
    }
    // SAFETY: the handle is live and the symbol is NUL-terminated.
    let dynapi_entry = unsafe { real(handle, c"SDL_DYNAPI_entry".as_ptr()) } as usize;
    let dynapi_entry_found =
        dynapi_entry != 0 && dynapi_entry != sdl_dynapi_entry_wrapper_address();
    if dynapi_entry_found {
        NEXT_SDL_DYNAPI_ENTRY.store(dynapi_entry, Ordering::Release);
    }
    let presentation_hooked = install_sdl_presentation_hooks_with(real, handle);
    if presentation_hooked {
        launch_diag!(SDL_HOOKS_INSTALLED, "sdl-presentation-hooks-installed");
    } else if !has_sdl_swap.is_null() && !own_sdl_swap {
        launch_diag!(
            SDL_HOOK_INSTALL_FAILED,
            "sdl-presentation-hooks-not-installed"
        );
    }
    if init_hooked || presentation_hooked || dynapi_entry_found {
        // SDL_Init opens unrelated graphics and input libraries. Retain this
        // handle only when the exact object exposes an SDL dynapi entry.
        SDL_HANDLE.store(handle as usize, Ordering::Release);
    }
    init_hooked || presentation_hooked || dynapi_entry_found
}

fn install_sdl_presentation_hooks(handle: *mut c_void) -> bool {
    let Some(real) = real_dlsym() else {
        return false;
    };
    install_sdl_presentation_hooks_with(real, handle)
}

fn install_sdl_presentation_hooks_with(real: Dlsym, handle: *mut c_void) -> bool {
    install_sdl_symbol_hook(
        real,
        handle,
        c"SDL_GL_SwapWindow",
        &NEXT_SDL_GL_SWAP_WINDOW,
        sdl_swap_wrapper_address(),
    ) | install_sdl_symbol_hook(
        real,
        handle,
        c"SDL_RenderPresent",
        &NEXT_SDL_RENDER_PRESENT,
        sdl_render_wrapper_address(),
    ) | install_sdl_symbol_hook(
        real,
        handle,
        c"SDL_GL_DeleteContext",
        &NEXT_SDL_GL_DELETE_CONTEXT,
        sdl_delete_context_wrapper_address(),
    )
}

fn install_sdl_symbol_hook(
    real: Dlsym,
    handle: *mut c_void,
    symbol: &CStr,
    next: &AtomicUsize,
    wrapper: usize,
) -> bool {
    // SAFETY: the handle is live and the symbol is NUL-terminated.
    let resolved = unsafe { real(handle, symbol.as_ptr()) };
    if resolved as usize == wrapper {
        launch_diag!(SDL_SELF_WRAPPER, "sdl-probe-resolved-to-redunar-wrapper");
        return false;
    }
    let Some(slot) = sdl_dynapi_slot(resolved) else {
        if !resolved.is_null() {
            launch_diag!(
                SDL_FOREIGN_UNPATCHABLE,
                "sdl-symbol-found-but-not-a-dynapi-stub"
            );
        }
        return false;
    };
    // SAFETY: the validated SDL dynapi slot is pointer-aligned writable data.
    let original = unsafe { (*slot).swap(wrapper, Ordering::AcqRel) };
    if original != 0 && original != wrapper {
        next.store(original, Ordering::Release);
    }
    true
}

#[cfg(target_arch = "x86_64")]
fn sdl_dynapi_slot(resolved: *mut c_void) -> Option<*mut AtomicUsize> {
    if resolved.is_null() {
        return None;
    }
    let stub = resolved.cast::<u8>();
    // SDL2's x86_64 dynapi exports are six-byte RIP-relative indirect jumps:
    // `ff 25 <disp32>`. Reject every other layout instead of patching blindly.
    // SAFETY: a successful dlsym result points to readable executable memory.
    if unsafe { *stub } != 0xff || unsafe { *stub.add(1) } != 0x25 {
        return None;
    }
    let mut displacement = [0_u8; 4];
    // SAFETY: the validated instruction has four displacement bytes at 2..6.
    unsafe { std::ptr::copy_nonoverlapping(stub.add(2), displacement.as_mut_ptr(), 4) };
    let displacement = i32::from_le_bytes(displacement) as isize;
    // SAFETY: the RIP-relative target is computed from the validated stub.
    let slot_address = unsafe { stub.add(6).offset(displacement) } as usize;
    slot_address
        .is_multiple_of(mem::align_of::<AtomicUsize>())
        .then_some(slot_address as *mut AtomicUsize)
}

#[cfg(not(target_arch = "x86_64"))]
fn sdl_dynapi_slot(_resolved: *mut c_void) -> Option<*mut AtomicUsize> {
    None
}

fn sdl_swap_wrapper_address() -> usize {
    sdl_gl_swap_window as *const () as usize
}

fn sdl_delete_context_wrapper_address() -> usize {
    sdl_gl_delete_context as *const () as usize
}

fn sdl_init_wrapper_address() -> usize {
    sdl_init as *const () as usize
}

fn sdl_dynapi_entry_wrapper_address() -> usize {
    sdl_dynapi_entry as *const () as usize
}

fn sdl_render_wrapper_address() -> usize {
    sdl_render_present as *const () as usize
}

fn desktop_context_key(fallback: usize) -> usize {
    let glx = GLX_GET_CURRENT_CONTEXT
        .get_or_init(|| resolve_next(b"glXGetCurrentContext\0"))
        .map_or(std::ptr::null_mut(), |get| unsafe { get() });
    if !glx.is_null() {
        return glx as usize;
    }
    let egl = EGL_GET_CURRENT_CONTEXT
        .get_or_init(|| resolve_next(b"eglGetCurrentContext\0"))
        .map_or(std::ptr::null_mut(), |get| unsafe { get() });
    if egl.is_null() {
        fallback
    } else {
        egl as usize
    }
}

/// Choose one stable presentation source per process. Auxiliary OpenGL
/// windows are common in launchers and games; selecting the largest current
/// viewport prevents their swaps from inflating frame metrics or repeatedly
/// replacing the Replay codec epoch. Context destruction clears the choice so
/// a replacement context can take ownership immediately.
fn select_presentation(context: usize) -> bool {
    if context == 0 {
        return false;
    }
    let Some(get_integer) =
        *GL_GET_INTEGER.get_or_init(|| resolve_gl_function::<GetInteger>(b"glGetIntegerv\0"))
    else {
        return false;
    };
    let mut viewport = [0_i32; 4];
    // SAFETY: the presentation hook runs with the target context current and
    // GL_VIEWPORT writes exactly four integers into local storage.
    unsafe { get_integer(GL_VIEWPORT, viewport.as_mut_ptr()) };
    let pixels = u64::try_from(viewport[2])
        .ok()
        .zip(u64::try_from(viewport[3]).ok())
        .and_then(|(width, height)| width.checked_mul(height))
        .unwrap_or(0);
    let Ok(mut target) = PRESENTATION_TARGET.try_lock() else {
        return false;
    };
    target.select(context, pixels)
}

#[unsafe(export_name = "glXSwapBuffers")]
unsafe extern "C" fn glx_swap_buffers(display: *mut c_void, drawable: usize) {
    if let Some(next) = *NEXT_GLX_SWAP_BUFFERS.get_or_init(|| resolve_next(b"glXSwapBuffers\0")) {
        launch_diag!(GLX_HOOK, "glx-present-hook-reached");
        let context = GLX_GET_CURRENT_CONTEXT
            .get_or_init(|| resolve_next(b"glXGetCurrentContext\0"))
            .map_or(std::ptr::null_mut(), |get| unsafe { get() });
        let context_key = if context.is_null() {
            drawable
        } else {
            context as usize
        };
        let selected = select_presentation(context_key);
        if selected {
            launch_diag!(GLX_SELECTED, "glx-presentation-selected");
        } else {
            launch_diag!(GLX_REJECTED, "glx-presentation-not-selected");
        }
        poll_replay_releases();
        if selected {
            overlay::render(context_key, overlay::ApiFlavor::Desktop);
            replay_readback::capture(context_key, overlay::ApiFlavor::Desktop);
            // SAFETY: the GLX current context matches the context key used for
            // the pool lookup in the export path.
            unsafe { replay_export::capture(context_key, overlay::ApiFlavor::Desktop) };
        }
        // SAFETY: the function was resolved from the next ELF object for the
        // exact GLX symbol and receives the caller's untouched arguments.
        unsafe { next(display, drawable) };
        if selected {
            record_present();
        }
    }
}

#[unsafe(export_name = "glXDestroyContext")]
unsafe extern "C" fn glx_destroy_context(display: *mut c_void, context: *mut c_void) {
    let Some(next) = *NEXT_GLX_DESTROY_CONTEXT
        .get_or_init(|| resolve_next::<GlxDestroyContext>(b"glXDestroyContext\0"))
    else {
        return;
    };
    destroy_context(context as usize);
    // SAFETY: the function was resolved from the next ELF object for this
    // exact GLX symbol and receives the caller's untouched handles.
    unsafe { next(display, context) };
}

#[unsafe(export_name = "eglDestroyContext")]
unsafe extern "C" fn egl_destroy_context(display: *mut c_void, context: *mut c_void) -> u32 {
    let Some(next) = *NEXT_EGL_DESTROY_CONTEXT
        .get_or_init(|| resolve_next::<EglDestroyContext>(b"eglDestroyContext\0"))
    else {
        return 0;
    };
    // SAFETY: the function was resolved from the next ELF object for this
    // exact EGL symbol and receives the caller's untouched handles.
    let result = unsafe { next(display, context) };
    if result != 0 {
        destroy_context(context as usize);
    }
    result
}

fn destroy_context(context: usize) {
    PRESENTATION_TARGET
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(context);
    overlay::destroy_context(context);
    replay_readback::destroy_context(context);
    replay_export::destroy_context(context);
}

#[unsafe(export_name = "eglSwapBuffers")]
unsafe extern "C" fn egl_swap_buffers(display: *mut c_void, surface: *mut c_void) -> u32 {
    let Some(next) = *NEXT_EGL_SWAP_BUFFERS.get_or_init(|| resolve_next(b"eglSwapBuffers\0"))
    else {
        return 0;
    };
    launch_diag!(EGL_HOOK, "egl-present-hook-reached");
    let context = EGL_GET_CURRENT_CONTEXT
        .get_or_init(|| resolve_next(b"eglGetCurrentContext\0"))
        .map_or(std::ptr::null_mut(), |get| unsafe { get() });
    let context_key = if context.is_null() {
        surface as usize
    } else {
        context as usize
    };
    let selected = select_presentation(context_key);
    if selected {
        launch_diag!(EGL_SELECTED, "egl-presentation-selected");
    } else {
        launch_diag!(EGL_REJECTED, "egl-presentation-not-selected");
    }
    poll_replay_releases();
    if selected {
        overlay::render(context_key, overlay::ApiFlavor::Embedded);
        replay_readback::capture(context_key, overlay::ApiFlavor::Embedded);
        // SAFETY: the EGL current context matches the context key used for the
        // pool lookup in the export path.
        unsafe { replay_export::capture(context_key, overlay::ApiFlavor::Embedded) };
    }
    // SAFETY: the function was resolved from the next ELF object for the
    // exact EGL symbol and receives the caller's untouched arguments.
    let result = unsafe { next(display, surface) };
    if result != 0 && selected {
        record_present();
    }
    result
}

#[unsafe(export_name = "glXGetProcAddress")]
unsafe extern "C" fn glx_get_proc_address(name: *const u8) -> *mut c_void {
    if requested_symbol_is(name, b"glXSwapBuffers\0") {
        launch_diag!(GLX_SWAP_RESOLVER, "glx-swap-resolver-requested");
    }
    if requested_symbol_is(name, b"glXDestroyContext\0") {
        return glx_destroy_context as *const () as *mut c_void;
    }
    unsafe {
        intercept_or_forward(
            name,
            b"glXSwapBuffers\0",
            glx_swap_buffers as *const () as *mut c_void,
            &NEXT_GLX_GET_PROC_ADDRESS,
            b"glXGetProcAddress\0",
        )
    }
}

#[unsafe(export_name = "glXGetProcAddressARB")]
unsafe extern "C" fn glx_get_proc_address_arb(name: *const u8) -> *mut c_void {
    if requested_symbol_is(name, b"glXSwapBuffers\0") {
        launch_diag!(GLX_SWAP_RESOLVER, "glx-swap-resolver-requested");
    }
    if requested_symbol_is(name, b"glXDestroyContext\0") {
        return glx_destroy_context as *const () as *mut c_void;
    }
    unsafe {
        intercept_or_forward(
            name,
            b"glXSwapBuffers\0",
            glx_swap_buffers as *const () as *mut c_void,
            &NEXT_GLX_GET_PROC_ADDRESS_ARB,
            b"glXGetProcAddressARB\0",
        )
    }
}

fn requested_symbol_is(name: *const u8, expected: &[u8]) -> bool {
    if name.is_null() {
        return false;
    }
    // SAFETY: GLX/EGL procedure resolvers require a valid NUL-terminated name.
    unsafe { CStr::from_ptr(name.cast()) }.to_bytes_with_nul() == expected
}

#[unsafe(export_name = "eglGetProcAddress")]
unsafe extern "C" fn egl_get_proc_address(name: *const u8) -> *mut c_void {
    if requested_symbol_is(name, b"eglDestroyContext\0") {
        return egl_destroy_context as *const () as *mut c_void;
    }
    unsafe {
        intercept_or_forward(
            name,
            b"eglSwapBuffers\0",
            egl_swap_buffers as *const () as *mut c_void,
            &NEXT_EGL_GET_PROC_ADDRESS,
            b"eglGetProcAddress\0",
        )
    }
}

unsafe fn intercept_or_forward(
    name: *const u8,
    intercepted_name: &[u8],
    intercepted: *mut c_void,
    next_slot: &OnceLock<Option<GetProcAddress>>,
    resolver_name: &[u8],
) -> *mut c_void {
    if name.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: GLX/EGL require a valid NUL-terminated procedure name.
    let requested = unsafe { CStr::from_ptr(name.cast()) }.to_bytes_with_nul();
    if requested == intercepted_name {
        return intercepted;
    }
    let Some(next) = *next_slot.get_or_init(|| resolve_next(resolver_name)) else {
        return std::ptr::null_mut();
    };
    // SAFETY: the next resolver is called with the original API-owned name.
    unsafe { next(name) }
}

fn resolve_next<T: Copy>(symbol: &[u8]) -> Option<T> {
    debug_assert_eq!(symbol.last(), Some(&0));
    let next_handle = (-1_isize) as *mut c_void;
    // SAFETY: `symbol` is statically NUL-terminated and RTLD_NEXT asks the
    // dynamic loader for the next definition after this interposer.
    let real = real_dlsym()?;
    // SAFETY: `symbol` is statically NUL-terminated and RTLD_NEXT asks the
    // dynamic loader for the next definition after this interposer.
    let address = unsafe { real(next_handle, symbol.as_ptr().cast()) };
    if address.is_null() {
        None
    } else {
        // SAFETY: every caller chooses `T` to match the exact symbol ABI.
        Some(unsafe { mem::transmute_copy(&address) })
    }
}

fn resolve_gl_function<T: Copy>(symbol: &[u8]) -> Option<T> {
    if let Some(resolved) = resolve_next(symbol) {
        return Some(resolved);
    }
    let handle = SDL_HANDLE.load(Ordering::Acquire) as *mut c_void;
    if handle.is_null() {
        return None;
    }
    let real = real_dlsym()?;
    // SAFETY: `handle` is retained from the loaded SDL object and the static
    // resolver name is NUL-terminated.
    let resolver = unsafe { real(handle, c"SDL_GL_GetProcAddress".as_ptr()) };
    if resolver.is_null() {
        return None;
    }
    // SAFETY: SDL_GL_GetProcAddress has this stable SDL2 ABI. The requested GL
    // symbol is NUL-terminated and the caller supplies its exact function type.
    let resolver: SdlGlGetProcAddress = unsafe { mem::transmute(resolver) };
    let address = unsafe { resolver(symbol.as_ptr().cast()) };
    if address.is_null() {
        None
    } else {
        // SAFETY: the SDL resolver returned the implementation for `symbol`.
        Some(unsafe { mem::transmute_copy(&address) })
    }
}

fn real_dlsym() -> Option<Dlsym> {
    *REAL_DLSYM.get_or_init(|| {
        let next_handle = (-1_isize) as *mut c_void;
        // SAFETY: glibc exposes dlsym at GLIBC_2.2.5 on supported x86_64 Linux
        // targets. Using dlvsym avoids recursing through our exported dlsym.
        let address = unsafe { dlvsym(next_handle, c"dlsym".as_ptr(), c"GLIBC_2.2.5".as_ptr()) };
        if address.is_null() {
            None
        } else {
            // SAFETY: the versioned lookup returned dlsym's exact ABI.
            Some(unsafe { mem::transmute_copy(&address) })
        }
    })
}

fn real_dlopen() -> Option<Dlopen> {
    *REAL_DLOPEN.get_or_init(|| {
        let next_handle = (-1_isize) as *mut c_void;
        // SAFETY: glibc exposes dlopen at GLIBC_2.2.5 on supported x86_64
        // Linux targets. The versioned lookup bypasses our exported wrapper.
        let address = unsafe { dlvsym(next_handle, c"dlopen".as_ptr(), c"GLIBC_2.2.5".as_ptr()) };
        if address.is_null() {
            None
        } else {
            // SAFETY: the versioned lookup returned dlopen's exact ABI.
            Some(unsafe { mem::transmute_copy(&address) })
        }
    })
}

fn record_present() {
    poll_replay_releases();
    let Some(now_ns) = monotonic_ns() else {
        return;
    };
    let previous_ns = LAST_PRESENT_NS.swap(now_ns, Ordering::Relaxed);
    if previous_ns == 0 || now_ns <= previous_ns {
        if let Ok(mut producer) = PRODUCER.try_lock() {
            producer.initialize(now_ns);
        }
        return;
    }
    let sequence = NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    overlay::record_interval(now_ns - previous_ns);
    let Ok(mut producer) = PRODUCER.try_lock() else {
        return;
    };
    producer.initialize(now_ns);
    producer.push(sequence, now_ns - previous_ns, now_ns);
}

fn poll_replay_releases() {
    let Ok(mut pending) = PENDING_REPLAY_RELEASES.try_lock() else {
        return;
    };
    drain_replay_releases(&mut pending, apply_replay_release);
    {
        let Ok(producer) = PRODUCER.try_lock() else {
            return;
        };
        let Some(socket) = producer.socket.as_ref() else {
            return;
        };
        let mut bytes = [0; MAX_MESSAGE_BYTES];
        while pending.len() < 8 {
            match socket.recv(&mut bytes) {
                Ok(length) => {
                    if let Ok(CaptureMessage::ReplayFrameReleased { sequence, .. }) =
                        redunar_capture::decode_message(&bytes[..length])
                    {
                        pending.push_back(sequence);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
    }
    drain_replay_releases(&mut pending, apply_replay_release);
}

fn apply_replay_release(sequence: u64) -> bool {
    // Both calls are idempotent. If either lock is busy, retry the token on
    // the next presentation instead of losing the daemon's ownership release.
    let readback_done = replay_readback::release_sequence(sequence);
    let export_done = replay_export::release_sequence(sequence);
    readback_done && export_done
}

fn drain_replay_releases(pending: &mut VecDeque<u64>, mut apply: impl FnMut(u64) -> bool) {
    while let Some(&sequence) = pending.front() {
        if !apply(sequence) {
            break;
        }
        pending.pop_front();
    }
}

pub(crate) fn record_overlay_active() {
    record_overlay_status(OverlayRuntimeStatus::Active);
}

pub(crate) fn record_overlay_error(reason: OverlayFailureReason) {
    record_overlay_status(OverlayRuntimeStatus::Error(reason));
}

pub(crate) fn record_replay_source_candidate(candidate: ReplaySourceCandidate) {
    record_replay_source_assessment(Ok(candidate));
}

pub(crate) fn record_replay_source_rejected(reason: ReplaySourceRejection) {
    record_replay_source_assessment(Err(reason));
}

fn record_replay_source_assessment(
    assessment: Result<ReplaySourceCandidate, ReplaySourceRejection>,
) {
    let started_ns = monotonic_ns().unwrap_or(1);
    let Ok(mut producer) = PRODUCER.try_lock() else {
        return;
    };
    producer.initialize(started_ns);
    let (Some(socket), Some(session_id)) = (&producer.socket, producer.session_id) else {
        return;
    };
    let message = match assessment {
        Ok(candidate) => CaptureMessage::ReplaySourceCandidate {
            session_id,
            candidate,
        },
        Err(reason) => CaptureMessage::ReplaySourceRejected { session_id, reason },
    };
    let mut buffer = [0; MAX_MESSAGE_BYTES];
    if let Ok(length) = encode_message(&message, &mut buffer) {
        let _ = socket.send(&buffer[..length]);
    }
}

/// Send one exported-frame announcement with its transferred descriptor.
/// The duplicate is consumed here in every path: a failed encode or send
/// still closes it, because no other owner exists.
pub(crate) fn record_replay_frame_exported(
    sequence: u64,
    descriptor: std::os::fd::RawFd,
    source: ReplaySourceCandidate,
    stride: u32,
    timestamp_ns: u64,
    duration_ns: u64,
) -> Result<(), ()> {
    let _guard = fd_lifecycle_guard(descriptor);
    let started_ns = monotonic_ns().unwrap_or(1);
    let Ok(mut producer) = PRODUCER.try_lock() else {
        return Err(());
    };
    producer.initialize(started_ns);
    let (Some(socket), Some(session_id)) = (&producer.socket, producer.session_id) else {
        return Err(());
    };
    let Ok(fd_number) = u32::try_from(descriptor) else {
        return Err(());
    };
    let message = CaptureMessage::ReplayFrameExported {
        session_id,
        sequence,
        fd_number,
        source,
        offset: 0,
        stride,
        modifier: 0,
        timestamp_ns,
        duration_ns,
    };
    let mut buffer = [0; MAX_MESSAGE_BYTES];
    let length = encode_message(&message, &mut buffer).map_err(|_| ())?;
    fd_transport::send_datagram_fd(socket, &buffer[..length], descriptor).map_err(|_| ())?;
    Ok(())
}

/// Close one producer-owned duplicate exactly once when the export attempt
/// ends, so every early return in `record_replay_frame_exported` is leak-free.
fn fd_lifecycle_guard(descriptor: std::os::fd::RawFd) -> std::os::fd::OwnedFd {
    // SAFETY: the caller transfers ownership of this fresh duplicate.
    unsafe { std::os::fd::OwnedFd::from_raw_fd(descriptor) }
}

pub(crate) fn record_replay_frame_copied(
    source: ReplaySourceCandidate,
    copied_bytes: u32,
    sample_checksum: u64,
) {
    let started_ns = monotonic_ns().unwrap_or(1);
    let Ok(mut producer) = PRODUCER.try_lock() else {
        return;
    };
    producer.initialize(started_ns);
    let (Some(socket), Some(session_id)) = (&producer.socket, producer.session_id) else {
        return;
    };
    let message = CaptureMessage::ReplayFrameCopied {
        session_id,
        sequence: NEXT_REPLAY_COPY_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        source,
        copied_bytes,
        sample_checksum,
    };
    let mut buffer = [0; MAX_MESSAGE_BYTES];
    if let Ok(length) = encode_message(&message, &mut buffer) {
        let _ = socket.send(&buffer[..length]);
    }
}

fn record_overlay_status(status: OverlayRuntimeStatus) {
    let started_ns = monotonic_ns().unwrap_or(1);
    let Ok(mut producer) = PRODUCER.try_lock() else {
        return;
    };
    producer.initialize(started_ns);
    producer.send_overlay_status(status);
}

impl Producer {
    fn initialize(&mut self, started_ns: u64) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        if env::var("REDUNAR_CAPTURE_PROTOCOL").ok().as_deref()
            != Some(&PROTOCOL_VERSION.to_string())
        {
            launch_diag!(PRODUCER_ENV_MISSING, "producer-launch-environment-missing");
            return;
        }
        let Some(session_id) = env::var("REDUNAR_CAPTURE_SESSION")
            .ok()
            .and_then(|value| CaptureSessionId::from_hex(&value).ok())
        else {
            launch_diag!(PRODUCER_ENV_MISSING, "producer-launch-environment-missing");
            return;
        };
        let Some(socket_path) = env::var_os("REDUNAR_CAPTURE_SOCKET") else {
            launch_diag!(PRODUCER_ENV_MISSING, "producer-launch-environment-missing");
            return;
        };
        if !Path::new(&socket_path).is_absolute() {
            launch_diag!(PRODUCER_ENV_MISSING, "producer-launch-environment-missing");
            return;
        }
        let reply_path = env::var_os("REDUNAR_CAPTURE_REPLY_SOCKET")
            .or_else(|| {
                Path::new(&socket_path)
                    .parent()
                    .map(|parent| parent.join("capture-reply.sock").into_os_string())
            })
            .and_then(|base| process_reply_path(Path::new(&base), std::process::id()));
        let Some(reply_path) = reply_path else {
            return;
        };
        if let Ok(metadata) = std::fs::symlink_metadata(&reply_path)
            && (!metadata.file_type().is_socket() || std::fs::remove_file(&reply_path).is_err())
        {
            launch_diag!(PRODUCER_SOCKET_FAILED, "producer-socket-setup-failed");
            return;
        }
        let Ok(socket) = UnixDatagram::bind(&reply_path) else {
            launch_diag!(PRODUCER_SOCKET_FAILED, "producer-socket-setup-failed");
            return;
        };
        if std::fs::set_permissions(&reply_path, std::fs::Permissions::from_mode(0o600)).is_err()
            || socket.set_nonblocking(true).is_err()
            || socket.connect(&socket_path).is_err()
        {
            let _ = std::fs::remove_file(&reply_path);
            launch_diag!(PRODUCER_SOCKET_FAILED, "producer-socket-setup-failed");
            return;
        }
        let hello = CaptureMessage::Hello {
            session_id,
            process_id: std::process::id(),
            api: CaptureApi::OpenGl,
            producer_started_monotonic_ns: started_ns.max(1),
        };
        let mut buffer = [0; MAX_MESSAGE_BYTES];
        let Ok(length) = encode_message(&hello, &mut buffer) else {
            launch_diag!(PRODUCER_SOCKET_FAILED, "producer-hello-send-failed");
            return;
        };
        if socket.send(&buffer[..length]).is_err() {
            let _ = std::fs::remove_file(&reply_path);
            launch_diag!(PRODUCER_SOCKET_FAILED, "producer-hello-send-failed");
            return;
        }
        launch_diag!(PRODUCER_HELLO_SENT, "producer-hello-sent");
        self.session_id = Some(session_id);
        self.socket = Some(socket);
        self.reply_path = Some(reply_path);
        self.last_flush_ns = started_ns;
        if overlay::renderer_requested_from_environment() {
            self.send_overlay_status(OverlayRuntimeStatus::Requested);
        }
        // SAFETY: the callback has C ABI, captures no borrowed state, and the
        // process invokes it at most once for this registration.
        let _ = unsafe { atexit(finish_producer) };
    }

    fn send_overlay_status(&mut self, status: OverlayRuntimeStatus) {
        if self.overlay_status == Some(status) {
            return;
        }
        if status != OverlayRuntimeStatus::Requested && self.overlay_status.is_none() {
            self.send_overlay_status(OverlayRuntimeStatus::Requested);
            if self.overlay_status.is_none() {
                return;
            }
        }
        let (Some(socket), Some(session_id)) = (&self.socket, self.session_id) else {
            return;
        };
        let message = CaptureMessage::OverlayStatus { session_id, status };
        let mut buffer = [0; MAX_MESSAGE_BYTES];
        if let Ok(length) = encode_message(&message, &mut buffer)
            && socket.send(&buffer[..length]).is_ok()
        {
            self.overlay_status = Some(status);
        }
    }

    fn push(&mut self, sequence: u64, interval_ns: u64, now_ns: u64) {
        if self.socket.is_none() {
            return;
        }
        if self
            .previous_sequence
            .is_some_and(|previous| sequence != previous.saturating_add(1))
        {
            self.flush(now_ns);
        }
        if self.interval_count == 0 {
            self.first_sequence = sequence;
        }
        self.intervals[self.interval_count] = interval_ns;
        self.interval_count += 1;
        self.previous_sequence = Some(sequence);
        if self.interval_count == BATCH_INTERVALS
            || now_ns.saturating_sub(self.last_flush_ns) >= MAX_BATCH_LATENCY_NS
        {
            self.flush(now_ns);
        }
    }

    fn flush(&mut self, now_ns: u64) {
        if self.interval_count == 0 {
            return;
        }
        let (Some(socket), Some(session_id)) = (&self.socket, self.session_id) else {
            self.interval_count = 0;
            return;
        };
        let mut buffer = [0; MAX_MESSAGE_BYTES];
        if let Ok(length) = encode_frame_batch(
            session_id,
            CaptureApi::OpenGl,
            self.first_sequence,
            &self.intervals[..self.interval_count],
            &mut buffer,
        ) {
            let _ = socket.send(&buffer[..length]);
        }
        self.interval_count = 0;
        self.last_flush_ns = now_ns;
    }

    fn finish(&mut self) {
        let now_ns = monotonic_ns().unwrap_or(self.last_flush_ns);
        self.flush(now_ns);
        if let (Some(socket), Some(session_id)) = (&self.socket, self.session_id) {
            let message = CaptureMessage::Goodbye {
                session_id,
                api: CaptureApi::OpenGl,
                last_sequence: NEXT_SEQUENCE.load(Ordering::Relaxed).saturating_sub(1),
                reason: GoodbyeReason::ProcessExit,
            };
            let mut buffer = [0; MAX_MESSAGE_BYTES];
            if let Ok(length) = encode_message(&message, &mut buffer) {
                let _ = socket.send(&buffer[..length]);
            }
        }
        self.socket = None;
        if let Some(path) = self.reply_path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn process_reply_path(base: &Path, process_id: u32) -> Option<std::path::PathBuf> {
    let parent = base.parent()?;
    if !base.is_absolute() || process_id == 0 {
        return None;
    }
    Some(parent.join(format!("r-{process_id}.sock")))
}

unsafe extern "C" fn finish_producer() {
    // Drop export bookkeeping first so no new pool can be created while the
    // session socket is closing. Pool GPU names belong to live contexts and
    // are intentionally released with the address space.
    replay_export::finish();
    lock(&PRODUCER).finish();
}

fn monotonic_ns() -> Option<u64> {
    let mut time = Timespec {
        seconds: 0,
        nanoseconds: 0,
    };
    // SAFETY: `time` is valid writable storage for one `Timespec`.
    if unsafe { clock_gettime(CLOCK_MONOTONIC, &raw mut time) } != 0
        || time.seconds < 0
        || !(0..1_000_000_000).contains(&time.nanoseconds)
    {
        return None;
    }
    u64::try_from(time.seconds)
        .ok()?
        .checked_mul(1_000_000_000)?
        .checked_add(u64::try_from(time.nanoseconds).ok()?)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::{
        CaptureSessionId, PresentationTarget, Producer, drain_replay_releases, process_reply_path,
    };
    use std::collections::VecDeque;
    use std::os::unix::net::UnixDatagram;
    use std::path::Path;

    #[test]
    fn every_opengl_process_gets_a_private_reply_socket() {
        let base = Path::new("/run/user/1000/redunar/session/capture-reply.sock");
        assert_eq!(
            process_reply_path(base, 101),
            Some(Path::new("/run/user/1000/redunar/session/r-101.sock").to_path_buf())
        );
        assert_eq!(
            process_reply_path(base, 202),
            Some(Path::new("/run/user/1000/redunar/session/r-202.sock").to_path_buf())
        );
        assert!(process_reply_path(base, 0).is_none());
        assert!(process_reply_path(Path::new("capture-reply.sock"), 101).is_none());
    }

    #[test]
    fn largest_live_viewport_owns_presentation_telemetry() {
        let mut target = PresentationTarget::default();
        assert!(target.select(11, 320 * 180));
        assert!(!target.select(12, 300 * 180));
        assert!(target.select(12, 1280 * 720));
        assert!(!target.select(11, 640 * 360));
        assert!(target.select(12, 800 * 600));
        target.remove(12);
        assert!(target.select(11, 640 * 360));
    }

    #[test]
    fn busy_capture_lock_retains_daemon_release_for_next_present() {
        let mut pending = VecDeque::from([17, 18]);
        let mut applied = Vec::new();
        drain_replay_releases(&mut pending, |sequence| {
            applied.push(sequence);
            sequence != 17
        });
        assert_eq!(pending, VecDeque::from([17, 18]));
        drain_replay_releases(&mut pending, |sequence| {
            applied.push(sequence);
            true
        });
        assert!(pending.is_empty());
        assert_eq!(applied, [17, 17, 18]);
    }

    #[test]
    fn lost_daemon_keeps_producer_telemetry_bounded_until_game_exit() {
        let (socket, daemon) = UnixDatagram::pair().expect("private socket pair");
        socket.set_nonblocking(true).expect("nonblocking producer");
        drop(daemon);
        let mut producer = Producer {
            initialized: true,
            socket: Some(socket),
            session_id: Some(
                CaptureSessionId::from_hex("0123456789abcdef0123456789abcdef")
                    .expect("valid session id"),
            ),
            ..Producer::default()
        };
        for sequence in 1..=10_000 {
            producer.push(sequence, 16_666_667, sequence * 16_666_667);
            assert!(producer.interval_count <= super::BATCH_INTERVALS);
        }
        producer.finish();
        assert_eq!(producer.interval_count, 0);
        assert!(producer.socket.is_none());
    }
}
