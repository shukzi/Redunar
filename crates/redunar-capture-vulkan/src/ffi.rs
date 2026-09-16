use std::ffi::{c_char, c_void};

pub(crate) type VkResult = i32;
pub(crate) const VK_SUCCESS: VkResult = 0;
pub(crate) const VK_ERROR_INITIALIZATION_FAILED: VkResult = -3;
pub(crate) const VK_ERROR_EXTENSION_NOT_PRESENT: VkResult = -7;
pub(crate) const VK_STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO: i32 = 47;
pub(crate) const VK_STRUCTURE_TYPE_LOADER_DEVICE_CREATE_INFO: i32 = 48;
pub(crate) const VK_LAYER_LINK_INFO: i32 = 0;

pub(crate) const VK_STRUCTURE_TYPE_SUBMIT_INFO: i32 = 4;
pub(crate) const VK_STRUCTURE_TYPE_FENCE_CREATE_INFO: i32 = 8;
pub(crate) const VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO: i32 = 9;
pub(crate) const VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO: i32 = 15;
pub(crate) const VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO: i32 = 16;
pub(crate) const VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO: i32 = 18;
pub(crate) const VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO: i32 = 19;
pub(crate) const VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO: i32 = 20;
pub(crate) const VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO: i32 = 22;
pub(crate) const VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO: i32 = 23;
pub(crate) const VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO: i32 = 24;
pub(crate) const VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO: i32 = 26;
pub(crate) const VK_STRUCTURE_TYPE_PIPELINE_DYNAMIC_STATE_CREATE_INFO: i32 = 27;
pub(crate) const VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO: i32 = 28;
pub(crate) const VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO: i32 = 30;
pub(crate) const VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO: i32 = 37;
pub(crate) const VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO: i32 = 38;
pub(crate) const VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO: i32 = 39;
pub(crate) const VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO: i32 = 40;
pub(crate) const VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO: i32 = 42;
pub(crate) const VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO: i32 = 12;
pub(crate) const VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO: i32 = 5;
pub(crate) const VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_BUFFER_CREATE_INFO: i32 = 1_000_072_000;
pub(crate) const VK_STRUCTURE_TYPE_EXPORT_MEMORY_ALLOCATE_INFO: i32 = 1_000_072_002;
pub(crate) const VK_STRUCTURE_TYPE_MEMORY_GET_FD_INFO_KHR: i32 = 1_000_074_002;
pub(crate) const VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER: i32 = 45;
pub(crate) const VK_STRUCTURE_TYPE_MEMORY_BARRIER: i32 = 46;
pub(crate) const VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO: i32 = 43;
pub(crate) const VK_STRUCTURE_TYPE_DEVICE_QUEUE_INFO_2: i32 = 1_000_145_003;
pub(crate) const VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR: i32 = 1_000_001_000;
pub(crate) const VK_STRUCTURE_TYPE_PRESENT_INFO_KHR: i32 = 1_000_001_001;

pub(crate) const VK_QUEUE_GRAPHICS_BIT: u32 = 0x0000_0001;
pub(crate) const VK_IMAGE_USAGE_TRANSFER_SRC_BIT: u32 = 0x0000_0001;
pub(crate) const VK_BUFFER_USAGE_TRANSFER_DST_BIT: u32 = 0x0000_0002;
pub(crate) const VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT: u32 = 0x0000_0010;
pub(crate) const VK_IMAGE_ASPECT_COLOR_BIT: u32 = 0x0000_0001;
pub(crate) const VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT: u32 = 0x0000_0400;
pub(crate) const VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT: u32 = 0x0000_2000;
pub(crate) const VK_PIPELINE_STAGE_TRANSFER_BIT: u32 = 0x0000_1000;
pub(crate) const VK_ACCESS_TRANSFER_READ_BIT: u32 = 0x0000_0800;
pub(crate) const VK_ACCESS_TRANSFER_WRITE_BIT: u32 = 0x0000_1000;
pub(crate) const VK_ACCESS_HOST_READ_BIT: u32 = 0x0000_2000;
pub(crate) const VK_PIPELINE_STAGE_HOST_BIT: u32 = 0x0000_4000;
pub(crate) const VK_ACCESS_COLOR_ATTACHMENT_READ_BIT: u32 = 0x0000_0080;
pub(crate) const VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT: u32 = 0x0000_0100;
pub(crate) const VK_DEPENDENCY_BY_REGION_BIT: u32 = 0x0000_0001;
pub(crate) const VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT: u32 = 0x0000_0002;
pub(crate) const VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT: u32 = 0x0000_0001;
pub(crate) const VK_FENCE_CREATE_SIGNALED_BIT: u32 = 0x0000_0001;
pub(crate) const VK_SWAPCHAIN_CREATE_PROTECTED_BIT_KHR: u32 = 0x0000_0002;
pub(crate) const VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT: u32 = 0x0000_0002;
pub(crate) const VK_MEMORY_PROPERTY_HOST_COHERENT_BIT: u32 = 0x0000_0004;
pub(crate) const VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT: u32 = 0x0000_0200;
pub(crate) const VK_SHARING_MODE_EXCLUSIVE: i32 = 0;
pub(crate) const VK_QUEUE_FAMILY_IGNORED: u32 = u32::MAX;

pub(crate) const VK_FORMAT_UNDEFINED: i32 = 0;
pub(crate) const VK_FORMAT_R8G8B8A8_UNORM: i32 = 37;
pub(crate) const VK_FORMAT_R8G8B8A8_SRGB: i32 = 43;
pub(crate) const VK_FORMAT_A2R10G10B10_UNORM_PACK32: i32 = 58;
pub(crate) const VK_FORMAT_A2B10G10R10_UNORM_PACK32: i32 = 64;
pub(crate) const VK_FORMAT_B8G8R8A8_UNORM: i32 = 44;
pub(crate) const VK_FORMAT_B8G8R8A8_SRGB: i32 = 50;
pub(crate) const VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL: i32 = 2;
pub(crate) const VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL: i32 = 6;
pub(crate) const VK_IMAGE_LAYOUT_PRESENT_SRC_KHR: i32 = 1_000_001_002;
pub(crate) const VK_COMPONENT_SWIZZLE_IDENTITY: i32 = 0;
pub(crate) const VK_IMAGE_VIEW_TYPE_2D: i32 = 1;
pub(crate) const VK_COMMAND_BUFFER_LEVEL_PRIMARY: i32 = 0;
pub(crate) const VK_PIPELINE_BIND_POINT_GRAPHICS: i32 = 0;
pub(crate) const VK_ATTACHMENT_LOAD_OP_LOAD: i32 = 0;
pub(crate) const VK_ATTACHMENT_LOAD_OP_DONT_CARE: i32 = 2;
pub(crate) const VK_ATTACHMENT_STORE_OP_STORE: i32 = 0;
pub(crate) const VK_ATTACHMENT_STORE_OP_DONT_CARE: i32 = 1;
pub(crate) const VK_SUBPASS_CONTENTS_INLINE: i32 = 0;
pub(crate) const VK_SAMPLE_COUNT_1_BIT: u32 = 0x0000_0001;
pub(crate) const VK_SUBPASS_EXTERNAL: u32 = u32::MAX;
pub(crate) const VK_SHADER_STAGE_VERTEX_BIT: u32 = 0x0000_0001;
pub(crate) const VK_SHADER_STAGE_FRAGMENT_BIT: u32 = 0x0000_0010;
pub(crate) const VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST: i32 = 3;
pub(crate) const VK_POLYGON_MODE_FILL: i32 = 0;
pub(crate) const VK_CULL_MODE_NONE: u32 = 0;
pub(crate) const VK_FRONT_FACE_COUNTER_CLOCKWISE: i32 = 1;
pub(crate) const VK_BLEND_FACTOR_ZERO: i32 = 0;
pub(crate) const VK_BLEND_FACTOR_ONE: i32 = 1;
pub(crate) const VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA: i32 = 7;
pub(crate) const VK_BLEND_FACTOR_CONSTANT_ALPHA: i32 = 12;
pub(crate) const VK_BLEND_FACTOR_ONE_MINUS_CONSTANT_ALPHA: i32 = 13;
pub(crate) const VK_BLEND_FACTOR_CONSTANT_COLOR: i32 = 10;
pub(crate) const VK_BLEND_OP_ADD: i32 = 0;
pub(crate) const VK_LOGIC_OP_COPY: i32 = 3;
pub(crate) const VK_COLOR_COMPONENT_R_BIT: u32 = 0x0000_0001;
pub(crate) const VK_COLOR_COMPONENT_G_BIT: u32 = 0x0000_0002;
pub(crate) const VK_COLOR_COMPONENT_B_BIT: u32 = 0x0000_0004;
pub(crate) const VK_DYNAMIC_STATE_VIEWPORT: i32 = 0;
pub(crate) const VK_DYNAMIC_STATE_SCISSOR: i32 = 1;
pub(crate) const VK_DYNAMIC_STATE_BLEND_CONSTANTS: i32 = 4;

#[repr(C)]
pub(crate) struct VkInstanceObject {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct VkPhysicalDeviceObject {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct VkDeviceObject {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct VkQueueObject {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct VkCommandBufferObject {
    _private: [u8; 0],
}

pub(crate) type VkInstance = *mut VkInstanceObject;
pub(crate) type VkPhysicalDevice = *mut VkPhysicalDeviceObject;
pub(crate) type VkDevice = *mut VkDeviceObject;
pub(crate) type VkQueue = *mut VkQueueObject;
pub(crate) type VkCommandBuffer = *mut VkCommandBufferObject;

// Vulkan non-dispatchable handles use an opaque 64-bit representation in the
// official C ABI. Keeping them as integers also gives every cached resource a
// portable zero/null value without exposing driver-owned object layouts.
pub(crate) type VkSwapchainKhr = u64;
pub(crate) type VkSurfaceKhr = u64;
pub(crate) type VkImage = u64;
pub(crate) type VkImageView = u64;
pub(crate) type VkRenderPass = u64;
pub(crate) type VkFramebuffer = u64;
pub(crate) type VkCommandPool = u64;
pub(crate) type VkSemaphore = u64;
pub(crate) type VkFence = u64;
pub(crate) type VkShaderModule = u64;
pub(crate) type VkPipelineLayout = u64;
pub(crate) type VkPipeline = u64;
pub(crate) type VkPipelineCache = u64;
pub(crate) type VkBuffer = u64;
pub(crate) type VkDeviceMemory = u64;

#[repr(C)]
pub(crate) struct VkAllocationCallbacks {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct VkInstanceCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) p_application_info: *const c_void,
    pub(crate) enabled_layer_count: u32,
    pub(crate) enabled_layer_names: *const *const c_char,
    pub(crate) enabled_extension_count: u32,
    pub(crate) enabled_extension_names: *const *const c_char,
}

#[repr(C)]
pub(crate) struct VkDeviceQueueCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) queue_family_index: u32,
    pub(crate) queue_count: u32,
    pub(crate) queue_priorities: *const f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct VkDeviceCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) queue_create_info_count: u32,
    pub(crate) queue_create_infos: *const VkDeviceQueueCreateInfo,
    pub(crate) enabled_layer_count: u32,
    pub(crate) enabled_layer_names: *const *const c_char,
    pub(crate) enabled_extension_count: u32,
    pub(crate) enabled_extension_names: *const *const c_char,
    pub(crate) enabled_features: *const c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct VkExtent2d {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct VkExtent3d {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) depth: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct VkOffset2d {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct VkRect2d {
    pub(crate) offset: VkOffset2d,
    pub(crate) extent: VkExtent2d,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct VkQueueFamilyProperties {
    pub(crate) queue_flags: u32,
    pub(crate) queue_count: u32,
    pub(crate) timestamp_valid_bits: u32,
    pub(crate) min_image_transfer_granularity: VkExtent3d,
}

#[repr(C)]
pub(crate) struct VkDeviceQueueInfo2 {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) queue_family_index: u32,
    pub(crate) queue_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct VkSwapchainCreateInfoKhr {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) surface: VkSurfaceKhr,
    pub(crate) min_image_count: u32,
    pub(crate) image_format: i32,
    pub(crate) image_color_space: i32,
    pub(crate) image_extent: VkExtent2d,
    pub(crate) image_array_layers: u32,
    pub(crate) image_usage: u32,
    pub(crate) image_sharing_mode: i32,
    pub(crate) queue_family_index_count: u32,
    pub(crate) queue_family_indices: *const u32,
    pub(crate) pre_transform: u32,
    pub(crate) composite_alpha: u32,
    pub(crate) present_mode: i32,
    pub(crate) clipped: u32,
    pub(crate) old_swapchain: VkSwapchainKhr,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct VkPresentInfoKhr {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) wait_semaphore_count: u32,
    pub(crate) wait_semaphores: *const VkSemaphore,
    pub(crate) swapchain_count: u32,
    pub(crate) swapchains: *const VkSwapchainKhr,
    pub(crate) image_indices: *const u32,
    pub(crate) results: *mut VkResult,
}

#[repr(C)]
pub(crate) struct VkSemaphoreCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
}

#[repr(C)]
pub(crate) struct VkFenceCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
}

#[repr(C)]
pub(crate) struct VkCommandPoolCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) queue_family_index: u32,
}

#[repr(C)]
pub(crate) struct VkCommandBufferAllocateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) command_pool: VkCommandPool,
    pub(crate) level: i32,
    pub(crate) command_buffer_count: u32,
}

#[repr(C)]
pub(crate) struct VkCommandBufferBeginInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) inheritance_info: *const c_void,
}

#[repr(C)]
pub(crate) struct VkBufferCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) size: u64,
    pub(crate) usage: u32,
    pub(crate) sharing_mode: i32,
    pub(crate) queue_family_index_count: u32,
    pub(crate) queue_family_indices: *const u32,
}

#[repr(C)]
pub(crate) struct VkMemoryRequirements {
    pub(crate) size: u64,
    pub(crate) alignment: u64,
    pub(crate) memory_type_bits: u32,
}

#[repr(C)]
pub(crate) struct VkMemoryAllocateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) allocation_size: u64,
    pub(crate) memory_type_index: u32,
}

#[repr(C)]
pub(crate) struct VkExternalMemoryBufferCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) handle_types: u32,
}

#[repr(C)]
pub(crate) struct VkExportMemoryAllocateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) handle_types: u32,
}

#[repr(C)]
pub(crate) struct VkMemoryGetFdInfoKhr {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) memory: VkDeviceMemory,
    pub(crate) handle_type: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct VkMemoryType {
    pub(crate) property_flags: u32,
    pub(crate) heap_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct VkMemoryHeap {
    pub(crate) size: u64,
    pub(crate) flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
#[allow(clippy::struct_field_names)]
pub(crate) struct VkPhysicalDeviceMemoryProperties {
    pub(crate) memory_type_count: u32,
    pub(crate) memory_types: [VkMemoryType; 32],
    pub(crate) memory_heap_count: u32,
    pub(crate) memory_heaps: [VkMemoryHeap; 16],
}

#[repr(C)]
pub(crate) struct VkImageMemoryBarrier {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) src_access_mask: u32,
    pub(crate) dst_access_mask: u32,
    pub(crate) old_layout: i32,
    pub(crate) new_layout: i32,
    pub(crate) src_queue_family_index: u32,
    pub(crate) dst_queue_family_index: u32,
    pub(crate) image: VkImage,
    pub(crate) subresource_range: VkImageSubresourceRange,
}

#[repr(C)]
pub(crate) struct VkMemoryBarrier {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) src_access_mask: u32,
    pub(crate) dst_access_mask: u32,
}

#[repr(C)]
pub(crate) struct VkBufferImageCopy {
    pub(crate) buffer_offset: u64,
    pub(crate) buffer_row_length: u32,
    pub(crate) buffer_image_height: u32,
    pub(crate) image_subresource: VkImageSubresourceLayers,
    pub(crate) image_offset: VkOffset3d,
    pub(crate) image_extent: VkExtent3d,
}

#[repr(C)]
pub(crate) struct VkImageSubresourceLayers {
    pub(crate) aspect_mask: u32,
    pub(crate) mip_level: u32,
    pub(crate) base_array_layer: u32,
    pub(crate) layer_count: u32,
}

#[repr(C)]
pub(crate) struct VkOffset3d {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
}

#[repr(C)]
pub(crate) struct VkSubmitInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) wait_semaphore_count: u32,
    pub(crate) wait_semaphores: *const VkSemaphore,
    pub(crate) wait_dst_stage_mask: *const u32,
    pub(crate) command_buffer_count: u32,
    pub(crate) command_buffers: *const VkCommandBuffer,
    pub(crate) signal_semaphore_count: u32,
    pub(crate) signal_semaphores: *const VkSemaphore,
}

#[repr(C)]
pub(crate) struct VkComponentMapping {
    pub(crate) r: i32,
    pub(crate) g: i32,
    pub(crate) b: i32,
    pub(crate) a: i32,
}

#[repr(C)]
pub(crate) struct VkImageSubresourceRange {
    pub(crate) aspect_mask: u32,
    pub(crate) base_mip_level: u32,
    pub(crate) level_count: u32,
    pub(crate) base_array_layer: u32,
    pub(crate) layer_count: u32,
}

#[repr(C)]
pub(crate) struct VkImageViewCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) image: VkImage,
    pub(crate) view_type: i32,
    pub(crate) format: i32,
    pub(crate) components: VkComponentMapping,
    pub(crate) subresource_range: VkImageSubresourceRange,
}

#[repr(C)]
pub(crate) struct VkShaderModuleCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) code_size: usize,
    pub(crate) code: *const u32,
}

#[repr(C)]
pub(crate) struct VkPipelineShaderStageCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) stage: u32,
    pub(crate) module: VkShaderModule,
    pub(crate) name: *const c_char,
    pub(crate) specialization_info: *const c_void,
}

#[repr(C)]
pub(crate) struct VkPipelineVertexInputStateCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) vertex_binding_description_count: u32,
    pub(crate) vertex_binding_descriptions: *const c_void,
    pub(crate) vertex_attribute_description_count: u32,
    pub(crate) vertex_attribute_descriptions: *const c_void,
}

#[repr(C)]
pub(crate) struct VkPipelineInputAssemblyStateCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) topology: i32,
    pub(crate) primitive_restart_enable: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct VkViewport {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) min_depth: f32,
    pub(crate) max_depth: f32,
}

#[repr(C)]
pub(crate) struct VkPipelineViewportStateCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) viewport_count: u32,
    pub(crate) viewports: *const VkViewport,
    pub(crate) scissor_count: u32,
    pub(crate) scissors: *const VkRect2d,
}

#[repr(C)]
pub(crate) struct VkPipelineRasterizationStateCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) depth_clamp_enable: u32,
    pub(crate) rasterizer_discard_enable: u32,
    pub(crate) polygon_mode: i32,
    pub(crate) cull_mode: u32,
    pub(crate) front_face: i32,
    pub(crate) depth_bias_enable: u32,
    pub(crate) depth_bias_constant_factor: f32,
    pub(crate) depth_bias_clamp: f32,
    pub(crate) depth_bias_slope_factor: f32,
    pub(crate) line_width: f32,
}

#[repr(C)]
pub(crate) struct VkPipelineMultisampleStateCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) rasterization_samples: u32,
    pub(crate) sample_shading_enable: u32,
    pub(crate) min_sample_shading: f32,
    pub(crate) sample_mask: *const u32,
    pub(crate) alpha_to_coverage_enable: u32,
    pub(crate) alpha_to_one_enable: u32,
}

#[repr(C)]
pub(crate) struct VkPipelineColorBlendAttachmentState {
    pub(crate) blend_enable: u32,
    pub(crate) src_color_blend_factor: i32,
    pub(crate) dst_color_blend_factor: i32,
    pub(crate) color_blend_op: i32,
    pub(crate) src_alpha_blend_factor: i32,
    pub(crate) dst_alpha_blend_factor: i32,
    pub(crate) alpha_blend_op: i32,
    pub(crate) color_write_mask: u32,
}

#[repr(C)]
pub(crate) struct VkPipelineColorBlendStateCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) logic_op_enable: u32,
    pub(crate) logic_op: i32,
    pub(crate) attachment_count: u32,
    pub(crate) attachments: *const VkPipelineColorBlendAttachmentState,
    pub(crate) blend_constants: [f32; 4],
}

#[repr(C)]
pub(crate) struct VkPipelineDynamicStateCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) dynamic_state_count: u32,
    pub(crate) dynamic_states: *const i32,
}

#[repr(C)]
pub(crate) struct VkPipelineLayoutCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) set_layout_count: u32,
    pub(crate) set_layouts: *const u64,
    pub(crate) push_constant_range_count: u32,
    pub(crate) push_constant_ranges: *const c_void,
}

#[repr(C)]
pub(crate) struct VkGraphicsPipelineCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) stage_count: u32,
    pub(crate) stages: *const VkPipelineShaderStageCreateInfo,
    pub(crate) vertex_input_state: *const VkPipelineVertexInputStateCreateInfo,
    pub(crate) input_assembly_state: *const VkPipelineInputAssemblyStateCreateInfo,
    pub(crate) tessellation_state: *const c_void,
    pub(crate) viewport_state: *const VkPipelineViewportStateCreateInfo,
    pub(crate) rasterization_state: *const VkPipelineRasterizationStateCreateInfo,
    pub(crate) multisample_state: *const VkPipelineMultisampleStateCreateInfo,
    pub(crate) depth_stencil_state: *const c_void,
    pub(crate) color_blend_state: *const VkPipelineColorBlendStateCreateInfo,
    pub(crate) dynamic_state: *const VkPipelineDynamicStateCreateInfo,
    pub(crate) layout: VkPipelineLayout,
    pub(crate) render_pass: VkRenderPass,
    pub(crate) subpass: u32,
    pub(crate) base_pipeline_handle: VkPipeline,
    pub(crate) base_pipeline_index: i32,
}

#[repr(C)]
pub(crate) struct VkAttachmentDescription {
    pub(crate) flags: u32,
    pub(crate) format: i32,
    pub(crate) samples: u32,
    pub(crate) load_op: i32,
    pub(crate) store_op: i32,
    pub(crate) stencil_load_op: i32,
    pub(crate) stencil_store_op: i32,
    pub(crate) initial_layout: i32,
    pub(crate) final_layout: i32,
}

#[repr(C)]
pub(crate) struct VkAttachmentReference {
    pub(crate) attachment: u32,
    pub(crate) layout: i32,
}

#[repr(C)]
pub(crate) struct VkSubpassDescription {
    pub(crate) flags: u32,
    pub(crate) pipeline_bind_point: i32,
    pub(crate) input_attachment_count: u32,
    pub(crate) input_attachments: *const VkAttachmentReference,
    pub(crate) color_attachment_count: u32,
    pub(crate) color_attachments: *const VkAttachmentReference,
    pub(crate) resolve_attachments: *const VkAttachmentReference,
    pub(crate) depth_stencil_attachment: *const VkAttachmentReference,
    pub(crate) preserve_attachment_count: u32,
    pub(crate) preserve_attachments: *const u32,
}

#[repr(C)]
pub(crate) struct VkSubpassDependency {
    pub(crate) src_subpass: u32,
    pub(crate) dst_subpass: u32,
    pub(crate) src_stage_mask: u32,
    pub(crate) dst_stage_mask: u32,
    pub(crate) src_access_mask: u32,
    pub(crate) dst_access_mask: u32,
    pub(crate) dependency_flags: u32,
}

#[repr(C)]
pub(crate) struct VkRenderPassCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) attachment_count: u32,
    pub(crate) attachments: *const VkAttachmentDescription,
    pub(crate) subpass_count: u32,
    pub(crate) subpasses: *const VkSubpassDescription,
    pub(crate) dependency_count: u32,
    pub(crate) dependencies: *const VkSubpassDependency,
}

#[repr(C)]
pub(crate) struct VkFramebufferCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) flags: u32,
    pub(crate) render_pass: VkRenderPass,
    pub(crate) attachment_count: u32,
    pub(crate) attachments: *const VkImageView,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) layers: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) union VkClearColorValue {
    pub(crate) float32: [f32; 4],
    pub(crate) int32: [i32; 4],
    pub(crate) uint32: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) union VkClearValue {
    pub(crate) color: VkClearColorValue,
    pub(crate) depth_stencil: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct VkClearAttachment {
    pub(crate) aspect_mask: u32,
    pub(crate) color_attachment: u32,
    pub(crate) clear_value: VkClearValue,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct VkClearRect {
    pub(crate) rect: VkRect2d,
    pub(crate) base_array_layer: u32,
    pub(crate) layer_count: u32,
}

#[repr(C)]
pub(crate) struct VkRenderPassBeginInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) render_pass: VkRenderPass,
    pub(crate) framebuffer: VkFramebuffer,
    pub(crate) render_area: VkRect2d,
    pub(crate) clear_value_count: u32,
    pub(crate) clear_values: *const VkClearValue,
}

pub(crate) type PfnVoidFunction = Option<unsafe extern "system" fn()>;
pub(crate) type PfnGetInstanceProcAddr =
    unsafe extern "system" fn(VkInstance, *const c_char) -> PfnVoidFunction;
pub(crate) type PfnGetDeviceProcAddr =
    unsafe extern "system" fn(VkDevice, *const c_char) -> PfnVoidFunction;
pub(crate) type PfnGetPhysicalDeviceProcAddr =
    unsafe extern "system" fn(VkInstance, *const c_char) -> PfnVoidFunction;
pub(crate) type PfnCreateInstance = unsafe extern "system" fn(
    *const VkInstanceCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkInstance,
) -> VkResult;
pub(crate) type PfnDestroyInstance =
    unsafe extern "system" fn(VkInstance, *const VkAllocationCallbacks);
pub(crate) type PfnCreateDevice = unsafe extern "system" fn(
    VkPhysicalDevice,
    *const VkDeviceCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkDevice,
) -> VkResult;
pub(crate) type PfnDestroyDevice =
    unsafe extern "system" fn(VkDevice, *const VkAllocationCallbacks);
pub(crate) type PfnQueuePresentKhr =
    unsafe extern "system" fn(VkQueue, *const VkPresentInfoKhr) -> VkResult;
pub(crate) type PfnQueueWaitIdle = unsafe extern "system" fn(VkQueue) -> VkResult;
pub(crate) type PfnGetPhysicalDeviceQueueFamilyProperties =
    unsafe extern "system" fn(VkPhysicalDevice, *mut u32, *mut VkQueueFamilyProperties);
pub(crate) type PfnGetPhysicalDeviceMemoryProperties =
    unsafe extern "system" fn(VkPhysicalDevice, *mut VkPhysicalDeviceMemoryProperties);
pub(crate) type PfnGetDeviceQueue = unsafe extern "system" fn(VkDevice, u32, u32, *mut VkQueue);
pub(crate) type PfnGetDeviceQueue2 =
    unsafe extern "system" fn(VkDevice, *const VkDeviceQueueInfo2, *mut VkQueue);
pub(crate) type PfnCreateSwapchainKhr = unsafe extern "system" fn(
    VkDevice,
    *const VkSwapchainCreateInfoKhr,
    *const VkAllocationCallbacks,
    *mut VkSwapchainKhr,
) -> VkResult;
pub(crate) type PfnDestroySwapchainKhr =
    unsafe extern "system" fn(VkDevice, VkSwapchainKhr, *const VkAllocationCallbacks);
pub(crate) type PfnGetSwapchainImagesKhr =
    unsafe extern "system" fn(VkDevice, VkSwapchainKhr, *mut u32, *mut VkImage) -> VkResult;
pub(crate) type PfnCreateSemaphore = unsafe extern "system" fn(
    VkDevice,
    *const VkSemaphoreCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkSemaphore,
) -> VkResult;
pub(crate) type PfnDestroySemaphore =
    unsafe extern "system" fn(VkDevice, VkSemaphore, *const VkAllocationCallbacks);
pub(crate) type PfnCreateFence = unsafe extern "system" fn(
    VkDevice,
    *const VkFenceCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkFence,
) -> VkResult;
pub(crate) type PfnDestroyFence =
    unsafe extern "system" fn(VkDevice, VkFence, *const VkAllocationCallbacks);
pub(crate) type PfnResetFences =
    unsafe extern "system" fn(VkDevice, u32, *const VkFence) -> VkResult;
pub(crate) type PfnGetFenceStatus = unsafe extern "system" fn(VkDevice, VkFence) -> VkResult;
pub(crate) type PfnCreateCommandPool = unsafe extern "system" fn(
    VkDevice,
    *const VkCommandPoolCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkCommandPool,
) -> VkResult;
pub(crate) type PfnDestroyCommandPool =
    unsafe extern "system" fn(VkDevice, VkCommandPool, *const VkAllocationCallbacks);
pub(crate) type PfnAllocateCommandBuffers = unsafe extern "system" fn(
    VkDevice,
    *const VkCommandBufferAllocateInfo,
    *mut VkCommandBuffer,
) -> VkResult;
pub(crate) type PfnResetCommandBuffer = unsafe extern "system" fn(VkCommandBuffer, u32) -> VkResult;
pub(crate) type PfnBeginCommandBuffer =
    unsafe extern "system" fn(VkCommandBuffer, *const VkCommandBufferBeginInfo) -> VkResult;
pub(crate) type PfnEndCommandBuffer = unsafe extern "system" fn(VkCommandBuffer) -> VkResult;
pub(crate) type PfnQueueSubmit =
    unsafe extern "system" fn(VkQueue, u32, *const VkSubmitInfo, VkFence) -> VkResult;
pub(crate) type PfnCreateBuffer = unsafe extern "system" fn(
    VkDevice,
    *const VkBufferCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkBuffer,
) -> VkResult;
pub(crate) type PfnDestroyBuffer =
    unsafe extern "system" fn(VkDevice, VkBuffer, *const VkAllocationCallbacks);
pub(crate) type PfnGetBufferMemoryRequirements =
    unsafe extern "system" fn(VkDevice, VkBuffer, *mut VkMemoryRequirements);
pub(crate) type PfnAllocateMemory = unsafe extern "system" fn(
    VkDevice,
    *const VkMemoryAllocateInfo,
    *const VkAllocationCallbacks,
    *mut VkDeviceMemory,
) -> VkResult;
pub(crate) type PfnFreeMemory =
    unsafe extern "system" fn(VkDevice, VkDeviceMemory, *const VkAllocationCallbacks);
pub(crate) type PfnBindBufferMemory =
    unsafe extern "system" fn(VkDevice, VkBuffer, VkDeviceMemory, u64) -> VkResult;
pub(crate) type PfnMapMemory = unsafe extern "system" fn(
    VkDevice,
    VkDeviceMemory,
    u64,
    u64,
    u32,
    *mut *mut c_void,
) -> VkResult;
pub(crate) type PfnUnmapMemory = unsafe extern "system" fn(VkDevice, VkDeviceMemory);
pub(crate) type PfnGetMemoryFdKhr =
    unsafe extern "system" fn(VkDevice, *const VkMemoryGetFdInfoKhr, *mut i32) -> VkResult;
pub(crate) type PfnCmdPipelineBarrier = unsafe extern "system" fn(
    VkCommandBuffer,
    u32,
    u32,
    u32,
    u32,
    *const c_void,
    u32,
    *const c_void,
    u32,
    *const VkImageMemoryBarrier,
);
pub(crate) type PfnCmdCopyImageToBuffer = unsafe extern "system" fn(
    VkCommandBuffer,
    VkImage,
    i32,
    VkBuffer,
    u32,
    *const VkBufferImageCopy,
);
pub(crate) type PfnCreateImageView = unsafe extern "system" fn(
    VkDevice,
    *const VkImageViewCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkImageView,
) -> VkResult;
pub(crate) type PfnDestroyImageView =
    unsafe extern "system" fn(VkDevice, VkImageView, *const VkAllocationCallbacks);
pub(crate) type PfnCreateShaderModule = unsafe extern "system" fn(
    VkDevice,
    *const VkShaderModuleCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkShaderModule,
) -> VkResult;
pub(crate) type PfnDestroyShaderModule =
    unsafe extern "system" fn(VkDevice, VkShaderModule, *const VkAllocationCallbacks);
pub(crate) type PfnCreatePipelineLayout = unsafe extern "system" fn(
    VkDevice,
    *const VkPipelineLayoutCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkPipelineLayout,
) -> VkResult;
pub(crate) type PfnDestroyPipelineLayout =
    unsafe extern "system" fn(VkDevice, VkPipelineLayout, *const VkAllocationCallbacks);
pub(crate) type PfnCreateGraphicsPipelines = unsafe extern "system" fn(
    VkDevice,
    VkPipelineCache,
    u32,
    *const VkGraphicsPipelineCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkPipeline,
) -> VkResult;
pub(crate) type PfnDestroyPipeline =
    unsafe extern "system" fn(VkDevice, VkPipeline, *const VkAllocationCallbacks);
pub(crate) type PfnCreateRenderPass = unsafe extern "system" fn(
    VkDevice,
    *const VkRenderPassCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkRenderPass,
) -> VkResult;
pub(crate) type PfnDestroyRenderPass =
    unsafe extern "system" fn(VkDevice, VkRenderPass, *const VkAllocationCallbacks);
pub(crate) type PfnCreateFramebuffer = unsafe extern "system" fn(
    VkDevice,
    *const VkFramebufferCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkFramebuffer,
) -> VkResult;
pub(crate) type PfnDestroyFramebuffer =
    unsafe extern "system" fn(VkDevice, VkFramebuffer, *const VkAllocationCallbacks);
pub(crate) type PfnCmdBeginRenderPass =
    unsafe extern "system" fn(VkCommandBuffer, *const VkRenderPassBeginInfo, i32);
pub(crate) type PfnCmdEndRenderPass = unsafe extern "system" fn(VkCommandBuffer);
pub(crate) type PfnCmdClearAttachments = unsafe extern "system" fn(
    VkCommandBuffer,
    u32,
    *const VkClearAttachment,
    u32,
    *const VkClearRect,
);
pub(crate) type PfnCmdBindPipeline = unsafe extern "system" fn(VkCommandBuffer, i32, VkPipeline);
pub(crate) type PfnCmdSetViewport =
    unsafe extern "system" fn(VkCommandBuffer, u32, u32, *const VkViewport);
pub(crate) type PfnCmdSetScissor =
    unsafe extern "system" fn(VkCommandBuffer, u32, u32, *const VkRect2d);
pub(crate) type PfnCmdSetBlendConstants = unsafe extern "system" fn(VkCommandBuffer, *const f32);
pub(crate) type PfnCmdPushConstants =
    unsafe extern "system" fn(VkCommandBuffer, VkPipelineLayout, u32, u32, u32, *const c_void);
pub(crate) type PfnCmdDraw = unsafe extern "system" fn(VkCommandBuffer, u32, u32, u32, u32);

#[repr(C)]
pub(crate) struct BaseInStructure {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const Self,
}

#[repr(C)]
pub(crate) struct LayerInstanceLink {
    pub(crate) next: *mut Self,
    pub(crate) next_get_instance_proc_addr: Option<PfnGetInstanceProcAddr>,
    pub(crate) next_get_physical_device_proc_addr: Option<PfnGetPhysicalDeviceProcAddr>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct LayerDeviceCallbacks {
    pub(crate) create_device: PfnVoidFunction,
    pub(crate) destroy_device: PfnVoidFunction,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) union LayerInstanceCreateInfoUnion {
    pub(crate) layer_info: *mut LayerInstanceLink,
    pub(crate) set_instance_loader_data: PfnVoidFunction,
    pub(crate) layer_device: LayerDeviceCallbacks,
    pub(crate) loader_features: u32,
}

#[repr(C)]
pub(crate) struct LayerInstanceCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) function: i32,
    pub(crate) data: LayerInstanceCreateInfoUnion,
}

#[repr(C)]
pub(crate) struct LayerDeviceLink {
    pub(crate) next: *mut Self,
    pub(crate) next_get_instance_proc_addr: Option<PfnGetInstanceProcAddr>,
    pub(crate) next_get_device_proc_addr: Option<PfnGetDeviceProcAddr>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) union LayerDeviceCreateInfoUnion {
    pub(crate) layer_info: *mut LayerDeviceLink,
    pub(crate) set_device_loader_data: PfnVoidFunction,
}

#[repr(C)]
pub(crate) struct LayerDeviceCreateInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) function: i32,
    pub(crate) data: LayerDeviceCreateInfoUnion,
}

pub(crate) const NEGOTIATE_LAYER_INTERFACE: i32 = 1;

#[repr(C)]
pub(crate) struct NegotiateLayerInterface {
    pub(crate) s_type: i32,
    pub(crate) p_next: *mut c_void,
    pub(crate) loader_layer_interface_version: u32,
    pub(crate) get_instance_proc_addr: Option<PfnGetInstanceProcAddr>,
    pub(crate) get_device_proc_addr: Option<PfnGetDeviceProcAddr>,
    pub(crate) get_physical_device_proc_addr: Option<PfnGetPhysicalDeviceProcAddr>,
}
