//! Diagnostic OpenGL Replay readback foundation.
//!
//! This path proves bounded source selection and asynchronous PBO completion.
//! It publishes metadata and a sampled checksum only; production Replay remains
//! disabled until the daemon has a validated GPU-shareable OpenGL transport.

use super::overlay::ApiFlavor;
use redunar_capture::{
    MAX_REPLAY_SOURCE_HEIGHT, MAX_REPLAY_SOURCE_WIDTH, ReplayPixelFormat, ReplaySourceCandidate,
    ReplaySourceRejection,
};
use std::env;
use std::ffi::{c_int, c_uint, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};

const REPLAY_TRANSFER_ENV: &str = "REDUNAR_REPLAY_TRANSFER";
const REPLAY_FRAME_RATE_ENV: &str = "REDUNAR_REPLAY_FRAME_RATE";
const DIAGNOSTIC_READBACK_ENV: &str = "REDUNAR_REPLAY_DIAGNOSTIC_OPENGL_READBACK";
const GL_VIEWPORT: c_uint = 0x0BA2;
const GL_VERSION: c_uint = 0x1F02;
const GL_EXTENSIONS: c_uint = 0x1F03;
const GL_NUM_EXTENSIONS: c_uint = 0x821D;
const GL_RGBA: c_uint = 0x1908;
const GL_UNSIGNED_BYTE: c_uint = 0x1401;
const GL_PIXEL_PACK_BUFFER: c_uint = 0x88EB;
const GL_PIXEL_PACK_BUFFER_BINDING: c_uint = 0x88ED;
const GL_PACK_ALIGNMENT: c_uint = 0x0D05;
const GL_PACK_ROW_LENGTH: c_uint = 0x0D02;
const GL_PACK_SKIP_ROWS: c_uint = 0x0D03;
const GL_PACK_SKIP_PIXELS: c_uint = 0x0D04;
const GL_STREAM_READ: c_uint = 0x88E1;
const GL_MAP_READ_BIT: c_uint = 0x0001;
const GL_SYNC_GPU_COMMANDS_COMPLETE: c_uint = 0x9117;
const GL_ALREADY_SIGNALED: c_uint = 0x911A;
const GL_CONDITION_SATISFIED: c_uint = 0x911C;
const GL_FRAMEBUFFER: c_uint = 0x8D40;
const GL_READ_FRAMEBUFFER: c_uint = 0x8CA8;
const GL_FRAMEBUFFER_BINDING: c_uint = 0x8CA6;
const GL_READ_FRAMEBUFFER_BINDING: c_uint = 0x8CAA;
const MIN_WIDTH: u32 = 320;
const MIN_HEIGHT: u32 = 180;
const PBO_COUNT: usize = 3;
const MAX_CONTEXTS: usize = 8;
const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;

type GetInteger = unsafe extern "C" fn(c_uint, *mut c_int);
type GetString = unsafe extern "C" fn(c_uint) -> *const u8;
type GetStringIndexed = unsafe extern "C" fn(c_uint, c_uint) -> *const u8;
type GenBuffers = unsafe extern "C" fn(c_int, *mut c_uint);
type BindBuffer = unsafe extern "C" fn(c_uint, c_uint);
type BufferData = unsafe extern "C" fn(c_uint, isize, *const c_void, c_uint);
type ReadPixels = unsafe extern "C" fn(c_int, c_int, c_int, c_int, c_uint, c_uint, *mut c_void);
type FenceSync = unsafe extern "C" fn(c_uint, c_uint) -> *mut c_void;
type ClientWaitSync = unsafe extern "C" fn(*mut c_void, c_uint, u64) -> c_uint;
type DeleteSync = unsafe extern "C" fn(*mut c_void);
type MapBufferRange = unsafe extern "C" fn(c_uint, isize, isize, c_uint) -> *mut c_void;
type UnmapBuffer = unsafe extern "C" fn(c_uint) -> u8;
type BindFramebuffer = unsafe extern "C" fn(c_uint, c_uint);
type PixelStore = unsafe extern "C" fn(c_uint, c_int);

#[derive(Clone, Copy)]
struct Functions {
    get_integer: GetInteger,
    get_string: GetString,
    get_string_indexed: Option<GetStringIndexed>,
    gen_buffers: GenBuffers,
    bind_buffer: BindBuffer,
    buffer_data: BufferData,
    read_pixels: ReadPixels,
    fence_sync: FenceSync,
    client_wait_sync: ClientWaitSync,
    delete_sync: DeleteSync,
    map_buffer_range: MapBufferRange,
    unmap_buffer: UnmapBuffer,
    bind_framebuffer: BindFramebuffer,
    pixel_store: PixelStore,
    external_memory_symbols: bool,
}

impl Functions {
    fn load() -> Option<Self> {
        let external_memory_symbols = [
            b"glCreateMemoryObjectsEXT\0".as_slice(),
            b"glDeleteMemoryObjectsEXT\0".as_slice(),
            b"glImportMemoryFdEXT\0".as_slice(),
            b"glBufferStorageMemEXT\0".as_slice(),
        ]
        .into_iter()
        .all(|symbol| super::resolve_gl_function::<unsafe extern "C" fn()>(symbol).is_some());
        Some(Self {
            get_integer: super::resolve_gl_function(b"glGetIntegerv\0")?,
            get_string: super::resolve_gl_function(b"glGetString\0")?,
            get_string_indexed: super::resolve_gl_function(b"glGetStringi\0"),
            gen_buffers: super::resolve_gl_function(b"glGenBuffers\0")?,
            bind_buffer: super::resolve_gl_function(b"glBindBuffer\0")?,
            buffer_data: super::resolve_gl_function(b"glBufferData\0")?,
            read_pixels: super::resolve_gl_function(b"glReadPixels\0")?,
            fence_sync: super::resolve_gl_function(b"glFenceSync\0")?,
            client_wait_sync: super::resolve_gl_function(b"glClientWaitSync\0")?,
            delete_sync: super::resolve_gl_function(b"glDeleteSync\0")?,
            map_buffer_range: super::resolve_gl_function(b"glMapBufferRange\0")?,
            unmap_buffer: super::resolve_gl_function(b"glUnmapBuffer\0")?,
            bind_framebuffer: super::resolve_gl_function(b"glBindFramebuffer\0")?,
            pixel_store: super::resolve_gl_function(b"glPixelStorei\0")?,
            external_memory_symbols,
        })
    }
}

#[derive(Clone, Copy, Default)]
struct Slot {
    buffer: c_uint,
    sync: usize,
    source: Option<ReplaySourceCandidate>,
    copied_bytes: u32,
    export_sequence: Option<u64>,
    export_released: bool,
}

impl Slot {
    fn is_available(&self) -> bool {
        self.sync == 0 && self.export_sequence.is_none()
    }

    fn release_export(&mut self, sequence: u64) {
        if self.export_sequence != Some(sequence) {
            return;
        }
        if self.sync == 0 {
            self.export_sequence = None;
            self.export_released = false;
        } else {
            self.export_released = true;
        }
    }
}

struct ContextResources {
    context: usize,
    slots: [Slot; PBO_COUNT],
    next_capture_ns: u64,
    announced_source: Option<ReplaySourceCandidate>,
}

static FUNCTIONS: OnceLock<Option<Functions>> = OnceLock::new();
static CONTEXTS: LazyLock<Mutex<Vec<ContextResources>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static EXTERNAL_MEMORY_REJECTION_REPORTED: AtomicBool = AtomicBool::new(false);

/// Release diagnostic bookkeeping immediately before context destruction.
/// Context teardown owns the PBO and sync deletion, including when another
/// context is current at the point the application destroys this one.
pub(crate) fn destroy_context(context: usize) {
    if context == 0 {
        return;
    }
    let mut contexts = CONTEXTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    contexts.retain(|entry| entry.context != context);
}

pub(crate) fn capture(context: usize, api: ApiFlavor) {
    if context == 0 || !replay_requested() {
        return;
    }
    let Some(functions) = *FUNCTIONS.get_or_init(Functions::load) else {
        return;
    };
    if !diagnostic_path_ready(&functions) {
        return;
    }
    let Some(now_ns) = super::monotonic_ns() else {
        return;
    };
    let Some(source) = current_source(&functions) else {
        return;
    };
    let Ok(mut contexts) = CONTEXTS.try_lock() else {
        return;
    };
    let index = if let Some(index) = contexts.iter().position(|entry| entry.context == context) {
        index
    } else {
        if contexts.len() >= MAX_CONTEXTS {
            return;
        }
        let mut buffers = [0_u32; PBO_COUNT];
        // SAFETY: the current context writes exactly `PBO_COUNT` buffer names.
        unsafe {
            (functions.gen_buffers)(i32::try_from(PBO_COUNT).unwrap_or(0), buffers.as_mut_ptr());
        }
        if buffers.contains(&0) {
            return;
        }
        contexts.push(ContextResources {
            context,
            slots: buffers.map(|buffer| Slot {
                buffer,
                ..Slot::default()
            }),
            next_capture_ns: 0,
            announced_source: None,
        });
        contexts.len() - 1
    };
    let resources = &mut contexts[index];
    let mut old_pack_buffer = 0;
    // SAFETY: the current context writes one integer binding value.
    unsafe { (functions.get_integer)(GL_PIXEL_PACK_BUFFER_BINDING, &raw mut old_pack_buffer) };
    complete_ready_slots(&functions, resources);
    if resources.announced_source != Some(source) {
        super::record_replay_source_candidate(source);
        resources.announced_source = Some(source);
    }
    if now_ns < resources.next_capture_ns {
        restore_pack_buffer(&functions, old_pack_buffer);
        return;
    }
    let Some(slot) = resources.slots.iter_mut().find(|slot| slot.is_available()) else {
        restore_pack_buffer(&functions, old_pack_buffer);
        return;
    };
    let Some(copied_bytes) = source
        .width
        .checked_mul(source.height)
        .and_then(|pixels| pixels.checked_mul(4))
    else {
        restore_pack_buffer(&functions, old_pack_buffer);
        return;
    };
    let (framebuffer_target, framebuffer_binding) = framebuffer_target(&functions, api);
    let pack_state = PackState::save_and_normalize(&functions, api);
    let mut old_framebuffer = 0;
    // SAFETY: every call operates on the current context. The original pack
    // buffer and read framebuffer are restored before returning to the game.
    unsafe {
        (functions.get_integer)(framebuffer_binding, &raw mut old_framebuffer);
        (functions.bind_framebuffer)(framebuffer_target, 0);
        (functions.bind_buffer)(GL_PIXEL_PACK_BUFFER, slot.buffer);
        (functions.buffer_data)(
            GL_PIXEL_PACK_BUFFER,
            isize::try_from(copied_bytes).unwrap_or(0),
            std::ptr::null(),
            GL_STREAM_READ,
        );
        (functions.read_pixels)(
            0,
            0,
            i32::try_from(source.width).unwrap_or(0),
            i32::try_from(source.height).unwrap_or(0),
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            std::ptr::null_mut(),
        );
        slot.sync = (functions.fence_sync)(GL_SYNC_GPU_COMMANDS_COMPLETE, 0) as usize;
        (functions.bind_framebuffer)(framebuffer_target, old_framebuffer.cast_unsigned());
    }
    pack_state.restore(&functions);
    slot.source = (slot.sync != 0).then_some(source);
    slot.copied_bytes = copied_bytes;
    resources.next_capture_ns = now_ns.saturating_add(frame_interval_ns(source));
    restore_pack_buffer(&functions, old_pack_buffer);
}

pub(crate) fn release_sequence(sequence: u64) -> bool {
    let Ok(mut contexts) = CONTEXTS.try_lock() else {
        return false;
    };
    for slot in contexts
        .iter_mut()
        .flat_map(|resources| resources.slots.iter_mut())
    {
        slot.release_export(sequence);
    }
    true
}

#[derive(Clone, Copy)]
struct PackState {
    alignment: i32,
    extended: Option<[i32; 3]>,
}

impl PackState {
    fn save_and_normalize(functions: &Functions, api: ApiFlavor) -> Self {
        let mut alignment = 4;
        // SAFETY: every queried value writes one integer owned by the current
        // context. The matching values are restored before presentation.
        unsafe {
            (functions.get_integer)(GL_PACK_ALIGNMENT, &raw mut alignment);
            (functions.pixel_store)(GL_PACK_ALIGNMENT, 4);
        }
        let extended = supports_extended_pack(functions, api).then(|| {
            let mut values = [0_i32; 3];
            // SAFETY: these pack parameters exist in desktop GL and GLES 3+.
            unsafe {
                for (value, name) in values.iter_mut().zip([
                    GL_PACK_ROW_LENGTH,
                    GL_PACK_SKIP_ROWS,
                    GL_PACK_SKIP_PIXELS,
                ]) {
                    (functions.get_integer)(name, value);
                    (functions.pixel_store)(name, 0);
                }
            }
            values
        });
        Self {
            alignment,
            extended,
        }
    }

    fn restore(self, functions: &Functions) {
        // SAFETY: these values were read from the same current context.
        unsafe {
            (functions.pixel_store)(GL_PACK_ALIGNMENT, self.alignment);
            if let Some(values) = self.extended {
                for (value, name) in values.into_iter().zip([
                    GL_PACK_ROW_LENGTH,
                    GL_PACK_SKIP_ROWS,
                    GL_PACK_SKIP_PIXELS,
                ]) {
                    (functions.pixel_store)(name, value);
                }
            }
        }
    }
}

fn complete_ready_slots(functions: &Functions, resources: &mut ContextResources) {
    for slot in &mut resources.slots {
        if slot.sync == 0 {
            continue;
        }
        let sync = slot.sync as *mut c_void;
        // SAFETY: the sync belongs to this current context and timeout zero is
        // a nonblocking readiness poll.
        let status = unsafe { (functions.client_wait_sync)(sync, 0, 0) };
        if !matches!(status, GL_ALREADY_SIGNALED | GL_CONDITION_SATISFIED) {
            continue;
        }
        // SAFETY: the signaled PBO contains exactly `copied_bytes` readable
        // bytes. Mapping and unmapping happen while its context is current.
        let checksum = unsafe {
            (functions.bind_buffer)(GL_PIXEL_PACK_BUFFER, slot.buffer);
            let pointer = (functions.map_buffer_range)(
                GL_PIXEL_PACK_BUFFER,
                0,
                isize::try_from(slot.copied_bytes).unwrap_or(0),
                GL_MAP_READ_BIT,
            );
            let checksum = (!pointer.is_null()).then(|| {
                sampled_checksum(std::slice::from_raw_parts(
                    pointer.cast::<u8>(),
                    usize::try_from(slot.copied_bytes).unwrap_or(0),
                ))
            });
            if !pointer.is_null() {
                let _ = (functions.unmap_buffer)(GL_PIXEL_PACK_BUFFER);
            }
            (functions.delete_sync)(sync);
            checksum
        };
        if let (Some(source), Some(checksum)) = (slot.source, checksum) {
            super::record_replay_frame_copied(source, slot.copied_bytes, checksum);
        }
        slot.sync = 0;
        slot.source = None;
        slot.copied_bytes = 0;
        if slot.export_released {
            slot.export_sequence = None;
            slot.export_released = false;
        }
    }
}

fn restore_pack_buffer(functions: &Functions, binding: i32) {
    // SAFETY: the saved binding came from this current context.
    unsafe {
        (functions.bind_buffer)(
            GL_PIXEL_PACK_BUFFER,
            c_uint::from_ne_bytes(binding.to_ne_bytes()),
        );
    }
}

fn source_candidate(width: u32, height: u32) -> Option<ReplaySourceCandidate> {
    (width >= MIN_WIDTH
        && height >= MIN_HEIGHT
        && width <= MAX_REPLAY_SOURCE_WIDTH
        && height <= MAX_REPLAY_SOURCE_HEIGHT)
        .then_some(ReplaySourceCandidate {
            width,
            height,
            pixel_format: ReplayPixelFormat::Rgba8Unorm,
            target_frames_per_second: target_frames_per_second(),
        })
}

fn current_source(functions: &Functions) -> Option<ReplaySourceCandidate> {
    let mut viewport = [0_i32; 4];
    // SAFETY: the current presentation context owns writable viewport storage.
    unsafe { (functions.get_integer)(GL_VIEWPORT, viewport.as_mut_ptr()) };
    source_candidate(
        u32::try_from(viewport[2]).ok()?,
        u32::try_from(viewport[3]).ok()?,
    )
}

fn diagnostic_requested() -> bool {
    env::var(DIAGNOSTIC_READBACK_ENV).ok().as_deref() == Some("1")
}

fn replay_requested() -> bool {
    env::var(REPLAY_TRANSFER_ENV).ok().as_deref() == Some("1")
}

fn diagnostic_path_ready(functions: &Functions) -> bool {
    if diagnostic_requested() {
        return true;
    }
    if !external_memory_supported(functions)
        && !EXTERNAL_MEMORY_REJECTION_REPORTED.swap(true, Ordering::Relaxed)
    {
        super::record_replay_source_rejected(ReplaySourceRejection::ExternalMemoryUnsupported);
    }
    false
}

fn external_memory_supported(functions: &Functions) -> bool {
    functions.external_memory_symbols
        && has_extension(functions, b"GL_EXT_memory_object")
        && has_extension(functions, b"GL_EXT_memory_object_fd")
}

fn has_extension(functions: &Functions, expected: &[u8]) -> bool {
    if let Some(get_string_indexed) = functions.get_string_indexed {
        let mut count = 0_i32;
        // SAFETY: the current context writes one extension count integer.
        unsafe { (functions.get_integer)(GL_NUM_EXTENSIONS, &raw mut count) };
        if (1..=4_096).contains(&count) {
            return (0..count.cast_unsigned()).any(|index| {
                // SAFETY: every index is below the context-reported count.
                let pointer = unsafe { get_string_indexed(GL_EXTENSIONS, index) };
                !pointer.is_null()
                    // SAFETY: GL returns a NUL-terminated context-owned token.
                    && unsafe { std::ffi::CStr::from_ptr(pointer.cast()) }.to_bytes() == expected
            });
        }
    }
    // SAFETY: legacy GL returns a NUL-terminated, space-delimited extension list.
    let pointer = unsafe { (functions.get_string)(GL_EXTENSIONS) };
    !pointer.is_null()
        && extension_list_contains(
            // SAFETY: the pointer remains valid while this context is current.
            unsafe { std::ffi::CStr::from_ptr(pointer.cast()) }.to_bytes(),
            expected,
        )
}

fn extension_list_contains(extensions: &[u8], expected: &[u8]) -> bool {
    extensions
        .split(u8::is_ascii_whitespace)
        .any(|extension| extension == expected)
}

fn target_frames_per_second() -> u8 {
    match env::var(REPLAY_FRAME_RATE_ENV).ok().as_deref() {
        Some("30") => 30,
        Some("120") => 120,
        _ => 60,
    }
}

fn frame_interval_ns(source: ReplaySourceCandidate) -> u64 {
    NANOSECONDS_PER_SECOND / u64::from(source.target_frames_per_second)
}

unsafe fn version_bytes(functions: &Functions) -> &'static [u8] {
    // SAFETY: GL_VERSION returns context-owned static storage while current.
    let pointer = unsafe { (functions.get_string)(GL_VERSION) };
    if pointer.is_null() {
        b""
    } else {
        // SAFETY: OpenGL returns a NUL-terminated version string.
        unsafe { std::ffi::CStr::from_ptr(pointer.cast()) }.to_bytes()
    }
}

fn framebuffer_target(functions: &Functions, api: ApiFlavor) -> (c_uint, c_uint) {
    // SAFETY: this only reads the current context's static version string.
    let version = unsafe { version_bytes(functions) };
    let legacy = api == ApiFlavor::Embedded
        || version
            .first()
            .is_none_or(|major| !major.is_ascii_digit() || *major < b'3');
    if legacy {
        (GL_FRAMEBUFFER, GL_FRAMEBUFFER_BINDING)
    } else {
        (GL_READ_FRAMEBUFFER, GL_READ_FRAMEBUFFER_BINDING)
    }
}

fn supports_extended_pack(functions: &Functions, api: ApiFlavor) -> bool {
    if api == ApiFlavor::Desktop {
        return true;
    }
    // SAFETY: this only reads the current context's static version string.
    let version = unsafe { version_bytes(functions) };
    version.starts_with(b"OpenGL ES 3.") || version.starts_with(b"OpenGL ES 4.")
}

fn sampled_checksum(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let step = (bytes.len() / 4_096).max(1);
    for byte in bytes.iter().step_by(step).take(4_096) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash ^ u64::try_from(bytes.len()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_candidate_is_bounded_and_uses_requested_rate() {
        let source = source_candidate(640, 480).expect("bounded source");
        assert_eq!(source.width, 640);
        assert_eq!(source.height, 480);
        assert_eq!(source.pixel_format, ReplayPixelFormat::Rgba8Unorm);
        assert!(source_candidate(319, 480).is_none());
        assert!(source_candidate(640, MAX_REPLAY_SOURCE_HEIGHT + 1).is_none());
    }

    #[test]
    fn sampled_checksum_changes_with_sampled_pixels() {
        let empty = sampled_checksum(&[0; 8_192]);
        let mut changed = [0_u8; 8_192];
        changed[4_096] = 1;
        assert_ne!(empty, sampled_checksum(&changed));
    }

    #[test]
    fn extension_matching_requires_a_complete_token() {
        let extensions = b"GL_EXT_memory_object GL_EXT_memory_object_fd GL_EXT_other";
        assert!(extension_list_contains(extensions, b"GL_EXT_memory_object"));
        assert!(extension_list_contains(
            extensions,
            b"GL_EXT_memory_object_fd"
        ));
        assert!(!extension_list_contains(extensions, b"GL_EXT_memory"));
        assert!(!extension_list_contains(
            extensions,
            b"GL_EXT_memory_object_f"
        ));
    }

    #[test]
    fn released_export_becomes_available_after_gpu_completion() {
        let mut completed = Slot {
            export_sequence: Some(17),
            ..Slot::default()
        };
        assert!(!completed.is_available());
        completed.release_export(17);
        assert!(completed.is_available());

        let mut pending = Slot {
            sync: 1,
            export_sequence: Some(18),
            ..Slot::default()
        };
        pending.release_export(18);
        assert!(pending.export_released);
        assert!(!pending.is_available());
        pending.sync = 0;
        if pending.export_released {
            pending.export_sequence = None;
            pending.export_released = false;
        }
        assert!(pending.is_available());
    }
}
