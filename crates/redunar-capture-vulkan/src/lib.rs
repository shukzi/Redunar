//! Clean-room Vulkan explicit-layer scaffold for Redunar frame telemetry.
//!
//! This crate is the project's only native ABI boundary. It observes present
//! call timing, sends bounded local messages, and always delegates graphics
//! work to the next Vulkan layer or driver. It does not draw, tune, upload, or
//! alter swapchains.

mod dispatch;
pub mod fd_transport;
mod ffi;
mod overlay;
mod producer;
mod replay_copy;
mod replay_transfer;
pub mod replay_video;

use crate::dispatch::{DeviceDispatch, InstanceDispatch};
use crate::ffi::{
    BaseInStructure, LayerDeviceCreateInfo, LayerInstanceCreateInfo, NEGOTIATE_LAYER_INTERFACE,
    NegotiateLayerInterface, PfnCreateDevice, PfnCreateInstance, PfnDestroyDevice,
    PfnDestroyInstance, PfnGetDeviceProcAddr, PfnGetDeviceQueue, PfnGetDeviceQueue2,
    PfnGetInstanceProcAddr, PfnGetPhysicalDeviceMemoryProperties,
    PfnGetPhysicalDeviceQueueFamilyProperties, PfnQueuePresentKhr, PfnVoidFunction,
    VK_ERROR_EXTENSION_NOT_PRESENT, VK_ERROR_INITIALIZATION_FAILED, VK_LAYER_LINK_INFO,
    VK_QUEUE_GRAPHICS_BIT, VK_STRUCTURE_TYPE_DEVICE_QUEUE_INFO_2,
    VK_STRUCTURE_TYPE_LOADER_DEVICE_CREATE_INFO, VK_STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO,
    VK_SUCCESS, VkAllocationCallbacks, VkDevice, VkDeviceCreateInfo, VkDeviceQueueInfo2,
    VkInstance, VkInstanceCreateInfo, VkPhysicalDevice, VkPhysicalDeviceMemoryProperties,
    VkPresentInfoKhr, VkQueue, VkQueueFamilyProperties, VkResult, VkSwapchainCreateInfoKhr,
    VkSwapchainKhr,
};
use std::ffi::{CStr, c_char, c_void};
use std::mem;

const LOADER_LAYER_INTERFACE_VERSION: u32 = 2;

const CREATE_INSTANCE: &[u8] = b"vkCreateInstance\0";
const DESTROY_INSTANCE: &[u8] = b"vkDestroyInstance\0";
const CREATE_DEVICE: &[u8] = b"vkCreateDevice\0";
const DESTROY_DEVICE: &[u8] = b"vkDestroyDevice\0";
const GET_DEVICE_QUEUE: &[u8] = b"vkGetDeviceQueue\0";
const GET_DEVICE_QUEUE_2: &[u8] = b"vkGetDeviceQueue2\0";
const GET_PHYSICAL_DEVICE_QUEUE_FAMILY_PROPERTIES: &[u8] =
    b"vkGetPhysicalDeviceQueueFamilyProperties\0";
const GET_PHYSICAL_DEVICE_MEMORY_PROPERTIES: &[u8] = b"vkGetPhysicalDeviceMemoryProperties\0";
const ENUMERATE_DEVICE_EXTENSION_PROPERTIES: &[u8] = b"vkEnumerateDeviceExtensionProperties\0";
const EXTERNAL_MEMORY_FD_EXTENSION: &[u8] = b"VK_KHR_external_memory_fd\0";
const EXTERNAL_MEMORY_DMA_BUF_EXTENSION: &[u8] = b"VK_EXT_external_memory_dma_buf\0";
const CREATE_SWAPCHAIN: &[u8] = b"vkCreateSwapchainKHR\0";
const DESTROY_SWAPCHAIN: &[u8] = b"vkDestroySwapchainKHR\0";
const QUEUE_PRESENT: &[u8] = b"vkQueuePresentKHR\0";
const GET_INSTANCE_PROC_ADDR: &[u8] = b"vkGetInstanceProcAddr\0";
const GET_DEVICE_PROC_ADDR: &[u8] = b"vkGetDeviceProcAddr\0";
const NEGOTIATE_INTERFACE: &[u8] = b"vkNegotiateLoaderLayerInterfaceVersion\0";
const MAX_DEVICE_EXTENSIONS: usize = 1_024;

#[repr(C)]
struct VkExtensionProperties {
    extension_name: [c_char; 256],
    spec_version: u32,
}

impl Default for VkExtensionProperties {
    fn default() -> Self {
        Self {
            extension_name: [0; 256],
            spec_version: 0,
        }
    }
}

type PfnEnumerateDeviceExtensionProperties = unsafe extern "system" fn(
    VkPhysicalDevice,
    *const c_char,
    *mut u32,
    *mut VkExtensionProperties,
) -> VkResult;

#[unsafe(export_name = "vkNegotiateLoaderLayerInterfaceVersion")]
unsafe extern "system" fn negotiate_loader_layer_interface_version(
    interface: *mut NegotiateLayerInterface,
) -> VkResult {
    if interface.is_null() {
        return VK_ERROR_INITIALIZATION_FAILED;
    }
    // SAFETY: the loader provides a writable interface structure for the
    // duration of this call; null was rejected above.
    let interface = unsafe { &mut *interface };
    if interface.s_type != NEGOTIATE_LAYER_INTERFACE
        || interface.loader_layer_interface_version == 0
    {
        return VK_ERROR_INITIALIZATION_FAILED;
    }
    interface.loader_layer_interface_version = interface
        .loader_layer_interface_version
        .min(LOADER_LAYER_INTERFACE_VERSION);
    interface.get_instance_proc_addr = Some(get_instance_proc_addr);
    interface.get_device_proc_addr = Some(get_device_proc_addr);
    interface.get_physical_device_proc_addr = Some(get_physical_device_proc_addr);
    VK_SUCCESS
}

#[unsafe(export_name = "vkGetInstanceProcAddr")]
unsafe extern "system" fn get_instance_proc_addr(
    instance: VkInstance,
    name: *const c_char,
) -> PfnVoidFunction {
    let name_bytes = unsafe { proc_name(name) }?;
    if let Some(intercept) = instance_intercept(name_bytes) {
        return Some(intercept);
    }
    if instance.is_null() {
        let next = dispatch::global_next_instance()?;
        // SAFETY: forwarding the loader-provided name and null instance to the
        // next layer uses the Vulkan GIPA contract.
        return unsafe { next(instance, name) };
    }
    let key = unsafe { dispatch_key(instance.cast::<c_void>()) }?;
    let next = dispatch::instance(key)?.next_get_instance_proc_addr;
    // SAFETY: the dispatch was captured for this instance at creation.
    unsafe { next(instance, name) }
}

#[unsafe(export_name = "vkGetDeviceProcAddr")]
unsafe extern "system" fn get_device_proc_addr(
    device: VkDevice,
    name: *const c_char,
) -> PfnVoidFunction {
    let name_bytes = unsafe { proc_name(name) }?;
    if let Some(intercept) = device_intercept(name_bytes) {
        return Some(intercept);
    }
    let key = unsafe { dispatch_key(device.cast::<c_void>()) }?;
    let next = dispatch::device(key)?.next_get_device_proc_addr;
    // SAFETY: the dispatch was captured for this device at creation.
    unsafe { next(device, name) }
}

unsafe extern "system" fn get_physical_device_proc_addr(
    instance: VkInstance,
    name: *const c_char,
) -> PfnVoidFunction {
    // SAFETY: physical-device extension lookup shares the instance dispatch
    // chain supplied by the loader.
    unsafe { get_instance_proc_addr(instance, name) }
}

#[unsafe(export_name = "vkCreateInstance")]
unsafe extern "system" fn create_instance(
    create_info: *const VkInstanceCreateInfo,
    allocator: *const VkAllocationCallbacks,
    instance: *mut VkInstance,
) -> VkResult {
    if create_info.is_null() || instance.is_null() {
        return VK_ERROR_INITIALIZATION_FAILED;
    }
    let Some(chain_info) = (unsafe { find_instance_link(create_info) }) else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    // SAFETY: `find_instance_link` returned a loader-owned link-info node with
    // the expected structure and union arm.
    let link = unsafe { (*chain_info).data.layer_info };
    if link.is_null() {
        return VK_ERROR_INITIALIZATION_FAILED;
    }
    // SAFETY: the loader link is valid for this call.
    let Some(next_gipa) = (unsafe { (*link).next_get_instance_proc_addr }) else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    // SAFETY: advancing the loader-owned chain before forwarding is required
    // by the Vulkan layer interface contract.
    unsafe { (*chain_info).data.layer_info = (*link).next };
    // SAFETY: the next GIPA owns the returned generic function pointer.
    let Some(next_create) = cast_create_instance(unsafe {
        next_gipa(std::ptr::null_mut(), CREATE_INSTANCE.as_ptr().cast())
    }) else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    dispatch::set_global_next_instance(next_gipa);
    // SAFETY: arguments are forwarded unchanged to the next layer.
    let result = unsafe { next_create(create_info, allocator, instance) };
    if result == VK_SUCCESS {
        // SAFETY: successful Vulkan creation initialized the output handle.
        let created = unsafe { *instance };
        if let Some(key) = unsafe { dispatch_key(created.cast::<c_void>()) } {
            // SAFETY: lookup is scoped to the newly created instance.
            let destroy = cast_destroy_instance(unsafe {
                next_gipa(created, DESTROY_INSTANCE.as_ptr().cast())
            });
            dispatch::insert_instance(
                key,
                InstanceDispatch {
                    instance_address: created.addr(),
                    next_get_instance_proc_addr: next_gipa,
                    destroy_instance: destroy,
                },
            );
        }
    }
    result
}

#[unsafe(export_name = "vkDestroyInstance")]
unsafe extern "system" fn destroy_instance(
    instance: VkInstance,
    allocator: *const VkAllocationCallbacks,
) {
    let dispatch =
        unsafe { dispatch_key(instance.cast::<c_void>()) }.and_then(dispatch::remove_instance);
    if let Some(next) = dispatch.and_then(|value| value.destroy_instance) {
        // SAFETY: this function was captured from the instance's next layer.
        unsafe { next(instance, allocator) };
    }
}

#[unsafe(export_name = "vkCreateDevice")]
unsafe extern "system" fn create_device(
    physical_device: VkPhysicalDevice,
    create_info: *const VkDeviceCreateInfo,
    allocator: *const VkAllocationCallbacks,
    device: *mut VkDevice,
) -> VkResult {
    if create_info.is_null() || device.is_null() {
        return VK_ERROR_INITIALIZATION_FAILED;
    }
    let Some(chain_info) = (unsafe { find_device_link(create_info) }) else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    // SAFETY: `find_device_link` validated the active union arm.
    let link = unsafe { (*chain_info).data.layer_info };
    if link.is_null() {
        return VK_ERROR_INITIALIZATION_FAILED;
    }
    // SAFETY: the loader link is valid for this call.
    let (Some(next_instance_proc), Some(next_device_proc)) = (unsafe {
        (
            (*link).next_get_instance_proc_addr,
            (*link).next_get_device_proc_addr,
        )
    }) else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    // SAFETY: advance the loader-owned chain before forwarding.
    unsafe { (*chain_info).data.layer_info = (*link).next };
    // SAFETY: the next GIPA owns the generic function pointer.
    let Some(next_create) = cast_create_device(unsafe {
        next_instance_proc(std::ptr::null_mut(), CREATE_DEVICE.as_ptr().cast())
    }) else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    let augmented_extensions = unsafe {
        replay_device_extensions(
            physical_device,
            create_info,
            next_instance_proc,
            replay_copy::external_memory_export_requested(),
        )
    };
    // The extension pointer array and its static names remain live through
    // vkCreateDevice. Every other field, including pNext, is preserved.
    let mut augmented_create_info = unsafe { *create_info };
    let forwarded_create_info = if let Some(extensions) = augmented_extensions.as_ref() {
        augmented_create_info.enabled_extension_count =
            u32::try_from(extensions.len()).unwrap_or(u32::MAX);
        augmented_create_info.enabled_extension_names = extensions.as_ptr();
        &raw const augmented_create_info
    } else {
        create_info
    };
    // SAFETY: arguments are forwarded to the next layer; the optional copied
    // extension list remains valid for the duration of this call.
    let result = unsafe { next_create(physical_device, forwarded_create_info, allocator, device) };
    if result == VK_SUCCESS {
        // SAFETY: successful Vulkan creation initialized the output handle.
        let created = unsafe { *device };
        if let Some(key) = unsafe { dispatch_key(created.cast::<c_void>()) } {
            // SAFETY: lookups are scoped to the newly created device.
            let destroy = cast_destroy_device(unsafe {
                next_device_proc(created, DESTROY_DEVICE.as_ptr().cast())
            });
            let queue_present = cast_queue_present(unsafe {
                next_device_proc(created, QUEUE_PRESENT.as_ptr().cast())
            });
            let get_device_queue = cast_get_device_queue(unsafe {
                next_device_proc(created, GET_DEVICE_QUEUE.as_ptr().cast())
            });
            let get_device_queue_2 = cast_get_device_queue_2(unsafe {
                next_device_proc(created, GET_DEVICE_QUEUE_2.as_ptr().cast())
            });
            let create_swapchain = cast_create_swapchain(unsafe {
                next_device_proc(created, CREATE_SWAPCHAIN.as_ptr().cast())
            });
            let destroy_swapchain = cast_destroy_swapchain(unsafe {
                next_device_proc(created, DESTROY_SWAPCHAIN.as_ptr().cast())
            });
            let graphics_queue_families = unsafe { graphics_queue_family_mask(physical_device) };
            // SAFETY: optional renderer commands are looked up on the newly
            // created device and are used only while that device is live.
            let overlay_functions =
                unsafe { overlay::DeviceFunctions::load(next_device_proc, created) };
            let replay_functions =
                unsafe { replay_copy::DeviceFunctions::load(next_device_proc, created) };
            let memory_properties = unsafe { physical_device_memory_properties(physical_device) };
            dispatch::insert_device(
                key,
                DeviceDispatch {
                    next_get_device_proc_addr: next_device_proc,
                    destroy_device: destroy,
                    get_device_queue,
                    get_device_queue2: get_device_queue_2,
                    create_swapchain,
                    destroy_swapchain,
                    queue_present,
                },
            );
            producer::device_created();
            overlay::device_created(key, created, overlay_functions, graphics_queue_families);
            let external_memory_supported =
                unsafe { device_external_memory(forwarded_create_info) };
            replay_copy::device_created(
                key,
                created,
                replay_functions,
                &memory_properties,
                graphics_queue_families,
                external_memory_supported,
            );
        }
    }
    result
}

unsafe fn replay_device_extensions(
    physical_device: VkPhysicalDevice,
    create_info: *const VkDeviceCreateInfo,
    next_instance_proc: PfnGetInstanceProcAddr,
    export_requested: bool,
) -> Option<Vec<*const c_char>> {
    if !export_requested {
        return None;
    }
    let info = unsafe { create_info.as_ref() }?;
    let current = if info.enabled_extension_count == 0 {
        &[][..]
    } else {
        if info.enabled_extension_names.is_null() {
            return None;
        }
        unsafe {
            std::slice::from_raw_parts(
                info.enabled_extension_names,
                usize::try_from(info.enabled_extension_count).ok()?,
            )
        }
    };
    let fd_enabled = extension_pointer_list_contains(current, b"VK_KHR_external_memory_fd");
    let dma_buf_enabled =
        extension_pointer_list_contains(current, b"VK_EXT_external_memory_dma_buf");
    if fd_enabled && dma_buf_enabled {
        return None;
    }

    let raw = unsafe {
        next_instance_proc(
            physical_device_instance(physical_device).unwrap_or(std::ptr::null_mut()),
            ENUMERATE_DEVICE_EXTENSION_PROPERTIES.as_ptr().cast(),
        )
    };
    let Some(raw) = raw else {
        eprintln!("Redunar Replay validation: vkEnumerateDeviceExtensionProperties is unavailable");
        return None;
    };
    let enumerate = unsafe {
        mem::transmute::<unsafe extern "system" fn(), PfnEnumerateDeviceExtensionProperties>(raw)
    };
    let supported = unsafe { enumerate_device_extensions(physical_device, enumerate) }?;
    if (!fd_enabled
        && !supported
            .iter()
            .any(|name| name == b"VK_KHR_external_memory_fd"))
        || (!dma_buf_enabled
            && !supported
                .iter()
                .any(|name| name == b"VK_EXT_external_memory_dma_buf"))
    {
        eprintln!(
            "Redunar Replay validation: DMA-BUF export extensions are unsupported by the game GPU"
        );
        return None;
    }

    let mut extensions = current.to_vec();
    if !fd_enabled {
        extensions.push(EXTERNAL_MEMORY_FD_EXTENSION.as_ptr().cast());
    }
    if !dma_buf_enabled {
        extensions.push(EXTERNAL_MEMORY_DMA_BUF_EXTENSION.as_ptr().cast());
    }
    eprintln!("Redunar Replay validation: enabled Vulkan DMA-BUF export extensions");
    Some(extensions)
}

unsafe fn physical_device_instance(physical_device: VkPhysicalDevice) -> Option<VkInstance> {
    let key = unsafe { dispatch_key(physical_device.cast::<c_void>()) }?;
    dispatch::instance(key).map(|instance| instance.instance_address as VkInstance)
}

fn extension_pointer_list_contains(names: &[*const c_char], expected: &[u8]) -> bool {
    names
        .iter()
        .copied()
        .any(|name| !name.is_null() && unsafe { CStr::from_ptr(name) }.to_bytes() == expected)
}

unsafe fn enumerate_device_extensions(
    physical_device: VkPhysicalDevice,
    enumerate: PfnEnumerateDeviceExtensionProperties,
) -> Option<Vec<Vec<u8>>> {
    let mut count = 0_u32;
    if unsafe {
        enumerate(
            physical_device,
            std::ptr::null(),
            &raw mut count,
            std::ptr::null_mut(),
        )
    } != VK_SUCCESS
    {
        return None;
    }
    let count = usize::try_from(count).ok()?.min(MAX_DEVICE_EXTENSIONS);
    let mut properties: Vec<VkExtensionProperties> = std::iter::repeat_with(Default::default)
        .take(count)
        .collect();
    let mut written = u32::try_from(count).ok()?;
    if unsafe {
        enumerate(
            physical_device,
            std::ptr::null(),
            &raw mut written,
            properties.as_mut_ptr(),
        )
    } != VK_SUCCESS
    {
        return None;
    }
    properties.truncate(usize::try_from(written).ok()?.min(properties.len()));
    Some(
        properties
            .into_iter()
            .map(|property| {
                let bytes = property.extension_name.map(i8::cast_unsigned);
                let end = bytes
                    .iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(bytes.len());
                bytes[..end].to_vec()
            })
            .collect(),
    )
}

unsafe fn device_external_memory(create_info: *const VkDeviceCreateInfo) -> bool {
    let Some(info) = (unsafe { create_info.as_ref() }) else {
        return false;
    };
    if info.enabled_extension_count == 0 || info.enabled_extension_names.is_null() {
        return false;
    }
    let names = unsafe {
        std::slice::from_raw_parts(
            info.enabled_extension_names,
            usize::try_from(info.enabled_extension_count).unwrap_or(0),
        )
    };
    let mut fd = false;
    let mut dma_buf = false;
    for &name in names {
        if name.is_null() {
            continue;
        }
        let value = unsafe { CStr::from_ptr(name) };
        fd |= value.to_bytes() == b"VK_KHR_external_memory_fd";
        dma_buf |= value.to_bytes() == b"VK_EXT_external_memory_dma_buf";
    }
    fd && dma_buf
}

#[unsafe(export_name = "vkDestroyDevice")]
unsafe extern "system" fn destroy_device(
    device: VkDevice,
    allocator: *const VkAllocationCallbacks,
) {
    let key = unsafe { dispatch_key(device.cast::<c_void>()) };
    let dispatch = key.and_then(dispatch::remove_device);
    if let Some(dispatch) = dispatch {
        if let Some(key) = key {
            replay_copy::device_destroyed(key);
            overlay::device_destroyed(key);
        }
        producer::device_destroyed();
        if let Some(next) = dispatch.destroy_device {
            // SAFETY: this function was captured from the device's next layer.
            unsafe { next(device, allocator) };
        }
    }
}

#[unsafe(export_name = "vkGetDeviceQueue")]
unsafe extern "system" fn get_device_queue(
    device: VkDevice,
    queue_family_index: u32,
    queue_index: u32,
    queue: *mut VkQueue,
) {
    let Some(key) = (unsafe { dispatch_key(device.cast::<c_void>()) }) else {
        return;
    };
    let Some(next) = dispatch::device(key).and_then(|value| value.get_device_queue) else {
        return;
    };
    // SAFETY: arguments are forwarded unchanged to the device's next layer.
    unsafe { next(device, queue_family_index, queue_index, queue) };
    if !queue.is_null() {
        // SAFETY: Vulkan initializes the output for a valid queue request.
        overlay::queue_observed(key, unsafe { *queue }, queue_family_index);
        replay_copy::queue_observed(key, unsafe { *queue }, queue_family_index, false);
    }
}

#[unsafe(export_name = "vkGetDeviceQueue2")]
unsafe extern "system" fn get_device_queue_2(
    device: VkDevice,
    queue_info: *const VkDeviceQueueInfo2,
    queue: *mut VkQueue,
) {
    let Some(key) = (unsafe { dispatch_key(device.cast::<c_void>()) }) else {
        return;
    };
    let Some(next) = dispatch::device(key).and_then(|value| value.get_device_queue2) else {
        return;
    };
    // SAFETY: arguments are forwarded unchanged to the device's next layer.
    unsafe { next(device, queue_info, queue) };
    if !queue_info.is_null()
        && !queue.is_null()
        // SAFETY: only the common type tag is inspected before other fields.
        && unsafe { (*queue_info).s_type } == VK_STRUCTURE_TYPE_DEVICE_QUEUE_INFO_2
    {
        // SAFETY: non-null pointers and the expected structure type were checked.
        overlay::queue_observed(key, unsafe { *queue }, unsafe {
            (*queue_info).queue_family_index
        });
        replay_copy::queue_observed(
            key,
            unsafe { *queue },
            unsafe { (*queue_info).queue_family_index },
            unsafe { (*queue_info).flags & 1 != 0 },
        );
    }
}

#[unsafe(export_name = "vkCreateSwapchainKHR")]
unsafe extern "system" fn create_swapchain(
    device: VkDevice,
    create_info: *const VkSwapchainCreateInfoKhr,
    allocator: *const VkAllocationCallbacks,
    swapchain: *mut VkSwapchainKhr,
) -> VkResult {
    let Some(key) = (unsafe { dispatch_key(device.cast::<c_void>()) }) else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    let Some(next) = dispatch::device(key).and_then(|value| value.create_swapchain) else {
        return VK_ERROR_EXTENSION_NOT_PRESENT;
    };
    let replay_info = unsafe { replay_transfer::transfer_source_create_info(create_info) };
    let forwarded_info = replay_info.as_ref().map_or(create_info, std::ptr::from_ref);
    // SAFETY: forwards either the original create info or a stack copy that
    // differs only by the diagnostic transfer-source usage bit.
    let mut result = unsafe { next(device, forwarded_info, allocator, swapchain) };
    let mut successful_info = forwarded_info;
    if result != VK_SUCCESS && replay_info.is_some() {
        if !swapchain.is_null() {
            // Prevent a failed diagnostic attempt from leaving an undefined
            // output value visible to the original retry.
            unsafe { *swapchain = 0 };
        }
        // SAFETY: the diagnostic path is fail-open and retries the exact
        // application request when the driver rejects the extra usage.
        result = unsafe { next(device, create_info, allocator, swapchain) };
        successful_info = create_info;
    }
    if result == VK_SUCCESS && !swapchain.is_null() {
        // SAFETY: success initialized the output; create_info remains live for
        // the duration of this wrapper call.
        unsafe { overlay::swapchain_created(key, successful_info, *swapchain) };
        unsafe { replay_copy::swapchain_created(key, successful_info, *swapchain) };
        // SAFETY: this reads only the still-live application create-info and
        // emits bounded metadata when an explicit replay request is present.
        if let Some(assessment) =
            unsafe { replay_transfer::assessment_from_create_info(successful_info) }
        {
            match assessment {
                Ok(candidate) => producer::record_replay_source_candidate(candidate),
                Err(reason) => producer::record_replay_source_rejected(reason),
            }
        }
    }
    result
}

#[unsafe(export_name = "vkDestroySwapchainKHR")]
unsafe extern "system" fn destroy_swapchain(
    device: VkDevice,
    swapchain: VkSwapchainKhr,
    allocator: *const VkAllocationCallbacks,
) {
    let Some(key) = (unsafe { dispatch_key(device.cast::<c_void>()) }) else {
        return;
    };
    let dispatch = dispatch::device(key);
    replay_copy::swapchain_destroyed(key, swapchain);
    overlay::swapchain_destroyed(key, swapchain);
    if let Some(next) = dispatch.and_then(|value| value.destroy_swapchain) {
        // SAFETY: forwards the original arguments after releasing only
        // Redunar-owned resources associated with this swapchain.
        unsafe { next(device, swapchain, allocator) };
    }
}

#[unsafe(export_name = "vkQueuePresentKHR")]
unsafe extern "system" fn queue_present(
    queue: VkQueue,
    present_info: *const VkPresentInfoKhr,
) -> VkResult {
    let Some(dispatch) =
        (unsafe { dispatch_key(queue.cast::<c_void>()) }).and_then(dispatch::device)
    else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    let Some(next) = dispatch.queue_present else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    // Reuse acknowledged exports before selecting a buffer for this present.
    producer::poll_replay_releases();
    // Optional submissions form one binary-semaphore chain. Replay must never
    // consume the application's waits independently after the overlay has
    // already consumed them.
    let overlay_wait = unsafe { overlay::prepare_present(queue, present_info) };
    if overlay_wait.is_some() {
        // The renderer becomes active only after its queue submission
        // succeeded and returned a semaphore for the forwarded present.
        producer::record_overlay_active();
    }
    let overlay_wait_storage;
    let overlay_forwarded;
    let replay_input = if let Some(semaphore) = overlay_wait {
        if present_info.is_null() {
            present_info
        } else {
            overlay_wait_storage = semaphore;
            overlay_forwarded = VkPresentInfoKhr {
                wait_semaphore_count: 1,
                wait_semaphores: &raw const overlay_wait_storage,
                ..unsafe { *present_info }
            };
            &raw const overlay_forwarded
        }
    } else {
        present_info
    };
    let replay_wait = unsafe { replay_copy::prepare_present(queue, replay_input) };
    let replay_wait_storage;
    let replay_forwarded;
    let forwarded_ptr = if let Some(semaphore) = replay_wait {
        if replay_input.is_null() {
            replay_input
        } else {
            replay_wait_storage = semaphore;
            replay_forwarded = VkPresentInfoKhr {
                wait_semaphore_count: 1,
                wait_semaphores: &raw const replay_wait_storage,
                ..unsafe { *replay_input }
            };
            &raw const replay_forwarded
        }
    } else {
        replay_input
    };
    let result = unsafe { next(queue, forwarded_ptr) };
    if !presentation_succeeded(result)
        && let Some(semaphore) = replay_wait
    {
        replay_copy::presentation_failed(semaphore);
    }
    if presentation_succeeded(result) {
        // SAFETY: the application externally synchronizes `queue` for this
        // complete vkQueuePresentKHR call, including this post-forward hook.
        unsafe { replay_copy::presentation_finished(queue) };
        if let Some(now_ns) = producer::monotonic_ns() {
            producer::record_present_at(now_ns);
            overlay::presentation_finished(now_ns);
        }
    }
    result
}

const fn presentation_succeeded(result: VkResult) -> bool {
    result >= VK_SUCCESS
}

fn instance_intercept(name: &[u8]) -> PfnVoidFunction {
    match name {
        value if value == CREATE_INSTANCE => Some(to_void_create_instance(create_instance)),
        value if value == DESTROY_INSTANCE => Some(to_void_destroy_instance(destroy_instance)),
        value if value == CREATE_DEVICE => Some(to_void_create_device(create_device)),
        value if value == GET_INSTANCE_PROC_ADDR => Some(to_void_gipa(get_instance_proc_addr)),
        value if value == GET_DEVICE_PROC_ADDR => Some(to_void_gdpa(get_device_proc_addr)),
        value if value == NEGOTIATE_INTERFACE => {
            Some(to_void_negotiate(negotiate_loader_layer_interface_version))
        }
        _ => None,
    }
}

fn device_intercept(name: &[u8]) -> PfnVoidFunction {
    match name {
        value if value == DESTROY_DEVICE => Some(to_void_destroy_device(destroy_device)),
        value if value == GET_DEVICE_QUEUE => Some(to_void_get_device_queue(get_device_queue)),
        value if value == GET_DEVICE_QUEUE_2 => {
            Some(to_void_get_device_queue_2(get_device_queue_2))
        }
        value if value == CREATE_SWAPCHAIN => Some(to_void_create_swapchain(create_swapchain)),
        value if value == DESTROY_SWAPCHAIN => Some(to_void_destroy_swapchain(destroy_swapchain)),
        value if value == QUEUE_PRESENT => Some(to_void_queue_present(queue_present)),
        value if value == GET_DEVICE_PROC_ADDR => Some(to_void_gdpa(get_device_proc_addr)),
        _ => None,
    }
}

unsafe fn proc_name<'a>(name: *const c_char) -> Option<&'a [u8]> {
    if name.is_null() {
        return None;
    }
    // SAFETY: Vulkan procedure names are loader-owned NUL-terminated strings
    // valid for the duration of a lookup.
    Some(unsafe { CStr::from_ptr(name) }.to_bytes_with_nul())
}

unsafe fn dispatch_key(handle: *mut c_void) -> Option<usize> {
    if handle.is_null() {
        return None;
    }
    // SAFETY: Vulkan dispatchable handles begin with a loader dispatch-table
    // pointer. Callers pass only non-null handles received from Vulkan.
    let table = unsafe { *handle.cast::<*const c_void>() };
    (!table.is_null()).then_some(table.addr())
}

unsafe fn graphics_queue_family_mask(physical_device: VkPhysicalDevice) -> u64 {
    let Some(instance_key) = (unsafe { dispatch_key(physical_device.cast::<c_void>()) }) else {
        return 0;
    };
    let Some(instance) = dispatch::instance(instance_key) else {
        return 0;
    };
    let instance_handle = instance.instance_address as VkInstance;
    // SAFETY: this lookup uses the physical device's owning instance dispatch.
    let raw = unsafe {
        (instance.next_get_instance_proc_addr)(
            instance_handle,
            GET_PHYSICAL_DEVICE_QUEUE_FAMILY_PROPERTIES.as_ptr().cast(),
        )
    };
    let Some(get_properties) = cast_get_queue_family_properties(raw) else {
        return 0;
    };
    let empty = VkQueueFamilyProperties {
        queue_flags: 0,
        queue_count: 0,
        timestamp_valid_bits: 0,
        min_image_transfer_granularity: crate::ffi::VkExtent3d {
            width: 0,
            height: 0,
            depth: 0,
        },
    };
    let mut properties = [empty; 64];
    let mut count = u32::try_from(overlay::max_queue_families()).unwrap_or(64);
    // SAFETY: the fixed array has `count` writable entries and the function was
    // resolved from the owning instance.
    unsafe { get_properties(physical_device, &raw mut count, properties.as_mut_ptr()) };
    let mut mask = 0_u64;
    for (index, family) in properties
        .iter()
        .take(usize::try_from(count).unwrap_or(0).min(properties.len()))
        .enumerate()
    {
        if family.queue_count > 0 && family.queue_flags & VK_QUEUE_GRAPHICS_BIT != 0 {
            mask |= 1_u64 << index;
        }
    }
    mask
}

unsafe fn physical_device_memory_properties(
    physical_device: VkPhysicalDevice,
) -> VkPhysicalDeviceMemoryProperties {
    let mut properties = VkPhysicalDeviceMemoryProperties::default();
    let Some(instance_key) = (unsafe { dispatch_key(physical_device.cast::<c_void>()) }) else {
        return properties;
    };
    let Some(instance) = dispatch::instance(instance_key) else {
        return properties;
    };
    let instance_handle = instance.instance_address as VkInstance;
    let raw = unsafe {
        (instance.next_get_instance_proc_addr)(
            instance_handle,
            GET_PHYSICAL_DEVICE_MEMORY_PROPERTIES.as_ptr().cast(),
        )
    };
    let Some(get_properties) = cast_get_memory_properties(raw) else {
        return properties;
    };
    unsafe { get_properties(physical_device, &raw mut properties) };
    properties
}

unsafe fn find_instance_link(
    create_info: *const VkInstanceCreateInfo,
) -> Option<*mut LayerInstanceCreateInfo> {
    // SAFETY: caller validated `create_info`; pNext is traversed using the
    // common Vulkan base-in-structure prefix.
    let mut current = unsafe { (*create_info).p_next.cast::<BaseInStructure>() };
    while !current.is_null() {
        // SAFETY: every pNext node has the base prefix.
        let node = unsafe { &*current };
        if node.s_type == VK_STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO {
            let layer = current.cast_mut().cast::<LayerInstanceCreateInfo>();
            // SAFETY: matching sType selects the loader structure layout.
            if unsafe { (*layer).function } == VK_LAYER_LINK_INFO {
                return Some(layer);
            }
        }
        current = node.p_next;
    }
    None
}

unsafe fn find_device_link(
    create_info: *const VkDeviceCreateInfo,
) -> Option<*mut LayerDeviceCreateInfo> {
    // SAFETY: caller validated `create_info`; pNext is traversed using the
    // common Vulkan base-in-structure prefix.
    let mut current = unsafe { (*create_info).p_next.cast::<BaseInStructure>() };
    while !current.is_null() {
        // SAFETY: every pNext node has the base prefix.
        let node = unsafe { &*current };
        if node.s_type == VK_STRUCTURE_TYPE_LOADER_DEVICE_CREATE_INFO {
            let layer = current.cast_mut().cast::<LayerDeviceCreateInfo>();
            // SAFETY: matching sType selects the loader structure layout.
            if unsafe { (*layer).function } == VK_LAYER_LINK_INFO {
                return Some(layer);
            }
        }
        current = node.p_next;
    }
    None
}

fn cast_create_instance(function: PfnVoidFunction) -> Option<PfnCreateInstance> {
    function.map(|value| {
        // SAFETY: the pointer was requested with the exact Vulkan command name.
        unsafe { mem::transmute::<unsafe extern "system" fn(), PfnCreateInstance>(value) }
    })
}

fn cast_destroy_instance(function: PfnVoidFunction) -> Option<PfnDestroyInstance> {
    function.map(|value| unsafe {
        mem::transmute::<unsafe extern "system" fn(), PfnDestroyInstance>(value)
    })
}

fn cast_create_device(function: PfnVoidFunction) -> Option<PfnCreateDevice> {
    function.map(|value| unsafe {
        mem::transmute::<unsafe extern "system" fn(), PfnCreateDevice>(value)
    })
}

fn cast_destroy_device(function: PfnVoidFunction) -> Option<PfnDestroyDevice> {
    function.map(|value| unsafe {
        mem::transmute::<unsafe extern "system" fn(), PfnDestroyDevice>(value)
    })
}

fn cast_queue_present(function: PfnVoidFunction) -> Option<PfnQueuePresentKhr> {
    function.map(|value| unsafe {
        mem::transmute::<unsafe extern "system" fn(), PfnQueuePresentKhr>(value)
    })
}

fn cast_get_queue_family_properties(
    function: PfnVoidFunction,
) -> Option<PfnGetPhysicalDeviceQueueFamilyProperties> {
    function.map(|value| unsafe {
        mem::transmute::<unsafe extern "system" fn(), PfnGetPhysicalDeviceQueueFamilyProperties>(
            value,
        )
    })
}

fn cast_get_memory_properties(
    function: PfnVoidFunction,
) -> Option<PfnGetPhysicalDeviceMemoryProperties> {
    function.map(|value| unsafe {
        mem::transmute::<unsafe extern "system" fn(), PfnGetPhysicalDeviceMemoryProperties>(value)
    })
}

fn cast_get_device_queue(function: PfnVoidFunction) -> Option<PfnGetDeviceQueue> {
    function.map(|value| unsafe {
        mem::transmute::<unsafe extern "system" fn(), PfnGetDeviceQueue>(value)
    })
}

fn cast_get_device_queue_2(function: PfnVoidFunction) -> Option<PfnGetDeviceQueue2> {
    function.map(|value| unsafe {
        mem::transmute::<unsafe extern "system" fn(), PfnGetDeviceQueue2>(value)
    })
}

fn cast_create_swapchain(function: PfnVoidFunction) -> Option<crate::ffi::PfnCreateSwapchainKhr> {
    function.map(|value| unsafe {
        mem::transmute::<unsafe extern "system" fn(), crate::ffi::PfnCreateSwapchainKhr>(value)
    })
}

fn cast_destroy_swapchain(function: PfnVoidFunction) -> Option<crate::ffi::PfnDestroySwapchainKhr> {
    function.map(|value| unsafe {
        mem::transmute::<unsafe extern "system" fn(), crate::ffi::PfnDestroySwapchainKhr>(value)
    })
}

fn to_void_create_instance(function: PfnCreateInstance) -> unsafe extern "system" fn() {
    unsafe { mem::transmute::<PfnCreateInstance, unsafe extern "system" fn()>(function) }
}

fn to_void_destroy_instance(function: PfnDestroyInstance) -> unsafe extern "system" fn() {
    unsafe { mem::transmute::<PfnDestroyInstance, unsafe extern "system" fn()>(function) }
}

fn to_void_create_device(function: PfnCreateDevice) -> unsafe extern "system" fn() {
    unsafe { mem::transmute::<PfnCreateDevice, unsafe extern "system" fn()>(function) }
}

fn to_void_destroy_device(function: PfnDestroyDevice) -> unsafe extern "system" fn() {
    unsafe { mem::transmute::<PfnDestroyDevice, unsafe extern "system" fn()>(function) }
}

fn to_void_get_device_queue(function: PfnGetDeviceQueue) -> unsafe extern "system" fn() {
    unsafe { mem::transmute::<PfnGetDeviceQueue, unsafe extern "system" fn()>(function) }
}

fn to_void_get_device_queue_2(function: PfnGetDeviceQueue2) -> unsafe extern "system" fn() {
    unsafe { mem::transmute::<PfnGetDeviceQueue2, unsafe extern "system" fn()>(function) }
}

fn to_void_create_swapchain(
    function: crate::ffi::PfnCreateSwapchainKhr,
) -> unsafe extern "system" fn() {
    unsafe {
        mem::transmute::<crate::ffi::PfnCreateSwapchainKhr, unsafe extern "system" fn()>(function)
    }
}

fn to_void_destroy_swapchain(
    function: crate::ffi::PfnDestroySwapchainKhr,
) -> unsafe extern "system" fn() {
    unsafe {
        mem::transmute::<crate::ffi::PfnDestroySwapchainKhr, unsafe extern "system" fn()>(function)
    }
}

fn to_void_queue_present(function: PfnQueuePresentKhr) -> unsafe extern "system" fn() {
    unsafe { mem::transmute::<PfnQueuePresentKhr, unsafe extern "system" fn()>(function) }
}

fn to_void_gipa(function: PfnGetInstanceProcAddr) -> unsafe extern "system" fn() {
    unsafe { mem::transmute::<PfnGetInstanceProcAddr, unsafe extern "system" fn()>(function) }
}

fn to_void_gdpa(function: PfnGetDeviceProcAddr) -> unsafe extern "system" fn() {
    unsafe { mem::transmute::<PfnGetDeviceProcAddr, unsafe extern "system" fn()>(function) }
}

type PfnNegotiate = unsafe extern "system" fn(*mut NegotiateLayerInterface) -> VkResult;

fn to_void_negotiate(function: PfnNegotiate) -> unsafe extern "system" fn() {
    unsafe { mem::transmute::<PfnNegotiate, unsafe extern "system" fn()>(function) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation_exposes_loader_interface_version_two_entry_points() {
        let mut interface = NegotiateLayerInterface {
            s_type: NEGOTIATE_LAYER_INTERFACE,
            p_next: std::ptr::null_mut(),
            loader_layer_interface_version: 5,
            get_instance_proc_addr: None,
            get_device_proc_addr: None,
            get_physical_device_proc_addr: None,
        };
        // SAFETY: the test supplies a valid writable negotiation structure.
        let result = unsafe { negotiate_loader_layer_interface_version(&raw mut interface) };
        assert_eq!(result, VK_SUCCESS);
        assert_eq!(interface.loader_layer_interface_version, 2);
        assert!(interface.get_instance_proc_addr.is_some());
        assert!(interface.get_device_proc_addr.is_some());
        assert!(interface.get_physical_device_proc_addr.is_some());
    }

    #[test]
    fn global_intercepts_are_available_without_an_instance() {
        // SAFETY: names are static NUL-terminated Vulkan command strings.
        assert!(
            unsafe {
                get_instance_proc_addr(std::ptr::null_mut(), CREATE_INSTANCE.as_ptr().cast())
            }
            .is_some()
        );
        // SAFETY: names are static NUL-terminated Vulkan command strings.
        assert!(
            unsafe {
                get_instance_proc_addr(std::ptr::null_mut(), GET_INSTANCE_PROC_ADDR.as_ptr().cast())
            }
            .is_some()
        );
    }

    #[test]
    fn failed_presentations_are_not_recordable_frames() {
        assert!(presentation_succeeded(VK_SUCCESS));
        assert!(presentation_succeeded(1_000_001_003));
        assert!(!presentation_succeeded(VK_ERROR_INITIALIZATION_FAILED));
    }
}
