use crate::{KmsMode, KmsOutput};
use redunar_core::ReplayFrameRate;
use std::error::Error;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KmsCapturePlan {
    pub card_index: u8,
    pub connector: String,
    pub source_mode: KmsMode,
    pub frame_rate: ReplayFrameRate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KmsPlanError {
    NoReportedMode,
    DimensionsUnsupported,
}

impl fmt::Display for KmsPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NoReportedMode => "the connected output has no reported capture mode",
            Self::DimensionsUnsupported => {
                "the selected frame rate does not support the output dimensions"
            }
        })
    }
}

impl Error for KmsPlanError {}

impl KmsCapturePlan {
    /// Build a read-only diagnostic plan from the connector's preferred mode.
    /// The actual framebuffer dimensions must be revalidated by the future KMS
    /// capture boundary before any DMA-BUF is accepted.
    ///
    /// # Errors
    ///
    /// Returns an error when no mode exists or the requested rate is outside
    /// Redunar's resolution policy.
    pub fn new(output: &KmsOutput, frame_rate: ReplayFrameRate) -> Result<Self, KmsPlanError> {
        let source_mode = *output.modes.first().ok_or(KmsPlanError::NoReportedMode)?;
        if !frame_rate.supports_dimensions(source_mode.width, source_mode.height) {
            return Err(KmsPlanError::DimensionsUnsupported);
        }
        Ok(Self {
            card_index: output.card_index,
            connector: output.connector.clone(),
            source_mode,
            frame_rate,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn output(width: u32, height: u32) -> KmsOutput {
        KmsOutput {
            card_index: 1,
            connector: "DP-1".to_owned(),
            card_path: PathBuf::from("/dev/dri/card1"),
            sysfs_path: PathBuf::from("/sys/class/drm/card1-DP-1"),
            modes: vec![KmsMode { width, height }],
        }
    }

    #[test]
    fn plan_applies_the_existing_120_fps_resolution_policy() {
        assert!(KmsCapturePlan::new(&output(1_920, 1_080), ReplayFrameRate::Fps120).is_ok());
        assert_eq!(
            KmsCapturePlan::new(&output(2_560, 1_440), ReplayFrameRate::Fps120),
            Err(KmsPlanError::DimensionsUnsupported)
        );
        assert!(KmsCapturePlan::new(&output(3_840, 2_160), ReplayFrameRate::Fps60).is_ok());
    }
}
