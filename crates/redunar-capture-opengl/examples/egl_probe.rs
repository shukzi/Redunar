//! Bounded EGL/OpenGL ES presentation target for Redunar's overlay probe.

use std::ffi::{c_int, c_uint, c_void};
use std::ptr;
use std::thread;
use std::time::Duration;

const EGL_NONE: c_int = 0x3038;
const EGL_SURFACE_TYPE: c_int = 0x3033;
const EGL_PBUFFER_BIT: c_int = 0x0001;
const EGL_RENDERABLE_TYPE: c_int = 0x3040;
const EGL_OPENGL_ES2_BIT: c_int = 0x0004;
const EGL_RED_SIZE: c_int = 0x3024;
const EGL_GREEN_SIZE: c_int = 0x3023;
const EGL_BLUE_SIZE: c_int = 0x3022;
const EGL_ALPHA_SIZE: c_int = 0x3021;
const EGL_WIDTH: c_int = 0x3057;
const EGL_HEIGHT: c_int = 0x3056;
const EGL_CONTEXT_CLIENT_VERSION: c_int = 0x3098;
const GL_COLOR_BUFFER_BIT: c_uint = 0x0000_4000;
const FRAME_COUNT: u16 = 180;

type EglDisplay = *mut c_void;
type EglConfig = *mut c_void;
type EglSurface = *mut c_void;
type EglContext = *mut c_void;

unsafe extern "C" {
    fn eglGetDisplay(native_display: *mut c_void) -> EglDisplay;
    fn eglInitialize(display: EglDisplay, major: *mut c_int, minor: *mut c_int) -> c_uint;
    fn eglChooseConfig(
        display: EglDisplay,
        attributes: *const c_int,
        configs: *mut EglConfig,
        config_size: c_int,
        config_count: *mut c_int,
    ) -> c_uint;
    fn eglCreatePbufferSurface(
        display: EglDisplay,
        config: EglConfig,
        attributes: *const c_int,
    ) -> EglSurface;
    fn eglCreateContext(
        display: EglDisplay,
        config: EglConfig,
        shared: EglContext,
        attributes: *const c_int,
    ) -> EglContext;
    fn eglMakeCurrent(
        display: EglDisplay,
        draw: EglSurface,
        read: EglSurface,
        context: EglContext,
    ) -> c_uint;
    fn eglSwapBuffers(display: EglDisplay, surface: EglSurface) -> c_uint;
    fn eglDestroyContext(display: EglDisplay, context: EglContext) -> c_uint;
    fn eglDestroySurface(display: EglDisplay, surface: EglSurface) -> c_uint;
    fn eglTerminate(display: EglDisplay) -> c_uint;

    fn glViewport(x: c_int, y: c_int, width: c_int, height: c_int);
    fn glClearColor(red: f32, green: f32, blue: f32, alpha: f32);
    fn glClear(mask: c_uint);
}

fn main() -> Result<(), &'static str> {
    // SAFETY: every EGL handle is checked before use and destroyed in reverse
    // creation order. Attribute lists are fixed and NUL-equivalent terminated.
    unsafe {
        let display = eglGetDisplay(ptr::null_mut());
        if display.is_null() || eglInitialize(display, ptr::null_mut(), ptr::null_mut()) == 0 {
            return Err("EGL probe could not initialize the default display");
        }
        let config_attributes = [
            EGL_SURFACE_TYPE,
            EGL_PBUFFER_BIT,
            EGL_RENDERABLE_TYPE,
            EGL_OPENGL_ES2_BIT,
            EGL_RED_SIZE,
            8,
            EGL_GREEN_SIZE,
            8,
            EGL_BLUE_SIZE,
            8,
            EGL_ALPHA_SIZE,
            8,
            EGL_NONE,
        ];
        let mut config = ptr::null_mut();
        let mut config_count = 0;
        if eglChooseConfig(
            display,
            config_attributes.as_ptr(),
            &raw mut config,
            1,
            &raw mut config_count,
        ) == 0
            || config_count != 1
        {
            eglTerminate(display);
            return Err("EGL probe found no RGBA OpenGL ES 2 pbuffer config");
        }
        let surface_attributes = [EGL_WIDTH, 320, EGL_HEIGHT, 180, EGL_NONE];
        let surface = eglCreatePbufferSurface(display, config, surface_attributes.as_ptr());
        if surface.is_null() {
            eglTerminate(display);
            return Err("EGL probe could not create a pbuffer surface");
        }
        let context_attributes = [EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE];
        let context = eglCreateContext(
            display,
            config,
            ptr::null_mut(),
            context_attributes.as_ptr(),
        );
        if context.is_null() || eglMakeCurrent(display, surface, surface, context) == 0 {
            eglDestroySurface(display, surface);
            eglTerminate(display);
            return Err("EGL probe could not create an OpenGL ES 2 context");
        }

        glViewport(0, 0, 320, 180);
        for frame in 0..FRAME_COUNT {
            let phase = f32::from(frame) / f32::from(FRAME_COUNT);
            glClearColor(0.05, phase * 0.3, 0.16, 1.0);
            glClear(GL_COLOR_BUFFER_BIT);
            if eglSwapBuffers(display, surface) == 0 {
                eglMakeCurrent(display, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
                eglDestroyContext(display, context);
                eglDestroySurface(display, surface);
                eglTerminate(display);
                return Err("EGL probe buffer swap failed");
            }
            thread::sleep(Duration::from_millis(8));
        }

        eglMakeCurrent(display, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
        eglDestroyContext(display, context);
        eglDestroySurface(display, surface);
        eglTerminate(display);
    }
    Ok(())
}

#[link(name = "EGL")]
#[link(name = "GLESv2")]
unsafe extern "C" {}
