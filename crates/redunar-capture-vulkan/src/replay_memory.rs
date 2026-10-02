//! External buffer queries shared by the game-local producer allocation path.
use crate::ffi::{
    VK_BUFFER_USAGE_TRANSFER_DST_BIT, VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT, VkBuffer,
    VkDevice, VkImage, VkMemoryRequirements, VkPhysicalDevice,
};
use std::ffi::c_void;
use std::ptr;

const VK_BUFFER_USAGE_STORAGE_BUFFER_BIT: u32 = 0x20;
const VK_EXTERNAL_MEMORY_FEATURE_DEDICATED_ONLY_BIT: u32 = 0x1;
const VK_EXTERNAL_MEMORY_FEATURE_EXPORTABLE_BIT: u32 = 0x2;
const VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_EXTERNAL_BUFFER_INFO: i32 = 1_000_071_002;
const VK_STRUCTURE_TYPE_EXTERNAL_BUFFER_PROPERTIES: i32 = 1_000_071_003;

#[repr(C)]
pub(crate) struct ExternalBufferInfo {
    s_type: i32,
    p_next: *const c_void,
    flags: u32,
    usage: u32,
    handle_type: u32,
}

#[repr(C)]
pub(crate) struct ExternalBufferProperties {
    s_type: i32,
    p_next: *mut c_void,
    features: u32,
    export_from_imported_handle_types: u32,
    compatible_handle_types: u32,
}

pub(crate) type GetExternalBufferProperties = unsafe extern "system" fn(
    VkPhysicalDevice,
    *const ExternalBufferInfo,
    *mut ExternalBufferProperties,
);

pub(crate) unsafe fn export_support(
    physical_device: VkPhysicalDevice,
    get: GetExternalBufferProperties,
) -> Option<bool> {
    let info = ExternalBufferInfo {
        s_type: VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_EXTERNAL_BUFFER_INFO,
        p_next: ptr::null(),
        flags: 0,
        usage: VK_BUFFER_USAGE_TRANSFER_DST_BIT | VK_BUFFER_USAGE_STORAGE_BUFFER_BIT,
        handle_type: VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT,
    };
    let mut properties = ExternalBufferProperties {
        s_type: VK_STRUCTURE_TYPE_EXTERNAL_BUFFER_PROPERTIES,
        p_next: ptr::null_mut(),
        features: 0,
        export_from_imported_handle_types: 0,
        compatible_handle_types: 0,
    };
    unsafe { get(physical_device, &raw const info, &raw mut properties) };
    (properties.features & VK_EXTERNAL_MEMORY_FEATURE_EXPORTABLE_BIT != 0
        && properties.compatible_handle_types & VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT != 0)
        .then_some(properties.features & VK_EXTERNAL_MEMORY_FEATURE_DEDICATED_ONLY_BIT != 0)
}

#[repr(C)]
pub(crate) struct BufferRequirementsInfo {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) buffer: VkBuffer,
}

#[repr(C)]
pub(crate) struct DedicatedRequirements {
    pub(crate) s_type: i32,
    pub(crate) p_next: *mut c_void,
    pub(crate) prefers: u32,
    pub(crate) requires: u32,
}

#[repr(C)]
pub(crate) struct Requirements2 {
    pub(crate) s_type: i32,
    pub(crate) p_next: *mut c_void,
    pub(crate) memory: VkMemoryRequirements,
}

pub(crate) type GetBufferRequirements2 =
    unsafe extern "system" fn(VkDevice, *const BufferRequirementsInfo, *mut Requirements2);

#[repr(C)]
pub(crate) struct DedicatedAllocation {
    pub(crate) s_type: i32,
    pub(crate) p_next: *const c_void,
    pub(crate) image: VkImage,
    pub(crate) buffer: VkBuffer,
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "system" fn fake_properties(
        device: VkPhysicalDevice,
        info: *const ExternalBufferInfo,
        out: *mut ExternalBufferProperties,
    ) {
        let info = unsafe { &*info };
        assert_eq!(
            info.usage,
            VK_BUFFER_USAGE_TRANSFER_DST_BIT | VK_BUFFER_USAGE_STORAGE_BUFFER_BIT
        );
        assert_eq!(
            info.handle_type,
            VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT
        );
        let out = unsafe { &mut *out };
        out.features = u32::try_from(device.addr()).unwrap();
        out.compatible_handle_types = VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT;
    }

    #[test]
    fn producer_queries_exact_usage_and_rejects_import_only_memory() {
        assert_eq!(
            unsafe { export_support(4_usize as VkPhysicalDevice, fake_properties) },
            None
        );
        assert_eq!(
            unsafe { export_support(2_usize as VkPhysicalDevice, fake_properties) },
            Some(false)
        );
        assert_eq!(
            unsafe { export_support(3_usize as VkPhysicalDevice, fake_properties) },
            Some(true)
        );
    }
}
