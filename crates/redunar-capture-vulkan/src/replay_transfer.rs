use crate::ffi::{
    VK_FORMAT_A2B10G10R10_UNORM_PACK32, VK_FORMAT_A2R10G10B10_UNORM_PACK32,
    VK_FORMAT_B8G8R8A8_SRGB, VK_FORMAT_B8G8R8A8_UNORM, VK_FORMAT_R8G8B8A8_SRGB,
    VK_FORMAT_R8G8B8A8_UNORM, VK_IMAGE_USAGE_TRANSFER_SRC_BIT,
    VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR, VK_SWAPCHAIN_CREATE_PROTECTED_BIT_KHR,
    VkSwapchainCreateInfoKhr,
};
use redunar_capture::{
    MAX_REPLAY_SOURCE_HEIGHT, MAX_REPLAY_SOURCE_WIDTH, ReplayPixelFormat, ReplaySourceCandidate,
    ReplaySourceRejection,
};
use std::env;

const REPLAY_TRANSFER_ENV: &str = "REDUNAR_REPLAY_TRANSFER";
const REPLAY_FRAME_RATE_ENV: &str = "REDUNAR_REPLAY_FRAME_RATE";
const DIAGNOSTIC_ADD_TRANSFER_SOURCE_ENV: &str = "REDUNAR_REPLAY_DIAGNOSTIC_ADD_TRANSFER_SRC";
const PRODUCTION_ENV: &str = "REDUNAR_REPLAY_PRODUCTION";
const MIN_SOURCE_WIDTH: u32 = 320;
const MIN_SOURCE_HEIGHT: u32 = 180;
// Four encoder slots plus one producer handoff are required for forward
// progress; the fifth export immediately releases the oldest completed slot.
const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;
// Match the encoder request's H.264 macroblock throughput bound. Variable
// mode announces nominal 120 while timestamps follow accepted presents.
const MAX_ENCODE_MACROBLOCKS_PER_SECOND: u32 = 2_073_600;
// Keep the four-slot encoder fixed while giving the variable-rate
// Vulkan producer three additional handoff buffers. Fixed-rate paths retain
// their existing five-buffer bound.
const VFR_PRODUCER_CONTEXT_COUNT: usize = 8;

pub(crate) const fn producer_context_count(variable_rate: bool) -> usize {
    if variable_rate {
        VFR_PRODUCER_CONTEXT_COUNT
    } else {
        redunar_capture::REPLAY_PRODUCER_CONTEXT_COUNT
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReplayTransferConfig {
    requested: bool,
    target_frames_per_second: u8,
    variable_rate: bool,
}

impl ReplayTransferConfig {
    fn from_environment() -> Self {
        Self::from_values(
            env::var(REPLAY_TRANSFER_ENV).ok().as_deref(),
            env::var(REPLAY_FRAME_RATE_ENV).ok().as_deref(),
        )
    }

    fn from_values(requested: Option<&str>, frame_rate: Option<&str>) -> Self {
        Self {
            requested: requested == Some("1"),
            target_frames_per_second: match frame_rate {
                Some("30") => 30,
                Some("120" | "variable") => 120,
                _ => 60,
            },
            variable_rate: frame_rate == Some("variable"),
        }
    }
}

pub(crate) fn variable_rate_requested() -> bool {
    ReplayTransferConfig::from_environment().variable_rate
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReplayTransferRejection {
    InvalidStructure,
    ProtectedSwapchain,
    ArrayLayersUnsupported,
    TransferSourceUsageMissing,
    DimensionsUnsupported,
    PixelFormatUnsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReplayTransferPlan {
    pub(crate) candidate: ReplaySourceCandidate,
    pub(crate) frame_interval_ns: u64,
    pub(crate) maximum_in_flight_images: u8,
}

/// Inspect only immutable swapchain creation metadata. No Vulkan resource is
/// created and no application field is modified.
pub(crate) unsafe fn assessment_from_create_info(
    create_info: *const VkSwapchainCreateInfoKhr,
) -> Option<Result<ReplaySourceCandidate, ReplaySourceRejection>> {
    let config = ReplayTransferConfig::from_environment();
    if !config.requested || create_info.is_null() {
        return None;
    }
    // SAFETY: the successful swapchain wrapper calls this while the original
    // application create-info remains valid.
    let info = unsafe { &*create_info };
    Some(
        plan(config, info)
            .map(|value| value.candidate)
            .map_err(ReplaySourceRejection::from),
    )
}

/// Add transfer-source usage for an explicit diagnostic or production Replay
/// request. The Vulkan hook retries the application's exact original request
/// when the driver rejects this optional usage, keeping the game fail-open.
pub(crate) unsafe fn transfer_source_create_info(
    create_info: *const VkSwapchainCreateInfoKhr,
) -> Option<VkSwapchainCreateInfoKhr> {
    if !ReplayTransferConfig::from_environment().requested
        || (env::var(DIAGNOSTIC_ADD_TRANSFER_SOURCE_ENV).ok().as_deref() != Some("1")
            && env::var(PRODUCTION_ENV).ok().as_deref() != Some("1"))
        || create_info.is_null()
    {
        return None;
    }
    // SAFETY: the swapchain wrapper calls this only while the application's
    // create info is valid.
    let mut augmented = unsafe { *create_info };
    if augmented.s_type != VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR
        || augmented.flags & VK_SWAPCHAIN_CREATE_PROTECTED_BIT_KHR != 0
        || augmented.image_array_layers != 1
    {
        return None;
    }
    augmented.image_usage |= VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
    Some(augmented)
}

impl From<ReplayTransferRejection> for ReplaySourceRejection {
    fn from(value: ReplayTransferRejection) -> Self {
        match value {
            ReplayTransferRejection::InvalidStructure => Self::InvalidStructure,
            ReplayTransferRejection::ProtectedSwapchain => Self::ProtectedSwapchain,
            ReplayTransferRejection::ArrayLayersUnsupported => Self::ArrayLayersUnsupported,
            ReplayTransferRejection::TransferSourceUsageMissing => Self::TransferSourceUsageMissing,
            ReplayTransferRejection::DimensionsUnsupported => Self::DimensionsUnsupported,
            ReplayTransferRejection::PixelFormatUnsupported => Self::PixelFormatUnsupported,
        }
    }
}

fn plan(
    config: ReplayTransferConfig,
    info: &VkSwapchainCreateInfoKhr,
) -> Result<ReplayTransferPlan, ReplayTransferRejection> {
    if info.s_type != VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR {
        return Err(ReplayTransferRejection::InvalidStructure);
    }
    if info.flags & VK_SWAPCHAIN_CREATE_PROTECTED_BIT_KHR != 0 {
        return Err(ReplayTransferRejection::ProtectedSwapchain);
    }
    if info.image_array_layers != 1 {
        return Err(ReplayTransferRejection::ArrayLayersUnsupported);
    }
    if info.image_usage & VK_IMAGE_USAGE_TRANSFER_SRC_BIT == 0 {
        return Err(ReplayTransferRejection::TransferSourceUsageMissing);
    }
    if info.image_extent.width < MIN_SOURCE_WIDTH
        || info.image_extent.height < MIN_SOURCE_HEIGHT
        || info.image_extent.width > MAX_REPLAY_SOURCE_WIDTH
        || info.image_extent.height > MAX_REPLAY_SOURCE_HEIGHT
    {
        return Err(ReplayTransferRejection::DimensionsUnsupported);
    }
    let pixel_format = match info.image_format {
        VK_FORMAT_R8G8B8A8_UNORM => ReplayPixelFormat::Rgba8Unorm,
        VK_FORMAT_R8G8B8A8_SRGB => ReplayPixelFormat::Rgba8Srgb,
        VK_FORMAT_B8G8R8A8_UNORM => ReplayPixelFormat::Bgra8Unorm,
        VK_FORMAT_B8G8R8A8_SRGB => ReplayPixelFormat::Bgra8Srgb,
        VK_FORMAT_A2B10G10R10_UNORM_PACK32 => ReplayPixelFormat::A2b10g10r10Unorm,
        VK_FORMAT_A2R10G10B10_UNORM_PACK32 => ReplayPixelFormat::A2r10g10b10Unorm,
        _ => {
            if env::var("REDUNAR_REPLAY_DEBUG_FORMAT").ok().as_deref() == Some("1") {
                eprintln!(
                    "redunar replay: unsupported Vulkan swapchain format {}",
                    info.image_format
                );
            }
            return Err(ReplayTransferRejection::PixelFormatUnsupported);
        }
    };
    if config.target_frames_per_second == 120
        && !config.variable_rate
        && !((info.image_extent.width <= 1_920 && info.image_extent.height <= 1_080)
            || (info.image_extent.height <= 1_920 && info.image_extent.width <= 1_080))
    {
        return Err(ReplayTransferRejection::DimensionsUnsupported);
    }
    if info
        .image_extent
        .width
        .div_ceil(16)
        .saturating_mul(info.image_extent.height.div_ceil(16))
        .saturating_mul(u32::from(config.target_frames_per_second))
        > MAX_ENCODE_MACROBLOCKS_PER_SECOND
    {
        return Err(ReplayTransferRejection::DimensionsUnsupported);
    }
    Ok(ReplayTransferPlan {
        candidate: ReplaySourceCandidate {
            width: info.image_extent.width,
            height: info.image_extent.height,
            pixel_format,
            target_frames_per_second: config.target_frames_per_second,
        },
        frame_interval_ns: NANOSECONDS_PER_SECOND / u64::from(config.target_frames_per_second),
        maximum_in_flight_images: u8::try_from(producer_context_count(config.variable_rate))
            .unwrap_or(u8::MAX),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::{VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR, VkExtent2d};

    fn create_info() -> VkSwapchainCreateInfoKhr {
        VkSwapchainCreateInfoKhr {
            s_type: VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR,
            p_next: std::ptr::null(),
            flags: 0,
            surface: 1,
            min_image_count: 3,
            image_format: VK_FORMAT_B8G8R8A8_UNORM,
            image_color_space: 0,
            image_extent: VkExtent2d {
                width: 2_560,
                height: 1_440,
            },
            image_array_layers: 1,
            image_usage: VK_IMAGE_USAGE_TRANSFER_SRC_BIT,
            image_sharing_mode: 0,
            queue_family_index_count: 0,
            queue_family_indices: std::ptr::null(),
            pre_transform: 0,
            composite_alpha: 0,
            present_mode: 0,
            clipped: 1,
            old_swapchain: 0,
        }
    }

    #[test]
    fn environment_configuration_is_closed_and_defaults_disabled() {
        assert_eq!(
            ReplayTransferConfig::from_values(None, None),
            ReplayTransferConfig {
                requested: false,
                target_frames_per_second: 60,
                variable_rate: false,
            }
        );
        assert_eq!(
            ReplayTransferConfig::from_values(Some("1"), Some("30")),
            ReplayTransferConfig {
                requested: true,
                target_frames_per_second: 30,
                variable_rate: false,
            }
        );
        assert_eq!(
            ReplayTransferConfig::from_values(Some("1"), Some("120")),
            ReplayTransferConfig {
                requested: true,
                target_frames_per_second: 120,
                variable_rate: false,
            }
        );
        assert_eq!(
            ReplayTransferConfig::from_values(Some("yes"), Some("999")),
            ReplayTransferConfig {
                requested: false,
                target_frames_per_second: 60,
                variable_rate: false,
            }
        );
    }

    #[test]
    fn eligible_plan_is_metadata_only_and_has_fixed_limits() {
        let transfer_plan = plan(
            ReplayTransferConfig {
                requested: true,
                target_frames_per_second: 30,
                variable_rate: false,
            },
            &create_info(),
        )
        .expect("eligible plan");
        assert_eq!(transfer_plan.candidate.width, 2_560);
        assert_eq!(transfer_plan.candidate.height, 1_440);
        assert_eq!(
            transfer_plan.candidate.pixel_format,
            ReplayPixelFormat::Bgra8Unorm
        );
        assert_eq!(transfer_plan.candidate.target_frames_per_second, 30);
        assert_eq!(transfer_plan.frame_interval_ns, NANOSECONDS_PER_SECOND / 30);
        assert_eq!(transfer_plan.maximum_in_flight_images, 5);
        assert!(std::mem::size_of::<ReplayTransferPlan>() <= 32);
    }

    #[test]
    fn fixed_120_fps_is_accepted_only_at_1080p_or_lower() {
        let config = ReplayTransferConfig {
            requested: true,
            target_frames_per_second: 120,
            variable_rate: false,
        };
        let mut info = create_info();
        assert_eq!(
            plan(config, &info),
            Err(ReplayTransferRejection::DimensionsUnsupported)
        );

        info.image_extent = VkExtent2d {
            width: 2_560,
            height: 720,
        };
        assert_eq!(
            plan(config, &info),
            Err(ReplayTransferRejection::DimensionsUnsupported)
        );

        info.image_extent = VkExtent2d {
            width: 1_920,
            height: 1_080,
        };
        assert_eq!(
            plan(config, &info)
                .expect("1080p 120 FPS plan")
                .candidate
                .target_frames_per_second,
            120
        );
    }

    #[test]
    fn hdr_wayland_packed_format_is_carried_as_four_byte_source() {
        let mut info = create_info();
        info.image_format = VK_FORMAT_A2B10G10R10_UNORM_PACK32;
        let blue_high_plan = plan(
            ReplayTransferConfig {
                requested: true,
                target_frames_per_second: 60,
                variable_rate: false,
            },
            &info,
        )
        .expect("eligible packed 10-bit plan");
        assert_eq!(
            blue_high_plan.candidate.pixel_format,
            ReplayPixelFormat::A2b10g10r10Unorm
        );

        info.image_format = VK_FORMAT_A2R10G10B10_UNORM_PACK32;
        let red_high_plan = plan(
            ReplayTransferConfig {
                requested: true,
                target_frames_per_second: 60,
                variable_rate: false,
            },
            &info,
        )
        .expect("eligible packed 10-bit Wayland plan");
        assert_eq!(
            red_high_plan.candidate.pixel_format,
            ReplayPixelFormat::A2r10g10b10Unorm
        );
    }

    #[test]
    fn every_unsafe_swapchain_shape_is_rejected_without_fallback() {
        let config = ReplayTransferConfig {
            requested: true,
            target_frames_per_second: 60,
            variable_rate: false,
        };
        let mut info = create_info();
        info.flags = VK_SWAPCHAIN_CREATE_PROTECTED_BIT_KHR;
        assert_eq!(
            plan(config, &info),
            Err(ReplayTransferRejection::ProtectedSwapchain)
        );
        info = create_info();
        info.image_array_layers = 2;
        assert_eq!(
            plan(config, &info),
            Err(ReplayTransferRejection::ArrayLayersUnsupported)
        );
        info = create_info();
        info.image_usage = 0;
        assert_eq!(
            plan(config, &info),
            Err(ReplayTransferRejection::TransferSourceUsageMissing)
        );
        info = create_info();
        info.image_extent.width = MAX_REPLAY_SOURCE_WIDTH + 1;
        assert_eq!(
            plan(config, &info),
            Err(ReplayTransferRejection::DimensionsUnsupported)
        );
        info = create_info();
        info.image_format = 999_999;
        assert_eq!(
            plan(config, &info),
            Err(ReplayTransferRejection::PixelFormatUnsupported)
        );
    }
}
