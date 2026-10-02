//! Stable, private device matching across the probe and open instances.
use super::{VkPhysicalDevice, c_void, ptr};

const VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2: i32 = 1_000_059_001;
const VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES: i32 = 1_000_071_004;

#[repr(C)]
pub(super) struct Properties2 {
    s_type: i32,
    p_next: *mut c_void,
    // Aligned storage exceeds VkPhysicalDeviceProperties. Only its official
    // four-u32 prefix is read; driver names and the limits tail stay private.
    properties: [u64; 512],
}

#[repr(C)]
struct IdProperties {
    s_type: i32,
    p_next: *mut c_void,
    device_uuid: [u8; 16],
    driver_uuid: [u8; 16],
    device_luid: [u8; 8],
    device_node_mask: u32,
    device_luid_valid: u32,
}

pub(super) type GetProperties2 = unsafe extern "system" fn(VkPhysicalDevice, *mut Properties2);

#[derive(Clone, Eq, PartialEq)]
pub(super) struct Identity {
    device_uuid: [u8; 16],
    driver_uuid: [u8; 16],
    pub(super) api_version: u32,
    pub(super) driver_version: u32,
    pub(super) vendor_id: u32,
    device_id: u32,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Identity(<private>)")
    }
}

impl Identity {
    pub(super) fn is_stable(&self) -> bool {
        self.device_uuid != [0; 16] && self.driver_uuid != [0; 16]
    }
}

pub(super) unsafe fn query(device: VkPhysicalDevice, get: GetProperties2) -> Identity {
    let mut ids = IdProperties {
        s_type: VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES,
        p_next: ptr::null_mut(),
        device_uuid: [0; 16],
        driver_uuid: [0; 16],
        device_luid: [0; 8],
        device_node_mask: 0,
        device_luid_valid: 0,
    };
    let mut properties = Properties2 {
        s_type: VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2,
        p_next: (&raw mut ids).cast(),
        properties: [0; 512],
    };
    // SAFETY: the live physical device and correctly aligned output chain are
    // valid for the official vkGetPhysicalDeviceProperties2 ABI.
    unsafe { get(device, &raw mut properties) };
    let prefix = properties.properties.as_ptr().cast::<u32>();
    Identity {
        device_uuid: ids.device_uuid,
        driver_uuid: ids.driver_uuid,
        // SAFETY: these four u32 fields begin VkPhysicalDeviceProperties.
        api_version: unsafe { *prefix },
        driver_version: unsafe { *prefix.add(1) },
        vendor_id: unsafe { *prefix.add(2) },
        device_id: unsafe { *prefix.add(3) },
    }
}

pub(super) unsafe fn select(
    devices: &[VkPhysicalDevice],
    expected: &Identity,
    get: GetProperties2,
) -> Option<VkPhysicalDevice> {
    if !expected.is_stable() {
        return None;
    }
    let mut selected = None;
    for device in devices.iter().copied().filter(|device| !device.is_null()) {
        // Changed driver/API capabilities must be reprobed; duplicate
        // identities must never select a device arbitrarily.
        if unsafe { query(device, get) } == *expected {
            if selected.is_some() {
                return None;
            }
            selected = Some(device);
        }
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "system" fn fake_properties(device: VkPhysicalDevice, out: *mut Properties2) {
        let out = unsafe { &mut *out };
        assert_eq!(out.s_type, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2);
        let id = unsafe { &mut *out.p_next.cast::<IdProperties>() };
        assert_eq!(id.s_type, VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES);
        id.device_uuid = [u8::try_from(device.addr()).unwrap(); 16];
        id.driver_uuid = [9; 16];
        let prefix = out.properties.as_mut_ptr().cast::<u32>();
        unsafe {
            ptr::copy_nonoverlapping([0x0040_3000, 0xdead_beef, 0x10de, 7].as_ptr(), prefix, 4);
        };
    }

    #[test]
    fn identity_survives_enumeration_reordering_and_rejects_missing_or_duplicate() {
        let first = 1_usize as VkPhysicalDevice;
        let selected = 2_usize as VkPhysicalDevice;
        let expected = unsafe { query(selected, fake_properties) };
        assert_eq!(
            unsafe { select(&[selected, first], &expected, fake_properties) },
            Some(selected)
        );
        assert_eq!(
            unsafe { select(&[first], &expected, fake_properties) },
            None
        );
        assert_eq!(
            unsafe { select(&[selected, selected], &expected, fake_properties) },
            None
        );
    }

    #[test]
    fn versions_are_numeric_and_identity_debug_is_private() {
        let mut expected = unsafe { query(2_usize as VkPhysicalDevice, fake_properties) };
        assert_eq!(expected.driver_version, 0xdead_beef);
        assert_eq!(expected.vendor_id, 0x10de);
        assert_eq!(format!("{expected:?}"), "Identity(<private>)");
        expected.driver_version ^= 1;
        assert_eq!(
            unsafe { select(&[2_usize as VkPhysicalDevice], &expected, fake_properties) },
            None
        );
        expected.device_uuid = [0; 16];
        assert!(!expected.is_stable());
    }
}
