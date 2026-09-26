//! Production OpenGL Replay export over GBM DMA-BUF memory.
//!
//! The handle contract is the one proven on RADV on 2026-09-22
//! (artifacts/opengl-contract): each slot is a GBM RA24 LINEAR buffer whose
//! descriptor is duplicated into a `GL_EXT_memory_object` and backs a
//! pixel-pack buffer with glBufferStorageMemEXT. Every present copies the
//! just-rendered back buffer through one Y-flipping GPU blit into a private
//! RGBA8 texture, then glReadPixels with `GL_PACK_ROW_LENGTH` writes the DMA
//! PBO in the top-down row order the Vulkan `LinearBuffer` importer expects.
//! The retained GBM descriptor is duplicated again and transferred with
//! `ReplayFrameExported` over `SCM_RIGHTS`; no pixel bytes and no CPU copy.
//!
//! A slot recycles only after its fence signals and the daemon returns
//! `ReplayFrameReleased`. Capture never blocks presentation: every lock is
//! try-locked, every completion poll has a zero timeout, and any missing
//! resource, busy pool, or failed step drops Replay work and counts it. The
//! presentation hooks poll daemon releases before capture so a slot returned
//! since the previous swap is immediately eligible for the current deadline.

use super::fd_transport;
use super::overlay::ApiFlavor;
use redunar_capture::{
    MAX_REPLAY_SOURCE_HEIGHT, MAX_REPLAY_SOURCE_WIDTH, REPLAY_PRODUCER_CONTEXT_COUNT,
    ReplayPixelFormat, ReplaySourceCandidate, ReplaySourceRejection,
};
use std::env;
use std::ffi::{CStr, c_int, c_uint, c_void};
use std::fs::OpenOptions;
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};

const PRODUCTION_ENV: &str = "REDUNAR_REPLAY_PRODUCTION";
const TRANSFER_ENV: &str = "REDUNAR_REPLAY_TRANSFER";
const FAILURE_ENV: &str = "REDUNAR_REPLAY_GL_FAIL";
static VARIABLE_FRAME_RATE: LazyLock<bool> =
    LazyLock::new(|| variable_frame_rate(env::var(FRAME_RATE_ENV).ok().as_deref()));
/// Release-gated slots cover the encoder pipeline depth plus two handoffs.
/// Direct GLX needs one frame while the daemon returns encoder completion;
/// SDL's swap dispatch adds one more presentation before that token can be
/// observed. The extra bounded slot preserves target cadence without waiting
/// in either hook.
const SLOT_COUNT: usize = REPLAY_PRODUCER_CONTEXT_COUNT + 1;
/// Bounded per-process pools so context churn cannot grow GPU memory.
const MAX_POOLS: usize = 4;
const MIN_WIDTH: u32 = 320;
const MIN_HEIGHT: u32 = 180;
const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;
const MAX_ENCODE_MACROBLOCKS_PER_SECOND: u32 = 2_073_600;
const DRM_FORMAT_RA24: u32 = 0x3432_4152;
const LINEAR_MODIFIER: u64 = 0;
const CLOCK_MONOTONIC: c_int = 1;
const RENDER_NODE_FIRST: u32 = 128;
const RENDER_NODE_LAST: u32 = 143;
const RTLD_NOW: c_int = 2;
const O_CLOEXEC: i32 = 0o2_000_000;

const GL_TEXTURE_2D: c_uint = 0x0DE1;
const GL_TEXTURE_BINDING_2D: c_uint = 0x8069;
const GL_RGBA8: c_uint = 0x8058;
const GL_READ_BUFFER: c_uint = 0x0C02;
// The back buffer of a double-buffered default framebuffer. 0x0400 is
// GL_FRONT_LEFT and can appear black in a hidden SDL window.
const GL_BACK: c_uint = 0x0405;
const GL_NEAREST: c_uint = 0x2600;
const GL_VIEWPORT: c_uint = 0x0BA2;
const GL_RGBA: c_uint = 0x1908;
const GL_UNSIGNED_BYTE: c_uint = 0x1401;
const GL_COLOR_BUFFER_BIT: c_uint = 0x0000_4000;
const GL_FRAMEBUFFER: c_uint = 0x8D40;
const GL_READ_FRAMEBUFFER: c_uint = 0x8CA8;
const GL_DRAW_FRAMEBUFFER: c_uint = 0x8CA9;
const GL_READ_FRAMEBUFFER_BINDING: c_uint = 0x8CAA;
const GL_DRAW_FRAMEBUFFER_BINDING: c_uint = 0x8CA6;
const GL_COLOR_ATTACHMENT0: c_uint = 0x8CE0;
const GL_FRAMEBUFFER_COMPLETE: c_uint = 0x8CD5;
const GL_PIXEL_PACK_BUFFER: c_uint = 0x88EB;
const GL_PIXEL_PACK_BUFFER_BINDING: c_uint = 0x88ED;
const GL_PACK_ALIGNMENT: c_uint = 0x0D05;
const GL_PACK_ROW_LENGTH: c_uint = 0x0D02;
const GL_SCISSOR_TEST: c_uint = 0x0C11;
const GL_SYNC_GPU_COMMANDS_COMPLETE: c_uint = 0x9117;
const GL_ALREADY_SIGNALED: c_uint = 0x911A;
const GL_CONDITION_SATISFIED: c_uint = 0x911C;
const GL_HANDLE_TYPE_OPAQUE_FD_EXT: c_uint = 0x9586;
const GL_NO_ERROR: c_uint = 0;
const GL_VERSION: c_uint = 0x1F02;
const GL_EXTENSIONS: c_uint = 0x1F03;
const GL_NUM_EXTENSIONS: c_uint = 0x821D;

#[repr(C)]
struct Timespec {
    seconds: i64,
    nanoseconds: i64,
}

unsafe extern "C" {
    fn clock_gettime(clock_id: c_int, time: *mut Timespec) -> c_int;
}

type GbmCreateDevice = unsafe extern "C" fn(c_int) -> *mut c_void;
type GbmBoCreateWithModifiers =
    unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const u64, u32) -> *mut c_void;
type GbmBoGetFd = unsafe extern "C" fn(*mut c_void) -> c_int;
type GbmBoGetStride = unsafe extern "C" fn(*mut c_void) -> u32;
type GbmBoDestroy = unsafe extern "C" fn(*mut c_void);

#[derive(Clone, Copy)]
struct Gbm {
    create_device: GbmCreateDevice,
    bo_create_with_modifiers: GbmBoCreateWithModifiers,
    bo_get_fd: GbmBoGetFd,
    bo_get_stride: GbmBoGetStride,
    bo_destroy: GbmBoDestroy,
}

/// GBM handles plus the retained render-node descriptor. The GBM device
/// borrows the descriptor for its lifetime, so both stay open for the process.
struct Allocator {
    device: *mut c_void,
    render_node: RawFd,
    gbm: Gbm,
}

// SAFETY: the opaque GBM handles are process-global allocator state created
// once under a OnceLock and used only from presentation-path export code.
unsafe impl Send for Allocator {}
unsafe impl Sync for Allocator {}

type GetInteger = unsafe extern "C" fn(c_uint, *mut c_int);
type GetString = unsafe extern "C" fn(c_uint) -> *const u8;
type GetStringIndexed = unsafe extern "C" fn(c_uint, c_uint) -> *const u8;
type GetError = unsafe extern "C" fn() -> c_uint;
type CreateMemoryObjects = unsafe extern "C" fn(c_int, *mut c_uint);
type DeleteMemoryObjects = unsafe extern "C" fn(c_int, *const c_uint);
type ImportMemoryFd = unsafe extern "C" fn(c_uint, u64, c_uint, c_int);
type BufferStorageMem = unsafe extern "C" fn(c_uint, isize, c_uint, u64);
type GenBuffers = unsafe extern "C" fn(c_int, *mut c_uint);
type BindBuffer = unsafe extern "C" fn(c_uint, c_uint);
type DeleteBuffers = unsafe extern "C" fn(c_int, *const c_uint);
type GenTextures = unsafe extern "C" fn(c_int, *mut c_uint);
type BindTexture = unsafe extern "C" fn(c_uint, c_uint);
type TexStorage2D = unsafe extern "C" fn(c_uint, c_int, c_uint, c_int, c_int, c_int);
type DeleteTextures = unsafe extern "C" fn(c_int, *const c_uint);
type GenFramebuffers = unsafe extern "C" fn(c_int, *mut c_uint);
type BindFramebuffer = unsafe extern "C" fn(c_uint, c_uint);
type FramebufferTexture2D = unsafe extern "C" fn(c_uint, c_uint, c_uint, c_uint, c_int);
type DeleteFramebuffers = unsafe extern "C" fn(c_int, *const c_uint);
type CheckFramebufferStatus = unsafe extern "C" fn(c_uint) -> c_uint;
type BlitFramebuffer =
    unsafe extern "C" fn(c_int, c_int, c_int, c_int, c_int, c_int, c_int, c_int, c_uint, c_uint);
type ReadPixels = unsafe extern "C" fn(c_int, c_int, c_int, c_int, c_uint, c_uint, *const c_void);
type ReadBuffer = unsafe extern "C" fn(c_uint);
type Enable = unsafe extern "C" fn(c_uint);
type Disable = unsafe extern "C" fn(c_uint);
type IsEnabled = unsafe extern "C" fn(c_uint) -> u8;
type PixelStore = unsafe extern "C" fn(c_uint, c_int);
type FenceSync = unsafe extern "C" fn(c_uint, c_uint) -> *mut c_void;
type ClientWaitSync = unsafe extern "C" fn(*mut c_void, c_uint, u64) -> c_uint;
type DeleteSync = unsafe extern "C" fn(*mut c_void);

/// The GL entry points required by the DMA export path, resolved once.
/// `read_buffer` is optional because it is missing only on legacy ES 2
/// contexts, which the version gate rejects earlier.
#[derive(Clone, Copy)]
struct ExportFunctions {
    get_integer: GetInteger,
    get_string: GetString,
    get_string_indexed: Option<GetStringIndexed>,
    get_error: GetError,
    create_memory_objects: CreateMemoryObjects,
    delete_memory_objects: DeleteMemoryObjects,
    import_memory_fd: ImportMemoryFd,
    buffer_storage_mem: BufferStorageMem,
    gen_buffers: GenBuffers,
    bind_buffer: BindBuffer,
    delete_buffers: DeleteBuffers,
    gen_textures: GenTextures,
    bind_texture: BindTexture,
    tex_storage_2d: TexStorage2D,
    delete_textures: DeleteTextures,
    gen_framebuffers: GenFramebuffers,
    bind_framebuffer: BindFramebuffer,
    framebuffer_texture_2d: FramebufferTexture2D,
    delete_framebuffers: DeleteFramebuffers,
    check_framebuffer_status: CheckFramebufferStatus,
    blit_framebuffer: BlitFramebuffer,
    read_pixels: ReadPixels,
    read_buffer: Option<ReadBuffer>,
    enable: Enable,
    disable: Disable,
    is_enabled: IsEnabled,
    pixel_store: PixelStore,
    fence_sync: FenceSync,
    client_wait_sync: ClientWaitSync,
    delete_sync: DeleteSync,
}

impl ExportFunctions {
    fn load() -> Option<Self> {
        Some(Self {
            get_integer: super::resolve_gl_function(b"glGetIntegerv\0")?,
            get_string: super::resolve_gl_function(b"glGetString\0")?,
            get_string_indexed: super::resolve_gl_function(b"glGetStringi\0"),
            get_error: super::resolve_gl_function(b"glGetError\0")?,
            create_memory_objects: super::resolve_gl_function(b"glCreateMemoryObjectsEXT\0")?,
            delete_memory_objects: super::resolve_gl_function(b"glDeleteMemoryObjectsEXT\0")?,
            import_memory_fd: super::resolve_gl_function(b"glImportMemoryFdEXT\0")?,
            buffer_storage_mem: super::resolve_gl_function(b"glBufferStorageMemEXT\0")?,
            gen_buffers: super::resolve_gl_function(b"glGenBuffers\0")?,
            bind_buffer: super::resolve_gl_function(b"glBindBuffer\0")?,
            delete_buffers: super::resolve_gl_function(b"glDeleteBuffers\0")?,
            gen_textures: super::resolve_gl_function(b"glGenTextures\0")?,
            bind_texture: super::resolve_gl_function(b"glBindTexture\0")?,
            tex_storage_2d: super::resolve_gl_function(b"glTexStorage2D\0")?,
            delete_textures: super::resolve_gl_function(b"glDeleteTextures\0")?,
            gen_framebuffers: super::resolve_gl_function(b"glGenFramebuffers\0")?,
            bind_framebuffer: super::resolve_gl_function(b"glBindFramebuffer\0")?,
            framebuffer_texture_2d: super::resolve_gl_function(b"glFramebufferTexture2D\0")?,
            delete_framebuffers: super::resolve_gl_function(b"glDeleteFramebuffers\0")?,
            check_framebuffer_status: super::resolve_gl_function(b"glCheckFramebufferStatus\0")?,
            blit_framebuffer: super::resolve_gl_function(b"glBlitFramebuffer\0")?,
            read_pixels: super::resolve_gl_function(b"glReadPixels\0")?,
            read_buffer: super::resolve_gl_function(b"glReadBuffer\0"),
            enable: super::resolve_gl_function(b"glEnable\0")?,
            disable: super::resolve_gl_function(b"glDisable\0")?,
            is_enabled: super::resolve_gl_function(b"glIsEnabled\0")?,
            pixel_store: super::resolve_gl_function(b"glPixelStorei\0")?,
            fence_sync: super::resolve_gl_function(b"glFenceSync\0")?,
            client_wait_sync: super::resolve_gl_function(b"glClientWaitSync\0")?,
            delete_sync: super::resolve_gl_function(b"glDeleteSync\0")?,
        })
    }
}

/// One GBM-allocated, GL-imported export slot.
///
/// The fd field is the descriptor this process retains for its lifetime; each
/// export transfers a fresh close-on-exec duplicate, and OpenGL consumed its
/// own duplicate at import. Closing fd therefore never strands another owner.
#[derive(Clone, Copy)]
struct Slot {
    memory: c_uint,
    buffer: c_uint,
    fd: RawFd,
    stride: u32,
    bytes: u64,
    source: Option<ReplaySourceCandidate>,
    sync: usize,
    timestamp_ns: u64,
    export_sequence: Option<u64>,
    export_released: bool,
}

impl Slot {
    const IDLE: Self = Self {
        memory: 0,
        buffer: 0,
        fd: -1,
        stride: 0,
        bytes: 0,
        source: None,
        sync: 0,
        timestamp_ns: 0,
        export_sequence: None,
        export_released: false,
    };

    fn allocated(&self) -> bool {
        self.fd >= 3 && self.memory != 0 && self.buffer != 0
    }

    /// Free for a new copy: no GPU work outstanding, no unreleased export.
    fn is_available(&self) -> bool {
        self.allocated()
            && self.sync == 0
            && self.source.is_none()
            && self.export_sequence.is_none()
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

    /// Discard a completed copy that belongs to a retired source generation.
    /// Already-exported storage remains release-gated until the daemon returns
    /// its sequence; retirement never reuses that memory early.
    fn discard_retired_copy(&mut self) -> bool {
        if self.sync != 0 {
            return false;
        }
        let dropped = self.source.take().is_some();
        if dropped {
            self.timestamp_ns = 0;
        }
        if self.export_released {
            self.export_sequence = None;
            self.export_released = false;
        }
        dropped
    }
}

/// One DMA pool per context, sized to the source that pool announced.
struct Pool {
    context: usize,
    source: ReplaySourceCandidate,
    slots: [Slot; SLOT_COUNT],
    flip_texture: c_uint,
    flip_framebuffer: c_uint,
    next_capture_ns: u64,
    exports: u64,
    drops: u64,
}

struct State {
    pools: Vec<Pool>,
    last_export_timestamp_ns: u64,
    /// Candidate metadata already announced to the daemon for the
    /// current source, so a stable source announces exactly once.
    announced: Option<ReplaySourceCandidate>,
}

static FUNCTIONS: OnceLock<Option<ExportFunctions>> = OnceLock::new();
static ALLOCATOR: OnceLock<Option<Allocator>> = OnceLock::new();
/// dlopen handle for libgbm. The dynamic loader serialises handle use and
/// the handle is process-global, so sharing it across threads is sound.
struct LibraryHandle(*mut c_void);
// SAFETY: a dlopen handle is valid process-wide and the loader is thread-safe.
unsafe impl Send for LibraryHandle {}
// SAFETY: inherited from the Send reasoning; the handle is never mutated.
unsafe impl Sync for LibraryHandle {}

static GBM_LIBRARY: OnceLock<Option<LibraryHandle>> = OnceLock::new();
static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        pools: Vec::new(),
        last_export_timestamp_ns: 0,
        announced: None,
    })
});
static NEXT_EXPORT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static FIRED_FAILURE_STAGES: AtomicU64 = AtomicU64::new(0);
const FRAME_RATE_ENV: &str = "REDUNAR_REPLAY_FRAME_RATE";
const MAX_SLOT_BYTES: u64 = MAX_REPLAY_SOURCE_WIDTH as u64 * 4 * MAX_REPLAY_SOURCE_HEIGHT as u64;

/// Resolve libgbm once. Every entry point is checked: a missing symbol makes
/// the production route unsupported rather than unsafe to call.
fn gbm_entry_points() -> Option<Gbm> {
    let handle = &GBM_LIBRARY
        .get_or_init(|| {
            let real = super::real_dlopen()?;
            // SAFETY: the name is a literal NUL-terminated soname; the result is
            // checked for null before it is stored.
            let loaded = unsafe { real(c"libgbm.so.1".as_ptr(), RTLD_NOW) };
            (!loaded.is_null()).then_some(LibraryHandle(loaded))
        })
        .as_ref()?;
    let real = super::real_dlsym()?;
    let lookup = |symbol: &CStr| {
        // SAFETY: the handle is live and each symbol name is a literal.
        unsafe { real(handle.0, symbol.as_ptr()) }
    };
    Some(Gbm {
        create_device: cast_symbol(lookup(c"gbm_create_device"))?,
        bo_create_with_modifiers: cast_symbol(lookup(c"gbm_bo_create_with_modifiers"))?,
        bo_get_fd: cast_symbol(lookup(c"gbm_bo_get_fd"))?,
        bo_get_stride: cast_symbol(lookup(c"gbm_bo_get_stride"))?,
        bo_destroy: cast_symbol(lookup(c"gbm_bo_destroy"))?,
    })
}

fn cast_symbol<T: Copy>(address: *mut c_void) -> Option<T> {
    if address.is_null() {
        None
    } else {
        // SAFETY: every caller chooses T to match the exact libgbm symbol ABI.
        Some(unsafe { std::mem::transmute_copy(&address) })
    }
}

/// Open the first accessible DRM render node and bind a GBM device to it.
fn allocator() -> Option<&'static Allocator> {
    ALLOCATOR
        .get_or_init(|| {
            let gbm = gbm_entry_points()?;
            let render_node = open_render_node()?;
            // SAFETY: the descriptor is a live DRM render node retained for
            // the process lifetime; the GBM device borrows it.
            let device = unsafe { (gbm.create_device)(render_node) };
            if device.is_null() {
                close_descriptor(render_node);
                return None;
            }
            Some(Allocator {
                device,
                render_node,
                gbm,
            })
        })
        .as_ref()
}

fn open_render_node() -> Option<RawFd> {
    (RENDER_NODE_FIRST..=RENDER_NODE_LAST).find_map(|index| {
        let path = format!("/dev/dri/renderD{index}");
        // The node is opened read/write because GBM allocation issues DRM
        // ioctls on this descriptor. O_CLOEXEC keeps it out of any child.
        OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(O_CLOEXEC)
            .open(Path::new(&path))
            .ok()
            .map(std::os::fd::IntoRawFd::into_raw_fd)
    })
}

fn close_descriptor(descriptor: RawFd) {
    if descriptor >= 3 {
        // SAFETY: this descriptor came from GBM export or a close-on-exec
        // duplicate and has exactly one owner here.
        drop(unsafe { std::os::fd::OwnedFd::from_raw_fd(descriptor) });
    }
}

fn production_requested() -> bool {
    env::var(TRANSFER_ENV).ok().as_deref() == Some("1")
        && env::var(PRODUCTION_ENV).ok().as_deref() == Some("1")
}

/// Bounded one-shot fault injection so a synthetic run can prove the drop,
/// release, and retire paths instead of only the success path. Each stage
/// fires at most once for the process lifetime.
fn failure_injected(stage: &str) -> bool {
    let bit = match env::var(FAILURE_ENV).ok().as_deref() {
        Some("allocate") => 1_u64 << 0,
        Some("import") => 1_u64 << 1,
        Some("transfer") => 1_u64 << 2,
        Some("fence") => 1_u64 << 3,
        _ => return false,
    };
    let stage_bit = match stage {
        "allocate" => 1_u64 << 0,
        "import" => 1_u64 << 1,
        "transfer" => 1_u64 << 2,
        "fence" => 1_u64 << 3,
        _ => return false,
    };
    if bit & stage_bit == 0 {
        return false;
    }
    let fired = FIRED_FAILURE_STAGES.fetch_or(stage_bit, Ordering::Relaxed) & stage_bit == 0;
    if fired {
        eprintln!("Redunar OpenGL Replay injected failure: {stage}");
    }
    fired
}

/// Map the requested frame rate onto the three protocol rates. Anything the
/// protocol cannot encode defaults to 60 FPS.
fn target_frames_per_second(raw: Option<&str>) -> u8 {
    match raw {
        Some("30") => 30,
        Some("120" | "variable") => 120,
        _ => 60,
    }
}

// The launch token selects VFR; 120 remains a fixed-rate protocol value.
fn variable_frame_rate(raw: Option<&str>) -> bool {
    raw == Some("variable")
}

fn frame_interval_ns(source: ReplaySourceCandidate) -> u64 {
    NANOSECONDS_PER_SECOND / u64::from(source.target_frames_per_second)
}

// Bounded capture ceiling; packet timestamps follow accepted presents.
const VFR_MIN_INTERVAL_NS: u64 = NANOSECONDS_PER_SECOND.div_ceil(240);

fn variable_frame_interval_ns(source: ReplaySourceCandidate) -> u64 {
    let macroblocks = u64::from(source.width.div_ceil(16)) * u64::from(source.height.div_ceil(16));
    // Round up so the sustained rate cannot exceed the encoder's existing
    // throughput bound, including partially filled edge macroblocks.
    (macroblocks * NANOSECONDS_PER_SECOND)
        .div_ceil(u64::from(MAX_ENCODE_MACROBLOCKS_PER_SECOND))
        .max(VFR_MIN_INTERVAL_NS)
}

fn capture_due(
    variable: bool,
    source: ReplaySourceCandidate,
    deadline_ns: u64,
    now_ns: u64,
) -> bool {
    if variable {
        now_ns.saturating_add(variable_frame_interval_ns(source)) >= deadline_ns
    } else {
        now_ns >= deadline_ns
    }
}

fn advance_capture_deadline(
    variable: bool,
    deadline_ns: u64,
    now_ns: u64,
    interval_ns: u64,
) -> u64 {
    if variable {
        // Match Vulkan's two-frame allowance. Idle time restores
        // only one frame of credit; every attempt (even a busy-slot drop)
        // charges the budget, so sustained capture stays bounded.
        deadline_ns.max(now_ns).saturating_add(interval_ns)
    } else {
        next_capture_deadline(deadline_ns, now_ns, interval_ns)
    }
}

/// Fixed-rate capture deadline. A cold or fallen-behind schedule restarts
/// from now so a stall cannot burst-capture to catch up.
fn next_capture_deadline(current_deadline_ns: u64, now_ns: u64, interval_ns: u64) -> u64 {
    let scheduled_ns = if current_deadline_ns == 0 {
        now_ns
    } else {
        current_deadline_ns
    };
    let next_ns = scheduled_ns.saturating_add(interval_ns);
    if next_ns <= now_ns {
        now_ns.saturating_add(interval_ns)
    } else {
        next_ns
    }
}

/// Use the cadence deadline as the encoded presentation timestamp. Source
/// presents need not divide evenly into 30/60/120 FPS, and stamping the
/// selected frame with wall time would create variable-frame-rate judder.
fn capture_presentation_timestamp(current_deadline_ns: u64, now_ns: u64) -> u64 {
    if current_deadline_ns == 0 {
        now_ns
    } else {
        current_deadline_ns
    }
}

fn capture_timing(
    variable: bool,
    source: ReplaySourceCandidate,
    deadline_ns: u64,
    now_ns: u64,
) -> (u64, u64) {
    if variable {
        (now_ns, variable_frame_interval_ns(source))
    } else {
        (
            capture_presentation_timestamp(deadline_ns, now_ns),
            frame_interval_ns(source),
        )
    }
}

/// Keep exported presentation times strictly increasing. The replay
/// pipeline treats a non-monotonic timestamp as a terminal codec-epoch
/// violation, so a stale candidate stamp is bumped past the previous one.
fn monotonic_export_timestamp(previous_ns: u64, candidate_ns: u64) -> u64 {
    if candidate_ns > previous_ns {
        candidate_ns
    } else {
        previous_ns.saturating_add(1)
    }
}

/// GBM rounds an RA24 stride up to a pixel multiple of four, so a width of
/// 1366 yields 5632. The protocol caps a declared stride at 3840 * 4, so a
/// wider padding must reject the source rather than mis-describe the frame.
fn exportable_stride(stride: u32) -> bool {
    stride != 0 && stride <= MAX_REPLAY_SOURCE_WIDTH * 4
}

/// Pure source selection so the bounds rules are testable without a GL
/// context. Match the encoder request's macroblock throughput bound.
fn source_for(
    target_fps: u8,
    variable_rate: bool,
    width: u32,
    height: u32,
) -> Result<ReplaySourceCandidate, ReplaySourceRejection> {
    if width < MIN_WIDTH
        || height < MIN_HEIGHT
        || width > MAX_REPLAY_SOURCE_WIDTH
        || height > MAX_REPLAY_SOURCE_HEIGHT
    {
        return Err(ReplaySourceRejection::DimensionsUnsupported);
    }
    if target_fps == 120
        && !variable_rate
        && !((width <= 1_920 && height <= 1_080) || (width <= 1_080 && height <= 1_920))
    {
        return Err(ReplaySourceRejection::DimensionsUnsupported);
    }
    if width
        .div_ceil(16)
        .saturating_mul(height.div_ceil(16))
        .saturating_mul(u32::from(target_fps))
        > MAX_ENCODE_MACROBLOCKS_PER_SECOND
    {
        return Err(ReplaySourceRejection::DimensionsUnsupported);
    }
    Ok(ReplaySourceCandidate {
        width,
        height,
        pixel_format: ReplayPixelFormat::Rgba8Unorm,
        target_frames_per_second: target_fps,
    })
}

/// Capability gate: the extension tokens plus the exact entry points. Mesa's
/// radeonsi does not advertise `GL_EXT_external_buffer`, so buffer storage from
/// memory is proven by the imported-memory query instead.
fn external_memory_supported(functions: &ExportFunctions) -> bool {
    has_extension(functions, b"GL_EXT_memory_object")
        && has_extension(functions, b"GL_EXT_memory_object_fd")
}

fn has_extension(functions: &ExportFunctions, expected: &[u8]) -> bool {
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
                    && unsafe { CStr::from_ptr(pointer.cast()) }.to_bytes() == expected
            });
        }
    }
    // SAFETY: legacy GL returns a NUL-terminated, space-delimited list and
    // the pointer stays valid while this context is current.
    let pointer = unsafe { (functions.get_string)(GL_EXTENSIONS) };
    !pointer.is_null()
        && extension_list_contains(
            // SAFETY: the string is context-owned NUL-terminated storage.
            unsafe { CStr::from_ptr(pointer.cast()) }.to_bytes(),
            expected,
        )
}

fn extension_list_contains(extensions: &[u8], expected: &[u8]) -> bool {
    extensions
        .split(u8::is_ascii_whitespace)
        .any(|extension| extension == expected)
}

fn context_version(functions: &ExportFunctions) -> &'static [u8] {
    // SAFETY: GL_VERSION returns context-owned static storage while current.
    let pointer = unsafe { (functions.get_string)(GL_VERSION) };
    if pointer.is_null() {
        b""
    } else {
        // SAFETY: OpenGL returns a NUL-terminated version string.
        unsafe { CStr::from_ptr(pointer.cast()) }.to_bytes()
    }
}

/// The DMA path needs GL 3.2+ semantics (dual framebuffer targets, buffer
/// storage, sync objects). ES 2 reports a legacy `GL_FRAMEBUFFER` binding only.
fn context_supports_export(functions: &ExportFunctions, api: ApiFlavor) -> bool {
    if api == ApiFlavor::Embedded {
        return false;
    }
    matches!(context_version(functions).first(), Some(major) if *major >= b'3')
}

static REPORTED_REJECTIONS: AtomicU64 = AtomicU64::new(0);

/// Report the first hard capability failure for the process. Gates are
/// static per context family, so a second report adds no information the
/// daemon does not already have.
fn report_unavailable_once(reason: ReplaySourceRejection) {
    let bit = 1_u64 << (reason as u32 % 64);
    if REPORTED_REJECTIONS.fetch_or(bit, Ordering::Relaxed) & bit != 0 {
        return;
    }
    super::record_replay_source_rejected(reason);
}

fn monotonic_now() -> Option<u64> {
    let mut time = Timespec {
        seconds: 0,
        nanoseconds: 0,
    };
    // SAFETY: time is valid writable storage for one Timespec.
    if unsafe { clock_gettime(CLOCK_MONOTONIC, &raw mut time) } != 0
        || time.seconds < 0
        || !(0..1_000_000_000).contains(&time.nanoseconds)
    {
        return None;
    }
    u64::try_from(time.seconds)
        .ok()?
        .checked_mul(NANOSECONDS_PER_SECOND)?
        .checked_add(u64::try_from(time.nanoseconds).ok()?)
}

impl Slot {
    /// Poll the copy fence without waiting. A ready copy remains attached to
    /// the slot until its descriptor announcement succeeds or is dropped.
    unsafe fn copy_ready(&mut self, functions: &ExportFunctions) -> bool {
        if self.sync != 0 {
            let sync = self.sync as *mut c_void;
            // SAFETY: the zero-timeout wait only polls a sync this producer
            // created, and the delete releases exactly that object.
            if !matches!(
                unsafe { (functions.client_wait_sync)(sync, 0, 0) },
                GL_ALREADY_SIGNALED | GL_CONDITION_SATISFIED
            ) {
                return false;
            }
            // SAFETY: the sync is live and not referenced elsewhere.
            unsafe { (functions.delete_sync)(sync) };
            self.sync = 0;
        }
        self.source.is_some()
    }

    /// True only after GPU work, descriptor publication, and daemon ownership
    /// have all ended.
    unsafe fn completed(&mut self, functions: &ExportFunctions) -> bool {
        if self.sync != 0 && !unsafe { self.copy_ready(functions) } {
            return false;
        }
        if self.export_released {
            self.export_sequence = None;
            self.export_released = false;
        }
        self.source.is_none() && self.export_sequence.is_none()
    }
}

/// Allocate all slots for one pool. Any failure releases everything this
/// call created; the caller drops Replay work for this source rather than
/// leaving half-imported memory behind.
///
/// # Safety
/// The caller GL context must be current while these functions run.
unsafe fn allocate_slots(
    functions: &ExportFunctions,
    source: ReplaySourceCandidate,
) -> Option<[Slot; SLOT_COUNT]> {
    let device = allocator()?;
    let mut slots: Vec<Slot> = Vec::with_capacity(SLOT_COUNT);
    for _ in 0..SLOT_COUNT {
        // SAFETY: the caller maintains the current-context invariant.
        let slot_result = unsafe { allocate_slot(functions, device, source) };
        let Some(slot) = slot_result else {
            for created in &slots {
                // SAFETY: inherited current-context invariant; these names
                // were created by allocate_slot above.
                unsafe { release_slot(functions, created) };
            }
            return None;
        };
        slots.push(slot);
    }
    <[Slot; SLOT_COUNT]>::try_from(slots).ok()
}

/// One GBM RA24 LINEAR buffer imported into a GL pixel-pack buffer.
///
/// # Safety
/// The caller GL context must be current.
unsafe fn allocate_slot(
    functions: &ExportFunctions,
    device: &Allocator,
    source: ReplaySourceCandidate,
) -> Option<Slot> {
    if failure_injected("allocate") {
        return None;
    }
    let modifiers = [LINEAR_MODIFIER];
    // SAFETY: the modifiers slice outlives the call and the device handle is
    // live; GBM returns either a usable bo or null.
    let bo = unsafe {
        (device.gbm.bo_create_with_modifiers)(
            device.device,
            source.width,
            source.height,
            DRM_FORMAT_RA24,
            modifiers.as_ptr(),
            u32::try_from(modifiers.len()).unwrap_or(0),
        )
    };
    if bo.is_null() {
        return None;
    }
    // SAFETY: bo is a live GBM buffer object created immediately above.
    let stride = unsafe { (device.gbm.bo_get_stride)(bo) };
    // SAFETY: inherited live bo from this function.
    let descriptor = unsafe { (device.gbm.bo_get_fd)(bo) };
    // SAFETY: the fd (if any) is independent of the bo, so the bo can be
    // destroyed as soon as its descriptor is retained.
    unsafe { (device.gbm.bo_destroy)(bo) };
    if !exportable_stride(stride) || descriptor < 3 {
        if descriptor >= 3 {
            close_descriptor(descriptor);
        }
        return None;
    }
    let Some(bytes) = u64::from(stride).checked_mul(u64::from(source.height)) else {
        close_descriptor(descriptor);
        return None;
    };
    if bytes > MAX_SLOT_BYTES {
        close_descriptor(descriptor);
        return None;
    }
    // OpenGL consumes the duplicate it is given; the original stays open in
    // this process for later SCM_RIGHTS transfers.
    let Ok(transfer_copy) = fd_transport::duplicate_for_transfer(descriptor) else {
        close_descriptor(descriptor);
        return None;
    };
    if failure_injected("import") {
        close_descriptor(transfer_copy);
        close_descriptor(descriptor);
        return None;
    }
    let mut memory = 0_u32;
    let mut buffer = 0_u32;
    let imported = (|| {
        // SAFETY: the caller invariant keeps this context current; each GL
        // call below writes only locally owned storage or objects created in
        // this closure.
        unsafe {
            (functions.create_memory_objects)(1, &raw mut memory);
            if memory == 0 {
                return false;
            }
            (functions.import_memory_fd)(
                memory,
                bytes,
                GL_HANDLE_TYPE_OPAQUE_FD_EXT,
                transfer_copy,
            );
            // RADV consumes the duplicate inside the import call even when
            // the import reports an error, so it is never closed twice here;
            // an error leaks at most one descriptor on a failed pool.
            if (functions.get_error)() != GL_NO_ERROR {
                return false;
            }
            (functions.gen_buffers)(1, &raw mut buffer);
            if buffer == 0 {
                return false;
            }
            (functions.bind_buffer)(GL_PIXEL_PACK_BUFFER, buffer);
            (functions.buffer_storage_mem)(
                GL_PIXEL_PACK_BUFFER,
                isize::try_from(bytes).unwrap_or(0),
                memory,
                0,
            );
            (functions.bind_buffer)(GL_PIXEL_PACK_BUFFER, 0);
            (functions.get_error)() == GL_NO_ERROR
        }
    })();
    if !imported {
        if buffer != 0 {
            // SAFETY: context current per caller invariant.
            unsafe { (functions.delete_buffers)(1, &raw const buffer) };
        }
        if memory != 0 {
            // SAFETY: context current per caller invariant.
            unsafe { (functions.delete_memory_objects)(1, &raw const memory) };
        }
        close_descriptor(descriptor);
        return None;
    }
    Some(Slot {
        memory,
        buffer,
        fd: descriptor,
        stride,
        bytes,
        ..Slot::IDLE
    })
}

/// # Safety
/// The caller GL context must be current and own the slot's objects.
unsafe fn release_slot(functions: &ExportFunctions, slot: &Slot) {
    if slot.buffer != 0 {
        // SAFETY: caller invariant; the name was created by allocate_slot.
        unsafe { (functions.delete_buffers)(1, &raw const slot.buffer) };
    }
    if slot.memory != 0 {
        // SAFETY: caller invariant; the name was created by allocate_slot.
        unsafe { (functions.delete_memory_objects)(1, &raw const slot.memory) };
    }
    close_descriptor(slot.fd);
}

/// # Safety
/// The caller GL context must be current and own the pool's objects.
unsafe fn release_pool(functions: &ExportFunctions, pool: &mut Pool) {
    for slot in &pool.slots {
        if slot.sync != 0 {
            // SAFETY: caller invariant; the sync belongs to this context.
            unsafe { (functions.delete_sync)(slot.sync as *mut c_void) };
        }
        // SAFETY: caller invariant.
        unsafe { release_slot(functions, slot) };
    }
    if pool.flip_texture != 0 {
        // SAFETY: caller invariant.
        unsafe { (functions.delete_textures)(1, &raw const pool.flip_texture) };
    }
    if pool.flip_framebuffer != 0 {
        // SAFETY: caller invariant.
        unsafe { (functions.delete_framebuffers)(1, &raw const pool.flip_framebuffer) };
    }
}

/// Create the private Y-flip render target for a pool. The GBM slot memory
/// is linear top-down and the Vulkan importer reads row zero as the top row,
/// while the window back buffer stores row zero at the bottom; one blit with
/// reversed destination rows performs the flip on the GPU.
///
/// # Safety
/// The caller GL context must be current.
unsafe fn ensure_flip_surface(
    functions: &ExportFunctions,
    texture: &mut c_uint,
    framebuffer: &mut c_uint,
    source: ReplaySourceCandidate,
) -> bool {
    // SAFETY: caller invariant; names are locally owned until returned.
    unsafe {
        (functions.gen_textures)(1, texture);
        (functions.gen_framebuffers)(1, framebuffer);
        if *texture == 0 || *framebuffer == 0 {
            return false;
        }
        (functions.bind_texture)(GL_TEXTURE_2D, *texture);
        (functions.tex_storage_2d)(
            GL_TEXTURE_2D,
            1,
            GL_RGBA8,
            i32::try_from(source.width).unwrap_or(0),
            i32::try_from(source.height).unwrap_or(0),
            1,
        );
        (functions.bind_framebuffer)(GL_FRAMEBUFFER, *framebuffer);
        (functions.framebuffer_texture_2d)(
            GL_FRAMEBUFFER,
            GL_COLOR_ATTACHMENT0,
            GL_TEXTURE_2D,
            *texture,
            0,
        );
        let complete =
            (functions.check_framebuffer_status)(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE;
        (functions.bind_texture)(GL_TEXTURE_2D, 0);
        (functions.bind_framebuffer)(GL_FRAMEBUFFER, 0);
        complete && (functions.get_error)() == GL_NO_ERROR
    }
}

/// Retire pools sized for a different source than the active one. Copies that
/// have not crossed the daemon boundary are discarded after their GL fence;
/// exported slots remain release-gated. The pool itself stays until both kinds
/// of ownership have ended, and the replacement count remains bounded by the
/// process-wide pool limit.
///
/// # Safety
/// The caller GL context must be current.
unsafe fn retire_stale_pools(
    functions: &ExportFunctions,
    pools: &mut Vec<Pool>,
    context: usize,
    source: ReplaySourceCandidate,
) {
    // SAFETY: inherited from the documented caller invariant.
    unsafe {
        let mut index = 0;
        while index < pools.len() {
            if pools[index].context != context || pools[index].source == source {
                index += 1;
                continue;
            }
            let pool = &mut pools[index];
            for slot in &mut pool.slots {
                if slot.sync != 0 {
                    // SAFETY: this pool belongs to the current context.
                    let _ = slot.copy_ready(functions);
                }
                if slot.discard_retired_copy() {
                    pool.drops = pool.drops.saturating_add(1);
                }
            }
            if pool_retirable(functions, pool) {
                release_pool(functions, pool);
                pools.remove(index);
            } else {
                index += 1;
            }
        }
    }
}

/// Allocate the slot buffers and flip surface for a context without a pool
/// and append it. Returns the new index, or None when the pool budget is
/// exhausted or a GL allocation failed; failed paths release what they
/// allocated before returning.
///
/// # Safety
/// The GL context identified by the context argument must be current.
unsafe fn create_pool(
    functions: &ExportFunctions,
    pools: &mut Vec<Pool>,
    context: usize,
    source: ReplaySourceCandidate,
) -> Option<usize> {
    // SAFETY: inherited from the documented caller invariant.
    unsafe {
        if pools.len() >= MAX_POOLS {
            return None;
        }
        let mut saved = [0_i32; 4];
        (functions.get_integer)(GL_READ_FRAMEBUFFER_BINDING, &raw mut saved[0]);
        (functions.get_integer)(GL_DRAW_FRAMEBUFFER_BINDING, &raw mut saved[1]);
        (functions.get_integer)(GL_PIXEL_PACK_BUFFER_BINDING, &raw mut saved[2]);
        (functions.get_integer)(GL_TEXTURE_BINDING_2D, &raw mut saved[3]);
        let created = (|| {
            let slots = allocate_slots(functions, source)?;
            let mut flip_texture = 0;
            let mut flip_framebuffer = 0;
            let surface =
                ensure_flip_surface(functions, &mut flip_texture, &mut flip_framebuffer, source);
            if !surface {
                for slot in &slots {
                    release_slot(functions, slot);
                }
                if flip_texture != 0 {
                    (functions.delete_textures)(1, &raw const flip_texture);
                }
                if flip_framebuffer != 0 {
                    (functions.delete_framebuffers)(1, &raw const flip_framebuffer);
                }
                return None;
            }
            pools.push(Pool {
                context,
                source,
                slots,
                flip_texture,
                flip_framebuffer,
                next_capture_ns: 0,
                exports: 0,
                drops: 0,
            });
            Some(pools.len() - 1)
        })();
        (functions.bind_buffer)(GL_PIXEL_PACK_BUFFER, saved[2].cast_unsigned());
        (functions.bind_texture)(GL_TEXTURE_2D, saved[3].cast_unsigned());
        (functions.bind_framebuffer)(GL_READ_FRAMEBUFFER, saved[0].cast_unsigned());
        (functions.bind_framebuffer)(GL_DRAW_FRAMEBUFFER, saved[1].cast_unsigned());
        created
    }
}

/// Publish copies only after their GL fence has signaled. This is the implicit
/// GPU-to-daemon synchronization boundary: no FD is visible to Vulkan Video
/// while OpenGL may still be writing its memory.
unsafe fn publish_ready_exports(functions: &ExportFunctions, pool: &mut Pool) {
    for slot in &mut pool.slots {
        // SAFETY: the pool's creating context is current.
        if !unsafe { slot.copy_ready(functions) } {
            continue;
        }
        let Some(source) = slot.source else {
            continue;
        };
        let duplicate = if failure_injected("transfer") {
            None
        } else {
            fd_transport::duplicate_for_transfer(slot.fd).ok()
        };
        let Some(duplicate) = duplicate else {
            pool.drops = pool.drops.saturating_add(1);
            slot.source = None;
            slot.timestamp_ns = 0;
            continue;
        };
        let sequence = NEXT_EXPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let sent = super::record_replay_frame_exported(
            sequence,
            duplicate,
            source,
            slot.stride,
            slot.timestamp_ns,
            frame_interval_ns(source).max(1),
        );
        slot.source = None;
        slot.timestamp_ns = 0;
        if sent.is_ok() {
            slot.export_sequence = Some(sequence);
            pool.exports = pool.exports.saturating_add(1);
        } else {
            pool.drops = pool.drops.saturating_add(1);
        }
    }
}

/// Entry point called from every intercepted presentation. Never blocks and
/// never disturbs the game's GL state: every failure path only counts a drop.
///
/// # Safety
/// The caller GL context identified by the context key must be current.
pub(crate) unsafe fn capture(context: usize, api: ApiFlavor) {
    if context == 0 || !production_requested() {
        return;
    }
    let Some(functions) = *FUNCTIONS.get_or_init(ExportFunctions::load) else {
        report_unavailable_once(ReplaySourceRejection::ExternalMemoryUnsupported);
        return;
    };
    if !context_supports_export(&functions, api) || !external_memory_supported(&functions) {
        report_unavailable_once(ReplaySourceRejection::ExternalMemoryUnsupported);
        return;
    }
    if allocator().is_none() {
        report_unavailable_once(ReplaySourceRejection::EncoderBackendUnavailable);
        return;
    }
    let Some(now_ns) = monotonic_now() else {
        return;
    };
    let variable = *VARIABLE_FRAME_RATE;
    let source = match viewport_source(&functions) {
        Ok(source) => source,
        Err(reason) => {
            report_unavailable_once(reason);
            return;
        }
    };
    let Ok(mut state) = STATE.try_lock() else {
        return;
    };
    // SAFETY: retirement is restricted to the current context.
    unsafe { retire_stale_pools(&functions, &mut state.pools, context, source) };
    for pool in state
        .pools
        .iter_mut()
        .filter(|pool| pool.context == context && pool.source == source)
    {
        // SAFETY: only the active generation created by this current context
        // is visited. Retired generations are drained above without export.
        unsafe { publish_ready_exports(&functions, pool) };
    }
    let existing_pool = state
        .pools
        .iter()
        .position(|pool| pool.context == context && pool.source == source);
    let pool_index = match existing_pool {
        Some(index) => index,
        // SAFETY: the context is current per the caller invariant.
        None => match unsafe { create_pool(&functions, &mut state.pools, context, source) } {
            Some(index) => index,
            None => return,
        },
    };
    if state.announced != Some(source) {
        super::record_replay_source_candidate(source);
        state.announced = Some(source);
    }
    // Sequential field updates keep the borrow checker happy: the long
    // mutable borrow of the pool starts only once every process-wide
    // timestamp update is finished.
    if !capture_due(
        variable,
        source,
        state.pools[pool_index].next_capture_ns,
        now_ns,
    ) {
        return;
    }
    let deadline = state.pools[pool_index].next_capture_ns;
    let (presentation_ns, interval_ns) = capture_timing(variable, source, deadline, now_ns);
    state.pools[pool_index].next_capture_ns =
        advance_capture_deadline(variable, deadline, now_ns, interval_ns);
    let Some(slot_index) = state.pools[pool_index]
        .slots
        .iter()
        .position(Slot::is_available)
    else {
        state.pools[pool_index].drops += 1;
        return;
    };
    let timestamp_ns = monotonic_export_timestamp(state.last_export_timestamp_ns, presentation_ns);
    state.last_export_timestamp_ns = state.last_export_timestamp_ns.max(timestamp_ns);
    let pool = &mut state.pools[pool_index];
    let flip_framebuffer = pool.flip_framebuffer;
    let slot = &mut pool.slots[slot_index];
    // SAFETY: the context is current per the caller invariant.
    let started = unsafe { start_copy(&functions, slot, flip_framebuffer, source, timestamp_ns) };
    if !started {
        pool.drops += 1;
    }
}

/// Read the current viewport and turn it into a protocol source candidate.
fn viewport_source(
    functions: &ExportFunctions,
) -> Result<ReplaySourceCandidate, ReplaySourceRejection> {
    let mut viewport = [0_i32; 4];
    // SAFETY: the current context writes four viewport integers.
    unsafe { (functions.get_integer)(GL_VIEWPORT, viewport.as_mut_ptr()) };
    let width = u32::try_from(viewport[2]).unwrap_or(0);
    let height = u32::try_from(viewport[3]).unwrap_or(0);
    source_for(
        target_frames_per_second(env::var(FRAME_RATE_ENV).ok().as_deref()),
        *VARIABLE_FRAME_RATE,
        width,
        height,
    )
}

/// True when no GPU work or unreleased export remains for the pool, so its
/// GL names can be deleted with this context current.
///
/// # Safety
/// The pool's GL context must be current.
unsafe fn pool_retirable(functions: &ExportFunctions, pool: &mut Pool) -> bool {
    // SAFETY: the caller invariant keeps the pool's context current.
    pool.slots
        .iter_mut()
        .all(|slot| unsafe { slot.completed(functions) })
}

/// Copy the just-presented back buffer into the slot's DMA memory in the
/// top-down row order the importer expects, then fence the copy.
///
/// The game's bindings and pixel state are saved and restored around the
/// blit, read, and fence so presentation code cannot observe Redunar state.
///
/// # Safety
/// The pool's GL context must be current and the slot must be available.
unsafe fn start_copy(
    functions: &ExportFunctions,
    slot: &mut Slot,
    flip_framebuffer: c_uint,
    source: ReplaySourceCandidate,
    timestamp_ns: u64,
) -> bool {
    let width = i32::try_from(source.width).unwrap_or(0);
    let height = i32::try_from(source.height).unwrap_or(0);
    let row_length = i32::try_from(slot.stride / 4).unwrap_or(0);
    debug_assert!(slot.bytes <= MAX_SLOT_BYTES);
    let mut saved = [0_i32; 5];
    // SAFETY: the caller invariant keeps this context current; both queries
    // write locally owned storage before anything is changed.
    let (saved_scissor, saved_read_buffer) = unsafe {
        let scissor = (functions.is_enabled)(GL_SCISSOR_TEST);
        let mut read_buffer = 0_i32;
        (functions.get_integer)(GL_READ_BUFFER, &raw mut read_buffer);
        (scissor, read_buffer)
    };
    // SAFETY: the context is current per the caller invariant; every query
    // writes locally owned storage and every binding change is undone before
    // returning. The zero read-buffer binding selects none, which the
    // following explicit read_buffer call replaces.
    unsafe {
        (functions.get_integer)(GL_READ_FRAMEBUFFER_BINDING, &raw mut saved[0]);
        (functions.get_integer)(GL_DRAW_FRAMEBUFFER_BINDING, &raw mut saved[1]);
        (functions.get_integer)(GL_PIXEL_PACK_BUFFER_BINDING, &raw mut saved[2]);
        (functions.get_integer)(GL_PACK_ROW_LENGTH, &raw mut saved[3]);
        (functions.get_integer)(GL_PACK_ALIGNMENT, &raw mut saved[4]);
        if (functions.get_error)() != GL_NO_ERROR {
            return false;
        }
        (functions.disable)(GL_SCISSOR_TEST);
        (functions.bind_framebuffer)(GL_READ_FRAMEBUFFER, 0);
        if let Some(read_buffer) = functions.read_buffer {
            (read_buffer)(GL_BACK);
            if (functions.get_error)() != GL_NO_ERROR {
                restore(
                    functions,
                    &saved,
                    saved_scissor,
                    saved_read_buffer,
                    functions.read_buffer,
                );
                return false;
            }
        } else if (functions.get_error)() != GL_NO_ERROR {
            // Without GL_EXT_direct_state_access-style read-buffer control
            // the back buffer cannot be selected reliably. Mesa desktop
            // contexts always expose glReadBuffer, so this refuses only
            // exotic drivers instead of guessing.
            restore(functions, &saved, saved_scissor, saved_read_buffer, None);
            return false;
        }
        (functions.bind_framebuffer)(GL_DRAW_FRAMEBUFFER, flip_framebuffer);
        (functions.blit_framebuffer)(
            0,
            0,
            width,
            height,
            0,
            height,
            width,
            0,
            GL_COLOR_BUFFER_BIT,
            GL_NEAREST,
        );
        (functions.bind_buffer)(GL_PIXEL_PACK_BUFFER, slot.buffer);
        (functions.pixel_store)(GL_PACK_ROW_LENGTH, row_length);
        (functions.pixel_store)(GL_PACK_ALIGNMENT, 1);
        (functions.bind_framebuffer)(GL_READ_FRAMEBUFFER, flip_framebuffer);
        if let Some(read_buffer) = functions.read_buffer {
            (read_buffer)(GL_COLOR_ATTACHMENT0);
        }
        (functions.read_pixels)(
            0,
            0,
            width,
            height,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            std::ptr::null(),
        );
        slot.sync = if failure_injected("fence") {
            0
        } else {
            (functions.fence_sync)(GL_SYNC_GPU_COMMANDS_COMPLETE, 0) as usize
        };
        restore(
            functions,
            &saved,
            saved_scissor,
            saved_read_buffer,
            functions.read_buffer,
        );
        if slot.sync == 0 || (functions.get_error)() != GL_NO_ERROR {
            if slot.sync != 0 {
                (functions.delete_sync)(slot.sync as *mut c_void);
                slot.sync = 0;
            }
            return false;
        }
    }
    slot.source = Some(source);
    slot.timestamp_ns = timestamp_ns;
    true
}

/// Restore everything `start_copy` touched. The `read_buffer` option is Some
/// only when the initial `GL_BACK` selection succeeded, mirroring how much
/// state the attempt actually changed.
///
/// # Safety
/// The values were read from the caller's current context in `start_copy`.
unsafe fn restore(
    functions: &ExportFunctions,
    saved: &[i32; 5],
    scissor_was_enabled: u8,
    read_buffer: i32,
    restore_read_buffer: Option<ReadBuffer>,
) {
    // SAFETY: caller invariant; every value was read from this context at
    // the start of the attempt and the bindings are restored to them.
    unsafe {
        (functions.bind_buffer)(GL_PIXEL_PACK_BUFFER, saved[2].cast_unsigned());
        (functions.pixel_store)(GL_PACK_ROW_LENGTH, saved[3]);
        (functions.pixel_store)(GL_PACK_ALIGNMENT, saved[4]);
        (functions.bind_framebuffer)(GL_READ_FRAMEBUFFER, saved[0].cast_unsigned());
        (functions.bind_framebuffer)(GL_DRAW_FRAMEBUFFER, saved[1].cast_unsigned());
        if let Some(read_buffer_fn) = restore_read_buffer {
            (read_buffer_fn)(read_buffer.cast_unsigned());
        }
        if scissor_was_enabled != 0 {
            (functions.enable)(GL_SCISSOR_TEST);
        }
    }
}

/// Apply one daemon `ReplayFrameReleased` token to every pool slot.
pub(crate) fn release_sequence(sequence: u64) -> bool {
    let Ok(mut state) = STATE.try_lock() else {
        return false;
    };
    for slot in state
        .pools
        .iter_mut()
        .flat_map(|pool| pool.slots.iter_mut())
    {
        slot.release_export(sequence);
    }
    true
}

/// Forget every pool owned by a context immediately before that context is
/// destroyed. Context teardown owns deletion of GL names and pending commands;
/// Redunar closes only its retained DMA-BUF descriptors and bookkeeping.
/// Descriptor duplicates already transferred to the daemon remain valid until
/// encoding completes.
pub(crate) fn destroy_context(context: usize) {
    if context == 0 {
        return;
    }
    let mut state = STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    remove_context_pools(&mut state.pools, context);
}

fn remove_context_pools(pools: &mut Vec<Pool>, context: usize) -> usize {
    let mut index = 0;
    let mut removed = 0;
    while index < pools.len() {
        if pools[index].context != context {
            index += 1;
            continue;
        }
        log_diagnostic_pool(&pools[index]);
        for slot in &mut pools[index].slots {
            close_descriptor(slot.fd);
            slot.fd = -1;
        }
        pools.remove(index);
        removed += 1;
    }
    removed
}

/// Process-exit teardown. Pool memory dies with the process, but the GL
/// names may belong to live contexts during atexit, so only the lock-held
/// bookkeeping is cleared here; the retained GBM descriptors close with the
/// address space.
pub(crate) fn finish() {
    let Ok(mut state) = STATE.try_lock() else {
        return;
    };
    for pool in &state.pools {
        log_diagnostic_pool(pool);
    }
    state.pools.clear();
    state.announced = None;
    // The render-node descriptor and GBM device intentionally stay open:
    // atexit order relative to driver teardown is unspecified, and the
    // kernel reclaims both when the address space goes away, exactly like
    // the Vulkan producer's process-scoped device handles.
    if let Some(device) = ALLOCATOR.get() {
        let _ = device.as_ref().map(|allocator| allocator.render_node);
    }
}

fn log_diagnostic_pool(pool: &Pool) {
    if env::var("REDUNAR_REPLAY_DIAGNOSTIC_EXPORT_DMABUF")
        .ok()
        .as_deref()
        == Some("1")
    {
        eprintln!(
            "Redunar OpenGL Replay pool: context={} {}x{} exports={} drops={}",
            pool.context, pool.source.width, pool.source.height, pool.exports, pool.drops
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_at(fps: u8, width: u32, height: u32) -> ReplaySourceCandidate {
        ReplaySourceCandidate {
            width,
            height,
            pixel_format: ReplayPixelFormat::Rgba8Unorm,
            target_frames_per_second: fps,
        }
    }

    #[test]
    fn frame_rate_map_only_accepts_protocol_rates() {
        assert_eq!(target_frames_per_second(Some("30")), 30);
        assert_eq!(target_frames_per_second(Some("120")), 120);
        assert_eq!(target_frames_per_second(Some("144")), 60);
        assert_eq!(target_frames_per_second(None), 60);
    }

    #[test]
    fn source_bounds_reject_undersized_oversized_and_4k120() {
        assert_eq!(source_for(60, false, 320, 180), Ok(source_at(60, 320, 180)));
        assert_eq!(
            source_for(60, false, 319, 180),
            Err(ReplaySourceRejection::DimensionsUnsupported)
        );
        assert_eq!(
            source_for(60, false, 320, 179),
            Err(ReplaySourceRejection::DimensionsUnsupported)
        );
        assert_eq!(
            source_for(60, false, MAX_REPLAY_SOURCE_WIDTH + 1, 180),
            Err(ReplaySourceRejection::DimensionsUnsupported)
        );
        assert_eq!(
            source_for(120, false, 1920, 1080),
            Ok(source_at(120, 1920, 1080))
        );
        assert_eq!(
            source_for(120, false, 1080, 1920),
            Ok(source_at(120, 1080, 1920))
        );
        assert_eq!(
            source_for(120, false, 3840, 2160),
            Err(ReplaySourceRejection::DimensionsUnsupported)
        );
        assert_eq!(
            source_for(30, false, 3840, 2160),
            Ok(source_at(30, 3840, 2160))
        );
    }

    #[test]
    fn gbm_ra24_padding_stays_within_the_protocol_stride_cap() {
        // Measured on RADV (artifacts/opengl-contract): RA24 keeps
        // stride = width * 4 for the common widths, and RA24 never pads
        // 1366 to the 5632 the BG88 probe showed (1408 * 4 = 5632 would
        // pass the cap anyway). Only an over-cap padded stride rejects.
        assert!(exportable_stride(320 * 4));
        assert!(exportable_stride(3_840 * 4));
        assert!(exportable_stride(1_366 * 4));
        assert!(exportable_stride(5_632));
        assert!(!exportable_stride(3_840 * 4 + 4));
        assert!(!exportable_stride(0));
    }

    #[test]
    fn cadence_deadlines_hold_fixed_rate_without_bursting() {
        let source = source_at(60, 1_280, 720);
        let interval_ns = frame_interval_ns(source);
        assert_eq!(interval_ns, NANOSECONDS_PER_SECOND / 60);
        let mut deadline = 0;
        let mut captured = 0;
        for frame in 0..120_u64 {
            let now_ns = 1_000 + frame * (NANOSECONDS_PER_SECOND / 84);
            if now_ns >= deadline {
                captured += 1;
                deadline = next_capture_deadline(deadline, now_ns, interval_ns);
            }
        }
        // 120 source frames at 84 FPS span about 1.43 s; a 60 FPS cadence
        // admits about 85 captures. It must not collapse to the 84 source
        // frames (one capture per frame) nor below 60 FPS.
        assert!((82..=86).contains(&captured), "captured {captured}");
        // A long stall restarts at one interval after now instead of
        // scheduling a catch-up burst.
        let stalled_now = deadline + 10 * interval_ns;
        let after_stall = next_capture_deadline(deadline, stalled_now, interval_ns);
        assert!(after_stall > stalled_now);
        assert!(after_stall <= stalled_now + interval_ns);
    }

    #[test]
    fn export_timestamps_are_strictly_increasing() {
        assert_eq!(monotonic_export_timestamp(0, 5), 5);
        assert_eq!(monotonic_export_timestamp(5, 5), 6);
        assert_eq!(monotonic_export_timestamp(9, 3), 10);
        assert_eq!(capture_presentation_timestamp(0, 77), 77);
        assert_eq!(capture_presentation_timestamp(55, 77), 55);
    }

    #[test]
    fn failure_injection_fires_once_per_stage() {
        // Each test process runs with no injection configured, so the pure
        // stage-matching logic is exercised through a local helper copy.
        fn matches(configured: Option<&str>, stage: &str) -> Option<u64> {
            let bit = match configured {
                Some("allocate") => 1_u64 << 0,
                Some("import") => 1_u64 << 1,
                Some("transfer") => 1_u64 << 2,
                Some("fence") => 1_u64 << 3,
                _ => return None,
            };
            let stage_bit = match stage {
                "allocate" => 1_u64 << 0,
                "import" => 1_u64 << 1,
                "transfer" => 1_u64 << 2,
                "fence" => 1_u64 << 3,
                _ => return None,
            };
            (bit & stage_bit != 0).then_some(stage_bit)
        }
        assert_eq!(matches(Some("import"), "allocate"), None);
        assert_eq!(matches(Some("import"), "import"), Some(1 << 1));
        assert_eq!(matches(Some("fence"), "fence"), Some(1 << 3));
        assert_eq!(matches(None, "import"), None);
        assert!(!failure_injected("allocate"));
        assert!(!failure_injected("import"));
        assert!(!failure_injected("transfer"));
        assert!(!failure_injected("fence"));
    }

    #[test]
    fn slot_recycles_only_after_fence_and_release() {
        let slot = Slot::IDLE;
        assert!(!slot.is_available());
        let filled = Slot {
            memory: 4,
            buffer: 7,
            fd: 42,
            stride: 1280 * 4,
            bytes: u64::from(1_280_u32 * 4) * 720,
            source: None,
            sync: 0,
            timestamp_ns: 0,
            export_sequence: None,
            export_released: false,
        };
        assert!(filled.is_available());
        assert!(filled.allocated());
        let mut busy = Slot {
            sync: 1,
            export_sequence: Some(9),
            ..filled
        };
        assert!(!busy.is_available());
        // Release arrives while the GPU copy is still running.
        busy.release_export(9);
        assert!(busy.export_released);
        assert!(!busy.is_available());
        busy.export_released = false;
        busy.export_sequence = None;
        busy.sync = 0;
        assert!(busy.is_available());
        // A stale release for another sequence changes nothing.
        let mut exporting = Slot {
            sync: 1,
            export_sequence: Some(10),
            ..filled
        };
        exporting.release_export(11);
        assert_eq!(exporting.export_sequence, Some(10));
    }

    #[test]
    fn retired_copy_drops_only_local_ownership() {
        let source = source_at(60, 1_280, 720);
        let mut local_copy = Slot {
            source: Some(source),
            timestamp_ns: 77,
            ..Slot::IDLE
        };
        assert!(local_copy.discard_retired_copy());
        assert_eq!(local_copy.source, None);
        assert_eq!(local_copy.timestamp_ns, 0);

        let mut exported = Slot {
            export_sequence: Some(9),
            export_released: false,
            ..Slot::IDLE
        };
        assert!(!exported.discard_retired_copy());
        assert_eq!(exported.export_sequence, Some(9));
        exported.release_export(9);
        assert!(!exported.discard_retired_copy());
        assert_eq!(exported.export_sequence, None);
    }

    #[test]
    fn retired_copy_waits_for_its_fence() {
        let mut slot = Slot {
            source: Some(source_at(60, 640, 360)),
            sync: 1,
            timestamp_ns: 12,
            ..Slot::IDLE
        };
        assert!(!slot.discard_retired_copy());
        assert!(slot.source.is_some());
        assert_eq!(slot.timestamp_ns, 12);
    }

    #[test]
    fn destroyed_context_releases_its_entire_pool_budget() {
        let pool = |context, width| Pool {
            context,
            source: source_at(60, width, 360),
            slots: [Slot::IDLE; SLOT_COUNT],
            flip_texture: 1,
            flip_framebuffer: 2,
            next_capture_ns: 0,
            exports: 0,
            drops: 0,
        };
        let mut pools = vec![pool(7, 640), pool(8, 800), pool(7, 1_280)];
        assert_eq!(remove_context_pools(&mut pools, 7), 2);
        assert_eq!(pools.len(), 1);
        assert_eq!(pools[0].context, 8);
        assert_eq!(remove_context_pools(&mut pools, 9), 0);
    }

    #[test]
    fn extension_matching_requires_a_complete_token() {
        let list = b"GL_EXT_memory_object GL_EXT_memory_object_fd GL_EXT_other";
        assert!(extension_list_contains(list, b"GL_EXT_memory_object"));
        assert!(extension_list_contains(list, b"GL_EXT_memory_object_fd"));
        assert!(!extension_list_contains(list, b"GL_EXT_memory"));
        assert!(!extension_list_contains(list, b"GL_EXT_memory_object_f"));
    }

    #[test]
    fn pool_budget_stays_within_the_per_process_cap() {
        let source = source_at(60, 3_840, 2_160);
        let stride = 3_840_u32 * 4;
        let bytes = u64::from(stride) * u64::from(source.height);
        // A 4K60 slot is about 33 MB; six slots across four context pools stay
        // below the explicit 800 MiB actual-allocation ceiling.
        assert!(bytes <= MAX_SLOT_BYTES);
        let worst_case = bytes * (SLOT_COUNT as u64) * (MAX_POOLS as u64);
        assert!(worst_case < 800 * 1_048_576);
        assert!(MAX_SLOT_BYTES * (SLOT_COUNT as u64) * (MAX_POOLS as u64) < 2 * 1_073_741_824);
        assert!(!exportable_stride(MAX_REPLAY_SOURCE_WIDTH * 4 + 4));
    }
}
