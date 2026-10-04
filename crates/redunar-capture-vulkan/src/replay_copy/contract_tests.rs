//! Injected producer API/lifecycle checks; no Vulkan loader is used.
use super::*;
use redunar_capture::ReplayPixelFormat;
use std::cell::RefCell;

#[test]
fn device_recreation_never_reuses_a_delayed_acknowledgement_identifier() {
    reset();
    let mut old = route(1);
    old.contexts[0].pending = true;
    old.contexts[0].captured_at_ns = 100;
    old.contexts[0].duration_ns = 1;
    let old_sequence = unsafe { take_completed_copy(&mut old) }
        .unwrap()
        .export_sequence
        .unwrap();
    crate::producer::device_destroyed();
    let mut new = route(2);
    new.contexts[0].pending = true;
    new.contexts[0].captured_at_ns = 200;
    new.contexts[0].duration_ns = 1;
    let new_sequence = unsafe { take_completed_copy(&mut new) }
        .unwrap()
        .export_sequence
        .unwrap();
    assert!(new_sequence > old_sequence);
    release_route_sequence(&mut new, old_sequence);
    assert!(!new.contexts[0].export_released);
    release_route_sequence(&mut new, new_sequence);
    assert!(new.contexts[0].export_released);
    dispose(&mut old);
    dispose(&mut new);
    unsafe {
        destroy_route(old);
        destroy_route(new);
    }
}

#[derive(Default)]
struct Driver {
    next_handle: u64,
    signaled: Vec<u64>,
    fences: BTreeMap<u64, bool>,
    destroyed: Vec<(String, u64)>,
    buffer_barriers: Vec<(u32, u32, u32, u32)>,
    dedicated_required: bool,
    dedicated_allocations: Vec<u64>,
    reject_buffer: bool,
    requirements_calls: u32,
    fail_operation: Option<&'static str>,
}
thread_local! { static DRIVER: RefCell<Driver> = RefCell::new(Driver { next_handle: 100, ..Default::default() }); }
fn handle() -> u64 {
    DRIVER.with(|driver| {
        let mut driver = driver.borrow_mut();
        driver.next_handle += 1;
        driver.next_handle
    })
}
fn failure(operation: &str) -> bool {
    DRIVER.with(|driver| driver.borrow().fail_operation == Some(operation))
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_get_swapchain_images(
    a0: VkDevice,
    a1: VkSwapchainKhr,
    a2: *mut u32,
    a3: *mut VkImage,
) -> VkResult {
    unsafe {
        *a2 = 3;
    }
    if !a3.is_null() {
        unsafe {
            std::ptr::copy_nonoverlapping([1, 2, 3].as_ptr(), a3, 3);
        }
    }
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_create_semaphore(
    a0: VkDevice,
    a1: *const VkSemaphoreCreateInfo,
    a2: *const VkAllocationCallbacks,
    a3: *mut VkSemaphore,
) -> VkResult {
    unsafe {
        *a3 = handle();
    }
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_destroy_semaphore(
    a0: VkDevice,
    a1: VkSemaphore,
    a2: *const VkAllocationCallbacks,
) {
    DRIVER.with(|driver| {
        driver
            .borrow_mut()
            .destroyed
            .push(("destroy_semaphore".to_owned(), a1));
    });
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_create_fence(
    a0: VkDevice,
    a1: *const VkFenceCreateInfo,
    a2: *const VkAllocationCallbacks,
    a3: *mut VkFence,
) -> VkResult {
    let fence = handle();
    unsafe {
        *a3 = fence;
    }
    DRIVER.with(|driver| {
        driver.borrow_mut().fences.insert(fence, true);
    });
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_destroy_fence(
    a0: VkDevice,
    a1: VkFence,
    a2: *const VkAllocationCallbacks,
) {
    DRIVER.with(|driver| {
        driver
            .borrow_mut()
            .destroyed
            .push(("destroy_fence".to_owned(), a1));
    });
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_reset_fences(a0: VkDevice, a1: u32, a2: *const VkFence) -> VkResult {
    if failure("reset_fences") {
        return -2;
    }
    for fence in unsafe { std::slice::from_raw_parts(a2, usize::try_from(a1).unwrap()) } {
        DRIVER.with(|driver| {
            driver.borrow_mut().fences.insert(*fence, false);
        });
    }
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_get_fence_status(a0: VkDevice, a1: VkFence) -> VkResult {
    DRIVER.with(|driver| {
        if driver.borrow().fences.get(&a1).copied().unwrap_or(true) {
            VK_SUCCESS
        } else {
            1
        }
    })
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_create_command_pool(
    a0: VkDevice,
    a1: *const VkCommandPoolCreateInfo,
    a2: *const VkAllocationCallbacks,
    a3: *mut VkCommandPool,
) -> VkResult {
    unsafe {
        *a3 = handle();
    }
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_destroy_command_pool(
    a0: VkDevice,
    a1: VkCommandPool,
    a2: *const VkAllocationCallbacks,
) {
    DRIVER.with(|driver| {
        driver
            .borrow_mut()
            .destroyed
            .push(("destroy_command_pool".to_owned(), a1));
    });
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_allocate_command_buffers(
    a0: VkDevice,
    a1: *const VkCommandBufferAllocateInfo,
    a2: *mut VkCommandBuffer,
) -> VkResult {
    let info = unsafe { &*a1 };
    for index in 0..usize::try_from(info.command_buffer_count).unwrap() {
        unsafe {
            *a2.add(index) = usize::try_from(handle()).unwrap() as VkCommandBuffer;
        }
    }
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_reset_command_buffer(a0: VkCommandBuffer, a1: u32) -> VkResult {
    if failure("reset_command") {
        return -2;
    }
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_begin_command_buffer(
    a0: VkCommandBuffer,
    a1: *const VkCommandBufferBeginInfo,
) -> VkResult {
    if failure("begin_command") {
        return -2;
    }
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_end_command_buffer(a0: VkCommandBuffer) -> VkResult {
    if failure("end_command") {
        return -2;
    }
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_queue_submit(
    a0: VkQueue,
    a1: u32,
    a2: *const VkSubmitInfo,
    a3: VkFence,
) -> VkResult {
    if failure("queue_submit") {
        return -2;
    }
    assert_eq!(a1, 1);
    let submit = unsafe { &*a2 };
    assert_eq!(submit.signal_semaphore_count, 1);
    let signal = unsafe { *submit.signal_semaphores };
    DRIVER.with(|driver| {
        let mut driver = driver.borrow_mut();
        assert!(
            !driver.signaled.contains(&signal),
            "binary presentation semaphore reused before its wait completed"
        );
        driver.signaled.push(signal);
        // Copy can complete immediately while presentation remains pending.
        driver.fences.insert(a3, true);
    });
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_create_buffer(
    a0: VkDevice,
    a1: *const VkBufferCreateInfo,
    a2: *const VkAllocationCallbacks,
    a3: *mut VkBuffer,
) -> VkResult {
    if DRIVER.with(|driver| driver.borrow().reject_buffer) {
        return -2;
    }
    let info = unsafe { &*a1 };
    assert_eq!(
        info.usage,
        VK_BUFFER_USAGE_TRANSFER_DST_BIT | VK_BUFFER_USAGE_STORAGE_BUFFER_BIT
    );
    unsafe {
        *a3 = handle();
    }
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_destroy_buffer(
    a0: VkDevice,
    a1: VkBuffer,
    a2: *const VkAllocationCallbacks,
) {
    DRIVER.with(|driver| {
        driver
            .borrow_mut()
            .destroyed
            .push(("destroy_buffer".to_owned(), a1));
    });
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_get_buffer_memory_requirements(
    a0: VkDevice,
    a1: VkBuffer,
    a2: *mut VkMemoryRequirements,
) {
    unsafe {
        *a2 = VkMemoryRequirements {
            size: 4096,
            alignment: 256,
            memory_type_bits: 1,
        };
    }
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_allocate_memory(
    a0: VkDevice,
    a1: *const VkMemoryAllocateInfo,
    a2: *const VkAllocationCallbacks,
    a3: *mut VkDeviceMemory,
) -> VkResult {
    let info = unsafe { &*a1 };
    if !info.p_next.is_null() {
        let export = unsafe { &*info.p_next.cast::<VkExportMemoryAllocateInfo>() };
        assert_eq!(export.s_type, VK_STRUCTURE_TYPE_EXPORT_MEMORY_ALLOCATE_INFO);
        assert_eq!(
            export.handle_types,
            VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT
        );
        if !export.p_next.is_null() {
            let dedicated = unsafe { &*export.p_next.cast::<replay_memory::DedicatedAllocation>() };
            assert_eq!(dedicated.s_type, 1_000_127_001);
            assert_eq!(dedicated.image, 0);
            assert_ne!(dedicated.buffer, 0);
            DRIVER.with(|driver| {
                driver
                    .borrow_mut()
                    .dedicated_allocations
                    .push(dedicated.buffer);
            });
        }
    }
    unsafe {
        *a3 = handle();
    }
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_free_memory(
    a0: VkDevice,
    a1: VkDeviceMemory,
    a2: *const VkAllocationCallbacks,
) {
    DRIVER.with(|driver| {
        driver
            .borrow_mut()
            .destroyed
            .push(("free_memory".to_owned(), a1));
    });
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_bind_buffer_memory(
    a0: VkDevice,
    a1: VkBuffer,
    a2: VkDeviceMemory,
    a3: u64,
) -> VkResult {
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_map_memory(
    a0: VkDevice,
    a1: VkDeviceMemory,
    a2: u64,
    a3: u64,
    a4: u32,
    a5: *mut *mut c_void,
) -> VkResult {
    unsafe {
        *a5 = std::ptr::dangling_mut::<u8>().cast();
    }
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_unmap_memory(a0: VkDevice, a1: VkDeviceMemory) {
    DRIVER.with(|driver| {
        driver
            .borrow_mut()
            .destroyed
            .push(("unmap_memory".to_owned(), a1));
    });
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_cmd_pipeline_barrier(
    a0: VkCommandBuffer,
    a1: u32,
    a2: u32,
    a3: u32,
    a4: u32,
    a5: *const c_void,
    a6: u32,
    a7: *const c_void,
    a8: u32,
    a9: *const VkImageMemoryBarrier,
) {
    if a6 != 0 {
        assert_eq!(a6, 1);
        let barrier = unsafe { &*a7.cast::<BufferBarrier>() };
        assert_eq!(barrier.s_type, VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER);
        assert_eq!(barrier.offset, 0);
        assert_eq!(barrier.size, u64::MAX);
        DRIVER.with(|driver| {
            driver.borrow_mut().buffer_barriers.push((
                barrier.src_queue_family_index,
                barrier.dst_queue_family_index,
                barrier.src_access_mask,
                barrier.dst_access_mask,
            ));
        });
    }
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_cmd_copy_image_to_buffer(
    a0: VkCommandBuffer,
    a1: VkImage,
    a2: i32,
    a3: VkBuffer,
    a4: u32,
    a5: *const VkBufferImageCopy,
) {
}
unsafe extern "system" fn fake_requirements2(
    _: VkDevice,
    info: *const replay_memory::BufferRequirementsInfo,
    out: *mut replay_memory::Requirements2,
) {
    let info = unsafe { &*info };
    assert_eq!(info.s_type, 1_000_146_000);
    let out = unsafe { &mut *out };
    assert_eq!(out.s_type, 1_000_146_003);
    out.memory = VkMemoryRequirements {
        size: 4096,
        alignment: 256,
        memory_type_bits: 1,
    };
    let dedicated = unsafe { &mut *out.p_next.cast::<replay_memory::DedicatedRequirements>() };
    assert_eq!(dedicated.s_type, 1_000_127_000);
    DRIVER.with(|driver| {
        let mut driver = driver.borrow_mut();
        driver.requirements_calls += 1;
        dedicated.requires = u32::from(driver.dedicated_required);
    });
}
unsafe extern "system" fn fake_memory_fd(
    _: VkDevice,
    _: *const VkMemoryGetFdInfoKhr,
    out: *mut i32,
) -> VkResult {
    // A fake export has no OS-owned FD; fence and lease bookkeeping still runs.
    unsafe {
        *out = 0;
    }
    VK_SUCCESS
}
fn functions() -> DeviceFunctions {
    DeviceFunctions {
        get_swapchain_images: fake_get_swapchain_images,
        create_semaphore: fake_create_semaphore,
        destroy_semaphore: fake_destroy_semaphore,
        create_fence: fake_create_fence,
        destroy_fence: fake_destroy_fence,
        reset_fences: fake_reset_fences,
        get_fence_status: fake_get_fence_status,
        create_command_pool: fake_create_command_pool,
        destroy_command_pool: fake_destroy_command_pool,
        allocate_command_buffers: fake_allocate_command_buffers,
        reset_command_buffer: fake_reset_command_buffer,
        begin_command_buffer: fake_begin_command_buffer,
        end_command_buffer: fake_end_command_buffer,
        queue_submit: fake_queue_submit,
        create_buffer: fake_create_buffer,
        destroy_buffer: fake_destroy_buffer,
        get_buffer_memory_requirements: fake_get_buffer_memory_requirements,
        get_buffer_requirements2: Some(fake_requirements2),
        allocate_memory: fake_allocate_memory,
        free_memory: fake_free_memory,
        bind_buffer_memory: fake_bind_buffer_memory,
        map_memory: fake_map_memory,
        unmap_memory: fake_unmap_memory,
        get_memory_fd: Some(fake_memory_fd),
        cmd_pipeline_barrier: fake_cmd_pipeline_barrier,
        cmd_copy_image_to_buffer: fake_cmd_copy_image_to_buffer,
    }
}
fn reset() {
    DRIVER.with(|driver| {
        *driver.borrow_mut() = Driver {
            next_handle: 100,
            ..Default::default()
        }
    });
}
fn candidate() -> ReplaySourceCandidate {
    ReplaySourceCandidate {
        gpu_identity: None,
        width: 16,
        height: 16,
        pixel_format: ReplayPixelFormat::Rgba8Unorm,
        target_frames_per_second: 60,
    }
}
fn memory() -> VkPhysicalDeviceMemoryProperties {
    let mut memory = VkPhysicalDeviceMemoryProperties {
        memory_type_count: 1,
        ..Default::default()
    };
    memory.memory_types[0].property_flags = VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT
        | VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT
        | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
    memory
}
fn route(swapchain: u64) -> Route {
    Route {
        device_key: 1,
        device_address: 1,
        queue_address: 42,
        functions: functions(),
        memory_properties: memory(),
        swapchain,
        images: [1, 2, 3, 0, 0, 0, 0, 0],
        image_count: 3,
        width: 16,
        height: 16,
        candidate: candidate(),
        copied_bytes: 1024,
        frame_interval_ns: 16_666_666,
        sampling_rate: 60,
        variable_min_interval_ns: 4_166_667,
        next_submit_ns: 0,
        command_pool: 10,
        presentation_semaphores: [70, 71, 72, 0, 0, 0, 0, 0],
        presentation_usable: [true; MAX_IMAGES],
        presentation_pending: [false; MAX_IMAGES],
        queue_family_index: 7,
        export_required: true,
        export_requires_dedicated: false,
        gpu_idle: false,
        contexts: (0..5_u64)
            .map(|i| CopyContext {
                command_buffer_address: usize::try_from(i + 100).unwrap(),
                fence: i + 10,
                buffer: i + 20,
                memory: i + 30,
                usable: true,
                export_fd: 100,
                export_released: true,
                ..Default::default()
            })
            .collect(),
    }
}
fn present() -> VkPresentInfoKhr {
    VkPresentInfoKhr {
        s_type: VK_STRUCTURE_TYPE_PRESENT_INFO_KHR,
        p_next: std::ptr::null(),
        wait_semaphore_count: 0,
        wait_semaphores: std::ptr::null(),
        swapchain_count: 0,
        swapchains: std::ptr::null(),
        image_indices: std::ptr::null(),
        results: std::ptr::null_mut(),
    }
}
fn retire_state() -> State {
    let mut state = State::default();
    retain_retired_route(&mut state, route(1));
    assert!(!state.disabled);
    retain_retired_route(&mut state, route(2));
    assert!(!state.disabled);
    retain_retired_route(&mut state, route(3));
    assert!(state.disabled);
    state
}
fn dispose(route: &mut Route) {
    // Fake handles never own the fabricated positive descriptor numbers.
    for context in &mut route.contexts {
        context.export_fd = -1;
    }
}

#[test]
fn completed_copy_fence_and_daemon_ack_cannot_reuse_another_images_pending_present_wait() {
    reset();
    let mut route = route(1);
    let present = present();
    let first = unsafe { submit_copy(&mut route, &present, 1, 10, 1, 0) };
    assert_eq!(first.0, 70);
    let second = unsafe { submit_copy(&mut route, &present, 2, 20, 1, 1) };
    assert_eq!(second.0, 71);
    release_route_sequence(&mut route, second.1.unwrap().export_sequence.unwrap());
    // Copy slot zero is now free, but its presentation wait is still pending.
    let third = unsafe { submit_copy(&mut route, &present, 3, 30, 1, 2) };
    assert_eq!(third.0, 72);
    DRIVER.with(|driver| assert_eq!(driver.borrow().signaled, [70, 71, 72]));
    // Reacquiring the same image completes that image's presentation wait.
    DRIVER.with(|driver| driver.borrow_mut().signaled.retain(|value| *value != 70));
    assert_eq!(
        unsafe { submit_copy(&mut route, &present, 1, 40, 1, 0) }.0,
        70
    );
    DRIVER.with(|driver| {
        let driver = driver.borrow();
        assert!(driver.buffer_barriers.contains(&(
            7,
            u32::MAX - 2,
            VK_ACCESS_TRANSFER_WRITE_BIT,
            0
        )));
        assert!(driver.buffer_barriers.contains(&(
            u32::MAX - 2,
            7,
            0,
            VK_ACCESS_TRANSFER_WRITE_BIT
        )));
    });
    dispose(&mut route);
    unsafe {
        destroy_route(route);
    }
}
#[test]
fn producer_uses_dedicated_allocation_from_either_external_support_or_buffer_requirements() {
    for (external, required) in [(true, false), (false, true), (false, false)] {
        reset();
        DRIVER.with(|driver| driver.borrow_mut().dedicated_required = required);
        let mut context = CopyContext::default();
        assert!(unsafe {
            create_context(
                1_usize as VkDevice,
                functions(),
                &memory(),
                1024,
                true,
                external,
                &mut context,
            )
        });
        DRIVER.with(|driver| {
            assert_eq!(driver.borrow().requirements_calls, 1);
            assert_eq!(
                driver.borrow().dedicated_allocations.len(),
                usize::from(external || required)
            );
        });
    }
    reset();
    let mut functions = functions();
    functions.get_buffer_requirements2 = None;
    let mut context = CopyContext::default();
    assert!(!unsafe {
        create_context(
            1_usize as VkDevice,
            functions,
            &memory(),
            1024,
            true,
            true,
            &mut context,
        )
    });
    DRIVER.with(|driver| assert!(driver.borrow().dedicated_allocations.is_empty()));
}
#[test]
fn ordinary_resize_keeps_two_replacements_bounded_then_pauses_without_destroying_pending_resources()
{
    reset();
    let mut state = retire_state();
    let retained_count = || DRIVER.with(|driver| driver.borrow().destroyed.len());
    assert_eq!(state.retired_routes.iter().flatten().count(), 2);
    assert!(state.abandoned_route.is_some());
    assert_eq!(retained_count(), 0);
    assert!(matches!(
        state.pending_assessment,
        Some(Err(
            redunar_capture::ReplaySourceRejection::EncoderBackendUnavailable
        ))
    ));
    for _ in 0..1000 {
        unsafe {
            reclaim_idle_released(&mut state);
        }
        assert!(state.disabled);
        assert_eq!(retained_count(), 0);
    }
    for route in state
        .retired_routes
        .iter_mut()
        .flatten()
        .chain(state.abandoned_route.iter_mut())
    {
        dispose(route);
    }
    // An unrelated queue supplies no completion proof.
    unsafe {
        reclaim_after_idle(&mut state, 99);
    }
    assert!(state.disabled);
    assert_eq!(retained_count(), 0);
    unsafe {
        reclaim_after_idle(&mut state, 42);
    }
    assert!(!state.disabled);
    assert!(state.retired_routes.iter().all(Option::is_none));
    assert!(state.abandoned_route.is_none());
    assert!(retained_count() > 0);
}
#[test]
fn idle_keeps_exported_fd_lease_and_ack_can_reclaim_without_another_idle() {
    reset();
    let mut state = retire_state();
    for route in state
        .retired_routes
        .iter_mut()
        .flatten()
        .chain(state.abandoned_route.iter_mut())
    {
        dispose(route);
    }
    let abandoned = state.abandoned_route.as_mut().unwrap();
    abandoned.contexts[0].export_sequence = 1001;
    abandoned.contexts[0].export_released = false;
    unsafe {
        reclaim_after_idle(&mut state, 42);
    }
    assert!(state.disabled);
    let held = state.abandoned_route.as_ref().unwrap();
    assert!(held.gpu_idle);
    assert_eq!(held.contexts[0].export_sequence, 1001);
    let before = DRIVER.with(|driver| driver.borrow().destroyed.len());
    release_route_sequence(state.abandoned_route.as_mut().unwrap(), 1001);
    unsafe {
        reclaim_idle_released(&mut state);
    }
    assert!(!state.disabled);
    assert!(state.abandoned_route.is_none());
    assert!(DRIVER.with(|driver| driver.borrow().destroyed.len()) > before);
}
#[test]
fn idle_reclaims_retirement_then_restarts_pending_generation_transactionally() {
    reset();
    let mut state = retire_state();
    for route in state
        .retired_routes
        .iter_mut()
        .flatten()
        .chain(state.abandoned_route.iter_mut())
    {
        dispose(route);
    }
    state.devices.insert(
        1,
        PendingDevice {
            device_address: 1,
            functions: functions(),
            memory_properties: memory(),
            graphics_queue_families: 1 << 7,
            queue_address: 42,
            queue_family_index: 7,
            export_support: Some(false),
        },
    );
    state.pending_swapchain = Some(PendingSwapchain {
        device_key: 1,
        replaces: 3,
        swapchain: 4,
        candidate: candidate(),
        copied_bytes: 1024,
    });
    unsafe {
        reclaim_after_idle(&mut state, 42);
    }
    assert!(!state.disabled);
    assert_eq!(state.route.as_ref().unwrap().swapchain, 4);
    assert!(state.pending_swapchain.is_none());
    assert_eq!(state.pending_assessment, Some(Ok(candidate())));
    let mut route = state.route.take().unwrap();
    dispose(&mut route);
    unsafe {
        destroy_route(route);
    }
}

#[test]
fn copy_idle_and_export_release_are_insufficient_to_destroy_pending_present_waits() {
    reset();
    let mut retained = route(10);
    dispose(&mut retained);
    retained.presentation_pending[0] = true;
    let mut state = State::default();
    retain_retired_route(&mut state, retained);
    unsafe {
        reclaim_after_idle(&mut state, 42);
    }
    assert!(state.retired_routes[0].as_ref().unwrap().gpu_idle);
    DRIVER.with(|driver| assert!(driver.borrow().destroyed.is_empty()));
    let retained = state.retired_routes[0].as_mut().unwrap();
    DRIVER.with(|driver| {
        driver.borrow_mut().fences.insert(999, false);
    });
    confirm_reacquisition(retained, 1, 10, 0, 999);
    assert!(retained.presentation_pending[0]);
    confirm_reacquisition(retained, 1, 10, 0, 0);
    assert!(retained.presentation_pending[0]);
    DRIVER.with(|driver| {
        driver.borrow_mut().fences.insert(999, true);
    });
    confirm_reacquisition(retained, 1, 99, 0, 999);
    assert!(retained.presentation_pending[0]);
    confirm_reacquisition(retained, 1, 10, 0, 999);
    assert!(!retained.presentation_pending[0]);
    unsafe {
        reclaim_idle_released(&mut state);
    }
    assert!(state.retired_routes[0].is_none());
    DRIVER.with(|driver| {
        assert_eq!(
            driver
                .borrow()
                .destroyed
                .iter()
                .filter(|(kind, _)| kind == "destroy_semaphore")
                .count(),
            3
        );
    });
}

fn pending_device() -> PendingDevice {
    PendingDevice {
        device_address: 1,
        functions: functions(),
        memory_properties: memory(),
        graphics_queue_families: 1 << 7,
        queue_address: 42,
        queue_family_index: 7,
        export_support: Some(false),
    }
}
fn pending_generation() -> PendingSwapchain {
    PendingSwapchain {
        device_key: 1,
        replaces: 0,
        swapchain: 4,
        candidate: candidate(),
        copied_bytes: 1024,
    }
}

#[test]
fn every_creation_entry_including_queue_retrieval_preserves_full_retirement_budget() {
    reset();
    let mut state = retire_state();
    for retained in state
        .retired_routes
        .iter_mut()
        .flatten()
        .chain(state.abandoned_route.iter_mut())
    {
        dispose(retained);
        retained.presentation_pending[0] = true;
    }
    state.devices.insert(1, pending_device());
    state.pending_swapchain = Some(pending_generation());
    for _ in 0..100 {
        observe_queue(&mut state, 1, 42_usize as VkQueue, 7, false);
        assert!(!install_route(&mut state, pending_generation()));
        resume_pending_route(&mut state, 42);
        unsafe {
            reclaim_after_idle(&mut state, 42);
        }
        assert!(state.route.is_none());
        assert!(state.disabled);
        assert_eq!(state.abandoned_route.as_ref().unwrap().swapchain, 3);
    }
    DRIVER.with(|driver| {
        assert_eq!(driver.borrow().next_handle, 100);
        assert!(driver.borrow().destroyed.is_empty());
    });
}

#[test]
fn retirement_invariant_violation_retains_ownership_without_panicking_or_replacing_it() {
    reset();
    let mut state = retire_state();
    let mut unexpected = route(4);
    dispose(&mut unexpected);
    retain_retired_route(&mut state, unexpected);
    assert_eq!(state.abandoned_route.as_ref().unwrap().swapchain, 3);
    assert_eq!(state.quarantined_route.as_ref().unwrap().swapchain, 4);
    assert!(state.disabled);
    assert!(!state.can_create_route());
    DRIVER.with(|driver| assert!(driver.borrow().destroyed.is_empty()));
}

#[test]
fn completed_export_proof_survives_each_next_submission_failure_and_unsent_lease_is_local() {
    for operation in [
        "reset_fences",
        "reset_command",
        "begin_command",
        "end_command",
        "queue_submit",
    ] {
        reset();
        DRIVER.with(|driver| driver.borrow_mut().fail_operation = Some(operation));
        let mut route = route(1);
        route.contexts[0].pending = true;
        route.contexts[0].captured_at_ns = 100;
        route.contexts[0].duration_ns = 1;
        let (semaphore, proof) = unsafe { submit_copy(&mut route, &present(), 2, 200, 1, 1) };
        assert_eq!(semaphore, 0, "{operation}");
        let proof = proof.expect("completed export must survive failed next setup");
        let sequence = proof.export_sequence.unwrap();
        assert_eq!(proof.timestamp_ns, 100);
        assert!(!route.contexts[0].export_released);
        let mut state = State {
            route: Some(route),
            ..Default::default()
        };
        complete_copy_handoff(&mut state, proof, |_, _| false);
        assert!(state.route.as_ref().unwrap().contexts[0].export_released);
        assert_eq!(state.last_export_timestamp_ns, 0);
        let mut route = state.route.take().unwrap();
        dispose(&mut route);
        unsafe {
            destroy_route(route);
        }
        assert_ne!(sequence, 0);
    }
}

#[test]
fn sent_completed_proof_remains_release_gated_when_next_recording_fails() {
    reset();
    DRIVER.with(|driver| driver.borrow_mut().fail_operation = Some("begin_command"));
    let mut route = route(1);
    route.contexts[0].pending = true;
    route.contexts[0].captured_at_ns = 100;
    route.contexts[0].duration_ns = 1;
    let (_, proof) = unsafe { submit_copy(&mut route, &present(), 2, 200, 1, 1) };
    let proof = proof.unwrap();
    let sequence = proof.export_sequence.unwrap();
    let mut state = State {
        route: Some(route),
        ..Default::default()
    };
    complete_copy_handoff(&mut state, proof, |_, _| true);
    assert!(!state.route.as_ref().unwrap().contexts[0].export_released);
    assert_eq!(state.last_export_timestamp_ns, 100);
    release_route_sequence(state.route.as_mut().unwrap(), sequence);
    assert!(state.route.as_ref().unwrap().contexts[0].export_released);
    let mut route = state.route.take().unwrap();
    dispose(&mut route);
    unsafe {
        destroy_route(route);
    }
}

#[test]
fn unsupported_auxiliary_is_local_but_failed_active_replacement_rejects_and_blocks_source() {
    reset();
    let mut state = State {
        route: Some(route(10)),
        ..Default::default()
    };
    let error = redunar_capture::ReplaySourceRejection::PixelFormatUnsupported;
    assert!(selected_source_assessment(&mut state, 1, 0, Err(error)).is_none());
    assert!(state.pending_assessment.is_none());
    assert!(!state.source_rejected);
    assert_eq!(state.route.as_ref().unwrap().swapchain, 10);
    assert!(selected_source_assessment(&mut state, 1, 10, Err(error)).is_none());
    assert_eq!(state.pending_assessment, Some(Err(error)));
    assert!(state.source_rejected);
    let mut route = state.route.take().unwrap();
    dispose(&mut route);
    unsafe {
        destroy_route(route);
    }
}
