//! Complete, bounded device-extension discovery shared by export and encoding.

use crate::ffi::{VK_SUCCESS, VkPhysicalDevice, VkResult};
use std::ffi::c_char;
use std::ptr;

const VK_INCOMPLETE: VkResult = 5;
// Some drivers expose more than 256 extensions. Keep the producer's existing
// bound, but never truncate discovery and mistake a partial list for support.
const MAX_DEVICE_EXTENSIONS: usize = 1_024;
const MAX_ENUMERATION_ATTEMPTS: usize = 4;

#[repr(C)]
pub(crate) struct VkExtensionProperties {
    pub(crate) extension_name: [c_char; 256],
    pub(crate) spec_version: u32,
}

impl Default for VkExtensionProperties {
    fn default() -> Self {
        Self {
            extension_name: [0; 256],
            spec_version: 0,
        }
    }
}

impl VkExtensionProperties {
    pub(crate) fn name_bytes(&self) -> &[u8] {
        // SAFETY: c_char and u8 have identical layout; the view is bounded by
        // the fixed array even if a faulty driver omits the terminating zero.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                self.extension_name.as_ptr().cast(),
                self.extension_name.len(),
            )
        };
        let end = bytes
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(bytes.len());
        &bytes[..end]
    }
}

pub(crate) type EnumerateDeviceExtensionProperties = unsafe extern "system" fn(
    VkPhysicalDevice,
    *const c_char,
    *mut u32,
    *mut VkExtensionProperties,
) -> VkResult;

/// The device and dispatch pointer must belong to the same live Vulkan instance.
pub(crate) unsafe fn enumerate(
    device: VkPhysicalDevice,
    enumerate: EnumerateDeviceExtensionProperties,
) -> Result<Vec<VkExtensionProperties>, VkResult> {
    for _ in 0..MAX_ENUMERATION_ATTEMPTS {
        let mut count = 0_u32;
        // SAFETY: writable count storage; null layer selects device extensions.
        let result = unsafe { enumerate(device, ptr::null(), &raw mut count, ptr::null_mut()) };
        if result != VK_SUCCESS {
            return Err(result);
        }
        let capacity = usize::try_from(count).map_err(|_| VK_INCOMPLETE)?;
        if capacity > MAX_DEVICE_EXTENSIONS {
            return Err(VK_INCOMPLETE);
        }
        if capacity == 0 {
            return Ok(Vec::new());
        }
        let mut properties: Vec<_> = std::iter::repeat_with(VkExtensionProperties::default)
            .take(capacity)
            .collect();
        let mut written = count;
        // SAFETY: initialized output storage holds exactly `count` records.
        let result = unsafe {
            enumerate(
                device,
                ptr::null(),
                &raw mut written,
                properties.as_mut_ptr(),
            )
        };
        if result == VK_INCOMPLETE {
            // The list can grow between calls. Re-query the count rather than
            // accepting truncated capabilities or retrying with the same cap.
            continue;
        }
        if result != VK_SUCCESS {
            return Err(result);
        }
        let written = usize::try_from(written).map_err(|_| VK_INCOMPLETE)?;
        if written > capacity {
            return Err(VK_INCOMPLETE);
        }
        properties.truncate(written);
        return Ok(properties);
    }
    Err(VK_INCOMPLETE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    struct Step {
        capacity: Option<u32>,
        count: u32,
        result: VkResult,
    }

    thread_local! {
        static STEPS: RefCell<VecDeque<Step>> = const { RefCell::new(VecDeque::new()) };
    }

    unsafe extern "system" fn fake_driver(
        device: VkPhysicalDevice,
        layer: *const c_char,
        count: *mut u32,
        properties: *mut VkExtensionProperties,
    ) -> VkResult {
        assert!(device.is_null() && layer.is_null());
        STEPS.with(|steps| {
            let step = steps
                .borrow_mut()
                .pop_front()
                .expect("unexpected driver call");
            // SAFETY: tests enter through the production helper's valid buffers.
            unsafe {
                assert_eq!((!properties.is_null()).then(|| *count), step.capacity);
                if let Some(capacity) = step.capacity {
                    for index in 0..capacity.min(step.count) {
                        let name = format!("VK_TEST_extension_{index}");
                        let property = &mut *properties.add(index as usize);
                        for (destination, byte) in
                            property.extension_name.iter_mut().zip(name.bytes())
                        {
                            *destination = byte.cast_signed();
                        }
                    }
                }
                *count = step.count;
            }
            step.result
        })
    }

    fn run(steps: Vec<Step>) -> Result<Vec<VkExtensionProperties>, VkResult> {
        STEPS.with(|state| *state.borrow_mut() = steps.into());
        // SAFETY: the fake driver accepts the null fixture device, no GPU access.
        let result = unsafe { enumerate(ptr::null_mut(), fake_driver) };
        STEPS.with(|state| assert!(state.borrow().is_empty()));
        result
    }

    fn query(count: u32) -> Step {
        Step {
            capacity: None,
            count,
            result: VK_SUCCESS,
        }
    }

    fn fill(capacity: u32, count: u32, result: VkResult) -> Step {
        Step {
            capacity: Some(capacity),
            count,
            result,
        }
    }

    #[test]
    fn supports_complete_lists_above_old_encoder_limit() {
        let list = run(vec![query(300), fill(300, 300, VK_SUCCESS)]).unwrap();
        assert_eq!(list.len(), 300);
        assert_eq!(list[299].name_bytes(), b"VK_TEST_extension_299");
        assert_eq!(std::mem::size_of::<VkExtensionProperties>(), 260);
    }

    #[test]
    fn retries_growth_and_discards_partial_lists() {
        let list = run(vec![
            query(2),
            fill(2, 2, VK_INCOMPLETE),
            query(4),
            fill(4, 4, VK_SUCCESS),
        ])
        .unwrap();
        assert_eq!(list.len(), 4);
        assert_eq!(list[3].name_bytes(), b"VK_TEST_extension_3");
    }

    #[test]
    fn respects_shrinkage_and_empty_lists() {
        assert_eq!(
            run(vec![query(4), fill(4, 2, VK_SUCCESS)]).unwrap().len(),
            2
        );
        assert!(run(vec![query(0)]).unwrap().is_empty());
    }

    #[test]
    fn unstable_and_oversized_lists_fail_with_bounded_work() {
        let steps = (0..MAX_ENUMERATION_ATTEMPTS)
            .flat_map(|_| [query(2), fill(2, 2, VK_INCOMPLETE)])
            .collect();
        assert_eq!(run(steps).err(), Some(VK_INCOMPLETE));
        assert_eq!(run(vec![query(u32::MAX)]).err(), Some(VK_INCOMPLETE));
        assert_eq!(
            run(vec![query(2), fill(2, 3, VK_SUCCESS)]).err(),
            Some(VK_INCOMPLETE)
        );
    }

    #[test]
    fn preserves_driver_errors_without_retrying() {
        assert_eq!(
            run(vec![Step {
                result: -4,
                ..query(0)
            }])
            .err(),
            Some(-4)
        );
        assert_eq!(run(vec![query(2), fill(2, 0, -3)]).err(), Some(-3));
    }
}
