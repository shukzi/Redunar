//! Native-call contract tests; no loader, device, GPU or game is opened.
use super::*;
use std::cell::RefCell;

#[derive(Default)]
struct Driver {
    wait_result: VkResult,
    fail_queue: usize,
    submissions: Vec<(usize, u64)>,
    waits: Vec<(Vec<u64>, u64)>,
    destroyed: Vec<&'static str>,
    bound: Vec<(i32, u64, u32)>,
    buffers: Vec<(u32, u32, u64, u64, u64, u64)>,
}
thread_local! { static DRIVER: RefCell<Driver> = RefCell::new(Driver::default()); }
#[allow(unused_variables)]
unsafe extern "system" fn fake_destroy_command_pool(
    a0: VkDevice,
    a1: VkCommandPool,
    a2: *const VkAllocationCallbacks,
) {
    DRIVER.with(|driver| driver.borrow_mut().destroyed.push("destroy_command_pool"));
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_destroy_semaphore(
    a0: VkDevice,
    a1: VkSemaphore,
    a2: *const VkAllocationCallbacks,
) {
    DRIVER.with(|driver| driver.borrow_mut().destroyed.push("destroy_semaphore"));
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_destroy_fence(
    a0: VkDevice,
    a1: VkFence,
    a2: *const VkAllocationCallbacks,
) {
    DRIVER.with(|driver| driver.borrow_mut().destroyed.push("destroy_fence"));
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_wait_for_fences(
    a0: VkDevice,
    a1: u32,
    a2: *const VkFence,
    a3: u32,
    a4: u64,
) -> VkResult {
    assert_eq!(a3, 1);
    let fences = unsafe { std::slice::from_raw_parts(a2, usize::try_from(a1).unwrap()) }.to_vec();
    DRIVER.with(|driver| {
        let mut driver = driver.borrow_mut();
        driver.waits.push((fences, a4));
        driver.wait_result
    })
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_reset_fences(a0: VkDevice, a1: u32, a2: *const VkFence) -> VkResult {
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_reset_command_buffer(a0: VkCommandBuffer, a1: u32) -> VkResult {
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_begin_command_buffer(
    a0: VkCommandBuffer,
    a1: *const VkCommandBufferBeginInfo,
) -> VkResult {
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_end_command_buffer(a0: VkCommandBuffer) -> VkResult {
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_cmd_pipeline_barrier2(
    a0: VkCommandBuffer,
    a1: *const VkDependencyInfo,
) {
    let info = unsafe { &*a1 };
    if info.buffer_memory_barrier_count != 0 {
        assert_eq!(info.buffer_memory_barrier_count, 1);
        assert_eq!(info.dependency_flags, 0);
        let barrier = unsafe { &*info.buffer_memory_barriers.cast::<VkBufferMemoryBarrier2>() };
        assert_eq!(barrier.s_type, VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER_2);
        assert_eq!(barrier.buffer, 99);
        assert_eq!(barrier.offset, 0);
        assert_eq!(barrier.size, u64::MAX);
        DRIVER.with(|driver| {
            driver.borrow_mut().buffers.push((
                barrier.src_queue_family_index,
                barrier.dst_queue_family_index,
                barrier.src_stage_mask,
                barrier.src_access_mask,
                barrier.dst_stage_mask,
                barrier.dst_access_mask,
            ));
        });
    }
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_cmd_bind_pipeline(a0: VkCommandBuffer, a1: i32, a2: VkPipeline) {}

#[allow(unused_variables)]
unsafe extern "system" fn fake_cmd_bind_descriptor_sets(
    a0: VkCommandBuffer,
    a1: i32,
    a2: VkPipelineLayout,
    a3: u32,
    a4: u32,
    a5: *const VkDescriptorSet,
    a6: u32,
    a7: *const u32,
) {
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_cmd_push_constants(
    a0: VkCommandBuffer,
    a1: VkPipelineLayout,
    a2: u32,
    a3: u32,
    a4: u32,
    a5: *const c_void,
) {
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_cmd_dispatch(a0: VkCommandBuffer, a1: u32, a2: u32, a3: u32) {}

#[allow(unused_variables)]
unsafe extern "system" fn fake_cmd_copy_image(
    a0: VkCommandBuffer,
    a1: VkImage,
    a2: i32,
    a3: VkImage,
    a4: i32,
    a5: u32,
    a6: *const VkImageCopy,
) {
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_update_descriptor_sets(
    a0: VkDevice,
    a1: u32,
    a2: *const VkWriteDescriptorSet,
    a3: u32,
    a4: *const c_void,
) {
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_queue_submit2(
    a0: VkQueue,
    a1: u32,
    a2: *const VkSubmitInfo2,
    a3: VkFence,
) -> VkResult {
    let info = unsafe { &*a2 };
    assert_eq!(a1, 1);
    assert_eq!(info.command_buffer_info_count, 1);
    if a0.addr() == 42 {
        assert_eq!(info.wait_semaphore_info_count, 0);
        assert_eq!(info.signal_semaphore_info_count, 1);
        assert_eq!(
            unsafe { (*info.signal_semaphore_infos).stage_mask },
            VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT
        );
    } else {
        assert_eq!(info.wait_semaphore_info_count, 1);
        assert_eq!(info.signal_semaphore_info_count, 0);
        assert_eq!(
            unsafe { (*info.wait_semaphore_infos).stage_mask },
            VK_PIPELINE_STAGE_2_VIDEO_ENCODE_BIT_KHR
        );
    }
    DRIVER.with(|driver| {
        let mut driver = driver.borrow_mut();
        driver.submissions.push((a0.addr(), a3));
        if driver.fail_queue == a0.addr() {
            -2
        } else {
            VK_SUCCESS
        }
    })
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_cmd_reset_query_pool(
    a0: VkCommandBuffer,
    a1: VkQueryPool,
    a2: u32,
    a3: u32,
) {
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_cmd_begin_query(
    a0: VkCommandBuffer,
    a1: VkQueryPool,
    a2: u32,
    a3: u32,
) {
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_cmd_end_query(a0: VkCommandBuffer, a1: VkQueryPool, a2: u32) {}

#[allow(unused_variables)]
unsafe extern "system" fn fake_cmd_begin_video_coding(
    a0: VkCommandBuffer,
    a1: *const VkVideoBeginCodingInfoKhr,
) {
    let info = unsafe { &*a1 };
    assert_eq!(info.s_type, VK_STRUCTURE_TYPE_VIDEO_BEGIN_CODING_INFO_KHR);
    let resources = unsafe {
        std::slice::from_raw_parts(
            info.reference_slots,
            usize::try_from(info.reference_slot_count).unwrap(),
        )
    };
    assert_eq!(resources[0].slot_index, -1);
    let pictures = resources
        .iter()
        .map(|resource| {
            let picture = unsafe { &*resource.picture_resource };
            (
                resource.slot_index,
                picture.image_view_binding,
                picture.base_array_layer,
            )
        })
        .collect();
    DRIVER.with(|driver| driver.borrow_mut().bound = pictures);
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_cmd_control_video_coding(
    a0: VkCommandBuffer,
    a1: *const VkVideoCodingControlInfoKhr,
) {
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_cmd_encode_video(
    a0: VkCommandBuffer,
    a1: *const VkVideoEncodeInfoKhr,
) {
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_cmd_end_video_coding(
    a0: VkCommandBuffer,
    a1: *const VkVideoEndCodingInfoKhr,
) {
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_get_query_pool_results(
    a0: VkDevice,
    a1: VkQueryPool,
    a2: u32,
    a3: u32,
    a4: usize,
    a5: *mut c_void,
    a6: u64,
    a7: u32,
) -> VkResult {
    VK_SUCCESS
}

#[allow(unused_variables)]
unsafe extern "system" fn fake_invalidate_mapped_memory_ranges(
    a0: VkDevice,
    a1: u32,
    a2: *const VkMappedMemoryRange,
) -> VkResult {
    VK_SUCCESS
}
fn functions() -> Functions {
    Functions {
        destroy_command_pool: fake_destroy_command_pool,
        destroy_semaphore: fake_destroy_semaphore,
        destroy_fence: fake_destroy_fence,
        wait_for_fences: fake_wait_for_fences,
        reset_fences: fake_reset_fences,
        reset_command_buffer: fake_reset_command_buffer,
        begin_command_buffer: fake_begin_command_buffer,
        end_command_buffer: fake_end_command_buffer,
        cmd_pipeline_barrier2: fake_cmd_pipeline_barrier2,
        cmd_bind_pipeline: fake_cmd_bind_pipeline,
        cmd_bind_descriptor_sets: fake_cmd_bind_descriptor_sets,
        cmd_push_constants: fake_cmd_push_constants,
        cmd_dispatch: fake_cmd_dispatch,
        cmd_copy_image: fake_cmd_copy_image,
        update_descriptor_sets: fake_update_descriptor_sets,
        queue_submit2: fake_queue_submit2,
        cmd_reset_query_pool: fake_cmd_reset_query_pool,
        cmd_begin_query: fake_cmd_begin_query,
        cmd_end_query: fake_cmd_end_query,
        cmd_begin_video_coding: fake_cmd_begin_video_coding,
        cmd_control_video_coding: fake_cmd_control_video_coding,
        cmd_encode_video: fake_cmd_encode_video,
        cmd_end_video_coding: fake_cmd_end_video_coding,
        get_query_pool_results: fake_get_query_pool_results,
        invalidate_mapped_memory_ranges: fake_invalidate_mapped_memory_ranges,
    }
}
fn resources(compute: bool, encode: bool) -> ExecutionResources {
    ExecutionResources {
        device_address: 1,
        functions: functions(),
        compute_pool: 10,
        encode_pool: 20,
        compute_commands: vec![100],
        encode_commands: vec![200],
        conversion_complete: vec![30],
        compute_complete: vec![11],
        slot_complete: vec![21],
        compute_submitted: vec![compute],
        encode_submitted: vec![encode],
        active: true,
    }
}
fn reset() {
    DRIVER.with(|driver| *driver.borrow_mut() = Driver::default());
}

#[test]
fn coding_scope_binds_setup_resource_for_idr_and_predicted_pictures_including_array_layers() {
    reset();
    let rate = VkVideoEncodeRateControlInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_RATE_CONTROL_INFO_KHR,
        p_next: ptr::null(),
        flags: 0,
        rate_control_mode: 0,
        layer_count: 0,
        layers: ptr::null(),
        virtual_buffer_size_ms: 0,
        initial_virtual_buffer_size_ms: 0,
    };
    for layer in [0, 1] {
        let current = video_picture_resource(77, layer, 1920, 1080);
        begin_coding_scope(
            fake_cmd_begin_video_coding,
            1_usize as VkCommandBuffer,
            1,
            2,
            &rate,
            &current,
            None,
        );
        DRIVER.with(|driver| assert_eq!(driver.borrow().bound, [(-1, 77, layer)]));
        let previous_picture = video_picture_resource(77, 1 - layer, 1920, 1080);
        let previous = VkVideoReferenceSlotInfoKhr {
            s_type: VK_STRUCTURE_TYPE_VIDEO_REFERENCE_SLOT_INFO_KHR,
            p_next: ptr::null(),
            slot_index: i32::try_from(1 - layer).unwrap(),
            picture_resource: &raw const previous_picture,
        };
        begin_coding_scope(
            fake_cmd_begin_video_coding,
            1_usize as VkCommandBuffer,
            1,
            2,
            &rate,
            &current,
            Some(&previous),
        );
        DRIVER.with(|driver| {
            assert_eq!(
                driver.borrow().bound,
                [
                    (-1, 77, layer),
                    (i32::try_from(1 - layer).unwrap(), 77, 1 - layer)
                ]
            );
        });
    }
}
#[test]
fn linear_dmabuf_including_gl_input_acquires_and_releases_foreign_cache_ownership() {
    reset();
    buffer_ownership(functions(), 1_usize as VkCommandBuffer, 99, 7, true);
    buffer_ownership(functions(), 1_usize as VkCommandBuffer, 99, 7, false);
    DRIVER.with(|driver| {
        assert_eq!(
            driver.borrow().buffers,
            [
                (
                    u32::MAX - 2,
                    7,
                    0,
                    0,
                    VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT,
                    VK_ACCESS_2_SHADER_STORAGE_READ_BIT
                ),
                (
                    7,
                    u32::MAX - 2,
                    VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT,
                    VK_ACCESS_2_SHADER_STORAGE_READ_BIT,
                    0,
                    0
                ),
            ]
        );
    });
    let image = foreign_image_barrier(
        99,
        VK_IMAGE_LAYOUT_GENERAL,
        VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
        0,
        0,
        VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT,
        VK_ACCESS_2_SHADER_SAMPLED_READ_BIT,
        7,
        true,
    );
    assert_eq!(image.src_queue_family_index, u32::MAX - 2);
    assert_ne!(image.src_queue_family_index, u32::MAX - 1);
}
#[test]
fn partial_compute_submission_has_its_own_bounded_completion_proof() {
    reset();
    let mut resources = resources(true, false);
    assert!(unsafe { resources.shutdown() }.is_ok());
    DRIVER.with(|driver| {
        let driver = driver.borrow();
        assert_eq!(driver.waits, [(vec![11], SLOT_WAIT_TIMEOUT_NS)]);
        assert_eq!(driver.destroyed.len(), 5);
    });
}
#[test]
fn timeout_and_device_loss_do_not_destroy_pending_commands_fences_or_semaphores() {
    for failure in [2, -4] {
        reset();
        DRIVER.with(|driver| driver.borrow_mut().wait_result = failure);
        let mut resources = resources(true, true);
        assert_eq!(
            unsafe { resources.shutdown() },
            Err(VulkanVideoDeviceError::DeviceWaitFailed(failure))
        );
        DRIVER.with(|driver| {
            let driver = driver.borrow();
            assert_eq!(driver.waits, [(vec![11, 21], SLOT_WAIT_TIMEOUT_NS)]);
            assert!(driver.destroyed.is_empty());
        });
        // Inject eventual completion to exercise the same object's idempotent
        // successful teardown without leaking fixture storage.
        DRIVER.with(|driver| driver.borrow_mut().wait_result = VK_SUCCESS);
        assert!(unsafe { resources.shutdown() }.is_ok());
        assert!(unsafe { resources.shutdown() }.is_ok());
        DRIVER.with(|driver| assert_eq!(driver.borrow().destroyed.len(), 5));
    }
}
#[test]
fn recording_or_import_failure_before_submit_does_not_wait_an_unsignaled_reset_fence() {
    reset();
    let mut resources = resources(false, false);
    assert!(unsafe { resources.shutdown() }.is_ok());
    DRIVER.with(|driver| assert!(driver.borrow().waits.is_empty()));
}

#[test]
fn failed_encode_submit_retains_both_queue_fences_and_never_reports_conversion_as_encoder_completion()
 {
    reset();
    DRIVER.with(|driver| {
        let mut driver = driver.borrow_mut();
        driver.fail_queue = 43;
        driver.wait_result = 2;
    });
    let mut resources = resources(false, false);
    assert_eq!(
        resources.submit_pair(42_usize as VkQueue, 43_usize as VkQueue, 0),
        Err(VulkanVideoDeviceError::QueueSubmissionFailed(-2))
    );
    DRIVER.with(|driver| assert_eq!(driver.borrow().submissions, [(42, 11), (43, 21)]));
    assert_eq!(
        unsafe { resources.shutdown() },
        Err(VulkanVideoDeviceError::DeviceWaitFailed(2))
    );
    DRIVER.with(|driver| {
        assert_eq!(
            driver.borrow().waits,
            [(vec![11, 21], SLOT_WAIT_TIMEOUT_NS)]
        );
        assert!(driver.borrow().destroyed.is_empty());
    });
    DRIVER.with(|driver| driver.borrow_mut().wait_result = VK_SUCCESS);
    assert!(unsafe { resources.shutdown() }.is_ok());
}
