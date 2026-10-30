//! Injected native calls validate the API profile contract and rollback.
use super::*;
use std::cell::RefCell;

#[derive(Default)]
struct Driver {
    buffer_profile: Option<[u32; 7]>,
    query_profile: Option<[u32; 7]>,
    fail_query: bool,
    destroyed: Vec<&'static str>,
    mapped: [u8; 16],
}
thread_local! { static DRIVER: RefCell<Driver> = RefCell::new(Driver::default()); }

unsafe fn profile_signature(profile: *const VkVideoProfileInfoKhr) -> [u32; 7] {
    let profile = unsafe { &*profile };
    assert_eq!(profile.s_type, VK_STRUCTURE_TYPE_VIDEO_PROFILE_INFO_KHR);
    let usage = unsafe { &*profile.p_next.cast::<VkVideoEncodeUsageInfoKhr>() };
    assert_eq!(usage.s_type, VK_STRUCTURE_TYPE_VIDEO_ENCODE_USAGE_INFO_KHR);
    let h264 = unsafe { &*usage.p_next.cast::<VkVideoEncodeH264ProfileInfoKhr>() };
    assert_eq!(
        h264.s_type,
        VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_PROFILE_INFO_KHR
    );
    assert_eq!(
        profile.video_codec_operation,
        VK_VIDEO_CODEC_OPERATION_ENCODE_H264_BIT_KHR
    );
    [
        profile.chroma_subsampling,
        profile.luma_bit_depth,
        profile.chroma_bit_depth,
        usage.video_usage_hints,
        usage.video_content_hints,
        u32::try_from(usage.tuning_mode).unwrap(),
        u32::try_from(h264.std_profile_idc).unwrap(),
    ]
}
unsafe extern "system" fn create_buffer(
    _: VkDevice,
    info: *const VkBufferCreateInfo,
    _: *const VkAllocationCallbacks,
    out: *mut VkBuffer,
) -> VkResult {
    let info = unsafe { &*info };
    assert_ne!(info.usage & VK_BUFFER_USAGE_VIDEO_ENCODE_DST_BIT_KHR, 0);
    // Independent VUID 04814 oracle: an encode destination needs a nonempty
    // video profile list unless the explicitly enabled independent flag is set.
    assert!(!info.p_next.is_null());
    let profiles = unsafe { &*info.p_next.cast::<VkVideoProfileListInfoKhr>() };
    assert_eq!(
        profiles.s_type,
        VK_STRUCTURE_TYPE_VIDEO_PROFILE_LIST_INFO_KHR
    );
    assert_eq!(profiles.profile_count, 1);
    let signature = unsafe { profile_signature(profiles.profiles) };
    DRIVER.with(|driver| driver.borrow_mut().buffer_profile = Some(signature));
    unsafe {
        *out = 11;
    }
    VK_SUCCESS
}
unsafe extern "system" fn requirements(_: VkDevice, _: VkBuffer, out: *mut VkMemoryRequirements) {
    unsafe {
        *out = VkMemoryRequirements {
            size: 4096,
            alignment: 256,
            memory_type_bits: 1,
        };
    }
}
unsafe extern "system" fn allocate(
    _: VkDevice,
    _: *const VkMemoryAllocateInfo,
    _: *const VkAllocationCallbacks,
    out: *mut VkDeviceMemory,
) -> VkResult {
    unsafe {
        *out = 12;
    }
    VK_SUCCESS
}
unsafe extern "system" fn bind(_: VkDevice, _: VkBuffer, _: VkDeviceMemory, _: u64) -> VkResult {
    VK_SUCCESS
}
unsafe extern "system" fn map(
    _: VkDevice,
    _: VkDeviceMemory,
    _: u64,
    _: u64,
    _: u32,
    out: *mut *mut c_void,
) -> VkResult {
    DRIVER.with(|driver| unsafe {
        *out = driver.borrow_mut().mapped.as_mut_ptr().cast();
    });
    VK_SUCCESS
}
unsafe extern "system" fn create_query(
    _: VkDevice,
    info: *const VkQueryPoolCreateInfo,
    _: *const VkAllocationCallbacks,
    out: *mut VkQueryPool,
) -> VkResult {
    let info = unsafe { &*info };
    let feedback = unsafe {
        &*info
            .p_next
            .cast::<VkQueryPoolVideoEncodeFeedbackCreateInfoKhr>()
    };
    assert_eq!(
        feedback.s_type,
        VK_STRUCTURE_TYPE_QUERY_POOL_VIDEO_ENCODE_FEEDBACK_CREATE_INFO_KHR
    );
    let signature = unsafe { profile_signature(feedback.p_next.cast()) };
    DRIVER.with(|driver| {
        let mut driver = driver.borrow_mut();
        driver.query_profile = Some(signature);
        if driver.fail_query {
            return -2;
        }
        unsafe {
            *out = 13;
        }
        VK_SUCCESS
    })
}
unsafe extern "system" fn destroy_buffer(
    _: VkDevice,
    _: VkBuffer,
    _: *const VkAllocationCallbacks,
) {
    DRIVER.with(|driver| driver.borrow_mut().destroyed.push("buffer"));
}
unsafe extern "system" fn free_memory(
    _: VkDevice,
    _: VkDeviceMemory,
    _: *const VkAllocationCallbacks,
) {
    DRIVER.with(|driver| driver.borrow_mut().destroyed.push("memory"));
}
unsafe extern "system" fn unmap(_: VkDevice, _: VkDeviceMemory) {
    DRIVER.with(|driver| driver.borrow_mut().destroyed.push("mapping"));
}
unsafe extern "system" fn destroy_query(
    _: VkDevice,
    _: VkQueryPool,
    _: *const VkAllocationCallbacks,
) {
    DRIVER.with(|driver| driver.borrow_mut().destroyed.push("query"));
}
fn cleanup() -> Functions {
    Functions {
        destroy_buffer,
        free_memory,
        unmap_memory: unmap,
        destroy_query_pool: destroy_query,
    }
}
unsafe fn slot() -> Result<EncodeSlot, VulkanVideoDeviceError> {
    let mut memory = VkPhysicalDeviceMemoryProperties {
        memory_type_count: 1,
        ..Default::default()
    };
    memory.memory_types[0].property_flags =
        VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
    unsafe {
        create_slot(
            1_usize as VkDevice,
            4096,
            &memory,
            create_buffer,
            requirements,
            allocate,
            bind,
            map,
            create_query,
            cleanup(),
        )
    }
}
#[test]
fn encode_destination_and_feedback_use_the_same_profile_and_cleanup_in_order() {
    DRIVER.with(|driver| *driver.borrow_mut() = Driver::default());
    let slot = unsafe { slot() }.unwrap();
    let resources = EncodeResources {
        device_address: 1,
        functions: cleanup(),
        slots: vec![slot],
        active: true,
    };
    drop(resources);
    DRIVER.with(|driver| {
        let driver = driver.borrow();
        assert!(driver.buffer_profile.is_some());
        assert_eq!(driver.buffer_profile, driver.query_profile);
        assert_eq!(driver.destroyed, ["query", "mapping", "buffer", "memory"]);
    });
}
#[test]
fn query_failure_releases_partial_slot_without_destroying_a_nonexistent_query() {
    DRIVER.with(|driver| {
        *driver.borrow_mut() = Driver {
            fail_query: true,
            ..Default::default()
        }
    });
    assert!(matches!(
        unsafe { slot() },
        Err(VulkanVideoDeviceError::EncodeQueryPoolCreationFailed(-2))
    ));
    DRIVER.with(|driver| assert_eq!(driver.borrow().destroyed, ["mapping", "buffer", "memory"]));
}
