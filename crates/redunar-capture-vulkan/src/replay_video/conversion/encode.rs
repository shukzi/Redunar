use super::*;
type VkQueryPool = u64;

#[repr(C)]
struct VkQueryPoolCreateInfo {
    s_type: i32,
    p_next: *const c_void,
    flags: u32,
    query_type: i32,
    query_count: u32,
    pipeline_statistics: u32,
}

const MAX_BITSTREAM_BYTES_PER_FRAME: u64 = 8 * 1024 * 1024;
const VK_STRUCTURE_TYPE_QUERY_POOL_CREATE_INFO: i32 = 11;
const VK_STRUCTURE_TYPE_QUERY_POOL_VIDEO_ENCODE_FEEDBACK_CREATE_INFO_KHR: i32 = 1_000_299_005;
const VK_BUFFER_USAGE_VIDEO_ENCODE_DST_BIT_KHR: u32 = 0x0000_8000;
const VK_QUERY_TYPE_VIDEO_ENCODE_FEEDBACK_KHR: i32 = 1_000_299_000;
const VK_VIDEO_ENCODE_FEEDBACK_BITSTREAM_BYTES_WRITTEN_BIT_KHR: u32 = 0x2;
const VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT: u32 = 0x2;
const VK_MEMORY_PROPERTY_HOST_COHERENT_BIT: u32 = 0x4;

#[repr(C)]
struct VkQueryPoolVideoEncodeFeedbackCreateInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    encode_feedback_flags: u32,
}

type MapMemory = unsafe extern "system" fn(
    VkDevice,
    VkDeviceMemory,
    u64,
    u64,
    u32,
    *mut *mut c_void,
) -> VkResult;
type UnmapMemory = unsafe extern "system" fn(VkDevice, VkDeviceMemory);
type CreateQueryPool = unsafe extern "system" fn(
    VkDevice,
    *const VkQueryPoolCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkQueryPool,
) -> VkResult;
type DestroyQueryPool =
    unsafe extern "system" fn(VkDevice, VkQueryPool, *const VkAllocationCallbacks);

#[derive(Clone, Copy)]
struct Functions {
    destroy_buffer: DestroyBuffer,
    free_memory: FreeMemory,
    unmap_memory: UnmapMemory,
    destroy_query_pool: DestroyQueryPool,
}

pub(super) struct EncodeSlot {
    pub(super) buffer: VkBuffer,
    pub(super) memory: VkDeviceMemory,
    pub(super) mapped_address: usize,
    pub(super) size: u64,
    pub(super) allocation_size: u64,
    pub(super) query_pool: VkQueryPool,
    pub(super) memory_coherent: bool,
}

pub(super) struct EncodeResources {
    device_address: usize,
    functions: Functions,
    pub(super) slots: Vec<EncodeSlot>,
    active: bool,
}

impl EncodeResources {
    pub(super) unsafe fn new(
        owner: &VulkanVideoH264Parameters,
        slot_count: usize,
    ) -> Result<Self, VulkanVideoDeviceError> {
        if owner
            .session
            .device
            .candidate
            .supported_encode_feedback_flags
            & VK_VIDEO_ENCODE_FEEDBACK_BITSTREAM_BYTES_WRITTEN_BIT_KHR
            == 0
        {
            return Err(VulkanVideoDeviceError::EncodeFeedbackUnsupported);
        }
        macro_rules! load {
            ($symbol:literal, $ty:ty) => {{
                let name = concat!($symbol, "\0");
                let Some(raw) = (unsafe {
                    resolve_device_command(&owner.session.device, name.as_ptr().cast())
                }) else {
                    return Err(VulkanVideoDeviceError::DeviceSymbolUnavailable($symbol));
                };
                // SAFETY: command name fixes the function ABI.
                unsafe { mem::transmute::<unsafe extern "system" fn(), $ty>(raw) }
            }};
        }
        let create_buffer = load!("vkCreateBuffer", CreateBuffer);
        let destroy_buffer = load!("vkDestroyBuffer", DestroyBuffer);
        let get_buffer_requirements =
            load!("vkGetBufferMemoryRequirements", GetBufferMemoryRequirements);
        let allocate_memory = load!("vkAllocateMemory", AllocateMemory);
        let free_memory = load!("vkFreeMemory", FreeMemory);
        let bind_buffer_memory = load!("vkBindBufferMemory", BindBufferMemory);
        let map_memory = load!("vkMapMemory", MapMemory);
        let unmap_memory = load!("vkUnmapMemory", UnmapMemory);
        let get_memory_properties = load!(
            "vkGetPhysicalDeviceMemoryProperties",
            GetPhysicalDeviceMemoryProperties
        );
        let create_query_pool = load!("vkCreateQueryPool", CreateQueryPool);
        let destroy_query_pool = load!("vkDestroyQueryPool", DestroyQueryPool);
        let device = owner.session.device.device_address as VkDevice;
        let functions = Functions {
            destroy_buffer,
            free_memory,
            unmap_memory,
            destroy_query_pool,
        };
        let alignment = owner
            .session
            .device
            .candidate
            .min_bitstream_size_alignment
            .max(1);
        let size = align_up(MAX_BITSTREAM_BYTES_PER_FRAME, alignment)
            .ok_or(VulkanVideoDeviceError::SessionParameterBytesExceeded)?;
        let mut memory_properties = VkPhysicalDeviceMemoryProperties::default();
        unsafe {
            get_memory_properties(
                owner.session.device.physical_device_address as VkPhysicalDevice,
                &raw mut memory_properties,
            );
        }
        let mut resources = Self {
            device_address: device.addr(),
            functions,
            slots: Vec::with_capacity(slot_count),
            active: true,
        };
        for _ in 0..slot_count {
            resources.slots.push(unsafe {
                create_slot(
                    device,
                    size,
                    &memory_properties,
                    create_buffer,
                    get_buffer_requirements,
                    allocate_memory,
                    bind_buffer_memory,
                    map_memory,
                    create_query_pool,
                    functions,
                )
            }?);
        }
        Ok(resources)
    }

    pub(super) unsafe fn destroy(&mut self) {
        self.destroy_owned();
    }

    fn destroy_owned(&mut self) {
        if !self.active {
            return;
        }
        let device = self.device_address as VkDevice;
        // SAFETY: every slot resource belongs to this live device. The caller
        // waits the device idle before normal teardown.
        unsafe {
            for slot in self.slots.drain(..).rev() {
                if slot.query_pool != 0 {
                    (self.functions.destroy_query_pool)(device, slot.query_pool, ptr::null());
                }
                if slot.mapped_address != 0 {
                    (self.functions.unmap_memory)(device, slot.memory);
                }
                if slot.buffer != 0 {
                    (self.functions.destroy_buffer)(device, slot.buffer, ptr::null());
                }
                if slot.memory != 0 {
                    (self.functions.free_memory)(device, slot.memory, ptr::null());
                }
            }
        }
        self.active = false;
    }
}

impl Drop for EncodeResources {
    fn drop(&mut self) {
        self.destroy_owned();
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "native function pointers keep this transactional slot constructor dependency-free"
)]
unsafe fn create_slot(
    device: VkDevice,
    size: u64,
    memory_properties: &VkPhysicalDeviceMemoryProperties,
    create_buffer: CreateBuffer,
    get_buffer_requirements: GetBufferMemoryRequirements,
    allocate_memory: AllocateMemory,
    bind_buffer_memory: BindBufferMemory,
    map_memory: MapMemory,
    create_query_pool: CreateQueryPool,
    cleanup: Functions,
) -> Result<EncodeSlot, VulkanVideoDeviceError> {
    let mut build = SlotBuildGuard {
        device,
        functions: cleanup,
        buffer: 0,
        memory: 0,
        mapped_address: 0,
        query_pool: 0,
        active: true,
    };
    let buffer_info = VkBufferCreateInfo {
        s_type: VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
        p_next: ptr::null(),
        flags: 0,
        size,
        usage: VK_BUFFER_USAGE_VIDEO_ENCODE_DST_BIT_KHR,
        sharing_mode: VK_SHARING_MODE_EXCLUSIVE,
        queue_family_index_count: 0,
        queue_family_indices: ptr::null(),
    };
    // SAFETY: create info and output storage are valid.
    let result = unsafe {
        create_buffer(
            device,
            &raw const buffer_info,
            ptr::null(),
            &raw mut build.buffer,
        )
    };
    if result != VK_SUCCESS || build.buffer == 0 {
        return Err(VulkanVideoDeviceError::BitstreamBufferCreationFailed(
            result,
        ));
    }
    let mut requirements = VkMemoryRequirements {
        size: 0,
        alignment: 0,
        memory_type_bits: 0,
    };
    unsafe { get_buffer_requirements(device, build.buffer, &raw mut requirements) };
    let Some((memory_type_index, coherent)) =
        host_visible_memory_type(memory_properties, requirements.memory_type_bits)
    else {
        return Err(VulkanVideoDeviceError::BitstreamMemoryTypeUnavailable);
    };
    let allocation_info = VkMemoryAllocateInfo {
        s_type: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        p_next: ptr::null(),
        allocation_size: requirements.size,
        memory_type_index,
    };
    let result = unsafe {
        allocate_memory(
            device,
            &raw const allocation_info,
            ptr::null(),
            &raw mut build.memory,
        )
    };
    if result != VK_SUCCESS || build.memory == 0 {
        return Err(VulkanVideoDeviceError::BitstreamMemoryAllocationFailed(
            result,
        ));
    }
    let result = unsafe { bind_buffer_memory(device, build.buffer, build.memory, 0) };
    if result != VK_SUCCESS {
        return Err(VulkanVideoDeviceError::BitstreamBufferBindFailed(result));
    }
    let mut mapped = ptr::null_mut();
    let result = unsafe {
        map_memory(
            device,
            build.memory,
            0,
            requirements.size,
            0,
            &raw mut mapped,
        )
    };
    if result != VK_SUCCESS || mapped.is_null() {
        return Err(VulkanVideoDeviceError::BitstreamMapFailed(result));
    }
    build.mapped_address = mapped.addr();
    let h264_profile = VkVideoEncodeH264ProfileInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_PROFILE_INFO_KHR,
        p_next: ptr::null(),
        std_profile_idc: STD_VIDEO_H264_PROFILE_IDC_HIGH,
    };
    let usage = VkVideoEncodeUsageInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_USAGE_INFO_KHR,
        p_next: (&raw const h264_profile).cast(),
        video_usage_hints: VK_VIDEO_ENCODE_USAGE_RECORDING_BIT_KHR,
        video_content_hints: VK_VIDEO_ENCODE_CONTENT_RENDERED_BIT_KHR,
        tuning_mode: VK_VIDEO_ENCODE_TUNING_MODE_DEFAULT_KHR,
    };
    let profile = VkVideoProfileInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_PROFILE_INFO_KHR,
        p_next: (&raw const usage).cast(),
        video_codec_operation: VK_VIDEO_CODEC_OPERATION_ENCODE_H264_BIT_KHR,
        chroma_subsampling: VK_VIDEO_CHROMA_SUBSAMPLING_420_BIT_KHR,
        luma_bit_depth: VK_VIDEO_COMPONENT_BIT_DEPTH_8_BIT_KHR,
        chroma_bit_depth: VK_VIDEO_COMPONENT_BIT_DEPTH_8_BIT_KHR,
    };
    let feedback_info = VkQueryPoolVideoEncodeFeedbackCreateInfoKhr {
        s_type: VK_STRUCTURE_TYPE_QUERY_POOL_VIDEO_ENCODE_FEEDBACK_CREATE_INFO_KHR,
        p_next: (&raw const profile).cast(),
        encode_feedback_flags: VK_VIDEO_ENCODE_FEEDBACK_BITSTREAM_BYTES_WRITTEN_BIT_KHR,
    };
    let query_info = VkQueryPoolCreateInfo {
        s_type: VK_STRUCTURE_TYPE_QUERY_POOL_CREATE_INFO,
        p_next: (&raw const feedback_info).cast(),
        flags: 0,
        query_type: VK_QUERY_TYPE_VIDEO_ENCODE_FEEDBACK_KHR,
        query_count: 1,
        pipeline_statistics: 0,
    };
    let result = unsafe {
        create_query_pool(
            device,
            &raw const query_info,
            ptr::null(),
            &raw mut build.query_pool,
        )
    };
    if result != VK_SUCCESS || build.query_pool == 0 {
        return Err(VulkanVideoDeviceError::EncodeQueryPoolCreationFailed(
            result,
        ));
    }
    let slot = EncodeSlot {
        buffer: build.buffer,
        memory: build.memory,
        mapped_address: build.mapped_address,
        size,
        allocation_size: requirements.size,
        query_pool: build.query_pool,
        memory_coherent: coherent,
    };
    build.active = false;
    Ok(slot)
}

struct SlotBuildGuard {
    device: VkDevice,
    functions: Functions,
    buffer: VkBuffer,
    memory: VkDeviceMemory,
    mapped_address: usize,
    query_pool: VkQueryPool,
    active: bool,
}

impl Drop for SlotBuildGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        // SAFETY: this guard owns every non-zero partial handle.
        unsafe {
            if self.query_pool != 0 {
                (self.functions.destroy_query_pool)(self.device, self.query_pool, ptr::null());
            }
            if self.mapped_address != 0 {
                (self.functions.unmap_memory)(self.device, self.memory);
            }
            if self.buffer != 0 {
                (self.functions.destroy_buffer)(self.device, self.buffer, ptr::null());
            }
            if self.memory != 0 {
                (self.functions.free_memory)(self.device, self.memory, ptr::null());
            }
        }
    }
}

fn host_visible_memory_type(
    properties: &VkPhysicalDeviceMemoryProperties,
    compatible_bits: u32,
) -> Option<(u32, bool)> {
    let count = properties.memory_type_count.min(32);
    let visible = |index: u32| {
        compatible_bits & (1_u32 << index) != 0
            && properties.memory_types[usize::try_from(index).unwrap_or(0)].property_flags
                & VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT
                != 0
    };
    (0..count)
        .find(|index| {
            visible(*index)
                && properties.memory_types[usize::try_from(*index).unwrap_or(0)].property_flags
                    & VK_MEMORY_PROPERTY_HOST_COHERENT_BIT
                    != 0
        })
        .map(|index| (index, true))
        .or_else(|| {
            (0..count)
                .find(|index| visible(*index))
                .map(|index| (index, false))
        })
}

fn align_up(value: u64, alignment: u64) -> Option<u64> {
    value
        .checked_add(alignment.checked_sub(1)?)
        .map(|value| value / alignment * alignment)
}
