//! Preserve handle-specific GLX targets that our global hooks cannot forward.

use super::{
    GlxDestroyContext, GlxSwapBuffers, NEXT_GLX_DESTROY_CONTEXT, NEXT_GLX_SWAP_BUFFERS,
    clear_loader_error, glx_destroy_context, glx_swap_buffers, resolve_next,
};
use std::ffi::c_void;

pub(super) fn swap_target(resolved: *mut c_void) -> *mut c_void {
    let next = NEXT_GLX_SWAP_BUFFERS
        .get()
        .copied()
        .unwrap_or_else(|| resolve_next::<GlxSwapBuffers>(b"glXSwapBuffers\0"));
    lookup_result(
        resolved,
        next.map(|function| function as *const () as *mut c_void),
        glx_swap_buffers as *const () as *mut c_void,
    )
}

pub(super) fn destroy_target(resolved: *mut c_void) -> *mut c_void {
    let next = NEXT_GLX_DESTROY_CONTEXT
        .get()
        .copied()
        .unwrap_or_else(|| resolve_next::<GlxDestroyContext>(b"glXDestroyContext\0"));
    lookup_result(
        resolved,
        next.map(|function| function as *const () as *mut c_void),
        glx_destroy_context as *const () as *mut c_void,
    )
}

fn lookup_result(
    resolved: *mut c_void,
    next: Option<*mut c_void>,
    hook: *mut c_void,
) -> *mut c_void {
    // A handle-specific lookup can see an RTLD_LOCAL GLX provider that our
    // RTLD_NEXT forwarding cannot see, or a different provider. Substituting
    // the hook would drop the application's call or send it to the wrong
    // library. Preserve that exact target; capture remains optional. Do not
    // cache failed probes, since a provider can become globally visible later.
    clear_loader_error();
    if std::ptr::eq(resolved, hook) || next.is_some_and(|target| std::ptr::eq(target, resolved)) {
        hook
    } else {
        resolved
    }
}
