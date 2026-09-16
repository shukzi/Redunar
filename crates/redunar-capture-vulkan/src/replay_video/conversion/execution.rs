use super::*;
use crate::ffi::{
    VkCommandBuffer, VkCommandBufferAllocateInfo, VkCommandPool, VkCommandPoolCreateInfo, VkFence,
    VkFenceCreateInfo, VkQueue, VkSemaphore, VkSemaphoreCreateInfo,
};

type VkQueryPool = u64;

const VK_STRUCTURE_TYPE_FENCE_CREATE_INFO: i32 = 8;
const VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO: i32 = 9;
const VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO: i32 = 39;
const VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO: i32 = 40;
const VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT: u32 = 0x2;
const VK_COMMAND_BUFFER_LEVEL_PRIMARY: i32 = 0;
const VK_FENCE_CREATE_SIGNALED_BIT: u32 = 0x1;
const VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO: i32 = 42;
const VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET: i32 = 35;
const VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER_2: i32 = 1_000_314_002;
const VK_STRUCTURE_TYPE_DEPENDENCY_INFO: i32 = 1_000_314_003;
const VK_STRUCTURE_TYPE_SUBMIT_INFO_2: i32 = 1_000_314_004;
const VK_STRUCTURE_TYPE_SEMAPHORE_SUBMIT_INFO: i32 = 1_000_314_005;
const VK_STRUCTURE_TYPE_COMMAND_BUFFER_SUBMIT_INFO: i32 = 1_000_314_006;
const VK_STRUCTURE_TYPE_VIDEO_PICTURE_RESOURCE_INFO_KHR: i32 = 1_000_023_002;
const VK_STRUCTURE_TYPE_VIDEO_BEGIN_CODING_INFO_KHR: i32 = 1_000_023_008;
const VK_STRUCTURE_TYPE_VIDEO_END_CODING_INFO_KHR: i32 = 1_000_023_009;
const VK_STRUCTURE_TYPE_VIDEO_CODING_CONTROL_INFO_KHR: i32 = 1_000_023_010;
const VK_STRUCTURE_TYPE_VIDEO_REFERENCE_SLOT_INFO_KHR: i32 = 1_000_023_011;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_PICTURE_INFO_KHR: i32 = 1_000_038_003;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_DPB_SLOT_INFO_KHR: i32 = 1_000_038_004;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_NALU_SLICE_INFO_KHR: i32 = 1_000_038_005;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_RATE_CONTROL_INFO_KHR: i32 = 1_000_038_008;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_RATE_CONTROL_LAYER_INFO_KHR: i32 = 1_000_038_009;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_INFO_KHR: i32 = 1_000_299_000;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_RATE_CONTROL_INFO_KHR: i32 = 1_000_299_001;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_RATE_CONTROL_LAYER_INFO_KHR: i32 = 1_000_299_002;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_QUALITY_LEVEL_INFO_KHR: i32 = 1_000_299_008;
const VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE: i32 = 6;
const VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT: u32 = 0x1;
const VK_PIPELINE_BIND_POINT_COMPUTE: i32 = 1;
const VK_DESCRIPTOR_TYPE_STORAGE_BUFFER: i32 = 7;
const VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER: i32 = 1;
const VK_SHADER_STAGE_COMPUTE_BIT: u32 = 0x20;
const VK_IMAGE_LAYOUT_UNDEFINED: i32 = 0;
const VK_IMAGE_LAYOUT_GENERAL: i32 = 1;
const VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL: i32 = 5;
const VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL: i32 = 6;
const VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL: i32 = 7;
const VK_IMAGE_LAYOUT_VIDEO_ENCODE_SRC_KHR: i32 = 1_000_299_001;
const VK_IMAGE_LAYOUT_VIDEO_ENCODE_DPB_KHR: i32 = 1_000_299_002;
const VK_IMAGE_ASPECT_COLOR_BIT: u32 = 0x1;
const VK_IMAGE_ASPECT_PLANE_0_BIT: u32 = 0x10;
const VK_IMAGE_ASPECT_PLANE_1_BIT: u32 = 0x20;
const VK_PIPELINE_STAGE_2_TOP_OF_PIPE_BIT: u64 = 0x1;
const VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT: u64 = 0x800;
const VK_PIPELINE_STAGE_2_TRANSFER_BIT: u64 = 0x1000;
const VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT: u64 = 0x1_0000;
const VK_PIPELINE_STAGE_2_VIDEO_ENCODE_BIT_KHR: u64 = 0x0800_0000;
const VK_ACCESS_2_SHADER_STORAGE_WRITE_BIT: u64 = 0x4_0000_0000;
const VK_ACCESS_2_SHADER_SAMPLED_READ_BIT: u64 = 0x1_0000_0000;
const VK_ACCESS_2_TRANSFER_READ_BIT: u64 = 0x800;
const VK_ACCESS_2_TRANSFER_WRITE_BIT: u64 = 0x1000;
const VK_ACCESS_2_VIDEO_ENCODE_READ_BIT_KHR: u64 = 0x20_0000_0000;
const VK_ACCESS_2_VIDEO_ENCODE_WRITE_BIT_KHR: u64 = 0x40_0000_0000;
const VK_DEPENDENCY_BY_REGION_BIT: u32 = 0x1;
const VK_QUEUE_FAMILY_IGNORED: u32 = u32::MAX;
const VK_QUEUE_FAMILY_FOREIGN_EXT: u32 = u32::MAX - 1;
const VK_VIDEO_CODING_CONTROL_RESET_BIT_KHR: u32 = 0x1;
const VK_VIDEO_CODING_CONTROL_ENCODE_RATE_CONTROL_BIT_KHR: u32 = 0x2;
const VK_VIDEO_CODING_CONTROL_ENCODE_QUALITY_LEVEL_BIT_KHR: u32 = 0x4;
const VK_VIDEO_ENCODE_RATE_CONTROL_MODE_CBR_BIT_KHR: i32 = 0x2;
const VK_VIDEO_ENCODE_H264_RATE_CONTROL_REGULAR_GOP_BIT_KHR: u32 = 0x2;
const VK_QUERY_RESULT_64_BIT: u32 = 0x1;
const VK_QUERY_RESULT_WAIT_BIT: u32 = 0x2;
const STD_VIDEO_H264_PICTURE_TYPE_P: i32 = 0;
const STD_VIDEO_H264_PICTURE_TYPE_IDR: i32 = 5;
const STD_VIDEO_H264_SLICE_TYPE_P: i32 = 0;
const STD_VIDEO_H264_SLICE_TYPE_I: i32 = 2;
const STD_VIDEO_H264_NO_REFERENCE_PICTURE: u8 = 0xff;
const SLOT_WAIT_TIMEOUT_NS: u64 = 2_000_000_000;

pub(super) const fn initial_rate_control_flags(frame_index: u64) -> Option<u32> {
    if frame_index == 0 {
        Some(
            VK_VIDEO_CODING_CONTROL_RESET_BIT_KHR
                | VK_VIDEO_CODING_CONTROL_ENCODE_RATE_CONTROL_BIT_KHR
                | VK_VIDEO_CODING_CONTROL_ENCODE_QUALITY_LEVEL_BIT_KHR,
        )
    } else {
        None
    }
}

#[repr(C)]
struct VkCommandBufferBeginInfo {
    s_type: i32,
    p_next: *const c_void,
    flags: u32,
    inheritance_info: *const c_void,
}

#[repr(C)]
struct VkImageSubresourceRange {
    aspect_mask: u32,
    base_mip_level: u32,
    level_count: u32,
    base_array_layer: u32,
    layer_count: u32,
}

#[repr(C)]
struct VkImageMemoryBarrier2 {
    s_type: i32,
    p_next: *const c_void,
    src_stage_mask: u64,
    src_access_mask: u64,
    dst_stage_mask: u64,
    dst_access_mask: u64,
    old_layout: i32,
    new_layout: i32,
    src_queue_family_index: u32,
    dst_queue_family_index: u32,
    image: VkImage,
    subresource_range: VkImageSubresourceRange,
}

#[repr(C)]
struct VkDependencyInfo {
    s_type: i32,
    p_next: *const c_void,
    dependency_flags: u32,
    memory_barrier_count: u32,
    memory_barriers: *const c_void,
    buffer_memory_barrier_count: u32,
    buffer_memory_barriers: *const c_void,
    image_memory_barrier_count: u32,
    image_memory_barriers: *const VkImageMemoryBarrier2,
}

#[repr(C)]
struct VkOffset3d {
    x: i32,
    y: i32,
    z: i32,
}

#[repr(C)]
struct VkImageSubresourceLayers {
    aspect_mask: u32,
    mip_level: u32,
    base_array_layer: u32,
    layer_count: u32,
}

#[repr(C)]
struct VkImageCopy {
    src_subresource: VkImageSubresourceLayers,
    src_offset: VkOffset3d,
    dst_subresource: VkImageSubresourceLayers,
    dst_offset: VkOffset3d,
    extent: VkExtent3d,
}

#[repr(C)]
struct SourcePushConstants {
    width: u32,
    height: u32,
    stride_words: u32,
    offset_words: u32,
    format: u32,
}

#[repr(C)]
struct VkDescriptorBufferInfo {
    buffer: VkBuffer,
    offset: u64,
    range: u64,
}

#[repr(C)]
struct VkDescriptorImageInfo {
    sampler: VkSampler,
    image_view: VkImageView,
    image_layout: i32,
}

#[repr(C)]
struct VkWriteDescriptorSet {
    s_type: i32,
    p_next: *const c_void,
    dst_set: VkDescriptorSet,
    dst_binding: u32,
    dst_array_element: u32,
    descriptor_count: u32,
    descriptor_type: i32,
    image_info: *const c_void,
    buffer_info: *const VkDescriptorBufferInfo,
    texel_buffer_view: *const c_void,
}

#[repr(C)]
struct VkSemaphoreSubmitInfo {
    s_type: i32,
    p_next: *const c_void,
    semaphore: VkSemaphore,
    value: u64,
    stage_mask: u64,
    device_index: u32,
}

#[repr(C)]
struct VkCommandBufferSubmitInfo {
    s_type: i32,
    p_next: *const c_void,
    command_buffer: VkCommandBuffer,
    device_mask: u32,
}

#[repr(C)]
struct VkSubmitInfo2 {
    s_type: i32,
    p_next: *const c_void,
    flags: u32,
    wait_semaphore_info_count: u32,
    wait_semaphore_infos: *const VkSemaphoreSubmitInfo,
    command_buffer_info_count: u32,
    command_buffer_infos: *const VkCommandBufferSubmitInfo,
    signal_semaphore_info_count: u32,
    signal_semaphore_infos: *const VkSemaphoreSubmitInfo,
}

#[repr(C)]
struct VkVideoPictureResourceInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    coded_offset: [i32; 2],
    coded_extent: VkExtent2d,
    base_array_layer: u32,
    image_view_binding: VkImageView,
}

#[repr(C)]
struct VkVideoReferenceSlotInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    slot_index: i32,
    picture_resource: *const VkVideoPictureResourceInfoKhr,
}

#[repr(C)]
struct VkVideoBeginCodingInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    flags: u32,
    video_session: u64,
    video_session_parameters: u64,
    reference_slot_count: u32,
    reference_slots: *const VkVideoReferenceSlotInfoKhr,
}

#[repr(C)]
struct VkVideoEndCodingInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    flags: u32,
}

#[repr(C)]
struct VkVideoCodingControlInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    flags: u32,
}

#[repr(C)]
struct VkVideoEncodeH264QpKhr {
    qp_i: i32,
    qp_p: i32,
    qp_b: i32,
}

#[repr(C)]
struct VkVideoEncodeH264FrameSizeKhr {
    frame_i_size: u32,
    frame_p_size: u32,
    frame_b_size: u32,
}

#[repr(C)]
struct VkVideoEncodeH264RateControlLayerInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    use_min_qp: u32,
    min_qp: VkVideoEncodeH264QpKhr,
    use_max_qp: u32,
    max_qp: VkVideoEncodeH264QpKhr,
    use_max_frame_size: u32,
    max_frame_size: VkVideoEncodeH264FrameSizeKhr,
}

#[repr(C)]
struct VkVideoEncodeRateControlLayerInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    average_bitrate: u64,
    max_bitrate: u64,
    frame_rate_numerator: u32,
    frame_rate_denominator: u32,
}

#[repr(C)]
struct VkVideoEncodeH264RateControlInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    flags: u32,
    gop_frame_count: u32,
    idr_period: u32,
    consecutive_b_frame_count: u32,
    temporal_layer_count: u32,
}

#[repr(C)]
struct VkVideoEncodeRateControlInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    flags: u32,
    rate_control_mode: i32,
    layer_count: u32,
    layers: *const VkVideoEncodeRateControlLayerInfoKhr,
    virtual_buffer_size_ms: u32,
    initial_virtual_buffer_size_ms: u32,
}

#[repr(C)]
struct VkVideoEncodeQualityLevelInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    quality_level: u32,
}

#[repr(C)]
struct StdVideoEncodeH264ReferenceListsInfo {
    flags: u32,
    num_ref_idx_l0_active_minus1: u8,
    num_ref_idx_l1_active_minus1: u8,
    ref_pic_list0: [u8; 32],
    ref_pic_list1: [u8; 32],
    ref_list0_mod_op_count: u8,
    ref_list1_mod_op_count: u8,
    ref_pic_marking_op_count: u8,
    reserved1: [u8; 7],
    ref_list0_mod_operations: *const c_void,
    ref_list1_mod_operations: *const c_void,
    ref_pic_marking_operations: *const c_void,
}

#[repr(C)]
struct StdVideoEncodeH264PictureInfo {
    flags: u32,
    seq_parameter_set_id: u8,
    pic_parameter_set_id: u8,
    idr_pic_id: u16,
    primary_pic_type: i32,
    frame_num: u32,
    pic_order_cnt: i32,
    temporal_id: u8,
    reserved1: [u8; 3],
    ref_lists: *const StdVideoEncodeH264ReferenceListsInfo,
}

#[repr(C)]
struct StdVideoEncodeH264ReferenceInfo {
    flags: u32,
    primary_pic_type: i32,
    frame_num: u32,
    pic_order_cnt: i32,
    long_term_pic_num: u16,
    long_term_frame_idx: u16,
    temporal_id: u8,
}

#[repr(C)]
struct StdVideoEncodeH264SliceHeader {
    flags: u32,
    first_mb_in_slice: u32,
    slice_type: i32,
    slice_alpha_c0_offset_div2: i8,
    slice_beta_offset_div2: i8,
    slice_qp_delta: i8,
    reserved1: u8,
    cabac_init_idc: i32,
    disable_deblocking_filter_idc: i32,
    weight_table: *const c_void,
}

#[repr(C)]
struct VkVideoEncodeH264NaluSliceInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    constant_qp: i32,
    std_slice_header: *const StdVideoEncodeH264SliceHeader,
}

#[repr(C)]
struct VkVideoEncodeH264PictureInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    nalu_slice_entry_count: u32,
    nalu_slice_entries: *const VkVideoEncodeH264NaluSliceInfoKhr,
    std_picture_info: *const StdVideoEncodeH264PictureInfo,
    generate_prefix_nalu: u32,
}

#[repr(C)]
struct VkVideoEncodeH264DpbSlotInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    std_reference_info: *const StdVideoEncodeH264ReferenceInfo,
}

#[repr(C)]
struct VkVideoEncodeInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    flags: u32,
    dst_buffer: VkBuffer,
    dst_buffer_offset: u64,
    dst_buffer_range: u64,
    src_picture_resource: VkVideoPictureResourceInfoKhr,
    setup_reference_slot: *const VkVideoReferenceSlotInfoKhr,
    reference_slot_count: u32,
    reference_slots: *const VkVideoReferenceSlotInfoKhr,
    preceding_externally_encoded_bytes: u32,
}

#[repr(C)]
struct VkMappedMemoryRange {
    s_type: i32,
    p_next: *const c_void,
    memory: VkDeviceMemory,
    offset: u64,
    size: u64,
}

type CreateCommandPool = unsafe extern "system" fn(
    VkDevice,
    *const VkCommandPoolCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkCommandPool,
) -> VkResult;
type DestroyCommandPool =
    unsafe extern "system" fn(VkDevice, VkCommandPool, *const VkAllocationCallbacks);
type AllocateCommandBuffers = unsafe extern "system" fn(
    VkDevice,
    *const VkCommandBufferAllocateInfo,
    *mut VkCommandBuffer,
) -> VkResult;
type CreateSemaphore = unsafe extern "system" fn(
    VkDevice,
    *const VkSemaphoreCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkSemaphore,
) -> VkResult;
type DestroySemaphore =
    unsafe extern "system" fn(VkDevice, VkSemaphore, *const VkAllocationCallbacks);
type CreateFence = unsafe extern "system" fn(
    VkDevice,
    *const VkFenceCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkFence,
) -> VkResult;
type DestroyFence = unsafe extern "system" fn(VkDevice, VkFence, *const VkAllocationCallbacks);
type DeviceWaitIdle = unsafe extern "system" fn(VkDevice) -> VkResult;
type WaitForFences = unsafe extern "system" fn(VkDevice, u32, *const VkFence, u32, u64) -> VkResult;
type ResetFences = unsafe extern "system" fn(VkDevice, u32, *const VkFence) -> VkResult;
type ResetCommandBuffer = unsafe extern "system" fn(VkCommandBuffer, u32) -> VkResult;
type BeginCommandBuffer =
    unsafe extern "system" fn(VkCommandBuffer, *const VkCommandBufferBeginInfo) -> VkResult;
type EndCommandBuffer = unsafe extern "system" fn(VkCommandBuffer) -> VkResult;
type CmdPipelineBarrier2 = unsafe extern "system" fn(VkCommandBuffer, *const VkDependencyInfo);
type CmdBindPipeline = unsafe extern "system" fn(VkCommandBuffer, i32, VkPipeline);
type CmdBindDescriptorSets = unsafe extern "system" fn(
    VkCommandBuffer,
    i32,
    VkPipelineLayout,
    u32,
    u32,
    *const VkDescriptorSet,
    u32,
    *const u32,
);
type CmdPushConstants =
    unsafe extern "system" fn(VkCommandBuffer, VkPipelineLayout, u32, u32, u32, *const c_void);
type CmdDispatch = unsafe extern "system" fn(VkCommandBuffer, u32, u32, u32);
type CmdCopyImage =
    unsafe extern "system" fn(VkCommandBuffer, VkImage, i32, VkImage, i32, u32, *const VkImageCopy);
type UpdateDescriptorSets =
    unsafe extern "system" fn(VkDevice, u32, *const VkWriteDescriptorSet, u32, *const c_void);
type QueueSubmit2 =
    unsafe extern "system" fn(VkQueue, u32, *const VkSubmitInfo2, VkFence) -> VkResult;
type CmdResetQueryPool = unsafe extern "system" fn(VkCommandBuffer, VkQueryPool, u32, u32);
type CmdBeginQuery = unsafe extern "system" fn(VkCommandBuffer, VkQueryPool, u32, u32);
type CmdEndQuery = unsafe extern "system" fn(VkCommandBuffer, VkQueryPool, u32);
type CmdBeginVideoCoding =
    unsafe extern "system" fn(VkCommandBuffer, *const VkVideoBeginCodingInfoKhr);
type CmdControlVideoCoding =
    unsafe extern "system" fn(VkCommandBuffer, *const VkVideoCodingControlInfoKhr);
type CmdEncodeVideo = unsafe extern "system" fn(VkCommandBuffer, *const VkVideoEncodeInfoKhr);
type CmdEndVideoCoding = unsafe extern "system" fn(VkCommandBuffer, *const VkVideoEndCodingInfoKhr);
type GetQueryPoolResults = unsafe extern "system" fn(
    VkDevice,
    VkQueryPool,
    u32,
    u32,
    usize,
    *mut c_void,
    u64,
    u32,
) -> VkResult;
type InvalidateMappedMemoryRanges =
    unsafe extern "system" fn(VkDevice, u32, *const VkMappedMemoryRange) -> VkResult;

#[derive(Clone, Copy)]
struct Functions {
    destroy_command_pool: DestroyCommandPool,
    destroy_semaphore: DestroySemaphore,
    destroy_fence: DestroyFence,
    device_wait_idle: DeviceWaitIdle,
    wait_for_fences: WaitForFences,
    reset_fences: ResetFences,
    reset_command_buffer: ResetCommandBuffer,
    begin_command_buffer: BeginCommandBuffer,
    end_command_buffer: EndCommandBuffer,
    cmd_pipeline_barrier2: CmdPipelineBarrier2,
    cmd_bind_pipeline: CmdBindPipeline,
    cmd_bind_descriptor_sets: CmdBindDescriptorSets,
    cmd_push_constants: CmdPushConstants,
    cmd_dispatch: CmdDispatch,
    cmd_copy_image: CmdCopyImage,
    update_descriptor_sets: UpdateDescriptorSets,
    queue_submit2: QueueSubmit2,
    cmd_reset_query_pool: CmdResetQueryPool,
    cmd_begin_query: CmdBeginQuery,
    cmd_end_query: CmdEndQuery,
    cmd_begin_video_coding: CmdBeginVideoCoding,
    cmd_control_video_coding: CmdControlVideoCoding,
    cmd_encode_video: CmdEncodeVideo,
    cmd_end_video_coding: CmdEndVideoCoding,
    get_query_pool_results: GetQueryPoolResults,
    invalidate_mapped_memory_ranges: InvalidateMappedMemoryRanges,
}

pub(super) struct ExecutionResources {
    device_address: usize,
    functions: Functions,
    compute_pool: VkCommandPool,
    encode_pool: VkCommandPool,
    pub(super) compute_commands: Vec<usize>,
    pub(super) encode_commands: Vec<usize>,
    pub(super) conversion_complete: Vec<VkSemaphore>,
    pub(super) slot_complete: Vec<VkFence>,
    active: bool,
}

impl ExecutionResources {
    pub(super) unsafe fn new(
        owner: &VulkanVideoH264Parameters,
        slot_count: usize,
    ) -> Result<Self, VulkanVideoDeviceError> {
        let device = owner.session.device.device_address as VkDevice;
        macro_rules! load {
            ($symbol:literal, $ty:ty) => {{
                let name = concat!($symbol, "\0");
                let Some(raw) = (unsafe {
                    resolve_device_command(&owner.session.device, name.as_ptr().cast())
                }) else {
                    return Err(VulkanVideoDeviceError::DeviceSymbolUnavailable($symbol));
                };
                // SAFETY: the Vulkan command name fixes this ABI.
                unsafe { mem::transmute::<unsafe extern "system" fn(), $ty>(raw) }
            }};
        }
        let create_command_pool = load!("vkCreateCommandPool", CreateCommandPool);
        let destroy_command_pool = load!("vkDestroyCommandPool", DestroyCommandPool);
        let allocate_command_buffers = load!("vkAllocateCommandBuffers", AllocateCommandBuffers);
        let create_semaphore = load!("vkCreateSemaphore", CreateSemaphore);
        let destroy_semaphore = load!("vkDestroySemaphore", DestroySemaphore);
        let create_fence = load!("vkCreateFence", CreateFence);
        let destroy_fence = load!("vkDestroyFence", DestroyFence);
        let device_wait_idle = load!("vkDeviceWaitIdle", DeviceWaitIdle);
        let wait_for_fences = load!("vkWaitForFences", WaitForFences);
        let reset_fences = load!("vkResetFences", ResetFences);
        let reset_command_buffer = load!("vkResetCommandBuffer", ResetCommandBuffer);
        let begin_command_buffer = load!("vkBeginCommandBuffer", BeginCommandBuffer);
        let end_command_buffer = load!("vkEndCommandBuffer", EndCommandBuffer);
        let cmd_pipeline_barrier2 = load!("vkCmdPipelineBarrier2", CmdPipelineBarrier2);
        let cmd_bind_pipeline = load!("vkCmdBindPipeline", CmdBindPipeline);
        let cmd_bind_descriptor_sets = load!("vkCmdBindDescriptorSets", CmdBindDescriptorSets);
        let cmd_push_constants = load!("vkCmdPushConstants", CmdPushConstants);
        let cmd_dispatch = load!("vkCmdDispatch", CmdDispatch);
        let cmd_copy_image = load!("vkCmdCopyImage", CmdCopyImage);
        let update_descriptor_sets = load!("vkUpdateDescriptorSets", UpdateDescriptorSets);
        let queue_submit2 = load!("vkQueueSubmit2", QueueSubmit2);
        let cmd_reset_query_pool = load!("vkCmdResetQueryPool", CmdResetQueryPool);
        let cmd_begin_query = load!("vkCmdBeginQuery", CmdBeginQuery);
        let cmd_end_query = load!("vkCmdEndQuery", CmdEndQuery);
        let cmd_begin_video_coding = load!("vkCmdBeginVideoCodingKHR", CmdBeginVideoCoding);
        let cmd_control_video_coding = load!("vkCmdControlVideoCodingKHR", CmdControlVideoCoding);
        let cmd_encode_video = load!("vkCmdEncodeVideoKHR", CmdEncodeVideo);
        let cmd_end_video_coding = load!("vkCmdEndVideoCodingKHR", CmdEndVideoCoding);
        let get_query_pool_results = load!("vkGetQueryPoolResults", GetQueryPoolResults);
        let invalidate_mapped_memory_ranges = load!(
            "vkInvalidateMappedMemoryRanges",
            InvalidateMappedMemoryRanges
        );
        let functions = Functions {
            destroy_command_pool,
            destroy_semaphore,
            destroy_fence,
            device_wait_idle,
            wait_for_fences,
            reset_fences,
            reset_command_buffer,
            begin_command_buffer,
            end_command_buffer,
            cmd_pipeline_barrier2,
            cmd_bind_pipeline,
            cmd_bind_descriptor_sets,
            cmd_push_constants,
            cmd_dispatch,
            cmd_copy_image,
            update_descriptor_sets,
            queue_submit2,
            cmd_reset_query_pool,
            cmd_begin_query,
            cmd_end_query,
            cmd_begin_video_coding,
            cmd_control_video_coding,
            cmd_encode_video,
            cmd_end_video_coding,
            get_query_pool_results,
            invalidate_mapped_memory_ranges,
        };
        let mut resources = Self {
            device_address: device.addr(),
            functions,
            compute_pool: 0,
            encode_pool: 0,
            compute_commands: Vec::new(),
            encode_commands: Vec::new(),
            conversion_complete: Vec::new(),
            slot_complete: Vec::new(),
            active: true,
        };
        resources.compute_pool = unsafe {
            create_pool(
                device,
                owner.session.device.candidate.compute_queue_family_index,
                create_command_pool,
            )
        }?;
        resources.encode_pool = unsafe {
            create_pool(
                device,
                owner.session.device.candidate.queue_family_index,
                create_command_pool,
            )
        }?;
        resources.compute_commands = unsafe {
            allocate_commands(
                device,
                resources.compute_pool,
                slot_count,
                allocate_command_buffers,
            )
        }?;
        resources.encode_commands = unsafe {
            allocate_commands(
                device,
                resources.encode_pool,
                slot_count,
                allocate_command_buffers,
            )
        }?;
        let semaphore_info = VkSemaphoreCreateInfo {
            s_type: VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
        };
        let fence_info = VkFenceCreateInfo {
            s_type: VK_STRUCTURE_TYPE_FENCE_CREATE_INFO,
            p_next: ptr::null(),
            flags: VK_FENCE_CREATE_SIGNALED_BIT,
        };
        for _ in 0..slot_count {
            let mut semaphore = 0;
            // SAFETY: create info and output storage are valid.
            let result = unsafe {
                create_semaphore(
                    device,
                    &raw const semaphore_info,
                    ptr::null(),
                    &raw mut semaphore,
                )
            };
            if result != VK_SUCCESS || semaphore == 0 {
                return Err(VulkanVideoDeviceError::SemaphoreCreationFailed(result));
            }
            resources.conversion_complete.push(semaphore);
            let mut fence = 0;
            // SAFETY: create info and output storage are valid.
            let result =
                unsafe { create_fence(device, &raw const fence_info, ptr::null(), &raw mut fence) };
            if result != VK_SUCCESS || fence == 0 {
                return Err(VulkanVideoDeviceError::FenceCreationFailed(result));
            }
            resources.slot_complete.push(fence);
        }
        Ok(resources)
    }

    pub(super) unsafe fn shutdown(&mut self) {
        if !self.active {
            return;
        }
        let device = self.device_address as VkDevice;
        // The daemon owns this logical device exclusively, so device idle does
        // not stall the game process or another application's Vulkan work.
        let _ = unsafe { (self.functions.device_wait_idle)(device) };
        self.destroy_owned();
    }

    fn destroy_owned(&mut self) {
        if !self.active {
            return;
        }
        let device = self.device_address as VkDevice;
        // SAFETY: every handle belongs to this live device.
        unsafe {
            for fence in self.slot_complete.drain(..) {
                (self.functions.destroy_fence)(device, fence, ptr::null());
            }
            for semaphore in self.conversion_complete.drain(..) {
                (self.functions.destroy_semaphore)(device, semaphore, ptr::null());
            }
            if self.encode_pool != 0 {
                (self.functions.destroy_command_pool)(device, self.encode_pool, ptr::null());
                self.encode_pool = 0;
            }
            if self.compute_pool != 0 {
                (self.functions.destroy_command_pool)(device, self.compute_pool, ptr::null());
                self.compute_pool = 0;
            }
        }
        self.compute_commands.clear();
        self.encode_commands.clear();
        self.active = false;
    }
}

impl Drop for ExecutionResources {
    fn drop(&mut self) {
        if self.active {
            // Construction failures have not submitted work, while normal
            // teardown calls `shutdown` first. Destroying partial handles is safe.
            self.destroy_owned();
        }
    }
}

pub(super) fn encode_frame(
    encoder: &mut VulkanVideoH264Encoder,
    frame: VulkanVideoDmaBufFrame,
    force_keyframe: bool,
) -> Result<Vec<VulkanVideoEncodedAccessUnit>, VulkanVideoDeviceError> {
    if encoder.poisoned {
        return Err(VulkanVideoDeviceError::EncoderPoisoned);
    }
    let slot_index = encoder.next_slot;
    let mut completed = Vec::new();
    match collect_slot(encoder, slot_index) {
        Ok(Some(access_unit)) => completed.push(access_unit),
        Ok(None) => {}
        Err(error) => {
            encoder.poisoned = true;
            return Err(error);
        }
    }
    if let Err(error) = reset_slot(encoder, slot_index) {
        encoder.poisoned = true;
        return Err(error);
    }
    let imported = encoder.parameters.import_frame(frame)?;
    let gop_interval = u32::from(encoder.request().frames_per_second).saturating_mul(2);
    let keyframe =
        force_keyframe || encoder.frame_index == 0 || encoder.gop_position >= gop_interval.max(1);
    if keyframe {
        encoder.gop_position = 0;
        encoder.previous_reference_slot = None;
        if encoder.frame_index != 0 {
            encoder.idr_pic_id = encoder.idr_pic_id.wrapping_add(1);
        }
    }
    let frame_num = encoder.gop_position;
    let picture_order_count = i32::try_from(frame_num.saturating_mul(2)).unwrap_or(i32::MAX);
    if let Err(error) = record_conversion(encoder, slot_index, &imported) {
        encoder.poisoned = true;
        return Err(error);
    }
    if let Err(error) = record_encode(
        encoder,
        slot_index,
        keyframe,
        frame_num,
        picture_order_count,
    ) {
        encoder.poisoned = true;
        return Err(error);
    }
    let metadata = PendingFrame {
        timestamp_ns: imported.timestamp_ns,
        duration_ns: imported.duration_ns,
        requested_keyframe: keyframe,
        imported,
    };
    encoder.pending[slot_index] = Some(metadata);
    if let Err(error) = submit_slot(encoder, slot_index) {
        // Submission failures leave fence/semaphore state ambiguous. Retain
        // the imported DMA-BUF until device-idle teardown and reject further
        // work on this encoder instance.
        encoder.poisoned = true;
        return Err(error);
    }
    encoder.initialized_slots[slot_index] = true;
    encoder.previous_reference_slot = Some(slot_index);
    encoder.previous_reference_was_idr = keyframe;
    encoder.gop_position = encoder.gop_position.saturating_add(1);
    encoder.frame_index = encoder.frame_index.saturating_add(1);
    encoder.next_slot = (slot_index + 1) % encoder.slots.len();
    Ok(completed)
}

pub(super) fn drain(
    encoder: &mut VulkanVideoH264Encoder,
) -> Result<Vec<VulkanVideoEncodedAccessUnit>, VulkanVideoDeviceError> {
    if encoder.poisoned {
        return Err(VulkanVideoDeviceError::EncoderPoisoned);
    }
    let mut completed = Vec::new();
    for slot_index in 0..encoder.slots.len() {
        if encoder.pending[slot_index].is_some() {
            match collect_slot(encoder, slot_index) {
                Ok(Some(access_unit)) => completed.push(access_unit),
                Ok(None) => {}
                Err(error) => {
                    encoder.poisoned = true;
                    return Err(error);
                }
            }
        }
    }
    completed.sort_by_key(|unit| unit.timestamp_ns);
    Ok(completed)
}

fn collect_slot(
    encoder: &mut VulkanVideoH264Encoder,
    slot_index: usize,
) -> Result<Option<VulkanVideoEncodedAccessUnit>, VulkanVideoDeviceError> {
    let device = encoder.parameters.session.device.device_address as VkDevice;
    let fence = encoder.execution.slot_complete[slot_index];
    // SAFETY: this fence belongs to the selected slot and remains live.
    let result = unsafe {
        (encoder.execution.functions.wait_for_fences)(
            device,
            1,
            &raw const fence,
            1,
            SLOT_WAIT_TIMEOUT_NS,
        )
    };
    if result != VK_SUCCESS {
        return Err(VulkanVideoDeviceError::FenceWaitFailed(result));
    }
    let Some(pending) = encoder.pending[slot_index].take() else {
        return Ok(None);
    };
    let output = &encoder.encode.slots[slot_index];
    let mut bytes_written = 0_u64;
    // SAFETY: the fence proves the query is complete and output storage is valid.
    let result = unsafe {
        (encoder.execution.functions.get_query_pool_results)(
            device,
            output.query_pool,
            0,
            1,
            mem::size_of::<u64>(),
            (&raw mut bytes_written).cast(),
            mem::size_of::<u64>() as u64,
            VK_QUERY_RESULT_64_BIT | VK_QUERY_RESULT_WAIT_BIT,
        )
    };
    if result != VK_SUCCESS {
        return Err(VulkanVideoDeviceError::EncodeFeedbackReadFailed(result));
    }
    if bytes_written == 0 || bytes_written > output.size {
        return Err(VulkanVideoDeviceError::EncodedBytesExceeded);
    }
    if !output.memory_coherent {
        let range = VkMappedMemoryRange {
            s_type: VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE,
            p_next: ptr::null(),
            memory: output.memory,
            offset: 0,
            size: output.allocation_size,
        };
        // SAFETY: the persistent mapping is live and the range is within its allocation.
        let result = unsafe {
            (encoder.execution.functions.invalidate_mapped_memory_ranges)(
                device,
                1,
                &raw const range,
            )
        };
        if result != VK_SUCCESS {
            return Err(VulkanVideoDeviceError::EncodeFeedbackReadFailed(result));
        }
    }
    let byte_count =
        usize::try_from(bytes_written).map_err(|_| VulkanVideoDeviceError::EncodedBytesExceeded)?;
    // SAFETY: the persistent mapping covers `output.size`, validated above,
    // and the completion fence makes hardware writes visible to the host.
    let bytes = unsafe {
        std::slice::from_raw_parts(output.mapped_address as *const u8, byte_count).to_vec()
    };
    drop(pending.imported);
    Ok(Some(VulkanVideoEncodedAccessUnit {
        timestamp_ns: pending.timestamp_ns,
        duration_ns: pending.duration_ns,
        requested_keyframe: pending.requested_keyframe,
        annex_b: bytes.into_boxed_slice(),
    }))
}

fn reset_slot(
    encoder: &mut VulkanVideoH264Encoder,
    slot_index: usize,
) -> Result<(), VulkanVideoDeviceError> {
    for command_address in [
        encoder.execution.compute_commands[slot_index],
        encoder.execution.encode_commands[slot_index],
    ] {
        // SAFETY: each command buffer belongs to a RESET_COMMAND_BUFFER pool and is idle.
        let result = unsafe {
            (encoder.execution.functions.reset_command_buffer)(
                command_address as VkCommandBuffer,
                0,
            )
        };
        if result != VK_SUCCESS {
            return Err(VulkanVideoDeviceError::CommandBufferResetFailed(result));
        }
    }
    Ok(())
}

fn record_conversion(
    encoder: &VulkanVideoH264Encoder,
    slot_index: usize,
    imported: &VulkanVideoImportedBuffer,
) -> Result<(), VulkanVideoDeviceError> {
    let device = encoder.parameters.session.device.device_address as VkDevice;
    let slot = &encoder.slots[slot_index];
    let command = encoder.execution.compute_commands[slot_index] as VkCommandBuffer;
    let mut buffer_info = VkDescriptorBufferInfo {
        buffer: 0,
        offset: 0,
        range: 0,
    };
    let mut image_info = VkDescriptorImageInfo {
        sampler: 0,
        image_view: 0,
        image_layout: VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
    };
    let (descriptor_type, buffer_pointer, image_pointer) = match &imported.resource {
        VulkanVideoImportedResource::Buffer {
            buffer,
            buffer_size,
            ..
        } if encoder.input_mode == VulkanVideoInputMode::LinearBuffer => {
            buffer_info.buffer = *buffer;
            buffer_info.range = *buffer_size;
            (
                VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
                &raw const buffer_info,
                ptr::null(),
            )
        }
        VulkanVideoImportedResource::DrmImage { view, .. }
            if encoder.input_mode == VulkanVideoInputMode::DrmImage =>
        {
            image_info.sampler = encoder.sampler;
            image_info.image_view = *view;
            (
                VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                ptr::null(),
                (&raw const image_info).cast(),
            )
        }
        _ => return Err(VulkanVideoDeviceError::InvalidDmaBufFrame),
    };
    let write = VkWriteDescriptorSet {
        s_type: VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
        p_next: ptr::null(),
        dst_set: slot.descriptor_set,
        dst_binding: 0,
        dst_array_element: 0,
        descriptor_count: 1,
        descriptor_type,
        image_info: image_pointer,
        buffer_info: buffer_pointer,
        texel_buffer_view: ptr::null(),
    };
    // SAFETY: the descriptor set is idle after the slot fence and all handles are live.
    unsafe {
        (encoder.execution.functions.update_descriptor_sets)(
            device,
            1,
            &raw const write,
            0,
            ptr::null(),
        );
    }
    begin_command(encoder.execution.functions, command)?;
    let initialized = encoder.initialized_slots[slot_index];
    let mut initial_barriers = Vec::with_capacity(4);
    if let VulkanVideoImportedResource::DrmImage { image, .. } = &imported.resource {
        let compute_family = encoder
            .parameters
            .session
            .device
            .candidate
            .compute_queue_family_index;
        initial_barriers.push(foreign_image_barrier(
            *image,
            VK_IMAGE_LAYOUT_GENERAL,
            VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
            0,
            0,
            VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT,
            VK_ACCESS_2_SHADER_SAMPLED_READ_BIT,
            compute_family,
            true,
        ));
    }
    initial_barriers.extend([
        image_barrier(
            slot.luma.image,
            VK_IMAGE_ASPECT_COLOR_BIT,
            if initialized {
                VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL
            } else {
                VK_IMAGE_LAYOUT_UNDEFINED
            },
            VK_IMAGE_LAYOUT_GENERAL,
            VK_PIPELINE_STAGE_2_TOP_OF_PIPE_BIT,
            0,
            VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT,
            VK_ACCESS_2_SHADER_STORAGE_WRITE_BIT,
        ),
        image_barrier(
            slot.chroma.image,
            VK_IMAGE_ASPECT_COLOR_BIT,
            if initialized {
                VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL
            } else {
                VK_IMAGE_LAYOUT_UNDEFINED
            },
            VK_IMAGE_LAYOUT_GENERAL,
            VK_PIPELINE_STAGE_2_TOP_OF_PIPE_BIT,
            0,
            VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT,
            VK_ACCESS_2_SHADER_STORAGE_WRITE_BIT,
        ),
        image_barrier(
            slot.encode_input.image,
            VK_IMAGE_ASPECT_COLOR_BIT,
            if initialized {
                VK_IMAGE_LAYOUT_VIDEO_ENCODE_SRC_KHR
            } else {
                VK_IMAGE_LAYOUT_UNDEFINED
            },
            VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
            VK_PIPELINE_STAGE_2_TOP_OF_PIPE_BIT,
            0,
            VK_PIPELINE_STAGE_2_TRANSFER_BIT,
            VK_ACCESS_2_TRANSFER_WRITE_BIT,
        ),
    ]);
    pipeline_barriers(encoder.execution.functions, command, &initial_barriers);
    let push = SourcePushConstants {
        width: imported.width,
        height: imported.height,
        stride_words: imported.stride_words,
        offset_words: imported.offset_words,
        format: match imported.format {
            VulkanPackedPixelFormat::Rgba8 => 0,
            VulkanPackedPixelFormat::Bgra8 => 1,
            VulkanPackedPixelFormat::A2b10g10r10 => 2,
            VulkanPackedPixelFormat::A2r10g10b10 => 3,
        },
    };
    // SAFETY: pipeline, descriptor, layout, and push-constant range match creation.
    unsafe {
        (encoder.execution.functions.cmd_bind_pipeline)(
            command,
            VK_PIPELINE_BIND_POINT_COMPUTE,
            encoder.pipeline,
        );
        (encoder.execution.functions.cmd_bind_descriptor_sets)(
            command,
            VK_PIPELINE_BIND_POINT_COMPUTE,
            encoder.pipeline_layout,
            0,
            1,
            &raw const slot.descriptor_set,
            0,
            ptr::null(),
        );
        (encoder.execution.functions.cmd_push_constants)(
            command,
            encoder.pipeline_layout,
            VK_SHADER_STAGE_COMPUTE_BIT,
            0,
            u32::try_from(mem::size_of::<SourcePushConstants>()).unwrap_or(0),
            (&raw const push).cast(),
        );
        (encoder.execution.functions.cmd_dispatch)(
            command,
            imported.width.div_ceil(16),
            imported.height.div_ceil(16),
            1,
        );
    }
    let mut copy_barriers = Vec::with_capacity(3);
    if let VulkanVideoImportedResource::DrmImage { image, .. } = &imported.resource {
        let compute_family = encoder
            .parameters
            .session
            .device
            .candidate
            .compute_queue_family_index;
        copy_barriers.push(foreign_image_barrier(
            *image,
            VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
            VK_IMAGE_LAYOUT_GENERAL,
            VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT,
            VK_ACCESS_2_SHADER_SAMPLED_READ_BIT,
            0,
            0,
            compute_family,
            false,
        ));
    }
    copy_barriers.extend([
        image_barrier(
            slot.luma.image,
            VK_IMAGE_ASPECT_COLOR_BIT,
            VK_IMAGE_LAYOUT_GENERAL,
            VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
            VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT,
            VK_ACCESS_2_SHADER_STORAGE_WRITE_BIT,
            VK_PIPELINE_STAGE_2_TRANSFER_BIT,
            VK_ACCESS_2_TRANSFER_READ_BIT,
        ),
        image_barrier(
            slot.chroma.image,
            VK_IMAGE_ASPECT_COLOR_BIT,
            VK_IMAGE_LAYOUT_GENERAL,
            VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
            VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT,
            VK_ACCESS_2_SHADER_STORAGE_WRITE_BIT,
            VK_PIPELINE_STAGE_2_TRANSFER_BIT,
            VK_ACCESS_2_TRANSFER_READ_BIT,
        ),
    ]);
    pipeline_barriers(encoder.execution.functions, command, &copy_barriers);
    let luma_copy = image_copy(
        VK_IMAGE_ASPECT_COLOR_BIT,
        VK_IMAGE_ASPECT_PLANE_0_BIT,
        imported.width,
        imported.height,
    );
    let chroma_copy = image_copy(
        VK_IMAGE_ASPECT_COLOR_BIT,
        VK_IMAGE_ASPECT_PLANE_1_BIT,
        imported.width / 2,
        imported.height / 2,
    );
    // SAFETY: source/destination images and plane-compatible extents are valid.
    unsafe {
        (encoder.execution.functions.cmd_copy_image)(
            command,
            slot.luma.image,
            VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
            slot.encode_input.image,
            VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
            1,
            &raw const luma_copy,
        );
        (encoder.execution.functions.cmd_copy_image)(
            command,
            slot.chroma.image,
            VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
            slot.encode_input.image,
            VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
            1,
            &raw const chroma_copy,
        );
    }
    let encode_barrier = [image_barrier(
        slot.encode_input.image,
        VK_IMAGE_ASPECT_COLOR_BIT,
        VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        VK_IMAGE_LAYOUT_VIDEO_ENCODE_SRC_KHR,
        VK_PIPELINE_STAGE_2_TRANSFER_BIT,
        VK_ACCESS_2_TRANSFER_WRITE_BIT,
        0,
        0,
    )];
    pipeline_barriers(encoder.execution.functions, command, &encode_barrier);
    end_command(encoder.execution.functions, command)
}

fn record_encode(
    encoder: &VulkanVideoH264Encoder,
    slot_index: usize,
    keyframe: bool,
    frame_num: u32,
    picture_order_count: i32,
) -> Result<(), VulkanVideoDeviceError> {
    let slot = &encoder.slots[slot_index];
    let output = &encoder.encode.slots[slot_index];
    let command = encoder.execution.encode_commands[slot_index] as VkCommandBuffer;
    begin_command(encoder.execution.functions, command)?;
    // Conversion and encode may use different queue families. The input image
    // is created with concurrent sharing; the semaphore wait plus this acquire
    // barrier makes the compute transfer writes visible to video encode without
    // recording a video-only stage on the compute command buffer.
    let input_acquire = [image_barrier(
        slot.encode_input.image,
        VK_IMAGE_ASPECT_COLOR_BIT,
        VK_IMAGE_LAYOUT_VIDEO_ENCODE_SRC_KHR,
        VK_IMAGE_LAYOUT_VIDEO_ENCODE_SRC_KHR,
        0,
        0,
        VK_PIPELINE_STAGE_2_VIDEO_ENCODE_BIT_KHR,
        VK_ACCESS_2_VIDEO_ENCODE_READ_BIT_KHR,
    )];
    pipeline_barriers(encoder.execution.functions, command, &input_acquire);
    let dpb_initialized = encoder.initialized_slots[slot_index];
    let setup_dpb_dependency = [image_layer_barrier(
        slot.dpb.image,
        slot.dpb_layer,
        VK_IMAGE_ASPECT_COLOR_BIT,
        if dpb_initialized {
            VK_IMAGE_LAYOUT_VIDEO_ENCODE_DPB_KHR
        } else {
            VK_IMAGE_LAYOUT_UNDEFINED
        },
        VK_IMAGE_LAYOUT_VIDEO_ENCODE_DPB_KHR,
        if dpb_initialized {
            VK_PIPELINE_STAGE_2_VIDEO_ENCODE_BIT_KHR
        } else {
            VK_PIPELINE_STAGE_2_TOP_OF_PIPE_BIT
        },
        if dpb_initialized {
            VK_ACCESS_2_VIDEO_ENCODE_READ_BIT_KHR | VK_ACCESS_2_VIDEO_ENCODE_WRITE_BIT_KHR
        } else {
            0
        },
        VK_PIPELINE_STAGE_2_VIDEO_ENCODE_BIT_KHR,
        VK_ACCESS_2_VIDEO_ENCODE_WRITE_BIT_KHR,
    )];
    pipeline_barriers(encoder.execution.functions, command, &setup_dpb_dependency);
    let previous_index = encoder.previous_reference_slot.unwrap_or(0);
    let previous_dpb_index = encoder.slots[previous_index].dpb_slot_index;
    let previous_picture = video_picture_resource(
        encoder.slots[previous_index].dpb.view,
        encoder.slots[previous_index].dpb_layer,
        encoder.request().width,
        encoder.request().height,
    );
    let previous_std_reference = StdVideoEncodeH264ReferenceInfo {
        flags: 0,
        primary_pic_type: if encoder.previous_reference_was_idr {
            STD_VIDEO_H264_PICTURE_TYPE_IDR
        } else {
            STD_VIDEO_H264_PICTURE_TYPE_P
        },
        frame_num: frame_num.saturating_sub(1),
        pic_order_cnt: picture_order_count.saturating_sub(2),
        long_term_pic_num: 0,
        long_term_frame_idx: 0,
        temporal_id: 0,
    };
    let previous_dpb_info = VkVideoEncodeH264DpbSlotInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_DPB_SLOT_INFO_KHR,
        p_next: ptr::null(),
        std_reference_info: &raw const previous_std_reference,
    };
    let previous_slot = VkVideoReferenceSlotInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_REFERENCE_SLOT_INFO_KHR,
        p_next: (&raw const previous_dpb_info).cast(),
        slot_index: i32::try_from(previous_dpb_index).unwrap_or(0),
        picture_resource: &raw const previous_picture,
    };
    let has_reference = !keyframe && encoder.previous_reference_slot.is_some();
    if has_reference {
        let reference_dependency = [image_layer_barrier(
            encoder.slots[previous_index].dpb.image,
            encoder.slots[previous_index].dpb_layer,
            VK_IMAGE_ASPECT_COLOR_BIT,
            VK_IMAGE_LAYOUT_VIDEO_ENCODE_DPB_KHR,
            VK_IMAGE_LAYOUT_VIDEO_ENCODE_DPB_KHR,
            VK_PIPELINE_STAGE_2_VIDEO_ENCODE_BIT_KHR,
            VK_ACCESS_2_VIDEO_ENCODE_WRITE_BIT_KHR,
            VK_PIPELINE_STAGE_2_VIDEO_ENCODE_BIT_KHR,
            VK_ACCESS_2_VIDEO_ENCODE_READ_BIT_KHR,
        )];
        pipeline_barriers(encoder.execution.functions, command, &reference_dependency);
    }
    // Query reset occurs outside the video-coding scope as required by the
    // query-pool lifecycle rules.
    unsafe { (encoder.execution.functions.cmd_reset_query_pool)(command, output.query_pool, 0, 1) };
    let qp_min = encoder
        .parameters
        .session
        .device
        .candidate
        .min_qp
        .clamp(0, 51);
    let qp_max = encoder
        .parameters
        .session
        .device
        .candidate
        .max_qp
        .clamp(qp_min, 51);
    let h264_layer = VkVideoEncodeH264RateControlLayerInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_RATE_CONTROL_LAYER_INFO_KHR,
        p_next: ptr::null(),
        use_min_qp: 1,
        min_qp: h264_qp(qp_min),
        use_max_qp: 1,
        max_qp: h264_qp(qp_max),
        use_max_frame_size: 0,
        max_frame_size: VkVideoEncodeH264FrameSizeKhr {
            frame_i_size: 0,
            frame_p_size: 0,
            frame_b_size: 0,
        },
    };
    let bitrate = u64::from(encoder.request().target_megabits_per_second).saturating_mul(1_000_000);
    let rate_layer = VkVideoEncodeRateControlLayerInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_RATE_CONTROL_LAYER_INFO_KHR,
        p_next: (&raw const h264_layer).cast(),
        average_bitrate: bitrate,
        max_bitrate: bitrate,
        frame_rate_numerator: u32::from(encoder.request().frames_per_second),
        frame_rate_denominator: 1,
    };
    let gop_frames = u32::from(encoder.request().frames_per_second).saturating_mul(2);
    let h264_rate = VkVideoEncodeH264RateControlInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_RATE_CONTROL_INFO_KHR,
        p_next: ptr::null(),
        flags: VK_VIDEO_ENCODE_H264_RATE_CONTROL_REGULAR_GOP_BIT_KHR,
        gop_frame_count: gop_frames,
        idr_period: gop_frames,
        consecutive_b_frame_count: 0,
        temporal_layer_count: 1,
    };
    let rate = VkVideoEncodeRateControlInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_RATE_CONTROL_INFO_KHR,
        p_next: (&raw const h264_rate).cast(),
        flags: 0,
        rate_control_mode: VK_VIDEO_ENCODE_RATE_CONTROL_MODE_CBR_BIT_KHR,
        layer_count: 1,
        layers: &raw const rate_layer,
        virtual_buffer_size_ms: 1_000,
        initial_virtual_buffer_size_ms: 500,
    };
    let begin_info = VkVideoBeginCodingInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_BEGIN_CODING_INFO_KHR,
        // Every encode scope must declare the rate-control state that will be
        // effective when it executes. This declaration does not change state.
        p_next: (&raw const rate).cast(),
        flags: 0,
        video_session: encoder.parameters.session.session,
        video_session_parameters: encoder.parameters.parameters,
        reference_slot_count: u32::from(has_reference),
        reference_slots: if has_reference {
            &raw const previous_slot
        } else {
            ptr::null()
        },
    };
    // SAFETY: all referenced session, rate-control, and DPB resources remain
    // live through command recording and execution.
    unsafe { (encoder.execution.functions.cmd_begin_video_coding)(command, &raw const begin_info) };
    if let Some(control_flags) = initial_rate_control_flags(encoder.frame_index) {
        let quality_level = VkVideoEncodeQualityLevelInfoKhr {
            s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_QUALITY_LEVEL_INFO_KHR,
            p_next: (&raw const rate).cast(),
            quality_level: encoder.request().quality_level(
                encoder
                    .parameters
                    .session
                    .device
                    .candidate
                    .max_quality_levels,
            ),
        };
        let control = VkVideoCodingControlInfoKhr {
            s_type: VK_STRUCTURE_TYPE_VIDEO_CODING_CONTROL_INFO_KHR,
            p_next: (&raw const quality_level).cast(),
            flags: control_flags,
        };
        // A new video session is reset and configured once. Repeating this on
        // every picture restarts stream-level rate control and can serialize
        // otherwise independent frame submissions on real drivers.
        unsafe {
            (encoder.execution.functions.cmd_control_video_coding)(command, &raw const control);
        }
    }
    // SAFETY: this slot's query pool is idle after its completion fence.
    unsafe {
        (encoder.execution.functions.cmd_begin_query)(command, output.query_pool, 0, 0);
    }
    let mut reference_lists = StdVideoEncodeH264ReferenceListsInfo {
        flags: 0,
        num_ref_idx_l0_active_minus1: 0,
        num_ref_idx_l1_active_minus1: 0,
        ref_pic_list0: [STD_VIDEO_H264_NO_REFERENCE_PICTURE; 32],
        ref_pic_list1: [STD_VIDEO_H264_NO_REFERENCE_PICTURE; 32],
        ref_list0_mod_op_count: 0,
        ref_list1_mod_op_count: 0,
        ref_pic_marking_op_count: 0,
        reserved1: [0; 7],
        ref_list0_mod_operations: ptr::null(),
        ref_list1_mod_operations: ptr::null(),
        ref_pic_marking_operations: ptr::null(),
    };
    if has_reference {
        reference_lists.ref_pic_list0[0] = u8::try_from(previous_dpb_index).unwrap_or(0);
    }
    let std_picture = StdVideoEncodeH264PictureInfo {
        flags: if keyframe { 0x3 } else { 0x2 },
        seq_parameter_set_id: 0,
        pic_parameter_set_id: 0,
        idr_pic_id: encoder.idr_pic_id,
        primary_pic_type: if keyframe {
            STD_VIDEO_H264_PICTURE_TYPE_IDR
        } else {
            STD_VIDEO_H264_PICTURE_TYPE_P
        },
        frame_num,
        pic_order_cnt: picture_order_count,
        temporal_id: 0,
        reserved1: [0; 3],
        ref_lists: if has_reference {
            &raw const reference_lists
        } else {
            ptr::null()
        },
    };
    let slice_header = StdVideoEncodeH264SliceHeader {
        flags: 0,
        first_mb_in_slice: 0,
        slice_type: if keyframe {
            STD_VIDEO_H264_SLICE_TYPE_I
        } else {
            STD_VIDEO_H264_SLICE_TYPE_P
        },
        slice_alpha_c0_offset_div2: 0,
        slice_beta_offset_div2: 0,
        slice_qp_delta: 0,
        reserved1: 0,
        cabac_init_idc: 0,
        disable_deblocking_filter_idc: 0,
        weight_table: ptr::null(),
    };
    let slice = VkVideoEncodeH264NaluSliceInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_NALU_SLICE_INFO_KHR,
        p_next: ptr::null(),
        constant_qp: 0,
        std_slice_header: &raw const slice_header,
    };
    let picture = VkVideoEncodeH264PictureInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_PICTURE_INFO_KHR,
        p_next: ptr::null(),
        nalu_slice_entry_count: 1,
        nalu_slice_entries: &raw const slice,
        std_picture_info: &raw const std_picture,
        generate_prefix_nalu: 0,
    };
    let current_picture = video_picture_resource(
        slot.dpb.view,
        slot.dpb_layer,
        encoder.request().width,
        encoder.request().height,
    );
    let current_std_reference = StdVideoEncodeH264ReferenceInfo {
        flags: 0,
        primary_pic_type: if keyframe {
            STD_VIDEO_H264_PICTURE_TYPE_IDR
        } else {
            STD_VIDEO_H264_PICTURE_TYPE_P
        },
        frame_num,
        pic_order_cnt: picture_order_count,
        long_term_pic_num: 0,
        long_term_frame_idx: 0,
        temporal_id: 0,
    };
    let current_dpb_info = VkVideoEncodeH264DpbSlotInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_DPB_SLOT_INFO_KHR,
        p_next: ptr::null(),
        std_reference_info: &raw const current_std_reference,
    };
    let setup_slot = VkVideoReferenceSlotInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_REFERENCE_SLOT_INFO_KHR,
        p_next: (&raw const current_dpb_info).cast(),
        slot_index: i32::try_from(slot.dpb_slot_index).unwrap_or(0),
        picture_resource: &raw const current_picture,
    };
    let source = video_picture_resource(
        slot.encode_input.view,
        0,
        encoder.request().width,
        encoder.request().height,
    );
    let encode_info = VkVideoEncodeInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_INFO_KHR,
        p_next: (&raw const picture).cast(),
        flags: 0,
        dst_buffer: output.buffer,
        dst_buffer_offset: 0,
        dst_buffer_range: output.size,
        src_picture_resource: source,
        setup_reference_slot: &raw const setup_slot,
        reference_slot_count: u32::from(has_reference),
        reference_slots: if has_reference {
            &raw const previous_slot
        } else {
            ptr::null()
        },
        preceding_externally_encoded_bytes: 0,
    };
    // SAFETY: every command parameter remains live for the recording call and
    // all referenced Vulkan resources outlive command execution.
    unsafe {
        (encoder.execution.functions.cmd_encode_video)(command, &raw const encode_info);
        (encoder.execution.functions.cmd_end_query)(command, output.query_pool, 0);
        let end = VkVideoEndCodingInfoKhr {
            s_type: VK_STRUCTURE_TYPE_VIDEO_END_CODING_INFO_KHR,
            p_next: ptr::null(),
            flags: 0,
        };
        (encoder.execution.functions.cmd_end_video_coding)(command, &raw const end);
    }
    end_command(encoder.execution.functions, command)
}

fn submit_slot(
    encoder: &VulkanVideoH264Encoder,
    slot_index: usize,
) -> Result<(), VulkanVideoDeviceError> {
    let device = encoder.parameters.session.device.device_address as VkDevice;
    let fence = encoder.execution.slot_complete[slot_index];
    // All fallible import and command recording completed while the slot fence
    // remained signaled. Reset it only at the submission boundary.
    let result = unsafe { (encoder.execution.functions.reset_fences)(device, 1, &raw const fence) };
    if result != VK_SUCCESS {
        return Err(VulkanVideoDeviceError::FenceResetFailed(result));
    }
    let compute_command = encoder.execution.compute_commands[slot_index] as VkCommandBuffer;
    let encode_command = encoder.execution.encode_commands[slot_index] as VkCommandBuffer;
    let semaphore = encoder.execution.conversion_complete[slot_index];
    let command_info = command_submit_info(compute_command);
    let signal_info = semaphore_submit_info(semaphore, VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT);
    let compute_submit = VkSubmitInfo2 {
        s_type: VK_STRUCTURE_TYPE_SUBMIT_INFO_2,
        p_next: ptr::null(),
        flags: 0,
        wait_semaphore_info_count: 0,
        wait_semaphore_infos: ptr::null(),
        command_buffer_info_count: 1,
        command_buffer_infos: &raw const command_info,
        signal_semaphore_info_count: 1,
        signal_semaphore_infos: &raw const signal_info,
    };
    // SAFETY: the compute command is executable and the semaphore is idle.
    let result = unsafe {
        (encoder.execution.functions.queue_submit2)(
            encoder.parameters.session.device.compute_queue_address as VkQueue,
            1,
            &raw const compute_submit,
            0,
        )
    };
    if result != VK_SUCCESS {
        return Err(VulkanVideoDeviceError::QueueSubmissionFailed(result));
    }
    let wait_info = semaphore_submit_info(semaphore, VK_PIPELINE_STAGE_2_VIDEO_ENCODE_BIT_KHR);
    let encode_command_info = command_submit_info(encode_command);
    let encode_submit = VkSubmitInfo2 {
        s_type: VK_STRUCTURE_TYPE_SUBMIT_INFO_2,
        p_next: ptr::null(),
        flags: 0,
        wait_semaphore_info_count: 1,
        wait_semaphore_infos: &raw const wait_info,
        command_buffer_info_count: 1,
        command_buffer_infos: &raw const encode_command_info,
        signal_semaphore_info_count: 0,
        signal_semaphore_infos: ptr::null(),
    };
    // SAFETY: the encode command waits for conversion and signals the slot fence.
    let result = unsafe {
        (encoder.execution.functions.queue_submit2)(
            encoder.parameters.session.device.encode_queue_address as VkQueue,
            1,
            &raw const encode_submit,
            encoder.execution.slot_complete[slot_index],
        )
    };
    if result != VK_SUCCESS {
        return Err(VulkanVideoDeviceError::QueueSubmissionFailed(result));
    }
    Ok(())
}

fn begin_command(
    functions: Functions,
    command: VkCommandBuffer,
) -> Result<(), VulkanVideoDeviceError> {
    let begin = VkCommandBufferBeginInfo {
        s_type: VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
        p_next: ptr::null(),
        flags: VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT,
        inheritance_info: ptr::null(),
    };
    // SAFETY: the reset primary command buffer and begin info are valid.
    let result = unsafe { (functions.begin_command_buffer)(command, &raw const begin) };
    if result != VK_SUCCESS {
        return Err(VulkanVideoDeviceError::CommandBufferBeginFailed(result));
    }
    Ok(())
}

fn end_command(
    functions: Functions,
    command: VkCommandBuffer,
) -> Result<(), VulkanVideoDeviceError> {
    // SAFETY: command recording is active and all commands were structurally complete.
    let result = unsafe { (functions.end_command_buffer)(command) };
    if result != VK_SUCCESS {
        return Err(VulkanVideoDeviceError::CommandBufferEndFailed(result));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn image_barrier(
    image: VkImage,
    aspect_mask: u32,
    old_layout: i32,
    new_layout: i32,
    src_stage_mask: u64,
    src_access_mask: u64,
    dst_stage_mask: u64,
    dst_access_mask: u64,
) -> VkImageMemoryBarrier2 {
    image_layer_barrier(
        image,
        0,
        aspect_mask,
        old_layout,
        new_layout,
        src_stage_mask,
        src_access_mask,
        dst_stage_mask,
        dst_access_mask,
    )
}

#[allow(clippy::too_many_arguments)]
fn foreign_image_barrier(
    image: VkImage,
    old_layout: i32,
    new_layout: i32,
    src_stage_mask: u64,
    src_access_mask: u64,
    dst_stage_mask: u64,
    dst_access_mask: u64,
    queue_family_index: u32,
    acquire: bool,
) -> VkImageMemoryBarrier2 {
    let mut barrier = image_barrier(
        image,
        VK_IMAGE_ASPECT_COLOR_BIT,
        old_layout,
        new_layout,
        src_stage_mask,
        src_access_mask,
        dst_stage_mask,
        dst_access_mask,
    );
    if acquire {
        barrier.src_queue_family_index = VK_QUEUE_FAMILY_FOREIGN_EXT;
        barrier.dst_queue_family_index = queue_family_index;
    } else {
        barrier.src_queue_family_index = queue_family_index;
        barrier.dst_queue_family_index = VK_QUEUE_FAMILY_FOREIGN_EXT;
    }
    barrier
}

#[allow(clippy::too_many_arguments)]
fn image_layer_barrier(
    image: VkImage,
    base_array_layer: u32,
    aspect_mask: u32,
    old_layout: i32,
    new_layout: i32,
    src_stage_mask: u64,
    src_access_mask: u64,
    dst_stage_mask: u64,
    dst_access_mask: u64,
) -> VkImageMemoryBarrier2 {
    VkImageMemoryBarrier2 {
        s_type: VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER_2,
        p_next: ptr::null(),
        src_stage_mask,
        src_access_mask,
        dst_stage_mask,
        dst_access_mask,
        old_layout,
        new_layout,
        src_queue_family_index: VK_QUEUE_FAMILY_IGNORED,
        dst_queue_family_index: VK_QUEUE_FAMILY_IGNORED,
        image,
        subresource_range: VkImageSubresourceRange {
            aspect_mask,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer,
            layer_count: 1,
        },
    }
}

fn pipeline_barriers(
    functions: Functions,
    command: VkCommandBuffer,
    barriers: &[VkImageMemoryBarrier2],
) {
    let dependency = VkDependencyInfo {
        s_type: VK_STRUCTURE_TYPE_DEPENDENCY_INFO,
        p_next: ptr::null(),
        dependency_flags: VK_DEPENDENCY_BY_REGION_BIT,
        memory_barrier_count: 0,
        memory_barriers: ptr::null(),
        buffer_memory_barrier_count: 0,
        buffer_memory_barriers: ptr::null(),
        image_memory_barrier_count: u32::try_from(barriers.len()).unwrap_or(0),
        image_memory_barriers: barriers.as_ptr(),
    };
    // SAFETY: every barrier references a live image and valid subresource.
    unsafe { (functions.cmd_pipeline_barrier2)(command, &raw const dependency) };
}

fn image_copy(src_aspect: u32, dst_aspect: u32, width: u32, height: u32) -> VkImageCopy {
    VkImageCopy {
        src_subresource: VkImageSubresourceLayers {
            aspect_mask: src_aspect,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        },
        src_offset: VkOffset3d { x: 0, y: 0, z: 0 },
        dst_subresource: VkImageSubresourceLayers {
            aspect_mask: dst_aspect,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        },
        dst_offset: VkOffset3d { x: 0, y: 0, z: 0 },
        extent: VkExtent3d {
            width,
            height,
            depth: 1,
        },
    }
}

fn h264_qp(value: i32) -> VkVideoEncodeH264QpKhr {
    VkVideoEncodeH264QpKhr {
        qp_i: value,
        qp_p: value,
        qp_b: value,
    }
}

fn video_picture_resource(
    image_view: VkImageView,
    base_array_layer: u32,
    width: u32,
    height: u32,
) -> VkVideoPictureResourceInfoKhr {
    VkVideoPictureResourceInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_PICTURE_RESOURCE_INFO_KHR,
        p_next: ptr::null(),
        coded_offset: [0, 0],
        coded_extent: VkExtent2d { width, height },
        base_array_layer,
        image_view_binding: image_view,
    }
}

fn command_submit_info(command_buffer: VkCommandBuffer) -> VkCommandBufferSubmitInfo {
    VkCommandBufferSubmitInfo {
        s_type: VK_STRUCTURE_TYPE_COMMAND_BUFFER_SUBMIT_INFO,
        p_next: ptr::null(),
        command_buffer,
        device_mask: 1,
    }
}

fn semaphore_submit_info(semaphore: VkSemaphore, stage_mask: u64) -> VkSemaphoreSubmitInfo {
    VkSemaphoreSubmitInfo {
        s_type: VK_STRUCTURE_TYPE_SEMAPHORE_SUBMIT_INFO,
        p_next: ptr::null(),
        semaphore,
        value: 0,
        stage_mask,
        device_index: 0,
    }
}

unsafe fn create_pool(
    device: VkDevice,
    queue_family_index: u32,
    create: CreateCommandPool,
) -> Result<VkCommandPool, VulkanVideoDeviceError> {
    let info = VkCommandPoolCreateInfo {
        s_type: VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
        p_next: ptr::null(),
        flags: VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT,
        queue_family_index,
    };
    let mut pool = 0;
    // SAFETY: create info and output storage are valid.
    let result = unsafe { create(device, &raw const info, ptr::null(), &raw mut pool) };
    if result != VK_SUCCESS || pool == 0 {
        return Err(VulkanVideoDeviceError::CommandPoolCreationFailed(result));
    }
    Ok(pool)
}

unsafe fn allocate_commands(
    device: VkDevice,
    pool: VkCommandPool,
    count: usize,
    allocate: AllocateCommandBuffers,
) -> Result<Vec<usize>, VulkanVideoDeviceError> {
    let info = VkCommandBufferAllocateInfo {
        s_type: VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        p_next: ptr::null(),
        command_pool: pool,
        level: VK_COMMAND_BUFFER_LEVEL_PRIMARY,
        command_buffer_count: u32::try_from(count).unwrap_or(0),
    };
    let mut commands = vec![ptr::null_mut(); count];
    // SAFETY: output vector holds exactly the requested handle count.
    let result = unsafe { allocate(device, &raw const info, commands.as_mut_ptr()) };
    if result != VK_SUCCESS || commands.iter().any(|command| command.is_null()) {
        return Err(VulkanVideoDeviceError::CommandBufferAllocationFailed(
            result,
        ));
    }
    Ok(commands.into_iter().map(VkCommandBuffer::addr).collect())
}
