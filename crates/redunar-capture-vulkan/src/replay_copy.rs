//! Diagnostic-only bounded swapchain readback. Production Replay remains
//! disabled until encoding and sustained performance are independently proven.

#[allow(clippy::wildcard_imports)]
use crate::ffi::*;
use crate::producer;
use crate::replay_transfer;
use redunar_capture::ReplaySourceCandidate;
use std::collections::BTreeMap;
use std::env;
use std::ffi::{c_char, c_void};
use std::mem;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};

unsafe extern "C" {
    fn close(fd: i32) -> i32;
}

const DIAGNOSTIC_ENV: &str = "REDUNAR_REPLAY_DIAGNOSTIC_ADD_TRANSFER_SRC";
const DIAGNOSTIC_FAIL_SETUP_ENV: &str = "REDUNAR_REPLAY_DIAGNOSTIC_FAIL_COPY_SETUP";
const PRODUCTION_ENV: &str = "REDUNAR_REPLAY_PRODUCTION";
const EXPORT_DMABUF_ENV: &str = "REDUNAR_REPLAY_DIAGNOSTIC_EXPORT_DMABUF";
const VK_BUFFER_USAGE_STORAGE_BUFFER_BIT: u32 = 0x20;
const VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT: u32 = 0x1;
const MAX_IMAGES: usize = 8;
// The hardware encoder owns four submitted DMA-BUFs before it collects the
// first completed slot. A fifth export context is therefore required to make
// that collection call; matching the encoder depth exactly would deadlock.
const MAX_WAIT_SEMAPHORES: usize = 8;
const MAX_PRESENT_SWAPCHAINS: usize = 8;
const BYTES_PER_PIXEL: u64 = 4;
const CHECKSUM_SAMPLES: usize = 64;
const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;

const GET_SWAPCHAIN_IMAGES: &[u8] = b"vkGetSwapchainImagesKHR\0";
const CREATE_SEMAPHORE: &[u8] = b"vkCreateSemaphore\0";
const DESTROY_SEMAPHORE: &[u8] = b"vkDestroySemaphore\0";
const CREATE_FENCE: &[u8] = b"vkCreateFence\0";
const DESTROY_FENCE: &[u8] = b"vkDestroyFence\0";
const RESET_FENCES: &[u8] = b"vkResetFences\0";
const GET_FENCE_STATUS: &[u8] = b"vkGetFenceStatus\0";
const CREATE_COMMAND_POOL: &[u8] = b"vkCreateCommandPool\0";
const DESTROY_COMMAND_POOL: &[u8] = b"vkDestroyCommandPool\0";
const ALLOCATE_COMMAND_BUFFERS: &[u8] = b"vkAllocateCommandBuffers\0";
const RESET_COMMAND_BUFFER: &[u8] = b"vkResetCommandBuffer\0";
const BEGIN_COMMAND_BUFFER: &[u8] = b"vkBeginCommandBuffer\0";
const END_COMMAND_BUFFER: &[u8] = b"vkEndCommandBuffer\0";
const QUEUE_SUBMIT: &[u8] = b"vkQueueSubmit\0";
const QUEUE_WAIT_IDLE: &[u8] = b"vkQueueWaitIdle\0";
const CREATE_BUFFER: &[u8] = b"vkCreateBuffer\0";
const DESTROY_BUFFER: &[u8] = b"vkDestroyBuffer\0";
const GET_BUFFER_MEMORY_REQUIREMENTS: &[u8] = b"vkGetBufferMemoryRequirements\0";
const ALLOCATE_MEMORY: &[u8] = b"vkAllocateMemory\0";
const FREE_MEMORY: &[u8] = b"vkFreeMemory\0";
const BIND_BUFFER_MEMORY: &[u8] = b"vkBindBufferMemory\0";
const MAP_MEMORY: &[u8] = b"vkMapMemory\0";
const UNMAP_MEMORY: &[u8] = b"vkUnmapMemory\0";
const GET_MEMORY_FD_KHR: &[u8] = b"vkGetMemoryFdKHR\0";
const CMD_PIPELINE_BARRIER: &[u8] = b"vkCmdPipelineBarrier\0";
const CMD_COPY_IMAGE_TO_BUFFER: &[u8] = b"vkCmdCopyImageToBuffer\0";

#[derive(Clone, Copy)]
pub(crate) struct DeviceFunctions {
    get_swapchain_images: PfnGetSwapchainImagesKhr,
    create_semaphore: PfnCreateSemaphore,
    destroy_semaphore: PfnDestroySemaphore,
    create_fence: PfnCreateFence,
    destroy_fence: PfnDestroyFence,
    reset_fences: PfnResetFences,
    get_fence_status: PfnGetFenceStatus,
    create_command_pool: PfnCreateCommandPool,
    destroy_command_pool: PfnDestroyCommandPool,
    allocate_command_buffers: PfnAllocateCommandBuffers,
    reset_command_buffer: PfnResetCommandBuffer,
    begin_command_buffer: PfnBeginCommandBuffer,
    end_command_buffer: PfnEndCommandBuffer,
    queue_submit: PfnQueueSubmit,
    queue_wait_idle: PfnQueueWaitIdle,
    create_buffer: PfnCreateBuffer,
    destroy_buffer: PfnDestroyBuffer,
    get_buffer_memory_requirements: PfnGetBufferMemoryRequirements,
    allocate_memory: PfnAllocateMemory,
    free_memory: PfnFreeMemory,
    bind_buffer_memory: PfnBindBufferMemory,
    map_memory: PfnMapMemory,
    unmap_memory: PfnUnmapMemory,
    get_memory_fd: Option<PfnGetMemoryFdKhr>,
    cmd_pipeline_barrier: PfnCmdPipelineBarrier,
    cmd_copy_image_to_buffer: PfnCmdCopyImageToBuffer,
}

impl DeviceFunctions {
    pub(crate) unsafe fn load(next: PfnGetDeviceProcAddr, device: VkDevice) -> Option<Self> {
        macro_rules! load {
            ($name:expr, $ty:ty) => {{
                let raw = unsafe { next(device, $name.as_ptr().cast::<c_char>()) }?;
                unsafe { mem::transmute::<unsafe extern "system" fn(), $ty>(raw) }
            }};
        }
        Some(Self {
            get_swapchain_images: load!(GET_SWAPCHAIN_IMAGES, PfnGetSwapchainImagesKhr),
            create_semaphore: load!(CREATE_SEMAPHORE, PfnCreateSemaphore),
            destroy_semaphore: load!(DESTROY_SEMAPHORE, PfnDestroySemaphore),
            create_fence: load!(CREATE_FENCE, PfnCreateFence),
            destroy_fence: load!(DESTROY_FENCE, PfnDestroyFence),
            reset_fences: load!(RESET_FENCES, PfnResetFences),
            get_fence_status: load!(GET_FENCE_STATUS, PfnGetFenceStatus),
            create_command_pool: load!(CREATE_COMMAND_POOL, PfnCreateCommandPool),
            destroy_command_pool: load!(DESTROY_COMMAND_POOL, PfnDestroyCommandPool),
            allocate_command_buffers: load!(ALLOCATE_COMMAND_BUFFERS, PfnAllocateCommandBuffers),
            reset_command_buffer: load!(RESET_COMMAND_BUFFER, PfnResetCommandBuffer),
            begin_command_buffer: load!(BEGIN_COMMAND_BUFFER, PfnBeginCommandBuffer),
            end_command_buffer: load!(END_COMMAND_BUFFER, PfnEndCommandBuffer),
            queue_submit: load!(QUEUE_SUBMIT, PfnQueueSubmit),
            queue_wait_idle: load!(QUEUE_WAIT_IDLE, PfnQueueWaitIdle),
            create_buffer: load!(CREATE_BUFFER, PfnCreateBuffer),
            destroy_buffer: load!(DESTROY_BUFFER, PfnDestroyBuffer),
            get_buffer_memory_requirements: load!(
                GET_BUFFER_MEMORY_REQUIREMENTS,
                PfnGetBufferMemoryRequirements
            ),
            allocate_memory: load!(ALLOCATE_MEMORY, PfnAllocateMemory),
            free_memory: load!(FREE_MEMORY, PfnFreeMemory),
            bind_buffer_memory: load!(BIND_BUFFER_MEMORY, PfnBindBufferMemory),
            map_memory: load!(MAP_MEMORY, PfnMapMemory),
            unmap_memory: load!(UNMAP_MEMORY, PfnUnmapMemory),
            get_memory_fd: {
                let raw = unsafe { next(device, GET_MEMORY_FD_KHR.as_ptr().cast::<c_char>()) };
                raw.map(|value| unsafe {
                    mem::transmute::<unsafe extern "system" fn(), PfnGetMemoryFdKhr>(value)
                })
            },
            cmd_pipeline_barrier: load!(CMD_PIPELINE_BARRIER, PfnCmdPipelineBarrier),
            cmd_copy_image_to_buffer: load!(CMD_COPY_IMAGE_TO_BUFFER, PfnCmdCopyImageToBuffer),
        })
    }
}

#[derive(Default)]
struct CopyContext {
    command_buffer_address: usize,
    semaphore: VkSemaphore,
    fence: VkFence,
    buffer: VkBuffer,
    memory: VkDeviceMemory,
    mapped_address: usize,
    captured_at_ns: u64,
    duration_ns: u64,
    pending: bool,
    usable: bool,
    export_fd: i32,
    export_sequence: u64,
    export_released: bool,
}

struct Route {
    device_key: usize,
    device_address: usize,
    queue_address: usize,
    functions: DeviceFunctions,
    memory_properties: VkPhysicalDeviceMemoryProperties,
    swapchain: VkSwapchainKhr,
    images: [VkImage; MAX_IMAGES],
    image_count: usize,
    width: u32,
    height: u32,
    candidate: ReplaySourceCandidate,
    copied_bytes: u32,
    frame_interval_ns: u64,
    next_submit_ns: u64,
    command_pool: VkCommandPool,
    contexts: Vec<CopyContext>,
}

impl Route {
    fn device(&self) -> VkDevice {
        self.device_address as VkDevice
    }

    fn queue(&self) -> VkQueue {
        self.queue_address as VkQueue
    }
}

#[derive(Default)]
struct State {
    devices: BTreeMap<usize, PendingDevice>,
    route: Option<Route>,
    pending_swapchain: Option<PendingSwapchain>,
    retired_routes: [Option<Route>; 1],
    abandoned_route: Option<Route>,
    disabled: bool,
    /// Highest presentation timestamp already exported to the daemon. This
    /// is process-wide because the daemon keeps one codec epoch across a
    /// swapchain route replacement.
    last_export_timestamp_ns: u64,
}

#[derive(Clone, Copy)]
struct PendingDevice {
    device_address: usize,
    functions: DeviceFunctions,
    memory_properties: VkPhysicalDeviceMemoryProperties,
    graphics_queue_families: u64,
    queue_address: usize,
    queue_family_index: u32,
    external_memory_supported: bool,
}

#[derive(Clone, Copy)]
struct PendingSwapchain {
    device_key: usize,
    replaces: VkSwapchainKhr,
    swapchain: VkSwapchainKhr,
    candidate: ReplaySourceCandidate,
    copied_bytes: u32,
}

#[derive(Clone, Copy)]
struct CopyProof {
    source: ReplaySourceCandidate,
    copied_bytes: u32,
    sample_checksum: u64,
    export_fd: i32,
    export_sequence: Option<u64>,
    timestamp_ns: u64,
    duration_ns: u64,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));
static NEXT_REPLAY_EXPORT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn requested() -> bool {
    env::var("REDUNAR_REPLAY_TRANSFER").ok().as_deref() == Some("1")
        && env::var(DIAGNOSTIC_ENV).ok().as_deref() == Some("1")
}

fn production_requested() -> bool {
    env::var("REDUNAR_REPLAY_TRANSFER").ok().as_deref() == Some("1")
        && env::var(PRODUCTION_ENV).ok().as_deref() == Some("1")
}

pub(crate) fn external_memory_export_requested() -> bool {
    env::var(EXPORT_DMABUF_ENV).ok().as_deref() == Some("1")
        || env::var(PRODUCTION_ENV).ok().as_deref() == Some("1")
}

fn frame_interval_for_fps(frames_per_second: u8) -> u64 {
    NANOSECONDS_PER_SECOND / u64::from(frames_per_second)
}

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

fn capture_presentation_timestamp(current_deadline_ns: u64, now_ns: u64) -> u64 {
    if current_deadline_ns == 0 {
        now_ns
    } else {
        current_deadline_ns
    }
}

/// Keep exported presentation times strictly increasing across route
/// replacement. Within one route the cadence deadline already rises, so this
/// only fires when a replacement route resets its deadline bookkeeping while
/// the newest timestamp already stamped is still ahead of wall time (a stale
/// future deadline from the retired route). The replay pipeline treats a
/// non-monotonic timestamp as a terminal codec-epoch violation, so one
/// interleaved stale stamp must never be exported.
fn monotonic_export_timestamp(previous_ns: u64, candidate_ns: u64) -> u64 {
    if candidate_ns > previous_ns {
        candidate_ns
    } else {
        previous_ns.saturating_add(1)
    }
}

pub(crate) fn device_created(
    device_key: usize,
    device: VkDevice,
    functions: Option<DeviceFunctions>,
    memory_properties: &VkPhysicalDeviceMemoryProperties,
    graphics_queue_families: u64,
    external_memory_supported: bool,
) {
    let validation = env::var(EXPORT_DMABUF_ENV).ok().as_deref() == Some("1");
    if (!requested() && !production_requested()) || graphics_queue_families == 0 {
        if validation {
            eprintln!(
                "Redunar Replay validation: device route skipped (requested={}, graphics_mask={:#x})",
                requested(),
                graphics_queue_families
            );
        }
        return;
    }
    let Some(functions) = functions else {
        if validation {
            eprintln!("Redunar Replay validation: required device commands are unavailable");
        }
        return;
    };
    let mut state = lock_state();
    if state.disabled {
        return;
    }
    if state.devices.len() >= 2 && !state.devices.contains_key(&device_key) {
        return;
    }
    state.devices.insert(
        device_key,
        PendingDevice {
            device_address: device.addr(),
            functions,
            memory_properties: *memory_properties,
            graphics_queue_families,
            queue_address: 0,
            queue_family_index: 0,
            external_memory_supported,
        },
    );
}

pub(crate) fn release_sequence(sequence: u64) {
    let Ok(mut state) = STATE.try_lock() else {
        return;
    };
    if let Some(route) = state.route.as_mut() {
        release_route_sequence(route, sequence);
    }
    for route in state.retired_routes.iter_mut().flatten() {
        release_route_sequence(route, sequence);
    }
    if let Some(route) = state.abandoned_route.as_mut() {
        release_route_sequence(route, sequence);
    }
}

fn release_route_sequence(route: &mut Route, sequence: u64) {
    for context in &mut route.contexts {
        if context.export_sequence == sequence {
            context.export_released = true;
        }
    }
}

pub(crate) fn reset_export_sequence() {
    NEXT_REPLAY_EXPORT_SEQUENCE.store(1, Ordering::Relaxed);
}

pub(crate) fn queue_observed(
    device_key: usize,
    queue: VkQueue,
    family_index: u32,
    protected: bool,
) {
    let validation = env::var(EXPORT_DMABUF_ENV).ok().as_deref() == Some("1");
    if queue.is_null() || protected || family_index >= 64 {
        if validation {
            eprintln!("Redunar Replay validation: observed queue is unusable");
        }
        return;
    }
    let mut state = lock_state();
    let Some(device) = state.devices.get_mut(&device_key) else {
        if validation {
            eprintln!("Redunar Replay validation: observed queue has no registered device");
        }
        return;
    };
    if device.queue_address == 0 && device.graphics_queue_families & (1_u64 << family_index) != 0 {
        device.queue_address = queue.addr();
        device.queue_family_index = family_index;
    }
    let pending = state
        .pending_swapchain
        .filter(|pending| pending.device_key == device_key && pending.replaces == 0);
    if state.route.is_none()
        && let Some(pending) = pending
        && let Some(device) = state.devices.get(&device_key)
        && device.queue_address != 0
        && let Some(route) = unsafe {
            create_route(
                device_key,
                device,
                pending.swapchain,
                pending.candidate,
                pending.copied_bytes,
            )
        }
    {
        state.pending_swapchain = None;
        state.route = Some(route);
    }
}

pub(crate) unsafe fn swapchain_created(
    device_key: usize,
    create_info: *const VkSwapchainCreateInfoKhr,
    swapchain: VkSwapchainKhr,
) {
    let validation = env::var(EXPORT_DMABUF_ENV).ok().as_deref() == Some("1");
    if create_info.is_null() || swapchain == 0 {
        return;
    }
    let info = unsafe { &*create_info };
    let candidate = match unsafe { replay_transfer::assessment_from_create_info(create_info) } {
        Some(Ok(candidate)) => candidate,
        Some(Err(rejection)) => {
            if validation {
                eprintln!(
                    "Redunar Replay validation: rejected swapchain format={} color_space={} reason={rejection:?}",
                    info.image_format, info.image_color_space
                );
            }
            return;
        }
        None => return,
    };
    let mut state = lock_state();
    let copied_bytes = u64::from(candidate.width)
        .checked_mul(u64::from(candidate.height))
        .and_then(|pixels| pixels.checked_mul(BYTES_PER_PIXEL))
        .and_then(|bytes| u32::try_from(bytes).ok());
    let Some(copied_bytes) = copied_bytes else {
        return;
    };
    if env::var(PRODUCTION_ENV).ok().as_deref() == Some("1")
        && state
            .devices
            .get(&device_key)
            .is_some_and(|device| !device.external_memory_supported)
    {
        producer::record_replay_source_rejected(
            redunar_capture::ReplaySourceRejection::ExternalMemoryUnsupported,
        );
        return;
    }
    if env::var(DIAGNOSTIC_FAIL_SETUP_ENV).ok().as_deref() == Some("1") {
        return;
    }
    if let Some(route) = state.route.as_ref() {
        let replaces_active = route.device_key == device_key
            && (info.old_swapchain == route.swapchain
                || state
                    .pending_swapchain
                    .is_some_and(|pending| info.old_swapchain == pending.swapchain));
        if replaces_active {
            state.pending_swapchain = Some(PendingSwapchain {
                device_key,
                replaces: route.swapchain,
                swapchain,
                candidate,
                copied_bytes,
            });
        }
        return;
    }
    let Some(device) = state.devices.get(&device_key).copied() else {
        if validation {
            eprintln!("Redunar Replay validation: swapchain has no registered copy device");
        }
        return;
    };
    if device.queue_address == 0 {
        state.pending_swapchain = Some(PendingSwapchain {
            device_key,
            replaces: 0,
            swapchain,
            candidate,
            copied_bytes,
        });
        return;
    }
    let Some(route) =
        (unsafe { create_route(device_key, &device, swapchain, candidate, copied_bytes) })
    else {
        return;
    };
    state.route = Some(route);
}

pub(crate) fn swapchain_destroyed(device_key: usize, swapchain: VkSwapchainKhr) {
    let mut state = lock_state();
    if state
        .pending_swapchain
        .is_some_and(|pending| pending.swapchain == swapchain)
    {
        state.pending_swapchain = None;
    }
    if state
        .route
        .as_ref()
        .is_some_and(|route| route.device_key == device_key && route.swapchain == swapchain)
    {
        let route = state.route.take().expect("matched route exists");
        retain_retired_route(&mut state, route);
        if !state.disabled
            && let Some(pending) = state
                .pending_swapchain
                .take()
                .filter(|pending| pending.device_key == device_key && pending.replaces == swapchain)
            && let Some(device) = state.devices.get(&device_key)
            && let Some(route) = unsafe {
                create_route(
                    device_key,
                    device,
                    pending.swapchain,
                    pending.candidate,
                    pending.copied_bytes,
                )
            }
        {
            state.route = Some(route);
        }
    }
}

pub(crate) fn device_destroyed(device_key: usize) {
    let mut state = lock_state();
    if state
        .route
        .as_ref()
        .is_some_and(|route| route.device_key == device_key)
    {
        let route = state.route.take().expect("matched route exists");
        // Vulkan requires device queues to be idle before vkDestroyDevice.
        // This is the one teardown point where every remaining Redunar child
        // can therefore be destroyed explicitly without adding a wait.
        unsafe { destroy_route(route) };
    }
    for slot in &mut state.retired_routes {
        if slot
            .as_ref()
            .is_some_and(|route| route.device_key == device_key)
        {
            let route = slot.take().expect("matched retired route exists");
            unsafe { destroy_route(route) };
        }
    }
    if state
        .abandoned_route
        .as_ref()
        .is_some_and(|route| route.device_key == device_key)
    {
        let route = state
            .abandoned_route
            .take()
            .expect("matched abandoned route exists");
        unsafe { destroy_route(route) };
    }
    state.devices.remove(&device_key);
    if state
        .pending_swapchain
        .is_some_and(|pending| pending.device_key == device_key)
    {
        state.pending_swapchain = None;
    }
    if state.devices.is_empty() {
        state.disabled = false;
    }
}

pub(crate) unsafe fn prepare_present(
    queue: VkQueue,
    present_info: *const VkPresentInfoKhr,
) -> Option<VkSemaphore> {
    if queue.is_null() || present_info.is_null() {
        return None;
    }
    let Ok(mut state) = STATE.try_lock() else {
        return None;
    };
    if state.disabled {
        return None;
    }
    let route = state.route.as_mut()?;
    if route.queue_address != queue.addr() {
        return None;
    }
    let now_ns = producer::monotonic_ns()?;
    if route.next_submit_ns != 0 && now_ns < route.next_submit_ns {
        return None;
    }
    let present = unsafe { &*present_info };
    if present.s_type != VK_STRUCTURE_TYPE_PRESENT_INFO_KHR
        || present.swapchain_count == 0
        || usize::try_from(present.swapchain_count).ok()? > MAX_PRESENT_SWAPCHAINS
        || present.swapchains.is_null()
        || present.image_indices.is_null()
        || usize::try_from(present.wait_semaphore_count).ok()? > MAX_WAIT_SEMAPHORES
        || (present.wait_semaphore_count > 0 && present.wait_semaphores.is_null())
    {
        return None;
    }
    let image_index = unsafe { presented_image_index(present, route.swapchain) }?;
    let image = *route.images.get(usize::try_from(image_index).ok()?)?;
    if image == 0 || usize::try_from(image_index).ok()? >= route.image_count {
        return None;
    }
    // Use the cadence deadline as the encoded presentation timestamp. Source
    // presents need not divide evenly into 30/60/120 FPS, and stamping the
    // selected frame with wall time would create variable-frame-rate judder.
    let presentation_timestamp_ns = capture_presentation_timestamp(route.next_submit_ns, now_ns);
    let (semaphore, proof) = unsafe {
        submit_copy(
            route,
            present,
            image,
            presentation_timestamp_ns,
            route.frame_interval_ns,
        )
    }?;
    if semaphore != 0 {
        route.next_submit_ns =
            next_capture_deadline(route.next_submit_ns, now_ns, route.frame_interval_ns);
    }
    let exported_timestamp_ns = proof.map(|proof| {
        let timestamp_ns =
            monotonic_export_timestamp(state.last_export_timestamp_ns, proof.timestamp_ns);
        state.last_export_timestamp_ns = timestamp_ns;
        (proof, timestamp_ns)
    });
    drop(state);
    if let Some((proof, timestamp_ns)) = exported_timestamp_ns {
        producer::record_replay_frame_copied(
            proof.source,
            proof.copied_bytes,
            proof.sample_checksum,
        );
        if proof.export_fd >= 3 {
            producer::record_replay_frame_exported(
                proof.export_sequence.unwrap_or(0),
                u32::try_from(proof.export_fd).unwrap_or(0),
                proof.source,
                0,
                proof.source.width.saturating_mul(4),
                0,
                timestamp_ns,
                proof.duration_ns,
            );
        }
    }
    (semaphore != 0).then_some(semaphore)
}

unsafe fn presented_image_index(present: &VkPresentInfoKhr, target: VkSwapchainKhr) -> Option<u32> {
    for index in 0..usize::try_from(present.swapchain_count).ok()? {
        // SAFETY: `prepare_present` validated both arrays and their bounded
        // Vulkan-specified length before calling this helper.
        if unsafe { *present.swapchains.add(index) } == target {
            // SAFETY: `pImageIndices` has the same swapchain-count length.
            return Some(unsafe { *present.image_indices.add(index) });
        }
    }
    None
}

pub(crate) fn presentation_failed(semaphore: VkSemaphore) {
    let Ok(mut state) = STATE.try_lock() else {
        return;
    };
    let Some(route) = state.route.as_mut() else {
        return;
    };
    if let Some(context) = route
        .contexts
        .iter_mut()
        .find(|value| value.semaphore == semaphore)
    {
        context.usable = false;
    }
}

fn retain_retired_route(state: &mut State, route: Route) {
    // A copy fence alone cannot prove presentation consumed the signaled binary
    // semaphore. Keep the complete resource set until a later synchronized
    // presentation can prove the queue idle, or until vkDestroyDevice.
    if let Some(slot) = state.retired_routes.iter_mut().find(|slot| slot.is_none()) {
        *slot = Some(route);
    } else {
        // Never free potentially in-use Vulkan resources. Pause capture until
        // a synchronized presentation reclaims both retained allocation sets.
        state.disabled = true;
        state.abandoned_route = Some(route);
        eprintln!("Redunar Replay: capture route retirement is waiting for the presentation queue");
    }
}

/// Reclaim swapchain-specific capture resources at a point where the caller
/// already externally synchronizes the presentation queue. `vkQueueWaitIdle`
/// completes the queued semaphore waits before any Redunar semaphore is
/// destroyed. Doing this after the first successful present on a replacement
/// swapchain prevents normal menu/fullscreen transitions from exhausting the
/// single bounded retired-route slot.
pub(crate) unsafe fn presentation_finished(queue: VkQueue) {
    if queue.is_null() {
        return;
    }
    let queue_address = queue.addr();
    let mut state = lock_state();
    let wait_idle = state
        .retired_routes
        .iter()
        .flatten()
        .chain(state.abandoned_route.iter())
        .find(|route| presentation_queue_can_reclaim(queue_address, route.queue_address))
        .map(|route| route.functions.queue_wait_idle);
    let Some(wait_idle) = wait_idle else {
        return;
    };
    // SAFETY: this hook runs before returning from vkQueuePresentKHR, while
    // the application still owns the queue's required external synchronization.
    if unsafe { wait_idle(queue) } != VK_SUCCESS {
        return;
    }

    for slot in &mut state.retired_routes {
        if slot
            .as_ref()
            .is_some_and(|route| presentation_queue_can_reclaim(queue_address, route.queue_address))
        {
            let route = slot.take().expect("matched retired route exists");
            // SAFETY: queue idle proves every submission and presentation wait
            // referring to this route's binary semaphores has completed.
            unsafe { destroy_route(route) };
        }
    }
    if state
        .abandoned_route
        .as_ref()
        .is_some_and(|route| presentation_queue_can_reclaim(queue_address, route.queue_address))
    {
        let route = state
            .abandoned_route
            .take()
            .expect("matched abandoned route exists");
        // SAFETY: guarded by the same queue-idle proof above.
        unsafe { destroy_route(route) };
    }

    state.disabled =
        state.retired_routes.iter().any(Option::is_some) || state.abandoned_route.is_some();
    if state.disabled || state.route.is_some() {
        return;
    }
    let pending_matches_queue = state.pending_swapchain.is_some_and(|pending| {
        state
            .devices
            .get(&pending.device_key)
            .is_some_and(|device| device.queue_address == queue_address)
    });
    if !pending_matches_queue {
        return;
    }
    let pending = state
        .pending_swapchain
        .take()
        .expect("matched pending swapchain exists");
    let Some(device) = state.devices.get(&pending.device_key) else {
        return;
    };
    // SAFETY: the replacement swapchain and device remain live; route creation
    // uses only the stored, bounded assessment made at vkCreateSwapchainKHR.
    state.route = unsafe {
        create_route(
            pending.device_key,
            device,
            pending.swapchain,
            pending.candidate,
            pending.copied_bytes,
        )
    };
    if state.route.is_some() {
        eprintln!("Redunar Replay: capture resumed after swapchain recreation");
    }
}

const fn presentation_queue_can_reclaim(
    presented_queue_address: usize,
    route_queue_address: usize,
) -> bool {
    presented_queue_address != 0 && presented_queue_address == route_queue_address
}

unsafe fn create_route(
    device_key: usize,
    pending: &PendingDevice,
    swapchain: VkSwapchainKhr,
    candidate: ReplaySourceCandidate,
    copied_bytes: u32,
) -> Option<Route> {
    let export_requested = external_memory_export_requested();
    if export_requested {
        eprintln!("Redunar Replay validation: creating exportable copy route");
    }
    let device = pending.device_address as VkDevice;
    let functions = pending.functions;
    let mut image_count = 0;
    let result = unsafe {
        (functions.get_swapchain_images)(
            device,
            swapchain,
            &raw mut image_count,
            std::ptr::null_mut(),
        )
    };
    if result != VK_SUCCESS {
        validation_setup_failure(export_requested, "vkGetSwapchainImagesKHR(count)", result);
        return None;
    }
    let context_count = context_count_for_swapchain(image_count)?;
    let mut images = [0; MAX_IMAGES];
    let mut supplied = image_count;
    let result = unsafe {
        (functions.get_swapchain_images)(device, swapchain, &raw mut supplied, images.as_mut_ptr())
    };
    if result != VK_SUCCESS || supplied != image_count {
        validation_setup_failure(export_requested, "vkGetSwapchainImagesKHR(images)", result);
        return None;
    }
    let pool_info = VkCommandPoolCreateInfo {
        s_type: VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT,
        queue_family_index: pending.queue_family_index,
    };
    let mut route = Route {
        device_key,
        device_address: pending.device_address,
        queue_address: pending.queue_address,
        functions,
        memory_properties: pending.memory_properties,
        swapchain,
        images,
        image_count: usize::try_from(image_count).ok()?,
        width: candidate.width,
        height: candidate.height,
        candidate,
        copied_bytes,
        frame_interval_ns: frame_interval_for_fps(candidate.target_frames_per_second),
        next_submit_ns: 0,
        command_pool: 0,
        // A binary semaphore is reused only after the same swapchain image is
        // reacquired. Keep one bounded staging context per image so that this
        // safety rule cannot starve capture on four-or-more-image swapchains.
        contexts: (0..context_count).map(|_| CopyContext::default()).collect(),
    };
    let result = unsafe {
        (functions.create_command_pool)(
            device,
            &raw const pool_info,
            std::ptr::null(),
            &raw mut route.command_pool,
        )
    };
    if result != VK_SUCCESS {
        validation_setup_failure(export_requested, "vkCreateCommandPool", result);
        return None;
    }
    if !unsafe { initialize_route_contexts(&mut route) } {
        unsafe { destroy_route(route) };
        return None;
    }
    Some(route)
}

unsafe fn initialize_route_contexts(route: &mut Route) -> bool {
    let device = route.device();
    let allocate = VkCommandBufferAllocateInfo {
        s_type: VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        p_next: std::ptr::null(),
        command_pool: route.command_pool,
        level: VK_COMMAND_BUFFER_LEVEL_PRIMARY,
        command_buffer_count: u32::try_from(route.contexts.len()).unwrap_or(0),
    };
    let mut command_buffers = vec![std::ptr::null_mut(); route.contexts.len()];
    let result = unsafe {
        (route.functions.allocate_command_buffers)(
            device,
            &raw const allocate,
            command_buffers.as_mut_ptr(),
        )
    };
    if result != VK_SUCCESS {
        validation_setup_failure(
            external_memory_export_requested(),
            "vkAllocateCommandBuffers",
            result,
        );
        return false;
    }
    for (context, command_buffer) in route.contexts.iter_mut().zip(command_buffers) {
        context.command_buffer_address = command_buffer.addr();
        if unsafe {
            !create_context(
                device,
                route.functions,
                &route.memory_properties,
                route.copied_bytes,
                context,
            )
        } {
            return false;
        }
    }
    true
}

fn context_count_for_swapchain(image_count: u32) -> Option<usize> {
    usize::try_from(image_count)
        .ok()
        .filter(|count| (1..=MAX_IMAGES).contains(count))
        // Keep one context beyond the four-slot encoder depth even on common
        // two/three-image swapchains. The fifth submission collects slot zero
        // and returns its export before this bounded set can fill again.
        .map(|count| {
            count
                .saturating_add(1)
                .clamp(redunar_capture::REPLAY_PRODUCER_CONTEXT_COUNT, MAX_IMAGES)
        })
}

#[expect(
    clippy::too_many_lines,
    reason = "one transactional Vulkan allocation scope keeps rollback auditable"
)]
unsafe fn create_context(
    device: VkDevice,
    functions: DeviceFunctions,
    memory_properties: &VkPhysicalDeviceMemoryProperties,
    copied_bytes: u32,
    context: &mut CopyContext,
) -> bool {
    context.export_fd = -1;
    let export_requested = external_memory_export_requested();
    let external_buffer = VkExternalMemoryBufferCreateInfo {
        s_type: VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_BUFFER_CREATE_INFO,
        p_next: std::ptr::null(),
        handle_types: VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT,
    };
    let semaphore_info = VkSemaphoreCreateInfo {
        s_type: VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
    };
    let fence_info = VkFenceCreateInfo {
        s_type: VK_STRUCTURE_TYPE_FENCE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: VK_FENCE_CREATE_SIGNALED_BIT,
    };
    let buffer_info = VkBufferCreateInfo {
        s_type: VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
        p_next: if export_requested {
            std::ptr::from_ref(&external_buffer).cast()
        } else {
            std::ptr::null()
        },
        flags: 0,
        size: u64::from(copied_bytes),
        usage: VK_BUFFER_USAGE_TRANSFER_DST_BIT | VK_BUFFER_USAGE_STORAGE_BUFFER_BIT,
        sharing_mode: VK_SHARING_MODE_EXCLUSIVE,
        queue_family_index_count: 0,
        queue_family_indices: std::ptr::null(),
    };
    let result = unsafe {
        (functions.create_semaphore)(
            device,
            &raw const semaphore_info,
            std::ptr::null(),
            &raw mut context.semaphore,
        )
    };
    if result != VK_SUCCESS {
        validation_setup_failure(export_requested, "vkCreateSemaphore", result);
        return false;
    }
    let result = unsafe {
        (functions.create_fence)(
            device,
            &raw const fence_info,
            std::ptr::null(),
            &raw mut context.fence,
        )
    };
    if result != VK_SUCCESS {
        validation_setup_failure(export_requested, "vkCreateFence", result);
        return false;
    }
    let result = unsafe {
        (functions.create_buffer)(
            device,
            &raw const buffer_info,
            std::ptr::null(),
            &raw mut context.buffer,
        )
    };
    if result != VK_SUCCESS {
        validation_setup_failure(export_requested, "vkCreateBuffer", result);
        return false;
    }
    let mut requirements = VkMemoryRequirements {
        size: 0,
        alignment: 0,
        memory_type_bits: 0,
    };
    unsafe {
        (functions.get_buffer_memory_requirements)(device, context.buffer, &raw mut requirements);
    };
    let memory_type_index = if export_requested {
        export_memory_type(memory_properties, requirements.memory_type_bits)
    } else {
        coherent_memory_type(memory_properties, requirements.memory_type_bits)
    };
    let Some(memory_type_index) = memory_type_index else {
        if export_requested {
            eprintln!(
                "Redunar Replay validation: no compatible export memory type (mask={:#x})",
                requirements.memory_type_bits
            );
        }
        return false;
    };
    let export_memory = VkExportMemoryAllocateInfo {
        s_type: VK_STRUCTURE_TYPE_EXPORT_MEMORY_ALLOCATE_INFO,
        p_next: std::ptr::null(),
        handle_types: VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT,
    };
    let allocate = VkMemoryAllocateInfo {
        s_type: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        p_next: if export_requested {
            std::ptr::from_ref(&export_memory).cast()
        } else {
            std::ptr::null()
        },
        allocation_size: requirements.size,
        memory_type_index,
    };
    let result = unsafe {
        (functions.allocate_memory)(
            device,
            &raw const allocate,
            std::ptr::null(),
            &raw mut context.memory,
        )
    };
    if result != VK_SUCCESS {
        validation_setup_failure(export_requested, "vkAllocateMemory", result);
        return false;
    }
    let result =
        unsafe { (functions.bind_buffer_memory)(device, context.buffer, context.memory, 0) };
    if result != VK_SUCCESS {
        validation_setup_failure(export_requested, "vkBindBufferMemory", result);
        return false;
    }
    if export_requested {
        let Some(get_memory_fd) = functions.get_memory_fd else {
            eprintln!("Redunar Replay validation: vkGetMemoryFdKHR is unavailable");
            return false;
        };
        let info = VkMemoryGetFdInfoKhr {
            s_type: VK_STRUCTURE_TYPE_MEMORY_GET_FD_INFO_KHR,
            p_next: std::ptr::null(),
            memory: context.memory,
            handle_type: VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT,
        };
        let result = unsafe { get_memory_fd(device, &raw const info, &raw mut context.export_fd) };
        if result != VK_SUCCESS || context.export_fd < 0 {
            validation_setup_failure(export_requested, "vkGetMemoryFdKHR", result);
            return false;
        }
        context.export_released = true;
        // The production/encode path never maps or reads pixels on the CPU.
        // Completion is established by the copy fence and the exported memory
        // remains device-local when the driver offers that placement.
        context.usable = true;
        return true;
    }
    let mut mapped = std::ptr::null_mut::<c_void>();
    if unsafe {
        (functions.map_memory)(
            device,
            context.memory,
            0,
            u64::from(copied_bytes),
            0,
            &raw mut mapped,
        )
    } != VK_SUCCESS
        || mapped.is_null()
    {
        return false;
    }
    context.mapped_address = mapped.addr();
    context.usable = true;
    true
}

fn validation_setup_failure(enabled: bool, operation: &str, result: i32) {
    if enabled {
        eprintln!("Redunar Replay validation: {operation} failed with VkResult {result}");
    }
}

fn coherent_memory_type(
    properties: &VkPhysicalDeviceMemoryProperties,
    allowed: u32,
) -> Option<u32> {
    let required = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
    properties
        .memory_types
        .iter()
        .take(usize::try_from(properties.memory_type_count).ok()?.min(32))
        .enumerate()
        .find(|(index, value)| {
            allowed & (1_u32 << index) != 0 && value.property_flags & required == required
        })
        .and_then(|(index, _)| u32::try_from(index).ok())
}

fn export_memory_type(properties: &VkPhysicalDeviceMemoryProperties, allowed: u32) -> Option<u32> {
    let count = usize::try_from(properties.memory_type_count).ok()?.min(32);
    properties
        .memory_types
        .iter()
        .take(count)
        .enumerate()
        .filter(|(index, _)| allowed & (1_u32 << index) != 0)
        .max_by_key(|(_, value)| {
            u32::from(value.property_flags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT != 0)
        })
        .and_then(|(index, _)| u32::try_from(index).ok())
}

unsafe fn submit_copy(
    route: &mut Route,
    present: &VkPresentInfoKhr,
    image: VkImage,
    timestamp_ns: u64,
    duration_ns: u64,
) -> Option<(VkSemaphore, Option<CopyProof>)> {
    let device = route.device();
    let queue = route.queue();
    let functions = route.functions;
    let proof = unsafe { take_completed_copy(route) };
    let context = route.contexts.iter_mut().find(|context| {
        context.usable
            && !context.pending
            && (context.export_fd < 3 || context.export_released)
            && unsafe { (functions.get_fence_status)(device, context.fence) } == VK_SUCCESS
    });
    let Some(context) = context else {
        return proof.map(|proof| (0, Some(proof)));
    };
    if unsafe { (functions.reset_fences)(device, 1, &raw const context.fence) } != VK_SUCCESS {
        context.usable = false;
        return None;
    }
    let command_buffer = context.command_buffer_address as VkCommandBuffer;
    if unsafe { (functions.reset_command_buffer)(command_buffer, 0) } != VK_SUCCESS {
        context.usable = false;
        return None;
    }
    let begin = VkCommandBufferBeginInfo {
        s_type: VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
        p_next: std::ptr::null(),
        flags: VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT,
        inheritance_info: std::ptr::null(),
    };
    if unsafe { (functions.begin_command_buffer)(command_buffer, &raw const begin) } != VK_SUCCESS {
        context.usable = false;
        return None;
    }
    unsafe {
        record_copy(
            functions,
            command_buffer,
            image,
            context.buffer,
            route.width,
            route.height,
        );
    };
    if unsafe { (functions.end_command_buffer)(command_buffer) } != VK_SUCCESS {
        context.usable = false;
        return None;
    }
    let wait_stages = [VK_PIPELINE_STAGE_TRANSFER_BIT; MAX_WAIT_SEMAPHORES];
    let submit = VkSubmitInfo {
        s_type: VK_STRUCTURE_TYPE_SUBMIT_INFO,
        p_next: std::ptr::null(),
        wait_semaphore_count: present.wait_semaphore_count,
        wait_semaphores: present.wait_semaphores,
        wait_dst_stage_mask: if present.wait_semaphore_count == 0 {
            std::ptr::null()
        } else {
            wait_stages.as_ptr()
        },
        command_buffer_count: 1,
        command_buffers: &raw const command_buffer,
        signal_semaphore_count: 1,
        signal_semaphores: &raw const context.semaphore,
    };
    if unsafe { (functions.queue_submit)(queue, 1, &raw const submit, context.fence) } != VK_SUCCESS
    {
        context.usable = false;
        return None;
    }
    context.captured_at_ns = timestamp_ns;
    context.duration_ns = duration_ns;
    context.pending = true;
    Some((context.semaphore, proof))
}

unsafe fn take_completed_copy(route: &mut Route) -> Option<CopyProof> {
    let device = route.device();
    let functions = route.functions;
    // Contexts are reused independently, so vector order is not capture order.
    // Never let a newer completed copy overtake the oldest pending copy: the
    // replay encoder requires strictly monotonic source timestamps.
    let context_index = oldest_pending_context_index(&route.contexts)?;
    let context = &mut route.contexts[context_index];
    if unsafe { (functions.get_fence_status)(device, context.fence) } != VK_SUCCESS {
        return None;
    }
    let checksum = if context.export_fd >= 3 {
        0
    } else {
        unsafe { sampled_checksum(context.mapped_address, route.copied_bytes) }
    };
    context.pending = false;
    let export_sequence = if context.export_fd >= 3 {
        context.export_released = false;
        let sequence = NEXT_REPLAY_EXPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        context.export_sequence = sequence;
        Some(sequence)
    } else {
        None
    };
    Some(CopyProof {
        source: route.candidate,
        copied_bytes: route.copied_bytes,
        sample_checksum: checksum,
        export_fd: context.export_fd,
        export_sequence,
        timestamp_ns: context.captured_at_ns,
        duration_ns: context.duration_ns,
    })
}

fn oldest_pending_context_index(contexts: &[CopyContext]) -> Option<usize> {
    contexts
        .iter()
        .enumerate()
        .filter(|(_, context)| context.usable && context.pending)
        .min_by_key(|(index, context)| (context.captured_at_ns, *index))
        .map(|(index, _)| index)
}

unsafe fn record_copy(
    functions: DeviceFunctions,
    command_buffer: VkCommandBuffer,
    image: VkImage,
    buffer: VkBuffer,
    width: u32,
    height: u32,
) {
    let range = VkImageSubresourceRange {
        aspect_mask: VK_IMAGE_ASPECT_COLOR_BIT,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    };
    let to_transfer = VkImageMemoryBarrier {
        s_type: VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
        p_next: std::ptr::null(),
        src_access_mask: 0,
        dst_access_mask: VK_ACCESS_TRANSFER_READ_BIT,
        old_layout: VK_IMAGE_LAYOUT_PRESENT_SRC_KHR,
        new_layout: VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        src_queue_family_index: VK_QUEUE_FAMILY_IGNORED,
        dst_queue_family_index: VK_QUEUE_FAMILY_IGNORED,
        image,
        subresource_range: range,
    };
    unsafe {
        (functions.cmd_pipeline_barrier)(
            command_buffer,
            VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT,
            VK_PIPELINE_STAGE_TRANSFER_BIT,
            0,
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            1,
            &raw const to_transfer,
        );
    };
    let region = VkBufferImageCopy {
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image_subresource: VkImageSubresourceLayers {
            aspect_mask: VK_IMAGE_ASPECT_COLOR_BIT,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        },
        image_offset: VkOffset3d { x: 0, y: 0, z: 0 },
        image_extent: VkExtent3d {
            width,
            height,
            depth: 1,
        },
    };
    unsafe {
        (functions.cmd_copy_image_to_buffer)(
            command_buffer,
            image,
            VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
            buffer,
            1,
            &raw const region,
        );
    };
    let to_present = VkImageMemoryBarrier {
        src_access_mask: VK_ACCESS_TRANSFER_READ_BIT,
        dst_access_mask: 0,
        old_layout: VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        new_layout: VK_IMAGE_LAYOUT_PRESENT_SRC_KHR,
        ..to_transfer
    };
    let host_visibility = VkMemoryBarrier {
        s_type: VK_STRUCTURE_TYPE_MEMORY_BARRIER,
        p_next: std::ptr::null(),
        src_access_mask: VK_ACCESS_TRANSFER_WRITE_BIT,
        dst_access_mask: VK_ACCESS_HOST_READ_BIT,
    };
    unsafe {
        (functions.cmd_pipeline_barrier)(
            command_buffer,
            VK_PIPELINE_STAGE_TRANSFER_BIT,
            VK_PIPELINE_STAGE_HOST_BIT | VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT,
            0,
            1,
            std::ptr::from_ref(&host_visibility).cast(),
            0,
            std::ptr::null(),
            1,
            &raw const to_present,
        );
    };
}

unsafe fn sampled_checksum(address: usize, copied_bytes: u32) -> u64 {
    let length = usize::try_from(copied_bytes).unwrap_or(0);
    if address == 0 || length == 0 {
        return 0;
    }
    let bytes = unsafe { std::slice::from_raw_parts(address as *const u8, length) };
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for sample in 0..CHECKSUM_SAMPLES.min(length) {
        let index = sample.saturating_mul(length.saturating_sub(1)) / CHECKSUM_SAMPLES.max(1);
        hash ^= u64::from(bytes[index]);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

unsafe fn destroy_route(mut route: Route) {
    let device = route.device();
    for context in &mut route.contexts {
        if context.export_fd >= 0 {
            unsafe { close(context.export_fd) };
            context.export_fd = -1;
        }
        if context.mapped_address != 0 && context.memory != 0 {
            unsafe { (route.functions.unmap_memory)(device, context.memory) };
        }
        if context.buffer != 0 {
            unsafe { (route.functions.destroy_buffer)(device, context.buffer, std::ptr::null()) };
        }
        if context.memory != 0 {
            unsafe { (route.functions.free_memory)(device, context.memory, std::ptr::null()) };
        }
        if context.fence != 0 {
            unsafe { (route.functions.destroy_fence)(device, context.fence, std::ptr::null()) };
        }
        if context.semaphore != 0 {
            unsafe {
                (route.functions.destroy_semaphore)(device, context.semaphore, std::ptr::null());
            };
        }
    }
    if route.command_pool != 0 {
        unsafe {
            (route.functions.destroy_command_pool)(device, route.command_pool, std::ptr::null());
        };
    }
}

fn lock_state() -> MutexGuard<'static, State> {
    STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coherent_memory_selection_respects_driver_mask() {
        let mut properties = VkPhysicalDeviceMemoryProperties {
            memory_type_count: 2,
            ..VkPhysicalDeviceMemoryProperties::default()
        };
        properties.memory_types[0].property_flags = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT;
        properties.memory_types[1].property_flags =
            VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
        assert_eq!(coherent_memory_type(&properties, 0b11), Some(1));
        assert_eq!(coherent_memory_type(&properties, 0b01), None);
    }

    #[test]
    fn exported_memory_prefers_device_local_without_requiring_host_mapping() {
        let mut properties = VkPhysicalDeviceMemoryProperties {
            memory_type_count: 2,
            ..VkPhysicalDeviceMemoryProperties::default()
        };
        properties.memory_types[0].property_flags = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT;
        properties.memory_types[1].property_flags = VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT;
        assert_eq!(export_memory_type(&properties, 0b11), Some(1));
        assert_eq!(export_memory_type(&properties, 0b01), Some(0));
    }

    #[test]
    fn checksum_samples_fixed_work_independent_of_frame_size() {
        let bytes = vec![7_u8; 1_000_000];
        let checksum = unsafe { sampled_checksum(bytes.as_ptr().addr(), 1_000_000) };
        assert_ne!(checksum, 0);
    }

    #[test]
    fn replay_copy_cadence_is_closed_to_thirty_or_sixty_fps() {
        assert_eq!(frame_interval_for_fps(30), 1_000_000_000 / 30);
        assert_eq!(frame_interval_for_fps(60), 1_000_000_000 / 60);
    }

    #[test]
    fn sixty_fps_deadlines_do_not_collapse_to_half_rate_on_an_eighty_four_fps_game() {
        let source_interval_ns = 1_000_000_000 / 84;
        let capture_interval_ns = frame_interval_for_fps(60);
        let mut deadline_ns = 0;
        let mut captured = 0;
        for source_frame in 0..84_u64 {
            let now_ns = source_frame.saturating_mul(source_interval_ns);
            if deadline_ns == 0 || now_ns >= deadline_ns {
                captured += 1;
                deadline_ns = next_capture_deadline(deadline_ns, now_ns, capture_interval_ns);
            }
        }

        assert!((59..=61).contains(&captured), "captured {captured}");
    }

    #[test]
    fn selected_frames_keep_fixed_presentation_cadence_on_an_eighty_four_fps_game() {
        let source_interval_ns = NANOSECONDS_PER_SECOND / 84;
        let capture_interval_ns = frame_interval_for_fps(60);
        let mut deadline_ns = 0;
        let mut presentation_timestamps = Vec::new();
        for source_frame in 1..=84_u64 {
            let now_ns = source_frame.saturating_mul(source_interval_ns);
            if deadline_ns == 0 || now_ns >= deadline_ns {
                presentation_timestamps.push(capture_presentation_timestamp(deadline_ns, now_ns));
                deadline_ns = next_capture_deadline(deadline_ns, now_ns, capture_interval_ns);
            }
        }

        assert!(presentation_timestamps.windows(2).all(|timestamps| {
            timestamps[1].saturating_sub(timestamps[0]) == capture_interval_ns
        }));
    }

    #[test]
    fn staging_contexts_cover_every_bounded_swapchain_image() {
        assert_eq!(context_count_for_swapchain(1), Some(5));
        assert_eq!(context_count_for_swapchain(2), Some(5));
        assert_eq!(context_count_for_swapchain(3), Some(5));
        assert_eq!(context_count_for_swapchain(4), Some(5));
        assert_eq!(context_count_for_swapchain(8), Some(8));
        assert_eq!(context_count_for_swapchain(0), None);
        assert_eq!(context_count_for_swapchain(9), None);
    }

    #[test]
    fn completed_copies_are_published_in_capture_order_not_storage_order() {
        let mut contexts = vec![CopyContext::default(), CopyContext::default()];
        contexts[0].usable = true;
        contexts[0].pending = true;
        contexts[0].captured_at_ns = 200;
        contexts[1].usable = true;
        contexts[1].pending = true;
        contexts[1].captured_at_ns = 100;

        assert_eq!(oldest_pending_context_index(&contexts), Some(1));

        contexts[1].pending = false;
        assert_eq!(oldest_pending_context_index(&contexts), Some(0));
    }

    #[test]
    fn replay_finds_its_image_inside_a_bounded_multi_swapchain_present() {
        let swapchains = [11, 22, 33];
        let image_indices = [1, 4, 7];
        let present = VkPresentInfoKhr {
            s_type: VK_STRUCTURE_TYPE_PRESENT_INFO_KHR,
            p_next: std::ptr::null(),
            wait_semaphore_count: 0,
            wait_semaphores: std::ptr::null(),
            swapchain_count: 3,
            swapchains: swapchains.as_ptr(),
            image_indices: image_indices.as_ptr(),
            results: std::ptr::null_mut(),
        };

        assert_eq!(unsafe { presented_image_index(&present, 22) }, Some(4));
        assert_eq!(unsafe { presented_image_index(&present, 44) }, None);
    }

    #[test]
    fn repeated_swapchain_recreation_reclaims_only_the_synchronized_queue() {
        let presentation_queue = 42;
        let retired_routes = [42, 42, 71];
        let reclaimable = retired_routes
            .into_iter()
            .filter(|route_queue| presentation_queue_can_reclaim(presentation_queue, *route_queue))
            .count();

        assert_eq!(reclaimable, 2);
        assert!(!presentation_queue_can_reclaim(0, presentation_queue));
        assert!(!presentation_queue_can_reclaim(presentation_queue, 71));
    }
}
