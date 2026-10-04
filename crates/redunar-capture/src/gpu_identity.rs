//! Private graphics-device identity for matching capture and encode devices.
//! UUID bytes stay on the bounded same-user transport and never enter logs.

use crate::ProtocolError;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct CaptureGpuIdentity {
    vendor_id: u32,
    device_uuid: [u8; 16],
    driver_uuid: [u8; 16],
}

impl std::fmt::Debug for CaptureGpuIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CaptureGpuIdentity(<private>)")
    }
}

impl CaptureGpuIdentity {
    /// # Errors
    /// Rejects missing UUIDs or a non-PCI vendor identity.
    pub fn new(
        vendor_id: u32,
        device_uuid: [u8; 16],
        driver_uuid: [u8; 16],
    ) -> Result<Self, ProtocolError> {
        if vendor_id == 0
            || vendor_id > u32::from(u16::MAX)
            || device_uuid == [0; 16]
            || driver_uuid == [0; 16]
        {
            return Err(ProtocolError::new("capture GPU identity is unavailable"));
        }
        Ok(Self {
            vendor_id,
            device_uuid,
            driver_uuid,
        })
    }

    #[must_use]
    pub const fn vendor_id(self) -> u32 {
        self.vendor_id
    }
    #[must_use]
    pub const fn device_uuid(self) -> [u8; 16] {
        self.device_uuid
    }
    #[must_use]
    pub const fn driver_uuid(self) -> [u8; 16] {
        self.driver_uuid
    }
}

pub(crate) const WIRE_BYTES: usize = 40;

pub(crate) fn encode(identity: Option<CaptureGpuIdentity>, output: &mut [u8]) {
    output[..WIRE_BYTES].fill(0);
    if let Some(identity) = identity {
        output[..4].copy_from_slice(&identity.vendor_id.to_le_bytes());
        output[4..20].copy_from_slice(&identity.device_uuid);
        output[20..36].copy_from_slice(&identity.driver_uuid);
    }
}

pub(crate) fn decode(input: &[u8]) -> Result<Option<CaptureGpuIdentity>, ProtocolError> {
    if input[..WIRE_BYTES].iter().all(|byte| *byte == 0) {
        return Ok(None);
    }
    if input[36..WIRE_BYTES].iter().any(|byte| *byte != 0) {
        return Err(ProtocolError::new(
            "capture GPU reserved fields are non-zero",
        ));
    }
    let vendor_id = u32::from_le_bytes(input[..4].try_into().expect("fixed GPU wire vendor"));
    let device = input[4..20].try_into().expect("fixed GPU wire UUID");
    let driver = input[20..36].try_into().expect("fixed GPU driver UUID");
    CaptureGpuIdentity::new(vendor_id, device, driver).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_and_noncanonical_identity_are_rejected_and_debug_is_private() {
        assert!(CaptureGpuIdentity::new(0x10de, [0; 16], [2; 16]).is_err());
        assert!(CaptureGpuIdentity::new(0x10de, [1; 16], [0; 16]).is_err());
        assert!(CaptureGpuIdentity::new(0x10000, [1; 16], [2; 16]).is_err());
        let identity = CaptureGpuIdentity::new(0x10de, [1; 16], [2; 16]).unwrap();
        assert_eq!(format!("{identity:?}"), "CaptureGpuIdentity(<private>)");
        let mut bytes = [0; WIRE_BYTES];
        encode(Some(identity), &mut bytes);
        assert_eq!(decode(&bytes).unwrap(), Some(identity));
        bytes[39] = 1;
        assert!(decode(&bytes).is_err());
    }
}
