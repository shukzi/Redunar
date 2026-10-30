//! Bounded GLX presentation target for Redunar's OpenGL telemetry probe.

use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};
use std::fs;
use std::path::PathBuf;
use std::ptr;
use std::thread;
use std::time::Duration;

const GLX_RGBA: c_int = 4;
const GLX_DOUBLEBUFFER: c_int = 5;
const INPUT_OUTPUT: c_uint = 1;
const ALLOC_NONE: c_int = 0;
const CW_COLORMAP: c_ulong = 1 << 13;
const GL_COLOR_BUFFER_BIT: c_uint = 0x0000_4000;
const GL_SCISSOR_TEST: c_uint = 0x0C11;
const CLIENT_MESSAGE: c_int = 33;
const SUBSTRUCTURE_NOTIFY_MASK: c_long = 1 << 19;
const SUBSTRUCTURE_REDIRECT_MASK: c_long = 1 << 20;
const GL_FRONT: c_uint = 0x0404;
const GL_RGB: c_uint = 0x1907;
const GL_UNSIGNED_BYTE: c_uint = 0x1401;
const DEFAULT_WIDTH: u32 = 320;
const DEFAULT_HEIGHT: u32 = 180;
const DEFAULT_FRAME_COUNT: u32 = 180;

type Window = c_ulong;
type Colormap = c_ulong;
type GlxContext = *mut c_void;

#[repr(C)]
struct XVisualInfo {
    visual: *mut c_void,
    visual_id: c_ulong,
    screen: c_int,
    depth: c_int,
    class: c_int,
    red_mask: c_ulong,
    green_mask: c_ulong,
    blue_mask: c_ulong,
    colormap_size: c_int,
    bits_per_rgb: c_int,
}

#[repr(C)]
struct XSetWindowAttributes {
    background_pixmap: c_ulong,
    background_pixel: c_ulong,
    border_pixmap: c_ulong,
    border_pixel: c_ulong,
    bit_gravity: c_int,
    win_gravity: c_int,
    backing_store: c_int,
    backing_planes: c_ulong,
    backing_pixel: c_ulong,
    save_under: c_int,
    event_mask: c_long,
    do_not_propagate_mask: c_long,
    override_redirect: c_int,
    colormap: Colormap,
    cursor: c_ulong,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XClientMessageEvent {
    event_type: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut c_void,
    window: Window,
    message_type: c_ulong,
    format: c_int,
    data: [c_long; 5],
}

#[repr(C)]
union XEvent {
    client: XClientMessageEvent,
    pad: [c_long; 24],
}

unsafe extern "C" {
    fn XOpenDisplay(name: *const c_char) -> *mut c_void;
    fn XDefaultScreen(display: *mut c_void) -> c_int;
    fn XRootWindow(display: *mut c_void, screen: c_int) -> Window;
    fn XCreateColormap(
        display: *mut c_void,
        window: Window,
        visual: *mut c_void,
        allocation: c_int,
    ) -> Colormap;
    fn XCreateWindow(
        display: *mut c_void,
        parent: Window,
        x: c_int,
        y: c_int,
        width: c_uint,
        height: c_uint,
        border_width: c_uint,
        depth: c_int,
        class: c_uint,
        visual: *mut c_void,
        value_mask: c_ulong,
        attributes: *mut XSetWindowAttributes,
    ) -> Window;
    fn XStoreName(display: *mut c_void, window: Window, name: *const c_char) -> c_int;
    fn XMapWindow(display: *mut c_void, window: Window) -> c_int;
    fn XResizeWindow(display: *mut c_void, window: Window, width: c_uint, height: c_uint) -> c_int;
    fn XGetGeometry(
        display: *mut c_void,
        window: Window,
        root: *mut Window,
        x: *mut c_int,
        y: *mut c_int,
        width: *mut c_uint,
        height: *mut c_uint,
        border: *mut c_uint,
        depth: *mut c_uint,
    ) -> c_int;
    fn XInternAtom(display: *mut c_void, name: *const c_char, only_if_exists: c_int) -> c_ulong;
    fn XSendEvent(
        display: *mut c_void,
        window: Window,
        propagate: c_int,
        event_mask: c_long,
        event: *mut XEvent,
    ) -> c_int;
    fn XSync(display: *mut c_void, discard: c_int) -> c_int;
    fn XFlush(display: *mut c_void) -> c_int;
    fn XDestroyWindow(display: *mut c_void, window: Window) -> c_int;
    fn XFreeColormap(display: *mut c_void, colormap: Colormap) -> c_int;
    fn XFree(data: *mut c_void) -> c_int;
    fn XCloseDisplay(display: *mut c_void) -> c_int;

    fn glXChooseVisual(
        display: *mut c_void,
        screen: c_int,
        attributes: *mut c_int,
    ) -> *mut XVisualInfo;
    fn glXCreateContext(
        display: *mut c_void,
        visual: *mut XVisualInfo,
        share: GlxContext,
        direct: c_int,
    ) -> GlxContext;
    fn glXMakeCurrent(display: *mut c_void, drawable: Window, context: GlxContext) -> c_int;
    fn glXSwapBuffers(display: *mut c_void, drawable: Window);
    fn glXDestroyContext(display: *mut c_void, context: GlxContext);
    fn glClearColor(red: f32, green: f32, blue: f32, alpha: f32);
    fn glClear(mask: c_uint);
    fn glEnable(capability: c_uint);
    fn glDisable(capability: c_uint);
    fn glScissor(x: c_int, y: c_int, width: c_int, height: c_int);
    fn glViewport(x: c_int, y: c_int, width: c_int, height: c_int);
    fn glReadBuffer(mode: c_uint);
    fn glReadPixels(
        x: c_int,
        y: c_int,
        width: c_int,
        height: c_int,
        format: c_uint,
        pixel_type: c_uint,
        pixels: *mut c_void,
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "the native GLX lifecycle probe keeps handle creation, replacement, and reverse-order teardown in one auditable scope"
)]
fn main() -> Result<(), &'static str> {
    let width = bounded_env_u32("REDUNAR_CAPTURE_PROBE_WIDTH", DEFAULT_WIDTH, 1, 3840)?;
    let height = bounded_env_u32("REDUNAR_CAPTURE_PROBE_HEIGHT", DEFAULT_HEIGHT, 1, 2160)?;
    let frame_count = bounded_env_u32(
        "REDUNAR_CAPTURE_PROBE_FRAMES",
        DEFAULT_FRAME_COUNT,
        2,
        1_000_000,
    )?;
    let frame_delay_us =
        bounded_env_u32("REDUNAR_CAPTURE_PROBE_FRAME_DELAY_US", 8_000, 0, 100_000)?;
    let width_i32 = c_int::try_from(width).map_err(|_| "GLX probe width is too large")?;
    let height_i32 = c_int::try_from(height).map_err(|_| "GLX probe height is too large")?;
    let resize = resize_request(frame_count)?;
    let requested_replacements = context_replacement_count()?;
    let auxiliary_window = std::env::var("REDUNAR_CAPTURE_PROBE_AUXILIARY_WINDOW")
        .ok()
        .as_deref()
        == Some("1");
    let orientation_pattern = std::env::var("REDUNAR_CAPTURE_PROBE_ORIENTATION_PATTERN")
        .ok()
        .as_deref()
        == Some("1");
    let fullscreen_requested = std::env::var("REDUNAR_CAPTURE_PROBE_FULLSCREEN")
        .ok()
        .as_deref()
        == Some("1");
    // SAFETY: every native handle is checked before use and destroyed in the
    // reverse order of creation. The fixed attributes and title are bounded.
    unsafe {
        let display = XOpenDisplay(ptr::null());
        if display.is_null() {
            return Err("GLX probe could not open the X display");
        }
        let screen = XDefaultScreen(display);
        let root = XRootWindow(display, screen);
        let mut visual_attributes = [GLX_RGBA, GLX_DOUBLEBUFFER, 0];
        let visual = glXChooseVisual(display, screen, visual_attributes.as_mut_ptr());
        if visual.is_null() {
            XCloseDisplay(display);
            return Err("GLX probe found no double-buffered visual");
        }
        let colormap = XCreateColormap(display, root, (*visual).visual, ALLOC_NONE);
        let mut window_attributes: XSetWindowAttributes = std::mem::zeroed();
        window_attributes.colormap = colormap;
        let window = XCreateWindow(
            display,
            root,
            0,
            0,
            width,
            height,
            0,
            (*visual).depth,
            INPUT_OUTPUT,
            (*visual).visual,
            CW_COLORMAP,
            &raw mut window_attributes,
        );
        if window == 0 {
            XFreeColormap(display, colormap);
            XFree(visual.cast());
            XCloseDisplay(display);
            return Err("GLX probe could not create a window");
        }
        let mut context = glXCreateContext(display, visual, ptr::null_mut(), 1);
        if context.is_null() || glXMakeCurrent(display, window, context) == 0 {
            XDestroyWindow(display, window);
            XFreeColormap(display, colormap);
            XFree(visual.cast());
            XCloseDisplay(display);
            return Err("GLX probe could not create a direct context");
        }
        XStoreName(display, window, c"Redunar OpenGL telemetry probe".as_ptr());
        XMapWindow(display, window);
        XFlush(display);
        glViewport(0, 0, width_i32, height_i32);
        let fullscreen_atoms = if fullscreen_requested {
            Some((
                XInternAtom(display, c"_NET_WM_STATE".as_ptr(), 0),
                XInternAtom(display, c"_NET_WM_STATE_FULLSCREEN".as_ptr(), 0),
            ))
        } else {
            None
        };

        let auxiliary = if auxiliary_window {
            let window = XCreateWindow(
                display,
                root,
                0,
                0,
                320,
                180,
                0,
                (*visual).depth,
                INPUT_OUTPUT,
                (*visual).visual,
                CW_COLORMAP,
                &raw mut window_attributes,
            );
            let context = glXCreateContext(display, visual, ptr::null_mut(), 1);
            if window == 0 || context.is_null() {
                return Err("GLX probe could not create its auxiliary window");
            }
            XStoreName(display, window, c"Redunar auxiliary GLX probe".as_ptr());
            XMapWindow(display, window);
            XSync(display, 0);
            Some((window, context))
        } else {
            None
        };

        let mut current_width = width;
        let mut current_height = height;
        let replacement_interval = frame_count / (requested_replacements + 1);
        let mut completed_replacements = 0;
        let mut saw_fullscreen = false;
        let mut saw_restored = false;
        for frame in 0..frame_count {
            if let Some((state_atom, fullscreen_atom)) = fullscreen_atoms {
                if frame == frame_count / 4 {
                    request_fullscreen(display, root, window, state_atom, fullscreen_atom, true)?;
                } else if frame == frame_count / 2 {
                    request_fullscreen(display, root, window, state_atom, fullscreen_atom, false)?;
                }
                let (actual_width, actual_height) = window_geometry(display, window)?;
                if frame > frame_count / 4
                    && frame < frame_count / 2
                    && actual_width > width
                    && actual_height > height
                {
                    saw_fullscreen = true;
                }
                if frame > frame_count / 2
                    && saw_fullscreen
                    && actual_width == width
                    && actual_height == height
                {
                    saw_restored = true;
                }
                if actual_width != current_width || actual_height != current_height {
                    current_width = actual_width;
                    current_height = actual_height;
                    glViewport(
                        0,
                        0,
                        c_int::try_from(actual_width).map_err(|_| "fullscreen width too large")?,
                        c_int::try_from(actual_height)
                            .map_err(|_| "fullscreen height too large")?,
                    );
                }
            }
            if let Some((resize_frame, resize_width, resize_height)) = resize
                && frame == resize_frame
            {
                XResizeWindow(display, window, resize_width, resize_height);
                XSync(display, 0);
                glViewport(
                    0,
                    0,
                    c_int::try_from(resize_width)
                        .map_err(|_| "GLX probe resize width is too large")?,
                    c_int::try_from(resize_height)
                        .map_err(|_| "GLX probe resize height is too large")?,
                );
                current_width = resize_width;
                current_height = resize_height;
            }
            if completed_replacements < requested_replacements
                && replacement_interval > 0
                && frame == replacement_interval * (completed_replacements + 1)
            {
                let replacement = glXCreateContext(display, visual, ptr::null_mut(), 1);
                if replacement.is_null() || glXMakeCurrent(display, window, replacement) == 0 {
                    return Err("GLX probe could not replace its context");
                }
                glViewport(
                    0,
                    0,
                    c_int::try_from(current_width)
                        .map_err(|_| "GLX probe current width is too large")?,
                    c_int::try_from(current_height)
                        .map_err(|_| "GLX probe current height is too large")?,
                );
                glXDestroyContext(display, context);
                context = replacement;
                completed_replacements += 1;
            }
            let phase = f32::from(u8::try_from(frame % 256).unwrap_or(0)) / 255.0;
            glClearColor(phase, 0.08, 0.16, 1.0);
            glClear(GL_COLOR_BUFFER_BIT);
            if orientation_pattern {
                // OpenGL coordinates start at the bottom. Distinct top and
                // bottom bars make an upside-down saved clip detectable.
                glEnable(GL_SCISSOR_TEST);
                let quarter = c_int::try_from(current_height / 4)
                    .map_err(|_| "GLX orientation bar is too tall")?;
                glScissor(
                    0,
                    0,
                    c_int::try_from(current_width)
                        .map_err(|_| "GLX orientation bar is too wide")?,
                    quarter,
                );
                glClearColor(0.05, 0.1, 0.9, 1.0);
                glClear(GL_COLOR_BUFFER_BIT);
                glScissor(
                    0,
                    c_int::try_from(current_height)
                        .map_err(|_| "GLX orientation height is too large")?
                        - quarter,
                    c_int::try_from(current_width)
                        .map_err(|_| "GLX orientation width is too large")?,
                    quarter,
                );
                glClearColor(0.9, 0.1, 0.05, 1.0);
                glClear(GL_COLOR_BUFFER_BIT);
                glDisable(GL_SCISSOR_TEST);
            }
            glXSwapBuffers(display, window);
            if let Some((auxiliary_window, auxiliary_context)) = auxiliary {
                if glXMakeCurrent(display, auxiliary_window, auxiliary_context) == 0 {
                    return Err("GLX probe could not bind its auxiliary context");
                }
                glViewport(0, 0, 320, 180);
                glClearColor(0.2, phase, 0.1, 1.0);
                glClear(GL_COLOR_BUFFER_BIT);
                glXSwapBuffers(display, auxiliary_window);
                if glXMakeCurrent(display, window, context) == 0 {
                    return Err("GLX probe could not restore its main context");
                }
            }
            if frame == 30 {
                dump_front_buffer(width, height)?;
            }
            if frame_delay_us > 0 {
                thread::sleep(Duration::from_micros(u64::from(frame_delay_us)));
            }
        }

        glXMakeCurrent(display, 0, ptr::null_mut());
        glXDestroyContext(display, context);
        if let Some((auxiliary_window, auxiliary_context)) = auxiliary {
            glXDestroyContext(display, auxiliary_context);
            XDestroyWindow(display, auxiliary_window);
        }
        XDestroyWindow(display, window);
        XFreeColormap(display, colormap);
        XFree(visual.cast());
        XCloseDisplay(display);
        eprintln!("GLX probe context replacements={completed_replacements}");
        eprintln!("GLX probe auxiliary window={auxiliary_window}");
        if fullscreen_requested {
            eprintln!("GLX probe fullscreen entered={saw_fullscreen} restored={saw_restored}");
            if !saw_fullscreen || !saw_restored {
                return Err("GLX probe fullscreen transition did not complete");
            }
        }
    }
    Ok(())
}

unsafe fn request_fullscreen(
    display: *mut c_void,
    root: Window,
    window: Window,
    state_atom: c_ulong,
    fullscreen_atom: c_ulong,
    enable: bool,
) -> Result<(), &'static str> {
    let mut event = XEvent {
        client: XClientMessageEvent {
            event_type: CLIENT_MESSAGE,
            serial: 0,
            send_event: 1,
            display,
            window,
            message_type: state_atom,
            format: 32,
            data: [c_long::from(enable), fullscreen_atom.cast_signed(), 0, 1, 0],
        },
    };
    // SAFETY: the checked display/window handles and initialized XEvent use
    // the exact Xlib client-message ABI; Xlib only reads this event.
    if unsafe {
        XSendEvent(
            display,
            root,
            0,
            SUBSTRUCTURE_NOTIFY_MASK | SUBSTRUCTURE_REDIRECT_MASK,
            &raw mut event,
        )
    } == 0
    {
        return Err("GLX probe fullscreen request was rejected");
    }
    // SAFETY: display remains open for the entire probe loop.
    unsafe { XFlush(display) };
    Ok(())
}

unsafe fn window_geometry(
    display: *mut c_void,
    window: Window,
) -> Result<(u32, u32), &'static str> {
    let (mut root, mut x, mut y, mut width, mut height, mut border, mut depth) =
        (0, 0, 0, 0, 0, 0, 0);
    // SAFETY: all outputs are valid initialized storage and the window is live.
    if unsafe {
        XGetGeometry(
            display,
            window,
            &raw mut root,
            &raw mut x,
            &raw mut y,
            &raw mut width,
            &raw mut height,
            &raw mut border,
            &raw mut depth,
        )
    } == 0
    {
        return Err("GLX probe could not read window geometry");
    }
    Ok((width, height))
}

fn context_replacement_count() -> Result<u32, &'static str> {
    let Some(raw) = std::env::var_os("REDUNAR_CAPTURE_PROBE_CONTEXT_REPLACEMENTS") else {
        return Ok(0);
    };
    raw.to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value <= 16)
        .ok_or("GLX probe context replacement count is invalid")
}

fn resize_request(frame_count: u32) -> Result<Option<(u32, u32, u32)>, &'static str> {
    let Some(width) = std::env::var_os("REDUNAR_CAPTURE_PROBE_RESIZE_WIDTH") else {
        return Ok(None);
    };
    let Some(height) = std::env::var_os("REDUNAR_CAPTURE_PROBE_RESIZE_HEIGHT") else {
        return Err("GLX probe resize height is missing");
    };
    let Some(frame) = std::env::var_os("REDUNAR_CAPTURE_PROBE_RESIZE_FRAME") else {
        return Err("GLX probe resize frame is missing");
    };
    let width = width
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| (1..=3_840).contains(value))
        .ok_or("GLX probe resize width is invalid")?;
    let height = height
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| (1..=2_160).contains(value))
        .ok_or("GLX probe resize height is invalid")?;
    let frame = frame
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0 && *value < frame_count)
        .ok_or("GLX probe resize frame is invalid")?;
    Ok(Some((frame, width, height)))
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
        .ok_or("OpenGL probe received an invalid numeric setting")?;
    if !(minimum..=maximum).contains(&value) {
        return Err("OpenGL probe received an out-of-range setting");
    }
    Ok(value)
}

unsafe fn dump_front_buffer(width: u32, height: u32) -> Result<(), &'static str> {
    let Some(path) = std::env::var_os("REDUNAR_CAPTURE_PROBE_FRAME_DUMP").map(PathBuf::from) else {
        return Ok(());
    };
    if !path.is_absolute() {
        return Err("REDUNAR_CAPTURE_PROBE_FRAME_DUMP must be absolute");
    }
    let row_bytes = width as usize * 3;
    let mut pixels = vec![0_u8; row_bytes * height as usize];
    let width_i32 = c_int::try_from(width).map_err(|_| "GLX probe width is too large")?;
    let height_i32 = c_int::try_from(height).map_err(|_| "GLX probe height is too large")?;
    // SAFETY: the current GLX context owns a width-by-height front buffer and `pixels`
    // has exact storage for its tightly packed RGB bytes.
    unsafe {
        glReadBuffer(GL_FRONT);
        glReadPixels(
            0,
            0,
            width_i32,
            height_i32,
            GL_RGB,
            GL_UNSIGNED_BYTE,
            pixels.as_mut_ptr().cast(),
        );
    }
    let header = format!("P6\n{width} {height}\n255\n");
    let mut ppm = Vec::with_capacity(header.len() + pixels.len());
    ppm.extend_from_slice(header.as_bytes());
    for row in (0..height as usize).rev() {
        let start = row * row_bytes;
        ppm.extend_from_slice(&pixels[start..start + row_bytes]);
    }
    fs::write(path, ppm).map_err(|_| "GLX probe could not write the frame dump")
}

#[link(name = "GL")]
#[link(name = "X11")]
unsafe extern "C" {}
