//! Safe, daemon-facing capability boundary for Redunar's Vulkan Video path.
//!
//! The daemon owns replay lifecycle and policy. This module owns only Vulkan's
//! native ABI and returns bounded, copyable capability facts. Discovery never
//! enables Replay by itself; DMA-BUF import, encoded output, and sustained
//! pacing still require separate live verification.

#![allow(clippy::too_many_lines, clippy::used_underscore_binding)]

mod conversion;

pub use conversion::VulkanVideoH264Encoder;

use crate::ffi::{
    PfnVoidFunction, VK_SUCCESS, VkAllocationCallbacks, VkBuffer, VkBufferCreateInfo, VkDevice,
    VkDeviceCreateInfo, VkDeviceMemory, VkDeviceQueueCreateInfo, VkExtent2d, VkExtent3d,
    VkExternalMemoryBufferCreateInfo, VkInstance, VkInstanceCreateInfo, VkMemoryAllocateInfo,
    VkMemoryRequirements, VkPhysicalDevice, VkPhysicalDeviceMemoryProperties,
    VkQueueFamilyProperties, VkResult,
};
use std::ffi::{CStr, c_char, c_void};
use std::fs::File;
use std::io::{Seek, SeekFrom};
use std::mem;
use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd};
use std::ptr;

const AMD_VENDOR_ID: u32 = 0x1002;
const NVIDIA_VENDOR_ID: u32 = 0x10de;
const DRM_FORMAT_XRGB8888: u32 = u32::from_le_bytes(*b"XR24");
const DRM_FORMAT_ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
const DRM_FORMAT_XBGR8888: u32 = u32::from_le_bytes(*b"XB24");
const DRM_FORMAT_ABGR8888: u32 = u32::from_le_bytes(*b"AB24");
const MAX_PHYSICAL_DEVICES: usize = 16;
const MAX_DEVICE_EXTENSIONS: usize = 256;
const MAX_QUEUE_FAMILIES: usize = 64;
const VK_STRUCTURE_TYPE_APPLICATION_INFO: i32 = 0;
const VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO: i32 = 1;
const VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO: i32 = 2;
const VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO: i32 = 3;
const VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO: i32 = 5;
const VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO: i32 = 12;
const VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2: i32 = 1_000_059_000;
const VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_EXTERNAL_BUFFER_INFO: i32 = 1_000_071_002;
const VK_STRUCTURE_TYPE_EXTERNAL_BUFFER_PROPERTIES: i32 = 1_000_071_003;
const VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_BUFFER_CREATE_INFO: i32 = 1_000_072_000;
const VK_STRUCTURE_TYPE_IMPORT_MEMORY_FD_INFO_KHR: i32 = 1_000_074_000;
const VK_STRUCTURE_TYPE_MEMORY_FD_PROPERTIES_KHR: i32 = 1_000_074_001;
const VK_STRUCTURE_TYPE_MEMORY_DEDICATED_REQUIREMENTS: i32 = 1_000_127_000;
const VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO: i32 = 1_000_127_001;
const VK_STRUCTURE_TYPE_BUFFER_MEMORY_REQUIREMENTS_INFO_2: i32 = 1_000_146_000;
const VK_STRUCTURE_TYPE_MEMORY_REQUIREMENTS_2: i32 = 1_000_146_003;
const VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SYNCHRONIZATION_2_FEATURES: i32 = 1_000_314_007;
const VK_STRUCTURE_TYPE_QUEUE_FAMILY_PROPERTIES_2: i32 = 1_000_059_005;
const VK_STRUCTURE_TYPE_VIDEO_PROFILE_INFO_KHR: i32 = 1_000_023_000;
const VK_STRUCTURE_TYPE_VIDEO_CAPABILITIES_KHR: i32 = 1_000_023_001;
const VK_STRUCTURE_TYPE_VIDEO_SESSION_MEMORY_REQUIREMENTS_KHR: i32 = 1_000_023_003;
const VK_STRUCTURE_TYPE_BIND_VIDEO_SESSION_MEMORY_INFO_KHR: i32 = 1_000_023_004;
const VK_STRUCTURE_TYPE_VIDEO_SESSION_CREATE_INFO_KHR: i32 = 1_000_023_005;
const VK_STRUCTURE_TYPE_VIDEO_SESSION_PARAMETERS_CREATE_INFO_KHR: i32 = 1_000_023_006;
const VK_STRUCTURE_TYPE_QUEUE_FAMILY_VIDEO_PROPERTIES_KHR: i32 = 1_000_023_012;
const VK_STRUCTURE_TYPE_VIDEO_PROFILE_LIST_INFO_KHR: i32 = 1_000_023_013;
const VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VIDEO_FORMAT_INFO_KHR: i32 = 1_000_023_014;
const VK_STRUCTURE_TYPE_VIDEO_FORMAT_PROPERTIES_KHR: i32 = 1_000_023_015;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_CAPABILITIES_KHR: i32 = 1_000_038_000;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_SESSION_PARAMETERS_CREATE_INFO_KHR: i32 = 1_000_038_001;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_SESSION_PARAMETERS_ADD_INFO_KHR: i32 = 1_000_038_002;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_PROFILE_INFO_KHR: i32 = 1_000_038_007;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_SESSION_CREATE_INFO_KHR: i32 = 1_000_038_010;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_SESSION_PARAMETERS_GET_INFO_KHR: i32 = 1_000_038_012;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_SESSION_PARAMETERS_FEEDBACK_INFO_KHR: i32 = 1_000_038_013;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_CAPABILITIES_KHR: i32 = 1_000_299_003;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_USAGE_INFO_KHR: i32 = 1_000_299_004;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_SESSION_PARAMETERS_GET_INFO_KHR: i32 = 1_000_299_009;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_SESSION_PARAMETERS_FEEDBACK_INFO_KHR: i32 = 1_000_299_010;
const VK_STRUCTURE_TYPE_VIDEO_ENCODE_QUALITY_LEVEL_INFO_KHR: i32 = 1_000_299_008;
const VK_QUEUE_VIDEO_ENCODE_BIT_KHR: u32 = 0x40;
const VK_QUEUE_COMPUTE_BIT: u32 = 0x2;
const VK_IMAGE_USAGE_VIDEO_ENCODE_SRC_BIT_KHR: u32 = 0x0000_4000;
const VK_IMAGE_USAGE_VIDEO_ENCODE_DPB_BIT_KHR: u32 = 0x0000_8000;
const VK_IMAGE_USAGE_TRANSFER_DST_BIT: u32 = 0x2;
const VK_BUFFER_USAGE_STORAGE_BUFFER_BIT: u32 = 0x20;
const VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT: u32 = 0x200;
const VK_EXTERNAL_MEMORY_FEATURE_DEDICATED_ONLY_BIT: u32 = 0x1;
const VK_EXTERNAL_MEMORY_FEATURE_IMPORTABLE_BIT: u32 = 0x4;
const VK_SHARING_MODE_EXCLUSIVE: i32 = 0;
const VK_FORMAT_G8_B8R8_2PLANE_420_UNORM: i32 = 1_000_156_003;
const VK_VIDEO_CODEC_OPERATION_ENCODE_H264_BIT_KHR: i32 = 0x0001_0000;
const VK_VIDEO_CHROMA_SUBSAMPLING_420_BIT_KHR: u32 = 0x2;
const VK_VIDEO_COMPONENT_BIT_DEPTH_8_BIT_KHR: u32 = 0x1;
const VK_VIDEO_ENCODE_USAGE_RECORDING_BIT_KHR: u32 = 0x4;
const VK_VIDEO_ENCODE_CONTENT_RENDERED_BIT_KHR: u32 = 0x4;
const VK_VIDEO_ENCODE_TUNING_MODE_DEFAULT_KHR: i32 = 0;
const VK_VIDEO_ENCODE_FEEDBACK_BITSTREAM_BYTES_WRITTEN_BIT_KHR: u32 = 0x2;
const VK_VIDEO_CAPABILITY_SEPARATE_REFERENCE_IMAGES_BIT_KHR: u32 = 0x2;
const VK_VIDEO_ENCODE_RATE_CONTROL_MODE_CBR_BIT_KHR: u32 = 0x2;
const STD_VIDEO_H264_PROFILE_IDC_HIGH: i32 = 100;
const VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT: u32 = 0x1;
const VK_PHYSICAL_DEVICE_FEATURE_COUNT: usize = 55;
const MAX_VIDEO_SESSION_MEMORY_BINDS: usize = 16;
const MAX_H264_PARAMETER_BYTES: usize = 64 * 1024;
const VULKAN_API_VERSION_1_3: u32 = (1 << 22) | (3 << 12);

const REQUIRED_DEVICE_EXTENSIONS: [&str; 7] = [
    "VK_KHR_video_queue",
    "VK_KHR_video_encode_queue",
    "VK_KHR_video_encode_h264",
    "VK_KHR_external_memory_fd",
    "VK_EXT_external_memory_dma_buf",
    "VK_EXT_image_drm_format_modifier",
    "VK_EXT_queue_family_foreign",
];
const REQUIRED_DEVICE_EXTENSION_NAMES: [&[u8]; 7] = [
    b"VK_KHR_video_queue\0",
    b"VK_KHR_video_encode_queue\0",
    b"VK_KHR_video_encode_h264\0",
    b"VK_KHR_external_memory_fd\0",
    b"VK_EXT_external_memory_dma_buf\0",
    b"VK_EXT_image_drm_format_modifier\0",
    b"VK_EXT_queue_family_foreign\0",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VulkanVideoH264Request {
    pub width: u32,
    pub height: u32,
    /// Fixed capture cadence, or the upper accepted cadence for VFR.
    pub frames_per_second: u8,
    pub variable_rate: bool,
    pub target_megabits_per_second: u16,
}

/// Bound VFR by the H.264 level 5.2 macroblock rate used by this encoder.
/// The Vulkan and OpenGL producers apply the same ceiling at admission time.
#[must_use]
pub fn maximum_variable_frame_rate(width: u32, height: u32) -> u8 {
    let macroblocks = width.div_ceil(16).saturating_mul(height.div_ceil(16));
    if macroblocks == 0 {
        return 0;
    }
    u8::try_from((2_073_600 / macroblocks).min(240)).unwrap_or(240)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VulkanPackedPixelFormat {
    Rgba8,
    Bgra8,
    A2b10g10r10,
    A2r10g10b10,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VulkanVideoInputMode {
    LinearBuffer,
    DrmImage,
}

pub struct VulkanVideoDmaBufFrame {
    file: File,
    pub width: u32,
    pub height: u32,
    pub format: VulkanPackedPixelFormat,
    pub offset: u32,
    pub stride: u32,
    pub timestamp_ns: u64,
    pub duration_ns: u64,
    allocation_size: u64,
    input_mode: VulkanVideoInputMode,
    drm_modifier: u64,
}

#[derive(Debug)]
pub struct VulkanVideoEncodedAccessUnit {
    pub timestamp_ns: u64,
    pub duration_ns: u64,
    pub requested_keyframe: bool,
    pub annex_b: Box<[u8]>,
}

impl VulkanVideoDmaBufFrame {
    /// Validate and take ownership of one buffer DMA-BUF exported by the
    /// capture layer. Image modifiers are intentionally absent: this is a
    /// Vulkan buffer allocation and must be imported as a buffer.
    ///
    /// # Errors
    ///
    /// Returns [`VulkanVideoDeviceError`] for an invalid packed layout or when
    /// the DMA-BUF allocation size cannot be queried safely.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fd: OwnedFd,
        width: u32,
        height: u32,
        format: VulkanPackedPixelFormat,
        offset: u32,
        stride: u32,
        timestamp_ns: u64,
        duration_ns: u64,
    ) -> Result<Self, VulkanVideoDeviceError> {
        if width == 0
            || height == 0
            || width > 3_840
            || height > 2_160
            || !width.is_multiple_of(2)
            || !height.is_multiple_of(2)
            || !offset.is_multiple_of(4)
            || !stride.is_multiple_of(4)
            || stride < width.saturating_mul(4)
            || duration_ns == 0
            || timestamp_ns.checked_add(duration_ns).is_none()
        {
            return Err(VulkanVideoDeviceError::InvalidDmaBufFrame);
        }
        let required_size = u64::from(offset)
            .checked_add(u64::from(stride).saturating_mul(u64::from(height.saturating_sub(1))))
            .and_then(|value| value.checked_add(u64::from(width).saturating_mul(4)))
            .ok_or(VulkanVideoDeviceError::InvalidDmaBufFrame)?;
        let mut file = File::from(fd);
        let allocation_size = file
            .seek(SeekFrom::End(0))
            .map_err(|_| VulkanVideoDeviceError::DmaBufSizeUnavailable)?;
        file.seek(SeekFrom::Start(0))
            .map_err(|_| VulkanVideoDeviceError::DmaBufSizeUnavailable)?;
        if allocation_size < required_size {
            return Err(VulkanVideoDeviceError::InvalidDmaBufFrame);
        }
        Ok(Self {
            file,
            width,
            height,
            format,
            offset,
            stride,
            timestamp_ns,
            duration_ns,
            allocation_size,
            input_mode: VulkanVideoInputMode::LinearBuffer,
            drm_modifier: 0,
        })
    }

    /// Validate and take ownership of one packed RGB KMS framebuffer object.
    /// The explicit DRM modifier and row layout are retained for Vulkan image
    /// import; pixels are never mapped into host memory.
    ///
    /// # Errors
    ///
    /// Returns [`VulkanVideoDeviceError`] for an unsupported packed format,
    /// invalid dimensions/layout, or unavailable DMA-BUF allocation size.
    #[allow(clippy::too_many_arguments)]
    pub fn new_drm_image(
        fd: OwnedFd,
        width: u32,
        height: u32,
        drm_fourcc: u32,
        modifier: u64,
        offset: u32,
        stride: u32,
        timestamp_ns: u64,
        duration_ns: u64,
    ) -> Result<Self, VulkanVideoDeviceError> {
        let format = match drm_fourcc {
            DRM_FORMAT_XBGR8888 | DRM_FORMAT_ABGR8888 => VulkanPackedPixelFormat::Rgba8,
            DRM_FORMAT_XRGB8888 | DRM_FORMAT_ARGB8888 => VulkanPackedPixelFormat::Bgra8,
            _ => return Err(VulkanVideoDeviceError::InvalidDmaBufFrame),
        };
        let mut frame = Self::new(
            fd,
            width,
            height,
            format,
            offset,
            stride,
            timestamp_ns,
            duration_ns,
        )?;
        frame.input_mode = VulkanVideoInputMode::DrmImage;
        frame.drm_modifier = modifier;
        Ok(frame)
    }

    #[must_use]
    pub const fn allocation_size(&self) -> u64 {
        self.allocation_size
    }

    #[must_use]
    pub const fn input_mode(&self) -> VulkanVideoInputMode {
        self.input_mode
    }
}

impl VulkanVideoH264Request {
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.width > 0
            && self.height > 0
            && self.width <= 3_840
            && self.height <= 2_160
            && self.width.is_multiple_of(2)
            && self.height.is_multiple_of(2)
            && if self.variable_rate {
                (120..=240).contains(&self.frames_per_second)
                    && self.frames_per_second
                        == maximum_variable_frame_rate(self.width, self.height)
            } else {
                matches!(self.frames_per_second, 30 | 60 | 120)
            }
            && self
                .width
                .div_ceil(16)
                .saturating_mul(self.height.div_ceil(16))
                .saturating_mul(u32::from(self.frames_per_second))
                <= 2_073_600
            && matches!(self.target_megabits_per_second, 12 | 24 | 40)
    }

    /// Map Redunar's Efficient/Balanced/High presets onto the quality levels
    /// reported by the selected Vulkan video profile.
    const fn quality_level(self, max_quality_levels: u32) -> u32 {
        let highest = max_quality_levels.saturating_sub(1);
        match self.target_megabits_per_second {
            24 => highest / 2,
            40 => highest,
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VulkanVideoH264Candidate {
    pub physical_device_index: u8,
    pub queue_family_index: u32,
    pub compute_queue_family_index: u32,
    pub encode_input_format: i32,
    pub dpb_format: i32,
    pub min_coded_width: u32,
    pub min_coded_height: u32,
    pub max_coded_width: u32,
    pub max_coded_height: u32,
    pub min_bitstream_offset_alignment: u64,
    pub min_bitstream_size_alignment: u64,
    pub max_bitrate: u64,
    pub max_quality_levels: u32,
    pub supported_encode_feedback_flags: u32,
    pub min_qp: i32,
    pub max_qp: i32,
    pub max_level_idc: i32,
    pub selected_level_idc: i32,
    pub max_dpb_slots: u32,
    pub max_active_reference_pictures: u32,
    pub separate_reference_images: bool,
    pub std_header_name: String,
    pub std_header_spec_version: u32,
    pub dma_buf_import_requires_dedicated_allocation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VulkanVideoProbeBlocker {
    InvalidRequest,
    LoaderUnavailable,
    LoaderSymbolUnavailable(&'static str),
    InstanceCreationFailed(i32),
    PhysicalDeviceEnumerationFailed(i32),
    NoSupportedPhysicalDevice,
    Vulkan13Unsupported,
    Synchronization2Unsupported,
    DeviceExtensionEnumerationFailed(i32),
    MissingDeviceExtension(&'static str),
    HardwareH264CodecUnavailable,
    DmaBufStorageBufferImportUnsupported,
    QueueFamilyEnumerationFailed,
    NoH264EncodeQueue,
    NoComputeQueue,
    H264ProfileUnsupported(i32),
    H264LevelUnsupported { required: i32, maximum: i32 },
    CodedExtentUnsupported,
    VideoFormatQueryFailed(i32),
    Nv12VideoFormatUnsupported,
    ReferencePicturesUnsupported,
    InvalidStdHeaderVersion,
    EncodeFeedbackUnsupported,
    RequestedBitrateUnsupported,
    RateControlUnsupported,
    PPictureReferenceUnsupported,
    GopRemainingFramesRequired,
    SeparateReferenceImagesUnsupported,
    SliceUnsupported,
    TemporalLayerUnsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VulkanVideoH264Probe {
    pub candidate: Option<VulkanVideoH264Candidate>,
    pub blockers: Vec<VulkanVideoProbeBlocker>,
}

impl VulkanVideoH264Probe {
    /// Query the local Vulkan loader and AMD physical devices without creating
    /// a logical device, importing a DMA-BUF, or starting an encode session.
    ///
    /// The result is only a construction candidate. `production_ready` stays
    /// false until the daemon's live import/output/performance gates pass.
    #[must_use]
    pub fn local(request: VulkanVideoH264Request) -> Self {
        Self::local_with_nvidia_beta(request, false)
    }

    /// Select NVIDIA devices for an explicitly eligible Beta access attempt.
    /// The ordinary AMD path continues to use `local`.
    #[must_use]
    pub fn local_with_nvidia_beta(
        request: VulkanVideoH264Request,
        allow_nvidia_beta: bool,
    ) -> Self {
        if !request.is_valid() {
            return Self::blocked(VulkanVideoProbeBlocker::InvalidRequest);
        }
        // SAFETY: `probe_local` contains the native loader boundary and copies
        // every result into owned Rust values before destroying the instance.
        unsafe { probe_local(request, allow_nvidia_beta) }
    }

    #[must_use]
    pub const fn production_ready(&self) -> bool {
        false
    }

    fn blocked(blocker: VulkanVideoProbeBlocker) -> Self {
        Self {
            candidate: None,
            blockers: vec![blocker],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VulkanVideoDeviceError {
    Probe(Vec<VulkanVideoProbeBlocker>),
    LoaderUnavailable,
    LoaderSymbolUnavailable(&'static str),
    InstanceCreationFailed(i32),
    PhysicalDeviceEnumerationFailed(i32),
    SelectedPhysicalDeviceUnavailable,
    LogicalDeviceCreationFailed(i32),
    QueueUnavailable,
    DeviceSymbolUnavailable(&'static str),
    VideoSessionCreationFailed(i32),
    VideoSessionMemoryQueryFailed(i32),
    VideoSessionMemoryBoundExceeded,
    DeviceLocalMemoryUnavailable,
    VideoSessionMemoryAllocationFailed(i32),
    VideoSessionMemoryBindFailed(i32),
    SessionParameterCreationFailed(i32),
    SessionParameterReadFailed(i32),
    SessionParameterBytesExceeded,
    InvalidDmaBufFrame,
    DmaBufSizeUnavailable,
    DmaBufPropertiesFailed(i32),
    ImportedBufferCreationFailed(i32),
    ImportedMemoryTypeUnavailable,
    ImportedMemoryAllocationFailed(i32),
    ImportedBufferBindFailed(i32),
    InvalidComputeShader,
    ImageCreationFailed(i32),
    ImageMemoryTypeUnavailable,
    ImageMemoryAllocationFailed(i32),
    ImageMemoryBindFailed(i32),
    ImageViewCreationFailed(i32),
    SamplerCreationFailed(i32),
    DescriptorSetLayoutCreationFailed(i32),
    DescriptorPoolCreationFailed(i32),
    DescriptorSetAllocationFailed(i32),
    PipelineLayoutCreationFailed(i32),
    ShaderModuleCreationFailed(i32),
    ComputePipelineCreationFailed(i32),
    CommandPoolCreationFailed(i32),
    CommandBufferAllocationFailed(i32),
    SemaphoreCreationFailed(i32),
    FenceCreationFailed(i32),
    DeviceWaitFailed(i32),
    BitstreamBufferCreationFailed(i32),
    BitstreamMemoryTypeUnavailable,
    BitstreamMemoryAllocationFailed(i32),
    BitstreamBufferBindFailed(i32),
    BitstreamMapFailed(i32),
    EncodeQueryPoolCreationFailed(i32),
    EncodeFeedbackUnsupported,
    FenceWaitFailed(i32),
    FenceResetFailed(i32),
    CommandBufferResetFailed(i32),
    CommandBufferBeginFailed(i32),
    CommandBufferEndFailed(i32),
    QueueSubmissionFailed(i32),
    EncodeFeedbackReadFailed(i32),
    EncodedBytesExceeded,
    EncoderPoisoned,
}

impl std::fmt::Display for VulkanVideoDeviceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Vulkan Video device error: {self:?}")
    }
}

impl std::error::Error for VulkanVideoDeviceError {}

/// Owned Vulkan logical device prepared for the Replay conversion and H.264
/// encode stages. Handles are stored as addresses so the safe wrapper can move
/// to the daemon's dedicated encoder worker; all Vulkan access remains
/// serialized by that owner.
pub struct VulkanVideoH264Device {
    _library: DynamicLibrary,
    instance_address: usize,
    device_address: usize,
    physical_device_address: usize,
    compute_queue_address: usize,
    encode_queue_address: usize,
    destroy_instance: DestroyInstance,
    destroy_device: DestroyDevice,
    get_instance_proc_addr: GetInstanceProcAddr,
    get_device_proc_addr: GetDeviceProcAddr,
    request: VulkanVideoH264Request,
    candidate: VulkanVideoH264Candidate,
}

/// Vulkan Video session plus every driver-requested memory allocation. This
/// type consumes the logical-device owner so session resources always tear
/// down before the device and loader.
pub struct VulkanVideoH264Session {
    device: VulkanVideoH264Device,
    session: u64,
    session_memory: Vec<VkDeviceMemory>,
    destroy_session: DestroyVideoSession,
    free_memory: FreeMemory,
}

pub struct VulkanVideoH264Parameters {
    session: VulkanVideoH264Session,
    parameters: u64,
    encoded_parameters: Box<[u8]>,
    destroy_parameters: DestroyVideoSessionParameters,
}

struct VulkanVideoImportedBuffer {
    device_address: usize,
    resource: VulkanVideoImportedResource,
    pub width: u32,
    pub height: u32,
    pub format: VulkanPackedPixelFormat,
    pub offset_words: u32,
    pub stride_words: u32,
    pub timestamp_ns: u64,
    pub duration_ns: u64,
}

enum VulkanVideoImportedResource {
    Buffer {
        buffer: VkBuffer,
        memory: VkDeviceMemory,
        buffer_size: u64,
        destroy_buffer: DestroyBuffer,
        free_memory: FreeMemory,
    },
    DrmImage {
        image: u64,
        view: u64,
        memory: VkDeviceMemory,
        destroy_image: unsafe extern "system" fn(VkDevice, u64, *const VkAllocationCallbacks),
        destroy_image_view: unsafe extern "system" fn(VkDevice, u64, *const VkAllocationCallbacks),
        free_memory: FreeMemory,
    },
}

impl VulkanVideoH264Device {
    /// Create the logical device and required compute/video queues after a
    /// fresh closed capability probe. No image, DMA-BUF, video session, or
    /// packet buffer is allocated at this stage.
    ///
    /// # Errors
    ///
    /// Returns [`VulkanVideoDeviceError`] if capability discovery changes,
    /// loader symbols are missing, or Vulkan refuses device/queue creation.
    pub fn open(request: VulkanVideoH264Request) -> Result<Self, VulkanVideoDeviceError> {
        Self::open_with_nvidia_beta(request, false)
    }

    /// Opted-in NVIDIA candidates use the same Vulkan Video capability checks.
    /// The caller must first establish unambiguous render-device ownership.
    ///
    /// # Errors
    ///
    /// Returns [`VulkanVideoDeviceError`] when the probe or device creation
    /// fails.
    pub fn open_with_nvidia_beta(
        request: VulkanVideoH264Request,
        allow_nvidia_beta: bool,
    ) -> Result<Self, VulkanVideoDeviceError> {
        let probe = VulkanVideoH264Probe::local_with_nvidia_beta(request, allow_nvidia_beta);
        let candidate = probe
            .candidate
            .ok_or(VulkanVideoDeviceError::Probe(probe.blockers))?;
        // SAFETY: `open_device` contains all native ownership transitions and
        // returns only after instance/device/queue creation succeeds.
        unsafe { open_device(request, candidate) }
    }

    #[must_use]
    pub const fn request(&self) -> VulkanVideoH264Request {
        self.request
    }

    #[must_use]
    pub const fn candidate(&self) -> &VulkanVideoH264Candidate {
        &self.candidate
    }

    #[must_use]
    pub const fn uses_distinct_queue_families(&self) -> bool {
        self.candidate.queue_family_index != self.candidate.compute_queue_family_index
    }

    #[must_use]
    pub const fn has_live_queues(&self) -> bool {
        self.compute_queue_address != 0 && self.encode_queue_address != 0
    }

    /// Create and bind every driver-requested memory object for one H.264
    /// video session. The logical device is consumed so no caller can destroy
    /// it before the session.
    ///
    /// # Errors
    ///
    /// Returns [`VulkanVideoDeviceError`] if a required device command is
    /// missing, session creation fails, memory requirements exceed the fixed
    /// bound, no device-local memory type matches, allocation fails, or the
    /// driver rejects the complete bind set. All partial resources are
    /// released before returning an error.
    pub fn create_session(self) -> Result<VulkanVideoH264Session, VulkanVideoDeviceError> {
        // SAFETY: this call consumes the unique device owner and the native
        // helper transactionally transfers it into the returned session.
        unsafe { create_video_session(self) }
    }
}

impl Drop for VulkanVideoH264Device {
    fn drop(&mut self) {
        if self.device_address != 0 {
            // SAFETY: this wrapper uniquely owns the logical device and all
            // later encoder resources must be dropped before this owner.
            unsafe {
                (self.destroy_device)(self.device_address as VkDevice, ptr::null());
            }
            self.device_address = 0;
        }
        if self.instance_address != 0 {
            // SAFETY: this wrapper uniquely owns the instance and its logical
            // device has already been destroyed above.
            unsafe {
                (self.destroy_instance)(self.instance_address as VkInstance, ptr::null());
            }
            self.instance_address = 0;
        }
    }
}

impl VulkanVideoH264Session {
    #[must_use]
    pub const fn request(&self) -> VulkanVideoH264Request {
        self.device.request
    }

    #[must_use]
    pub const fn memory_bind_count(&self) -> usize {
        self.session_memory.len()
    }

    #[must_use]
    pub const fn has_live_session(&self) -> bool {
        self.session != 0
    }

    /// Install Redunar's bounded SPS/PPS and retrieve the driver's encoded
    /// parameter byte stream for Matroska codec initialization.
    ///
    /// # Errors
    ///
    /// Returns [`VulkanVideoDeviceError`] when parameter creation or bounded
    /// retrieval fails. The video session remains owned and is released on
    /// every error path.
    pub fn create_parameters(self) -> Result<VulkanVideoH264Parameters, VulkanVideoDeviceError> {
        // SAFETY: this consumes the unique live session and transfers it only
        // after the parameter object and encoded headers both succeed.
        unsafe { create_h264_parameters(self) }
    }
}

impl Drop for VulkanVideoH264Session {
    fn drop(&mut self) {
        if self.session != 0 {
            // SAFETY: this wrapper owns the session and its device is live.
            unsafe {
                (self.destroy_session)(
                    self.device.device_address as VkDevice,
                    self.session,
                    ptr::null(),
                );
            }
            self.session = 0;
        }
        for memory in self.session_memory.drain(..) {
            // SAFETY: every allocation belongs to this live device and the
            // bound session was destroyed above.
            unsafe {
                (self.free_memory)(self.device.device_address as VkDevice, memory, ptr::null());
            }
        }
    }
}

impl VulkanVideoH264Parameters {
    #[must_use]
    pub fn encoded_parameters(&self) -> &[u8] {
        &self.encoded_parameters
    }

    #[must_use]
    pub const fn request(&self) -> VulkanVideoH264Request {
        self.session.request()
    }

    /// Import the capture layer's exported buffer without mapping pixels into
    /// host memory. The returned resource borrows this session so the Vulkan
    /// device cannot be destroyed before the buffer.
    ///
    /// # Errors
    ///
    /// Returns [`VulkanVideoDeviceError`] when the DMA-BUF properties, buffer,
    /// compatible memory type, import allocation, or bind operation fails.
    fn import_frame(
        &self,
        frame: VulkanVideoDmaBufFrame,
    ) -> Result<VulkanVideoImportedBuffer, VulkanVideoDeviceError> {
        if frame.width != self.request().width || frame.height != self.request().height {
            return Err(VulkanVideoDeviceError::InvalidDmaBufFrame);
        }
        // SAFETY: the frame owns its FD and this borrow keeps the destination
        // device live for the complete imported-resource lifetime.
        match frame.input_mode() {
            VulkanVideoInputMode::LinearBuffer => unsafe { import_dma_buf(self, frame) },
            VulkanVideoInputMode::DrmImage => unsafe { conversion::import_drm_image(self, frame) },
        }
    }
}

impl Drop for VulkanVideoH264Parameters {
    fn drop(&mut self) {
        if self.parameters != 0 {
            // SAFETY: this object owns the parameter handle and the nested
            // session/device remain live until after this Drop body.
            unsafe {
                (self.destroy_parameters)(
                    self.session.device.device_address as VkDevice,
                    self.parameters,
                    ptr::null(),
                );
            }
            self.parameters = 0;
        }
    }
}

impl Drop for VulkanVideoImportedBuffer {
    fn drop(&mut self) {
        let device = self.device_address as VkDevice;
        match &mut self.resource {
            VulkanVideoImportedResource::Buffer {
                buffer,
                memory,
                destroy_buffer,
                free_memory,
                ..
            } => {
                if *buffer != 0 {
                    // SAFETY: this object owns the buffer and its borrowed device is live.
                    unsafe { destroy_buffer(device, *buffer, ptr::null()) };
                    *buffer = 0;
                }
                if *memory != 0 {
                    // SAFETY: the bound buffer was destroyed and this object owns the memory.
                    unsafe { free_memory(device, *memory, ptr::null()) };
                    *memory = 0;
                }
            }
            VulkanVideoImportedResource::DrmImage {
                image,
                view,
                memory,
                destroy_image,
                destroy_image_view,
                free_memory,
            } => {
                if *view != 0 {
                    // SAFETY: this object owns the view and its image/device are live.
                    unsafe { destroy_image_view(device, *view, ptr::null()) };
                    *view = 0;
                }
                if *image != 0 {
                    // SAFETY: the owned image has no remaining view.
                    unsafe { destroy_image(device, *image, ptr::null()) };
                    *image = 0;
                }
                if *memory != 0 {
                    // SAFETY: the image was destroyed and this object owns the memory.
                    unsafe { free_memory(device, *memory, ptr::null()) };
                    *memory = 0;
                }
            }
        }
    }
}

#[repr(C)]
struct VkApplicationInfo {
    s_type: i32,
    p_next: *const c_void,
    application_name: *const c_char,
    application_version: u32,
    engine_name: *const c_char,
    engine_version: u32,
    api_version: u32,
}

#[repr(C)]
struct VkPhysicalDeviceSynchronization2Features {
    s_type: i32,
    p_next: *mut c_void,
    synchronization2: u32,
}

#[repr(C)]
struct VkPhysicalDeviceFeatures2 {
    s_type: i32,
    p_next: *mut c_void,
    // Redunar enables no Vulkan 1.0 feature. The fixed array exactly mirrors
    // the 55 VkBool32 fields in VkPhysicalDeviceFeatures while avoiding a
    // misleading set of feature names that this path never reads.
    features: [u32; VK_PHYSICAL_DEVICE_FEATURE_COUNT],
}

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

#[repr(C)]
struct VkQueueFamilyProperties2 {
    s_type: i32,
    p_next: *mut c_void,
    queue_family_properties: VkQueueFamilyProperties,
}

#[repr(C)]
struct VkQueueFamilyVideoPropertiesKhr {
    s_type: i32,
    p_next: *mut c_void,
    video_codec_operations: u32,
}

#[repr(C)]
struct VkVideoEncodeH264ProfileInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    std_profile_idc: i32,
}

#[repr(C)]
struct VkVideoEncodeUsageInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    video_usage_hints: u32,
    video_content_hints: u32,
    tuning_mode: i32,
}

#[repr(C)]
struct VkVideoEncodeQualityLevelInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    quality_level: u32,
}

#[repr(C)]
struct VkVideoProfileInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    video_codec_operation: i32,
    chroma_subsampling: u32,
    luma_bit_depth: u32,
    chroma_bit_depth: u32,
}

#[repr(C)]
struct VkVideoProfileListInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    profile_count: u32,
    profiles: *const VkVideoProfileInfoKhr,
}

#[repr(C)]
struct VkPhysicalDeviceVideoFormatInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    image_usage: u32,
}

#[repr(C)]
struct VkComponentMapping {
    r: i32,
    g: i32,
    b: i32,
    a: i32,
}

#[repr(C)]
struct VkVideoFormatPropertiesKhr {
    s_type: i32,
    p_next: *mut c_void,
    format: i32,
    component_mapping: VkComponentMapping,
    image_create_flags: u32,
    image_type: i32,
    image_tiling: i32,
    image_usage_flags: u32,
}

#[repr(C)]
struct VkVideoEncodeH264CapabilitiesKhr {
    s_type: i32,
    p_next: *mut c_void,
    flags: u32,
    max_level_idc: i32,
    max_slice_count: u32,
    max_p_picture_l0_reference_count: u32,
    max_b_picture_l0_reference_count: u32,
    max_l1_reference_count: u32,
    max_temporal_layer_count: u32,
    expect_dyadic_temporal_layer_pattern: u32,
    min_qp: i32,
    max_qp: i32,
    prefers_gop_remaining_frames: u32,
    requires_gop_remaining_frames: u32,
    std_syntax_flags: u32,
}

#[repr(C)]
struct VkVideoEncodeCapabilitiesKhr {
    s_type: i32,
    p_next: *mut c_void,
    flags: u32,
    rate_control_modes: u32,
    max_rate_control_layers: u32,
    max_bitrate: u64,
    max_quality_levels: u32,
    encode_input_picture_granularity: VkExtent2d,
    supported_encode_feedback_flags: u32,
}

#[repr(C)]
struct VkVideoCapabilitiesKhr {
    s_type: i32,
    p_next: *mut c_void,
    flags: u32,
    min_bitstream_buffer_offset_alignment: u64,
    min_bitstream_buffer_size_alignment: u64,
    picture_access_granularity: VkExtent2d,
    min_coded_extent: VkExtent2d,
    max_coded_extent: VkExtent2d,
    max_dpb_slots: u32,
    max_active_reference_pictures: u32,
    std_header_version: VkExtensionProperties,
}

type VkVideoSessionKhr = u64;
type VkVideoSessionParametersKhr = u64;

#[repr(C)]
struct VkVideoEncodeH264SessionCreateInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    use_max_level_idc: u32,
    max_level_idc: i32,
}

#[repr(C)]
struct VkVideoSessionCreateInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    queue_family_index: u32,
    flags: u32,
    video_profile: *const VkVideoProfileInfoKhr,
    picture_format: i32,
    max_coded_extent: VkExtent2d,
    reference_picture_format: i32,
    max_dpb_slots: u32,
    max_active_reference_pictures: u32,
    std_header_version: *const VkExtensionProperties,
}

#[repr(C)]
struct VkVideoSessionMemoryRequirementsKhr {
    s_type: i32,
    p_next: *mut c_void,
    memory_bind_index: u32,
    memory_requirements: VkMemoryRequirements,
}

#[repr(C)]
struct VkBindVideoSessionMemoryInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    memory_bind_index: u32,
    memory: VkDeviceMemory,
    memory_offset: u64,
    memory_size: u64,
}

#[repr(C)]
struct StdVideoH264SequenceParameterSetVui {
    flags: u32,
    aspect_ratio_idc: i32,
    sar_width: u16,
    sar_height: u16,
    video_format: u8,
    colour_primaries: u8,
    transfer_characteristics: u8,
    matrix_coefficients: u8,
    num_units_in_tick: u32,
    time_scale: u32,
    max_num_reorder_frames: u8,
    max_dec_frame_buffering: u8,
    chroma_sample_loc_type_top_field: u8,
    chroma_sample_loc_type_bottom_field: u8,
    reserved1: u32,
    hrd_parameters: *const c_void,
}

#[repr(C)]
struct StdVideoH264SequenceParameterSet {
    flags: u32,
    profile_idc: i32,
    level_idc: i32,
    chroma_format_idc: i32,
    seq_parameter_set_id: u8,
    bit_depth_luma_minus8: u8,
    bit_depth_chroma_minus8: u8,
    log2_max_frame_num_minus4: u8,
    pic_order_cnt_type: i32,
    offset_for_non_ref_pic: i32,
    offset_for_top_to_bottom_field: i32,
    log2_max_pic_order_cnt_lsb_minus4: u8,
    num_ref_frames_in_pic_order_cnt_cycle: u8,
    max_num_ref_frames: u8,
    reserved1: u8,
    pic_width_in_mbs_minus1: u32,
    pic_height_in_map_units_minus1: u32,
    frame_crop_left_offset: u32,
    frame_crop_right_offset: u32,
    frame_crop_top_offset: u32,
    frame_crop_bottom_offset: u32,
    reserved2: u32,
    offset_for_ref_frame: *const i32,
    scaling_lists: *const c_void,
    sequence_parameter_set_vui: *const StdVideoH264SequenceParameterSetVui,
}

#[repr(C)]
struct StdVideoH264PictureParameterSet {
    flags: u32,
    seq_parameter_set_id: u8,
    pic_parameter_set_id: u8,
    num_ref_idx_l0_default_active_minus1: u8,
    num_ref_idx_l1_default_active_minus1: u8,
    weighted_bipred_idc: i32,
    pic_init_qp_minus26: i8,
    pic_init_qs_minus26: i8,
    chroma_qp_index_offset: i8,
    second_chroma_qp_index_offset: i8,
    scaling_lists: *const c_void,
}

#[repr(C)]
struct VkVideoEncodeH264SessionParametersAddInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    std_sps_count: u32,
    std_sps: *const StdVideoH264SequenceParameterSet,
    std_pps_count: u32,
    std_pps: *const StdVideoH264PictureParameterSet,
}

#[repr(C)]
struct VkVideoEncodeH264SessionParametersCreateInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    max_std_sps_count: u32,
    max_std_pps_count: u32,
    parameters_add_info: *const VkVideoEncodeH264SessionParametersAddInfoKhr,
}

#[repr(C)]
struct VkVideoSessionParametersCreateInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    flags: u32,
    video_session_parameters_template: VkVideoSessionParametersKhr,
    video_session: VkVideoSessionKhr,
}

#[repr(C)]
struct VkVideoEncodeH264SessionParametersGetInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    write_std_sps: u32,
    write_std_pps: u32,
    std_sps_id: u32,
    std_pps_id: u32,
}

#[repr(C)]
struct VkVideoEncodeSessionParametersGetInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    video_session_parameters: VkVideoSessionParametersKhr,
}

#[repr(C)]
struct VkVideoEncodeH264SessionParametersFeedbackInfoKhr {
    s_type: i32,
    p_next: *mut c_void,
    has_std_sps_overrides: u32,
    has_std_pps_overrides: u32,
}

#[repr(C)]
struct VkVideoEncodeSessionParametersFeedbackInfoKhr {
    s_type: i32,
    p_next: *mut c_void,
    has_overrides: u32,
}

#[repr(C)]
struct VkImportMemoryFdInfoKhr {
    s_type: i32,
    p_next: *const c_void,
    handle_type: u32,
    fd: i32,
}

#[repr(C)]
struct VkMemoryFdPropertiesKhr {
    s_type: i32,
    p_next: *mut c_void,
    memory_type_bits: u32,
}

#[repr(C)]
struct VkPhysicalDeviceExternalBufferInfo {
    s_type: i32,
    p_next: *const c_void,
    flags: u32,
    usage: u32,
    handle_type: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
struct VkExternalMemoryProperties {
    external_memory_features: u32,
    export_from_imported_handle_types: u32,
    compatible_handle_types: u32,
}

#[repr(C)]
struct VkExternalBufferProperties {
    s_type: i32,
    p_next: *mut c_void,
    external_memory_properties: VkExternalMemoryProperties,
}

#[repr(C)]
struct VkMemoryDedicatedAllocateInfo {
    s_type: i32,
    p_next: *const c_void,
    image: u64,
    buffer: VkBuffer,
}

#[repr(C)]
struct VkBufferMemoryRequirementsInfo2 {
    s_type: i32,
    p_next: *const c_void,
    buffer: VkBuffer,
}

#[repr(C)]
struct VkMemoryDedicatedRequirements {
    s_type: i32,
    p_next: *mut c_void,
    prefers_dedicated_allocation: u32,
    requires_dedicated_allocation: u32,
}

#[repr(C)]
struct VkMemoryRequirements2 {
    s_type: i32,
    p_next: *mut c_void,
    memory_requirements: VkMemoryRequirements,
}

type CreateInstance = unsafe extern "system" fn(
    *const VkInstanceCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkInstance,
) -> VkResult;
type DestroyInstance = unsafe extern "system" fn(VkInstance, *const VkAllocationCallbacks);
type GetInstanceProcAddr = unsafe extern "system" fn(VkInstance, *const c_char) -> PfnVoidFunction;
type GetDeviceProcAddr = unsafe extern "system" fn(VkDevice, *const c_char) -> PfnVoidFunction;
type CreateDevice = unsafe extern "system" fn(
    VkPhysicalDevice,
    *const VkDeviceCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkDevice,
) -> VkResult;
type DestroyDevice = unsafe extern "system" fn(VkDevice, *const VkAllocationCallbacks);
type GetDeviceQueue = unsafe extern "system" fn(VkDevice, u32, u32, *mut *mut c_void);
type CreateVideoSession = unsafe extern "system" fn(
    VkDevice,
    *const VkVideoSessionCreateInfoKhr,
    *const VkAllocationCallbacks,
    *mut VkVideoSessionKhr,
) -> VkResult;
type DestroyVideoSession =
    unsafe extern "system" fn(VkDevice, VkVideoSessionKhr, *const VkAllocationCallbacks);
type GetVideoSessionMemoryRequirements = unsafe extern "system" fn(
    VkDevice,
    VkVideoSessionKhr,
    *mut u32,
    *mut VkVideoSessionMemoryRequirementsKhr,
) -> VkResult;
type BindVideoSessionMemory = unsafe extern "system" fn(
    VkDevice,
    VkVideoSessionKhr,
    u32,
    *const VkBindVideoSessionMemoryInfoKhr,
) -> VkResult;
type AllocateMemory = unsafe extern "system" fn(
    VkDevice,
    *const VkMemoryAllocateInfo,
    *const VkAllocationCallbacks,
    *mut VkDeviceMemory,
) -> VkResult;
type FreeMemory = unsafe extern "system" fn(VkDevice, VkDeviceMemory, *const VkAllocationCallbacks);
type GetPhysicalDeviceMemoryProperties =
    unsafe extern "system" fn(VkPhysicalDevice, *mut VkPhysicalDeviceMemoryProperties);
type CreateVideoSessionParameters = unsafe extern "system" fn(
    VkDevice,
    *const VkVideoSessionParametersCreateInfoKhr,
    *const VkAllocationCallbacks,
    *mut VkVideoSessionParametersKhr,
) -> VkResult;
type DestroyVideoSessionParameters =
    unsafe extern "system" fn(VkDevice, VkVideoSessionParametersKhr, *const VkAllocationCallbacks);
type GetEncodedVideoSessionParameters = unsafe extern "system" fn(
    VkDevice,
    *const VkVideoEncodeSessionParametersGetInfoKhr,
    *mut VkVideoEncodeSessionParametersFeedbackInfoKhr,
    *mut usize,
    *mut c_void,
) -> VkResult;
type GetMemoryFdProperties =
    unsafe extern "system" fn(VkDevice, u32, i32, *mut VkMemoryFdPropertiesKhr) -> VkResult;
type CreateBuffer = unsafe extern "system" fn(
    VkDevice,
    *const VkBufferCreateInfo,
    *const VkAllocationCallbacks,
    *mut VkBuffer,
) -> VkResult;
type DestroyBuffer = unsafe extern "system" fn(VkDevice, VkBuffer, *const VkAllocationCallbacks);
type GetBufferMemoryRequirements =
    unsafe extern "system" fn(VkDevice, VkBuffer, *mut VkMemoryRequirements);
type GetBufferMemoryRequirements2 = unsafe extern "system" fn(
    VkDevice,
    *const VkBufferMemoryRequirementsInfo2,
    *mut VkMemoryRequirements2,
);
type BindBufferMemory =
    unsafe extern "system" fn(VkDevice, VkBuffer, VkDeviceMemory, u64) -> VkResult;
type EnumeratePhysicalDevices =
    unsafe extern "system" fn(VkInstance, *mut u32, *mut VkPhysicalDevice) -> VkResult;
type GetPhysicalDeviceProperties = unsafe extern "system" fn(VkPhysicalDevice, *mut c_void);
type GetPhysicalDeviceFeatures2 =
    unsafe extern "system" fn(VkPhysicalDevice, *mut VkPhysicalDeviceFeatures2);
type EnumerateDeviceExtensionProperties = unsafe extern "system" fn(
    VkPhysicalDevice,
    *const c_char,
    *mut u32,
    *mut VkExtensionProperties,
) -> VkResult;
type GetQueueFamilyProperties2 =
    unsafe extern "system" fn(VkPhysicalDevice, *mut u32, *mut VkQueueFamilyProperties2);
type GetPhysicalDeviceVideoCapabilities = unsafe extern "system" fn(
    VkPhysicalDevice,
    *const VkVideoProfileInfoKhr,
    *mut VkVideoCapabilitiesKhr,
) -> VkResult;
type GetPhysicalDeviceVideoFormatProperties = unsafe extern "system" fn(
    VkPhysicalDevice,
    *const VkPhysicalDeviceVideoFormatInfoKhr,
    *mut u32,
    *mut VkVideoFormatPropertiesKhr,
) -> VkResult;
type GetPhysicalDeviceExternalBufferProperties = unsafe extern "system" fn(
    VkPhysicalDevice,
    *const VkPhysicalDeviceExternalBufferInfo,
    *mut VkExternalBufferProperties,
);

struct DynamicLibrary(usize);

impl Drop for DynamicLibrary {
    fn drop(&mut self) {
        if self.0 != 0 {
            // SAFETY: this handle was returned by `dlopen` and is dropped once.
            unsafe { dlclose(self.0 as *mut c_void) };
            self.0 = 0;
        }
    }
}

#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(name: *const c_char, flags: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> i32;
}

unsafe fn resolve_device_command(
    device: &VulkanVideoH264Device,
    name: *const c_char,
) -> PfnVoidFunction {
    let raw_device = device.device_address as VkDevice;
    let command = unsafe { (device.get_device_proc_addr)(raw_device, name) };
    command.or_else(|| unsafe {
        (device.get_instance_proc_addr)(device.instance_address as VkInstance, name)
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "one linear ownership scope makes native Vulkan instance teardown auditable"
)]
unsafe fn probe_local(
    request: VulkanVideoH264Request,
    allow_nvidia_beta: bool,
) -> VulkanVideoH264Probe {
    let library_name = b"libvulkan.so.1\0";
    // SAFETY: the name is a terminated static byte string.
    let library = DynamicLibrary(unsafe { dlopen(library_name.as_ptr().cast(), 2) } as usize);
    if library.0 == 0 {
        return VulkanVideoH264Probe::blocked(VulkanVideoProbeBlocker::LoaderUnavailable);
    }

    macro_rules! load {
        ($symbol:literal, $ty:ty) => {{
            let name = concat!($symbol, "\0");
            // SAFETY: the loader handle is live and the symbol name is terminated.
            let raw = unsafe { dlsym(library.0 as *mut c_void, name.as_ptr().cast()) };
            if raw.is_null() {
                return VulkanVideoH264Probe::blocked(
                    VulkanVideoProbeBlocker::LoaderSymbolUnavailable($symbol),
                );
            }
            // SAFETY: the Vulkan symbol fixes this exact function ABI.
            unsafe { mem::transmute::<*mut c_void, $ty>(raw) }
        }};
    }

    let create_instance = load!("vkCreateInstance", CreateInstance);
    let get_instance_proc_addr = load!("vkGetInstanceProcAddr", GetInstanceProcAddr);
    let destroy_instance = load!("vkDestroyInstance", DestroyInstance);
    let enumerate_physical_devices = load!("vkEnumeratePhysicalDevices", EnumeratePhysicalDevices);
    let get_physical_device_properties =
        load!("vkGetPhysicalDeviceProperties", GetPhysicalDeviceProperties);
    let get_physical_device_features2 =
        load!("vkGetPhysicalDeviceFeatures2", GetPhysicalDeviceFeatures2);
    let enumerate_device_extensions = load!(
        "vkEnumerateDeviceExtensionProperties",
        EnumerateDeviceExtensionProperties
    );
    let get_queue_families = load!(
        "vkGetPhysicalDeviceQueueFamilyProperties2",
        GetQueueFamilyProperties2
    );
    let get_external_buffer_properties = load!(
        "vkGetPhysicalDeviceExternalBufferProperties",
        GetPhysicalDeviceExternalBufferProperties
    );

    let application_name = b"Redunar Instant Replay\0";
    let engine_name = b"Redunar\0";
    let application_info = VkApplicationInfo {
        s_type: VK_STRUCTURE_TYPE_APPLICATION_INFO,
        p_next: ptr::null(),
        application_name: application_name.as_ptr().cast(),
        application_version: 1,
        engine_name: engine_name.as_ptr().cast(),
        engine_version: 1,
        api_version: VULKAN_API_VERSION_1_3,
    };
    let instance_info = VkInstanceCreateInfo {
        s_type: VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
        p_next: ptr::null(),
        flags: 0,
        p_application_info: (&raw const application_info).cast(),
        enabled_layer_count: 0,
        enabled_layer_names: ptr::null(),
        enabled_extension_count: 0,
        enabled_extension_names: ptr::null(),
    };
    let mut instance = ptr::null_mut();
    // SAFETY: create info and output storage live for the call.
    let result =
        unsafe { create_instance(&raw const instance_info, ptr::null(), &raw mut instance) };
    if result != VK_SUCCESS || instance.is_null() {
        return VulkanVideoH264Probe::blocked(VulkanVideoProbeBlocker::InstanceCreationFailed(
            result,
        ));
    }
    let _instance = InstanceGuard {
        instance_address: instance.addr(),
        destroy: destroy_instance,
    };

    macro_rules! load_instance {
        ($symbol:literal, $ty:ty) => {{
            let name = concat!($symbol, "\0");
            let Some(raw) = (unsafe { get_instance_proc_addr(instance, name.as_ptr().cast()) })
            else {
                return VulkanVideoH264Probe::blocked(
                    VulkanVideoProbeBlocker::LoaderSymbolUnavailable($symbol),
                );
            };
            unsafe { mem::transmute::<unsafe extern "system" fn(), $ty>(raw) }
        }};
    }
    let get_video_capabilities = load_instance!(
        "vkGetPhysicalDeviceVideoCapabilitiesKHR",
        GetPhysicalDeviceVideoCapabilities
    );
    let get_video_formats = load_instance!(
        "vkGetPhysicalDeviceVideoFormatPropertiesKHR",
        GetPhysicalDeviceVideoFormatProperties
    );

    let devices = match unsafe { physical_devices(instance, enumerate_physical_devices) } {
        Ok(devices) => devices,
        Err(result) => {
            return VulkanVideoH264Probe::blocked(
                VulkanVideoProbeBlocker::PhysicalDeviceEnumerationFailed(result),
            );
        }
    };
    let mut saw_supported_device = false;
    let mut last_blockers = Vec::new();
    for (device_index, physical_device) in devices.into_iter().enumerate() {
        let (api_version, vendor_id) = unsafe {
            physical_device_api_and_vendor(physical_device, get_physical_device_properties)
        };
        if (allow_nvidia_beta && vendor_id != NVIDIA_VENDOR_ID)
            || (!allow_nvidia_beta && vendor_id != AMD_VENDOR_ID)
        {
            continue;
        }
        saw_supported_device = true;
        // Synchronization2 is core in Vulkan 1.3. Redunar intentionally uses
        // that core path rather than conditionally enabling the older KHR
        // extension, so a lower device API version is not a valid candidate.
        if api_version < VULKAN_API_VERSION_1_3 {
            last_blockers = vec![VulkanVideoProbeBlocker::Vulkan13Unsupported];
            continue;
        }
        if !unsafe {
            physical_device_supports_synchronization2(
                physical_device,
                get_physical_device_features2,
            )
        } {
            last_blockers = vec![VulkanVideoProbeBlocker::Synchronization2Unsupported];
            continue;
        }
        let extensions = match unsafe {
            device_extensions(physical_device, enumerate_device_extensions)
        } {
            Ok(extensions) => extensions,
            Err(result) => {
                last_blockers = vec![VulkanVideoProbeBlocker::DeviceExtensionEnumerationFailed(
                    result,
                )];
                continue;
            }
        };
        let missing: Vec<_> = REQUIRED_DEVICE_EXTENSIONS
            .into_iter()
            .filter(|required| !extensions.iter().any(|extension| extension == required))
            .map(|required| {
                if required == "VK_KHR_video_encode_h264" {
                    VulkanVideoProbeBlocker::HardwareH264CodecUnavailable
                } else {
                    VulkanVideoProbeBlocker::MissingDeviceExtension(required)
                }
            })
            .collect();
        if !missing.is_empty() {
            last_blockers = missing;
            continue;
        }
        let Some(dma_buf_import_requires_dedicated_allocation) = (unsafe {
            dma_buf_storage_buffer_import_support(physical_device, get_external_buffer_properties)
        }) else {
            last_blockers = vec![VulkanVideoProbeBlocker::DmaBufStorageBufferImportUnsupported];
            continue;
        };
        let Some((queue_family_index, compute_queue_family_index)) =
            (unsafe { h264_encode_queues(physical_device, get_queue_families) })
        else {
            last_blockers = vec![
                VulkanVideoProbeBlocker::NoH264EncodeQueue,
                VulkanVideoProbeBlocker::NoComputeQueue,
            ];
            continue;
        };
        match unsafe { h264_capabilities(physical_device, request, get_video_capabilities) } {
            Ok(capability) => {
                let selected_level_idc = required_h264_level(request);
                // The zero-level workaround is established only for the
                // AMD/RADV path. Do not extend that exception to NVIDIA beta:
                // an unknown level there must fail before encoder allocation.
                if (capability.h264.max_level_idc == 0 && vendor_id != AMD_VENDOR_ID)
                    || (capability.h264.max_level_idc != 0
                        && selected_level_idc > capability.h264.max_level_idc)
                {
                    last_blockers = vec![VulkanVideoProbeBlocker::H264LevelUnsupported {
                        required: selected_level_idc,
                        maximum: capability.h264.max_level_idc,
                    }];
                    continue;
                }
                if capability.video.max_dpb_slots < 2
                    || capability.video.max_active_reference_pictures == 0
                {
                    last_blockers = vec![VulkanVideoProbeBlocker::ReferencePicturesUnsupported];
                    continue;
                }
                if capability.encode.rate_control_modes
                    & VK_VIDEO_ENCODE_RATE_CONTROL_MODE_CBR_BIT_KHR
                    == 0
                    || capability.encode.max_rate_control_layers == 0
                {
                    last_blockers = vec![VulkanVideoProbeBlocker::RateControlUnsupported];
                    continue;
                }
                if capability.h264.max_p_picture_l0_reference_count == 0 {
                    last_blockers = vec![VulkanVideoProbeBlocker::PPictureReferenceUnsupported];
                    continue;
                }
                if capability.h264.max_slice_count == 0 {
                    last_blockers = vec![VulkanVideoProbeBlocker::SliceUnsupported];
                    continue;
                }
                if capability.h264.max_temporal_layer_count == 0 {
                    last_blockers = vec![VulkanVideoProbeBlocker::TemporalLayerUnsupported];
                    continue;
                }
                if capability.h264.requires_gop_remaining_frames != 0 {
                    last_blockers = vec![VulkanVideoProbeBlocker::GopRemainingFramesRequired];
                    continue;
                }
                if capability.encode.supported_encode_feedback_flags
                    & VK_VIDEO_ENCODE_FEEDBACK_BITSTREAM_BYTES_WRITTEN_BIT_KHR
                    == 0
                {
                    last_blockers = vec![VulkanVideoProbeBlocker::EncodeFeedbackUnsupported];
                    continue;
                }
                if u64::from(request.target_megabits_per_second).saturating_mul(1_000_000)
                    > capability.encode.max_bitrate
                {
                    last_blockers = vec![VulkanVideoProbeBlocker::RequestedBitrateUnsupported];
                    continue;
                }
                let std_header_name = match unsafe {
                    CStr::from_ptr(capability.video.std_header_version.extension_name.as_ptr())
                        .to_str()
                } {
                    Ok(name) if !name.is_empty() => name.to_owned(),
                    _ => {
                        last_blockers = vec![VulkanVideoProbeBlocker::InvalidStdHeaderVersion];
                        continue;
                    }
                };
                let encode_input_format = match unsafe {
                    h264_video_format(
                        physical_device,
                        get_video_formats,
                        VK_IMAGE_USAGE_VIDEO_ENCODE_SRC_BIT_KHR | VK_IMAGE_USAGE_TRANSFER_DST_BIT,
                    )
                } {
                    Ok(format) => format,
                    Err(blocker) => {
                        last_blockers = vec![blocker];
                        continue;
                    }
                };
                let dpb_format = match unsafe {
                    h264_video_format(
                        physical_device,
                        get_video_formats,
                        VK_IMAGE_USAGE_VIDEO_ENCODE_DPB_BIT_KHR,
                    )
                } {
                    Ok(format) => format,
                    Err(blocker) => {
                        last_blockers = vec![blocker];
                        continue;
                    }
                };
                return VulkanVideoH264Probe {
                    candidate: Some(VulkanVideoH264Candidate {
                        physical_device_index: u8::try_from(device_index).unwrap_or(u8::MAX),
                        queue_family_index,
                        compute_queue_family_index,
                        encode_input_format,
                        dpb_format,
                        min_coded_width: capability.video.min_coded_extent.width,
                        min_coded_height: capability.video.min_coded_extent.height,
                        max_coded_width: capability.video.max_coded_extent.width,
                        max_coded_height: capability.video.max_coded_extent.height,
                        min_bitstream_offset_alignment: capability
                            .video
                            .min_bitstream_buffer_offset_alignment,
                        min_bitstream_size_alignment: capability
                            .video
                            .min_bitstream_buffer_size_alignment,
                        max_bitrate: capability.encode.max_bitrate,
                        max_quality_levels: capability.encode.max_quality_levels,
                        supported_encode_feedback_flags: capability
                            .encode
                            .supported_encode_feedback_flags,
                        min_qp: capability.h264.min_qp,
                        max_qp: capability.h264.max_qp,
                        max_level_idc: capability.h264.max_level_idc,
                        selected_level_idc,
                        max_dpb_slots: capability.video.max_dpb_slots,
                        max_active_reference_pictures: capability
                            .video
                            .max_active_reference_pictures,
                        separate_reference_images: capability.video.flags
                            & VK_VIDEO_CAPABILITY_SEPARATE_REFERENCE_IMAGES_BIT_KHR
                            != 0,
                        std_header_name,
                        std_header_spec_version: capability.video.std_header_version.spec_version,
                        dma_buf_import_requires_dedicated_allocation,
                    }),
                    blockers: Vec::new(),
                };
            }
            Err(blocker) => last_blockers = vec![blocker],
        }
    }
    if !saw_supported_device {
        return VulkanVideoH264Probe::blocked(VulkanVideoProbeBlocker::NoSupportedPhysicalDevice);
    }
    VulkanVideoH264Probe {
        candidate: None,
        blockers: if last_blockers.is_empty() {
            vec![VulkanVideoProbeBlocker::NoH264EncodeQueue]
        } else {
            last_blockers
        },
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one linear native ownership sequence keeps every failure teardown visible"
)]
unsafe fn open_device(
    request: VulkanVideoH264Request,
    candidate: VulkanVideoH264Candidate,
) -> Result<VulkanVideoH264Device, VulkanVideoDeviceError> {
    let library_name = b"libvulkan.so.1\0";
    // SAFETY: the name is a terminated static byte string.
    let library = DynamicLibrary(unsafe { dlopen(library_name.as_ptr().cast(), 2) } as usize);
    if library.0 == 0 {
        return Err(VulkanVideoDeviceError::LoaderUnavailable);
    }

    macro_rules! load {
        ($symbol:literal, $ty:ty) => {{
            let name = concat!($symbol, "\0");
            // SAFETY: the loader handle is live and the symbol name is terminated.
            let raw = unsafe { dlsym(library.0 as *mut c_void, name.as_ptr().cast()) };
            if raw.is_null() {
                return Err(VulkanVideoDeviceError::LoaderSymbolUnavailable($symbol));
            }
            // SAFETY: the Vulkan symbol fixes this exact function ABI.
            unsafe { mem::transmute::<*mut c_void, $ty>(raw) }
        }};
    }

    let create_instance = load!("vkCreateInstance", CreateInstance);
    let get_instance_proc_addr = load!("vkGetInstanceProcAddr", GetInstanceProcAddr);
    let get_device_proc_addr = load!("vkGetDeviceProcAddr", GetDeviceProcAddr);
    let destroy_instance = load!("vkDestroyInstance", DestroyInstance);
    let enumerate_physical_devices = load!("vkEnumeratePhysicalDevices", EnumeratePhysicalDevices);
    let create_device = load!("vkCreateDevice", CreateDevice);
    let destroy_device = load!("vkDestroyDevice", DestroyDevice);
    let get_device_queue = load!("vkGetDeviceQueue", GetDeviceQueue);

    let application_name = b"Redunar Instant Replay\0";
    let engine_name = b"Redunar\0";
    let application_info = VkApplicationInfo {
        s_type: VK_STRUCTURE_TYPE_APPLICATION_INFO,
        p_next: ptr::null(),
        application_name: application_name.as_ptr().cast(),
        application_version: 1,
        engine_name: engine_name.as_ptr().cast(),
        engine_version: 1,
        api_version: VULKAN_API_VERSION_1_3,
    };
    let instance_info = VkInstanceCreateInfo {
        s_type: VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
        p_next: ptr::null(),
        flags: 0,
        p_application_info: (&raw const application_info).cast(),
        enabled_layer_count: 0,
        enabled_layer_names: ptr::null(),
        enabled_extension_count: 0,
        enabled_extension_names: ptr::null(),
    };
    let mut instance = ptr::null_mut();
    // SAFETY: create info and output storage live for the call.
    let result =
        unsafe { create_instance(&raw const instance_info, ptr::null(), &raw mut instance) };
    if result != VK_SUCCESS || instance.is_null() {
        return Err(VulkanVideoDeviceError::InstanceCreationFailed(result));
    }
    let instance_guard = InstanceGuard {
        instance_address: instance.addr(),
        destroy: destroy_instance,
    };

    let devices = unsafe { physical_devices(instance, enumerate_physical_devices) }
        .map_err(VulkanVideoDeviceError::PhysicalDeviceEnumerationFailed)?;
    let physical_device = devices
        .get(usize::from(candidate.physical_device_index))
        .copied()
        .filter(|device| !device.is_null())
        .ok_or(VulkanVideoDeviceError::SelectedPhysicalDeviceUnavailable)?;

    let mut queue_families = vec![candidate.compute_queue_family_index];
    if candidate.queue_family_index != candidate.compute_queue_family_index {
        queue_families.push(candidate.queue_family_index);
    }
    let queue_priority = 1.0_f32;
    let queue_infos: Vec<VkDeviceQueueCreateInfo> = queue_families
        .iter()
        .map(|family| VkDeviceQueueCreateInfo {
            s_type: VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            queue_family_index: *family,
            queue_count: 1,
            queue_priorities: &raw const queue_priority,
        })
        .collect();
    let extension_names: Vec<*const c_char> = REQUIRED_DEVICE_EXTENSION_NAMES
        .iter()
        .map(|name| name.as_ptr().cast())
        .collect();
    let synchronization2 = VkPhysicalDeviceSynchronization2Features {
        s_type: VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SYNCHRONIZATION_2_FEATURES,
        p_next: ptr::null_mut(),
        synchronization2: 1,
    };
    let device_info = VkDeviceCreateInfo {
        s_type: VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
        p_next: (&raw const synchronization2).cast(),
        flags: 0,
        queue_create_info_count: u32::try_from(queue_infos.len()).unwrap_or(0),
        queue_create_infos: queue_infos.as_ptr(),
        enabled_layer_count: 0,
        enabled_layer_names: ptr::null(),
        enabled_extension_count: u32::try_from(extension_names.len()).unwrap_or(0),
        enabled_extension_names: extension_names.as_ptr(),
        enabled_features: ptr::null(),
    };
    let mut device = ptr::null_mut();
    // SAFETY: queue and extension arrays remain valid through device creation.
    let result = unsafe {
        create_device(
            physical_device,
            &raw const device_info,
            ptr::null(),
            &raw mut device,
        )
    };
    if result != VK_SUCCESS || device.is_null() {
        return Err(VulkanVideoDeviceError::LogicalDeviceCreationFailed(result));
    }

    let mut compute_queue = ptr::null_mut();
    let mut encode_queue = ptr::null_mut();
    // SAFETY: each family was enabled with queue index zero on this device.
    unsafe {
        get_device_queue(
            device,
            candidate.compute_queue_family_index,
            0,
            &raw mut compute_queue,
        );
        get_device_queue(
            device,
            candidate.queue_family_index,
            0,
            &raw mut encode_queue,
        );
    }
    if compute_queue.is_null() || encode_queue.is_null() {
        // SAFETY: the device was created successfully and owns no child
        // resources yet, so immediate teardown is valid.
        unsafe { destroy_device(device, ptr::null()) };
        return Err(VulkanVideoDeviceError::QueueUnavailable);
    }

    mem::forget(instance_guard);
    Ok(VulkanVideoH264Device {
        _library: library,
        instance_address: instance.addr(),
        device_address: device.addr(),
        physical_device_address: physical_device.addr(),
        compute_queue_address: compute_queue.addr(),
        encode_queue_address: encode_queue.addr(),
        destroy_instance,
        destroy_device,
        get_instance_proc_addr,
        get_device_proc_addr,
        request,
        candidate,
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "the transaction keeps video-session creation and every rollback step together"
)]
unsafe fn create_video_session(
    device: VulkanVideoH264Device,
) -> Result<VulkanVideoH264Session, VulkanVideoDeviceError> {
    macro_rules! load {
        ($symbol:literal, $ty:ty) => {{
            let name = concat!($symbol, "\0");
            let Some(raw) = (unsafe { resolve_device_command(&device, name.as_ptr().cast()) })
            else {
                return Err(VulkanVideoDeviceError::DeviceSymbolUnavailable($symbol));
            };
            // SAFETY: the Vulkan command name fixes this exact ABI type.
            unsafe { mem::transmute::<unsafe extern "system" fn(), $ty>(raw) }
        }};
    }

    let create_session = load!("vkCreateVideoSessionKHR", CreateVideoSession);
    let destroy_session = load!("vkDestroyVideoSessionKHR", DestroyVideoSession);
    let get_memory_requirements = load!(
        "vkGetVideoSessionMemoryRequirementsKHR",
        GetVideoSessionMemoryRequirements
    );
    let bind_session_memory = load!("vkBindVideoSessionMemoryKHR", BindVideoSessionMemory);
    let allocate_memory = load!("vkAllocateMemory", AllocateMemory);
    let free_memory = load!("vkFreeMemory", FreeMemory);
    let get_memory_properties = load!(
        "vkGetPhysicalDeviceMemoryProperties",
        GetPhysicalDeviceMemoryProperties
    );

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
    let h264_session_info = VkVideoEncodeH264SessionCreateInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_SESSION_CREATE_INFO_KHR,
        p_next: ptr::null(),
        use_max_level_idc: u32::from(device.candidate.max_level_idc != 0),
        max_level_idc: device.candidate.selected_level_idc,
    };
    let mut std_header_version = VkExtensionProperties::default();
    let header_name = device.candidate.std_header_name.as_bytes();
    if header_name.is_empty() || header_name.len() >= std_header_version.extension_name.len() {
        return Err(VulkanVideoDeviceError::Probe(vec![
            VulkanVideoProbeBlocker::InvalidStdHeaderVersion,
        ]));
    }
    for (target, source) in std_header_version
        .extension_name
        .iter_mut()
        .zip(header_name.iter().copied())
    {
        *target = source.cast_signed();
    }
    std_header_version.spec_version = device.candidate.std_header_spec_version;
    let session_info = VkVideoSessionCreateInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_SESSION_CREATE_INFO_KHR,
        p_next: (&raw const h264_session_info).cast(),
        queue_family_index: device.candidate.queue_family_index,
        flags: 0,
        video_profile: &raw const profile,
        picture_format: device.candidate.encode_input_format,
        max_coded_extent: VkExtent2d {
            width: device.request.width,
            height: device.request.height,
        },
        reference_picture_format: device.candidate.dpb_format,
        max_dpb_slots: device.candidate.max_dpb_slots.min(2),
        max_active_reference_pictures: device.candidate.max_active_reference_pictures.min(1),
        std_header_version: &raw const std_header_version,
    };
    let raw_device = device.device_address as VkDevice;
    let mut session = 0_u64;
    // SAFETY: the complete profile/header chains and output storage live for the call.
    let result = unsafe {
        create_session(
            raw_device,
            &raw const session_info,
            ptr::null(),
            &raw mut session,
        )
    };
    if result != VK_SUCCESS || session == 0 {
        return Err(VulkanVideoDeviceError::VideoSessionCreationFailed(result));
    }
    let mut build = VideoSessionBuildGuard {
        device: raw_device,
        session,
        memories: Vec::new(),
        destroy_session,
        free_memory,
        active: true,
    };

    let mut requirement_count = 0_u32;
    // SAFETY: the session is live and count storage is valid.
    let result = unsafe {
        get_memory_requirements(
            raw_device,
            session,
            &raw mut requirement_count,
            ptr::null_mut(),
        )
    };
    if result != VK_SUCCESS {
        return Err(VulkanVideoDeviceError::VideoSessionMemoryQueryFailed(
            result,
        ));
    }
    let requirement_count = usize::try_from(requirement_count).unwrap_or(usize::MAX);
    if requirement_count > MAX_VIDEO_SESSION_MEMORY_BINDS {
        return Err(VulkanVideoDeviceError::VideoSessionMemoryBoundExceeded);
    }
    let mut requirements: Vec<VkVideoSessionMemoryRequirementsKhr> = (0..requirement_count)
        .map(|_| VkVideoSessionMemoryRequirementsKhr {
            s_type: VK_STRUCTURE_TYPE_VIDEO_SESSION_MEMORY_REQUIREMENTS_KHR,
            p_next: ptr::null_mut(),
            memory_bind_index: 0,
            memory_requirements: VkMemoryRequirements {
                size: 0,
                alignment: 0,
                memory_type_bits: 0,
            },
        })
        .collect();
    let mut output_count = u32::try_from(requirement_count).unwrap_or(0);
    // SAFETY: the bounded output vector is initialized with the required sType values.
    let result = unsafe {
        get_memory_requirements(
            raw_device,
            session,
            &raw mut output_count,
            requirements.as_mut_ptr(),
        )
    };
    if result != VK_SUCCESS
        || usize::try_from(output_count).unwrap_or(usize::MAX) > requirement_count
    {
        return Err(VulkanVideoDeviceError::VideoSessionMemoryQueryFailed(
            result,
        ));
    }
    requirements.truncate(usize::try_from(output_count).unwrap_or(0));

    let mut memory_properties = VkPhysicalDeviceMemoryProperties::default();
    // SAFETY: the physical device belongs to this instance and output storage is valid.
    unsafe {
        get_memory_properties(
            device.physical_device_address as VkPhysicalDevice,
            &raw mut memory_properties,
        );
    }
    for requirement in &requirements {
        let memory_type_index = device_local_memory_type(
            &memory_properties,
            requirement.memory_requirements.memory_type_bits,
        )
        .ok_or(VulkanVideoDeviceError::DeviceLocalMemoryUnavailable)?;
        let allocation_info = VkMemoryAllocateInfo {
            s_type: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
            p_next: ptr::null(),
            allocation_size: requirement.memory_requirements.size,
            memory_type_index,
        };
        let mut memory = 0;
        // SAFETY: allocation info selects a compatible device-local memory type.
        let result = unsafe {
            allocate_memory(
                raw_device,
                &raw const allocation_info,
                ptr::null(),
                &raw mut memory,
            )
        };
        if result != VK_SUCCESS || memory == 0 {
            return Err(VulkanVideoDeviceError::VideoSessionMemoryAllocationFailed(
                result,
            ));
        }
        build.memories.push(memory);
    }
    let binds: Vec<VkBindVideoSessionMemoryInfoKhr> = requirements
        .iter()
        .zip(build.memories.iter().copied())
        .map(|(requirement, memory)| VkBindVideoSessionMemoryInfoKhr {
            s_type: VK_STRUCTURE_TYPE_BIND_VIDEO_SESSION_MEMORY_INFO_KHR,
            p_next: ptr::null(),
            memory_bind_index: requirement.memory_bind_index,
            memory,
            memory_offset: 0,
            memory_size: requirement.memory_requirements.size,
        })
        .collect();
    // SAFETY: every bind references a live compatible allocation and the full
    // driver-requested bind set is submitted atomically.
    let result = unsafe {
        bind_session_memory(
            raw_device,
            session,
            u32::try_from(binds.len()).unwrap_or(0),
            binds.as_ptr(),
        )
    };
    if result != VK_SUCCESS {
        return Err(VulkanVideoDeviceError::VideoSessionMemoryBindFailed(result));
    }

    let session_memory = build.disarm();
    drop(build);
    Ok(VulkanVideoH264Session {
        device,
        session,
        session_memory,
        destroy_session,
        free_memory,
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "the SPS/PPS values and driver retrieval must remain one auditable transaction"
)]
unsafe fn create_h264_parameters(
    session: VulkanVideoH264Session,
) -> Result<VulkanVideoH264Parameters, VulkanVideoDeviceError> {
    macro_rules! load {
        ($symbol:literal, $ty:ty) => {{
            let name = concat!($symbol, "\0");
            let Some(raw) =
                (unsafe { resolve_device_command(&session.device, name.as_ptr().cast()) })
            else {
                return Err(VulkanVideoDeviceError::DeviceSymbolUnavailable($symbol));
            };
            // SAFETY: the Vulkan command name fixes this exact ABI type.
            unsafe { mem::transmute::<unsafe extern "system" fn(), $ty>(raw) }
        }};
    }
    let create_parameters = load!(
        "vkCreateVideoSessionParametersKHR",
        CreateVideoSessionParameters
    );
    let destroy_parameters = load!(
        "vkDestroyVideoSessionParametersKHR",
        DestroyVideoSessionParameters
    );
    let get_encoded_parameters = load!(
        "vkGetEncodedVideoSessionParametersKHR",
        GetEncodedVideoSessionParameters
    );

    let request = session.request();
    let macroblock_width = request.width.div_ceil(16);
    let macroblock_height = request.height.div_ceil(16);
    let coded_width = macroblock_width.saturating_mul(16);
    let coded_height = macroblock_height.saturating_mul(16);
    let crop_right = coded_width.saturating_sub(request.width) / 2;
    let crop_bottom = coded_height.saturating_sub(request.height) / 2;
    let frame_cropping = crop_right != 0 || crop_bottom != 0;

    let vui = StdVideoH264SequenceParameterSetVui {
        // Packet timestamps carry VFR pacing; only fixed modes declare a
        // constant H.264 timing relationship in the VUI.
        flags: (1 << 0)
            | (1 << 3)
            | (1 << 5)
            | (1 << 7)
            | (u32::from(!request.variable_rate) << 8)
            | (1 << 9),
        aspect_ratio_idc: 1,
        sar_width: 0,
        sar_height: 0,
        video_format: 5,
        colour_primaries: 1,
        transfer_characteristics: 1,
        matrix_coefficients: 1,
        num_units_in_tick: 1,
        time_scale: u32::from(request.frames_per_second).saturating_mul(2),
        max_num_reorder_frames: 0,
        max_dec_frame_buffering: 1,
        chroma_sample_loc_type_top_field: 0,
        chroma_sample_loc_type_bottom_field: 0,
        reserved1: 0,
        hrd_parameters: ptr::null(),
    };
    let sps = StdVideoH264SequenceParameterSet {
        // direct_8x8_inference, frame_mbs_only, optional cropping, and VUI.
        flags: (1 << 6) | (1 << 8) | (u32::from(frame_cropping) << 13) | (1 << 15),
        profile_idc: STD_VIDEO_H264_PROFILE_IDC_HIGH,
        level_idc: session.device.candidate.selected_level_idc,
        chroma_format_idc: 1,
        seq_parameter_set_id: 0,
        bit_depth_luma_minus8: 0,
        bit_depth_chroma_minus8: 0,
        log2_max_frame_num_minus4: 4,
        pic_order_cnt_type: 0,
        offset_for_non_ref_pic: 0,
        offset_for_top_to_bottom_field: 0,
        log2_max_pic_order_cnt_lsb_minus4: 4,
        num_ref_frames_in_pic_order_cnt_cycle: 0,
        max_num_ref_frames: 1,
        reserved1: 0,
        pic_width_in_mbs_minus1: macroblock_width.saturating_sub(1),
        pic_height_in_map_units_minus1: macroblock_height.saturating_sub(1),
        frame_crop_left_offset: 0,
        frame_crop_right_offset: crop_right,
        frame_crop_top_offset: 0,
        frame_crop_bottom_offset: crop_bottom,
        reserved2: 0,
        offset_for_ref_frame: ptr::null(),
        scaling_lists: ptr::null(),
        sequence_parameter_set_vui: &raw const vui,
    };
    let pps = StdVideoH264PictureParameterSet {
        // Deblocking-filter control is present; CAVLC and default scaling keep
        // the first implementation bounded and broadly supported.
        flags: 1 << 3,
        seq_parameter_set_id: 0,
        pic_parameter_set_id: 0,
        num_ref_idx_l0_default_active_minus1: 0,
        num_ref_idx_l1_default_active_minus1: 0,
        weighted_bipred_idc: 0,
        pic_init_qp_minus26: 0,
        pic_init_qs_minus26: 0,
        chroma_qp_index_offset: 0,
        second_chroma_qp_index_offset: 0,
        scaling_lists: ptr::null(),
    };
    let add_info = VkVideoEncodeH264SessionParametersAddInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_SESSION_PARAMETERS_ADD_INFO_KHR,
        p_next: ptr::null(),
        std_sps_count: 1,
        std_sps: &raw const sps,
        std_pps_count: 1,
        std_pps: &raw const pps,
    };
    let h264_create_info = VkVideoEncodeH264SessionParametersCreateInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_SESSION_PARAMETERS_CREATE_INFO_KHR,
        p_next: ptr::null(),
        max_std_sps_count: 1,
        max_std_pps_count: 1,
        parameters_add_info: &raw const add_info,
    };
    let quality_level_info = VkVideoEncodeQualityLevelInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_QUALITY_LEVEL_INFO_KHR,
        p_next: (&raw const h264_create_info).cast(),
        quality_level: request.quality_level(session.device.candidate.max_quality_levels),
    };
    let create_info = VkVideoSessionParametersCreateInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_SESSION_PARAMETERS_CREATE_INFO_KHR,
        p_next: (&raw const quality_level_info).cast(),
        flags: 0,
        video_session_parameters_template: 0,
        video_session: session.session,
    };
    let raw_device = session.device.device_address as VkDevice;
    let mut parameters = 0;
    // SAFETY: the Redunar-authored SPS/PPS and all pNext nodes live for this call.
    let result = unsafe {
        create_parameters(
            raw_device,
            &raw const create_info,
            ptr::null(),
            &raw mut parameters,
        )
    };
    if result != VK_SUCCESS || parameters == 0 {
        return Err(VulkanVideoDeviceError::SessionParameterCreationFailed(
            result,
        ));
    }
    let mut output = VulkanVideoH264Parameters {
        session,
        parameters,
        encoded_parameters: Box::default(),
        destroy_parameters,
    };

    let h264_get = VkVideoEncodeH264SessionParametersGetInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_SESSION_PARAMETERS_GET_INFO_KHR,
        p_next: ptr::null(),
        write_std_sps: 1,
        write_std_pps: 1,
        std_sps_id: 0,
        std_pps_id: 0,
    };
    let get_info = VkVideoEncodeSessionParametersGetInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_SESSION_PARAMETERS_GET_INFO_KHR,
        p_next: (&raw const h264_get).cast(),
        video_session_parameters: parameters,
    };
    let mut h264_feedback = VkVideoEncodeH264SessionParametersFeedbackInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_SESSION_PARAMETERS_FEEDBACK_INFO_KHR,
        p_next: ptr::null_mut(),
        has_std_sps_overrides: 0,
        has_std_pps_overrides: 0,
    };
    let mut feedback = VkVideoEncodeSessionParametersFeedbackInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_SESSION_PARAMETERS_FEEDBACK_INFO_KHR,
        p_next: (&raw mut h264_feedback).cast(),
        has_overrides: 0,
    };
    let mut byte_count = 0_usize;
    // SAFETY: the parameter object is live and the first call queries only size.
    let result = unsafe {
        get_encoded_parameters(
            raw_device,
            &raw const get_info,
            &raw mut feedback,
            &raw mut byte_count,
            ptr::null_mut(),
        )
    };
    if result != VK_SUCCESS {
        return Err(VulkanVideoDeviceError::SessionParameterReadFailed(result));
    }
    if byte_count == 0 || byte_count > MAX_H264_PARAMETER_BYTES {
        return Err(VulkanVideoDeviceError::SessionParameterBytesExceeded);
    }
    let mut bytes = vec![0_u8; byte_count];
    // SAFETY: `bytes` has the exact bounded capacity reported by the driver.
    let result = unsafe {
        get_encoded_parameters(
            raw_device,
            &raw const get_info,
            &raw mut feedback,
            &raw mut byte_count,
            bytes.as_mut_ptr().cast(),
        )
    };
    if result != VK_SUCCESS || byte_count == 0 || byte_count > bytes.len() {
        return Err(VulkanVideoDeviceError::SessionParameterReadFailed(result));
    }
    bytes.truncate(byte_count);
    output.encoded_parameters = bytes.into_boxed_slice();
    Ok(output)
}

unsafe fn import_dma_buf(
    owner: &VulkanVideoH264Parameters,
    frame: VulkanVideoDmaBufFrame,
) -> Result<VulkanVideoImportedBuffer, VulkanVideoDeviceError> {
    macro_rules! load {
        ($symbol:literal, $ty:ty) => {{
            let name = concat!($symbol, "\0");
            let Some(raw) =
                (unsafe { resolve_device_command(&owner.session.device, name.as_ptr().cast()) })
            else {
                return Err(VulkanVideoDeviceError::DeviceSymbolUnavailable($symbol));
            };
            // SAFETY: the Vulkan command name fixes this exact ABI type.
            unsafe { mem::transmute::<unsafe extern "system" fn(), $ty>(raw) }
        }};
    }
    let get_fd_properties = load!("vkGetMemoryFdPropertiesKHR", GetMemoryFdProperties);
    let create_buffer = load!("vkCreateBuffer", CreateBuffer);
    let destroy_buffer = load!("vkDestroyBuffer", DestroyBuffer);
    let get_buffer_requirements = load!(
        "vkGetBufferMemoryRequirements2",
        GetBufferMemoryRequirements2
    );
    let allocate_memory = load!("vkAllocateMemory", AllocateMemory);
    let free_memory = load!("vkFreeMemory", FreeMemory);
    let bind_buffer_memory = load!("vkBindBufferMemory", BindBufferMemory);
    let get_memory_properties = load!(
        "vkGetPhysicalDeviceMemoryProperties",
        GetPhysicalDeviceMemoryProperties
    );

    let VulkanVideoDmaBufFrame {
        file,
        width,
        height,
        format,
        offset,
        stride,
        timestamp_ns,
        duration_ns,
        allocation_size,
        input_mode,
        drm_modifier: _,
    } = frame;
    if input_mode != VulkanVideoInputMode::LinearBuffer {
        return Err(VulkanVideoDeviceError::InvalidDmaBufFrame);
    }
    let device = owner.session.device.device_address as VkDevice;
    let raw_fd = file.into_raw_fd();
    let mut fd_properties = VkMemoryFdPropertiesKhr {
        s_type: VK_STRUCTURE_TYPE_MEMORY_FD_PROPERTIES_KHR,
        p_next: ptr::null_mut(),
        memory_type_bits: 0,
    };
    // SAFETY: `raw_fd` is live and still owned locally during this property query.
    let result = unsafe {
        get_fd_properties(
            device,
            VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT,
            raw_fd,
            &raw mut fd_properties,
        )
    };
    if result != VK_SUCCESS {
        // SAFETY: Vulkan did not consume the FD during a property query.
        drop(unsafe { File::from_raw_fd(raw_fd) });
        return Err(VulkanVideoDeviceError::DmaBufPropertiesFailed(result));
    }

    let external_info = VkExternalMemoryBufferCreateInfo {
        s_type: VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_BUFFER_CREATE_INFO,
        p_next: ptr::null(),
        handle_types: VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT,
    };
    let buffer_info = VkBufferCreateInfo {
        s_type: VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
        p_next: (&raw const external_info).cast(),
        flags: 0,
        size: allocation_size,
        usage: VK_BUFFER_USAGE_STORAGE_BUFFER_BIT,
        sharing_mode: VK_SHARING_MODE_EXCLUSIVE,
        queue_family_index_count: 0,
        queue_family_indices: ptr::null(),
    };
    let mut buffer = 0;
    // SAFETY: create info requests one external storage buffer of the DMA-BUF size.
    let result =
        unsafe { create_buffer(device, &raw const buffer_info, ptr::null(), &raw mut buffer) };
    if result != VK_SUCCESS || buffer == 0 {
        // SAFETY: no import occurred, so the local File still owns this FD.
        drop(unsafe { File::from_raw_fd(raw_fd) });
        return Err(VulkanVideoDeviceError::ImportedBufferCreationFailed(result));
    }

    let requirements_info = VkBufferMemoryRequirementsInfo2 {
        s_type: VK_STRUCTURE_TYPE_BUFFER_MEMORY_REQUIREMENTS_INFO_2,
        p_next: ptr::null(),
        buffer,
    };
    let mut dedicated_requirements = VkMemoryDedicatedRequirements {
        s_type: VK_STRUCTURE_TYPE_MEMORY_DEDICATED_REQUIREMENTS,
        p_next: ptr::null_mut(),
        prefers_dedicated_allocation: 0,
        requires_dedicated_allocation: 0,
    };
    let mut requirements = VkMemoryRequirements2 {
        s_type: VK_STRUCTURE_TYPE_MEMORY_REQUIREMENTS_2,
        p_next: (&raw mut dedicated_requirements).cast(),
        memory_requirements: VkMemoryRequirements {
            size: 0,
            alignment: 0,
            memory_type_bits: 0,
        },
    };
    // SAFETY: the newly created buffer and output storage are valid.
    unsafe {
        get_buffer_requirements(device, &raw const requirements_info, &raw mut requirements);
    }
    let requirements = requirements.memory_requirements;
    if requirements.size > allocation_size {
        // SAFETY: no import occurred, so destroy the buffer and close the FD.
        unsafe { destroy_buffer(device, buffer, ptr::null()) };
        drop(unsafe { File::from_raw_fd(raw_fd) });
        return Err(VulkanVideoDeviceError::InvalidDmaBufFrame);
    }
    let mut memory_properties = VkPhysicalDeviceMemoryProperties::default();
    // SAFETY: the physical device and output storage are valid.
    unsafe {
        get_memory_properties(
            owner.session.device.physical_device_address as VkPhysicalDevice,
            &raw mut memory_properties,
        );
    }
    let compatible_bits = requirements.memory_type_bits & fd_properties.memory_type_bits;
    let memory_type_index = compatible_memory_type(&memory_properties, compatible_bits)
        .ok_or_else(|| {
            // SAFETY: no import occurred, so destroy the buffer and close the FD.
            unsafe { destroy_buffer(device, buffer, ptr::null()) };
            // SAFETY: Vulkan has not consumed this FD.
            drop(unsafe { File::from_raw_fd(raw_fd) });
            VulkanVideoDeviceError::ImportedMemoryTypeUnavailable
        })?;
    let import_info = VkImportMemoryFdInfoKhr {
        s_type: VK_STRUCTURE_TYPE_IMPORT_MEMORY_FD_INFO_KHR,
        p_next: ptr::null(),
        handle_type: VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT,
        fd: raw_fd,
    };
    let dedicated_info = VkMemoryDedicatedAllocateInfo {
        s_type: VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO,
        p_next: (&raw const import_info).cast(),
        image: 0,
        buffer,
    };
    let allocation_chain = if owner
        .session
        .device
        .candidate
        .dma_buf_import_requires_dedicated_allocation
        || dedicated_requirements.requires_dedicated_allocation != 0
    {
        (&raw const dedicated_info).cast()
    } else {
        (&raw const import_info).cast()
    };
    let allocation_info = VkMemoryAllocateInfo {
        s_type: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        p_next: allocation_chain,
        allocation_size,
        memory_type_index,
    };
    let mut memory = 0;
    // SAFETY: allocation imports the live FD with a compatible memory type.
    let result = unsafe {
        allocate_memory(
            device,
            &raw const allocation_info,
            ptr::null(),
            &raw mut memory,
        )
    };
    if result != VK_SUCCESS || memory == 0 {
        // Vulkan consumes an imported FD only on successful allocation.
        unsafe { destroy_buffer(device, buffer, ptr::null()) };
        // SAFETY: failed allocation leaves FD ownership with the application.
        drop(unsafe { File::from_raw_fd(raw_fd) });
        return Err(VulkanVideoDeviceError::ImportedMemoryAllocationFailed(
            result,
        ));
    }
    // `raw_fd` ownership transferred to Vulkan on successful allocation.
    // SAFETY: buffer and imported memory belong to this device; offset zero is aligned.
    let result = unsafe { bind_buffer_memory(device, buffer, memory, 0) };
    if result != VK_SUCCESS {
        // SAFETY: free imported memory after destroying the unbound buffer.
        unsafe {
            destroy_buffer(device, buffer, ptr::null());
            free_memory(device, memory, ptr::null());
        }
        return Err(VulkanVideoDeviceError::ImportedBufferBindFailed(result));
    }
    Ok(VulkanVideoImportedBuffer {
        device_address: owner.session.device.device_address,
        resource: VulkanVideoImportedResource::Buffer {
            buffer,
            memory,
            buffer_size: allocation_size,
            destroy_buffer,
            free_memory,
        },
        width,
        height,
        format,
        offset_words: offset / 4,
        stride_words: stride / 4,
        timestamp_ns,
        duration_ns,
    })
}

struct VideoSessionBuildGuard {
    device: VkDevice,
    session: VkVideoSessionKhr,
    memories: Vec<VkDeviceMemory>,
    destroy_session: DestroyVideoSession,
    free_memory: FreeMemory,
    active: bool,
}

impl VideoSessionBuildGuard {
    fn disarm(&mut self) -> Vec<VkDeviceMemory> {
        self.active = false;
        mem::take(&mut self.memories)
    }
}

impl Drop for VideoSessionBuildGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        // SAFETY: the guard owns the partially built session.
        unsafe { (self.destroy_session)(self.device, self.session, ptr::null()) };
        for memory in self.memories.drain(..) {
            // SAFETY: the session was destroyed and each allocation belongs to this device.
            unsafe { (self.free_memory)(self.device, memory, ptr::null()) };
        }
    }
}

fn device_local_memory_type(
    properties: &VkPhysicalDeviceMemoryProperties,
    compatible_bits: u32,
) -> Option<u32> {
    let count = properties.memory_type_count.min(32);
    (0..count).find(|index| {
        compatible_bits & (1_u32 << index) != 0
            && properties.memory_types[usize::try_from(*index).unwrap_or(0)].property_flags
                & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT
                != 0
    })
}

fn compatible_memory_type(
    properties: &VkPhysicalDeviceMemoryProperties,
    compatible_bits: u32,
) -> Option<u32> {
    let count = properties.memory_type_count.min(32);
    let compatible = |index: u32| compatible_bits & (1_u32 << index) != 0;
    (0..count)
        .find(|index| {
            compatible(*index)
                && properties.memory_types[usize::try_from(*index).unwrap_or(0)].property_flags
                    & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT
                    != 0
        })
        .or_else(|| (0..count).find(|index| compatible(*index)))
}

fn required_h264_level(request: VulkanVideoH264Request) -> i32 {
    let macroblock_width = request.width.div_ceil(16);
    let macroblock_height = request.height.div_ceil(16);
    let frame_macroblocks = macroblock_width.saturating_mul(macroblock_height);
    let macroblocks_per_second =
        frame_macroblocks.saturating_mul(u32::from(request.frames_per_second));
    let bitrate = u64::from(request.target_megabits_per_second).saturating_mul(1_000_000);
    // StdVideoH264LevelIdc enum values paired with the H.264 maximum frame
    // size, macroblock processing rate, and High-profile MaxBR. Bitrate is a
    // separate level constraint: a small frame at 40 Mb/s still requires
    // Level 4.1 even when its macroblock rate would fit Level 3.1.
    [
        (8, 3_600, 108_000, 17_500_000_u64),
        (9, 5_120, 216_000, 25_000_000),
        (10, 8_192, 245_760, 25_000_000),
        (11, 8_192, 245_760, 62_500_000),
        (12, 8_704, 522_240, 62_500_000),
        (13, 22_080, 589_824, 168_750_000),
        (14, 36_864, 983_040, 300_000_000),
        (15, 36_864, 2_073_600, 300_000_000),
    ]
    .into_iter()
    .find(|(_, max_frame, max_rate, max_bitrate)| {
        frame_macroblocks <= *max_frame
            && macroblocks_per_second <= *max_rate
            && bitrate <= *max_bitrate
    })
    .map_or(15, |(level, _, _, _)| level)
}

struct InstanceGuard {
    instance_address: usize,
    destroy: DestroyInstance,
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // SAFETY: the guard uniquely owns a successfully created instance.
        unsafe { (self.destroy)(self.instance_address as VkInstance, ptr::null()) };
    }
}

unsafe fn physical_devices(
    instance: VkInstance,
    enumerate: EnumeratePhysicalDevices,
) -> Result<Vec<VkPhysicalDevice>, i32> {
    let mut count = 0_u32;
    // SAFETY: count storage is valid and the first enumeration has no output array.
    let result = unsafe { enumerate(instance, &raw mut count, ptr::null_mut()) };
    if result != VK_SUCCESS {
        return Err(result);
    }
    let count = usize::try_from(count)
        .unwrap_or(MAX_PHYSICAL_DEVICES + 1)
        .min(MAX_PHYSICAL_DEVICES);
    let mut devices = vec![ptr::null_mut(); count];
    let mut output_count = u32::try_from(count).unwrap_or(0);
    // SAFETY: the bounded vector provides output storage for `output_count` handles.
    let result = unsafe { enumerate(instance, &raw mut output_count, devices.as_mut_ptr()) };
    if result != VK_SUCCESS {
        return Err(result);
    }
    devices.truncate(
        usize::try_from(output_count)
            .unwrap_or(0)
            .min(devices.len()),
    );
    Ok(devices)
}

unsafe fn physical_device_api_and_vendor(
    device: VkPhysicalDevice,
    get_properties: GetPhysicalDeviceProperties,
) -> (u32, u32) {
    // VkPhysicalDeviceProperties begins with apiVersion, driverVersion,
    // vendorID, deviceID. The oversized aligned buffer avoids reproducing the
    // large driver-limits tail while preserving the official ABI storage size.
    let mut properties = [0_u64; 512];
    // SAFETY: the aligned 4 KiB buffer exceeds VkPhysicalDeviceProperties.
    unsafe { get_properties(device, properties.as_mut_ptr().cast()) };
    let words = properties.as_ptr().cast::<u32>();
    // SAFETY: apiVersion and vendorID are the first and third u32 values in
    // the documented structure prefix.
    unsafe { (*words, *words.add(2)) }
}

const fn synchronization2_feature_enabled(value: u32) -> bool {
    value != 0
}

unsafe fn physical_device_supports_synchronization2(
    device: VkPhysicalDevice,
    get_features: GetPhysicalDeviceFeatures2,
) -> bool {
    let mut synchronization2 = VkPhysicalDeviceSynchronization2Features {
        s_type: VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SYNCHRONIZATION_2_FEATURES,
        p_next: ptr::null_mut(),
        synchronization2: 0,
    };
    let mut features = VkPhysicalDeviceFeatures2 {
        s_type: VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2,
        p_next: (&raw mut synchronization2).cast(),
        features: [0; VK_PHYSICAL_DEVICE_FEATURE_COUNT],
    };
    // SAFETY: the physical device and complete writable feature chain are valid.
    unsafe { get_features(device, &raw mut features) };
    synchronization2_feature_enabled(synchronization2.synchronization2)
}

unsafe fn device_extensions(
    device: VkPhysicalDevice,
    enumerate: EnumerateDeviceExtensionProperties,
) -> Result<Vec<String>, i32> {
    let mut count = 0_u32;
    // SAFETY: count storage is valid and no layer name requests global device extensions.
    let result = unsafe { enumerate(device, ptr::null(), &raw mut count, ptr::null_mut()) };
    if result != VK_SUCCESS {
        return Err(result);
    }
    let count = usize::try_from(count)
        .unwrap_or(MAX_DEVICE_EXTENSIONS + 1)
        .min(MAX_DEVICE_EXTENSIONS);
    let mut properties: Vec<VkExtensionProperties> =
        std::iter::repeat_with(VkExtensionProperties::default)
            .take(count)
            .collect();
    let mut output_count = u32::try_from(count).unwrap_or(0);
    // SAFETY: the bounded vector provides storage for `output_count` records.
    let result = unsafe {
        enumerate(
            device,
            ptr::null(),
            &raw mut output_count,
            properties.as_mut_ptr(),
        )
    };
    if result != VK_SUCCESS {
        return Err(result);
    }
    properties.truncate(
        usize::try_from(output_count)
            .unwrap_or(0)
            .min(properties.len()),
    );
    Ok(properties
        .iter()
        .filter_map(|property| {
            // SAFETY: Vulkan guarantees a terminated extensionName array.
            unsafe { CStr::from_ptr(property.extension_name.as_ptr()) }
                .to_str()
                .ok()
                .map(str::to_owned)
        })
        .collect())
}

fn dma_buf_import_requirement(properties: VkExternalMemoryProperties) -> Option<bool> {
    let features = properties.external_memory_features;
    let compatible = properties.compatible_handle_types;
    (features & VK_EXTERNAL_MEMORY_FEATURE_IMPORTABLE_BIT != 0
        && compatible & VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT != 0)
        .then_some(features & VK_EXTERNAL_MEMORY_FEATURE_DEDICATED_ONLY_BIT != 0)
}

unsafe fn dma_buf_storage_buffer_import_support(
    device: VkPhysicalDevice,
    get_properties: GetPhysicalDeviceExternalBufferProperties,
) -> Option<bool> {
    let info = VkPhysicalDeviceExternalBufferInfo {
        s_type: VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_EXTERNAL_BUFFER_INFO,
        p_next: ptr::null(),
        flags: 0,
        usage: VK_BUFFER_USAGE_STORAGE_BUFFER_BIT,
        handle_type: VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT,
    };
    let mut properties = VkExternalBufferProperties {
        s_type: VK_STRUCTURE_TYPE_EXTERNAL_BUFFER_PROPERTIES,
        p_next: ptr::null_mut(),
        external_memory_properties: VkExternalMemoryProperties::default(),
    };
    // SAFETY: the physical device and complete input/output records are valid.
    unsafe { get_properties(device, &raw const info, &raw mut properties) };
    dma_buf_import_requirement(properties.external_memory_properties)
}

unsafe fn h264_encode_queues(
    device: VkPhysicalDevice,
    get_properties: GetQueueFamilyProperties2,
) -> Option<(u32, u32)> {
    let mut count = 0_u32;
    // SAFETY: count storage is valid and the first query has no output array.
    unsafe { get_properties(device, &raw mut count, ptr::null_mut()) };
    let count = usize::try_from(count)
        .unwrap_or(MAX_QUEUE_FAMILIES + 1)
        .min(MAX_QUEUE_FAMILIES);
    if count == 0 {
        return None;
    }
    let mut video: Vec<VkQueueFamilyVideoPropertiesKhr> = (0..count)
        .map(|_| VkQueueFamilyVideoPropertiesKhr {
            s_type: VK_STRUCTURE_TYPE_QUEUE_FAMILY_VIDEO_PROPERTIES_KHR,
            p_next: ptr::null_mut(),
            video_codec_operations: 0,
        })
        .collect();
    let mut properties: Vec<VkQueueFamilyProperties2> = video
        .iter_mut()
        .map(|video| VkQueueFamilyProperties2 {
            s_type: VK_STRUCTURE_TYPE_QUEUE_FAMILY_PROPERTIES_2,
            p_next: ptr::from_mut(video).cast(),
            queue_family_properties: VkQueueFamilyProperties {
                queue_flags: 0,
                queue_count: 0,
                timestamp_valid_bits: 0,
                min_image_transfer_granularity: VkExtent3d {
                    width: 0,
                    height: 0,
                    depth: 0,
                },
            },
        })
        .collect();
    let mut output_count = u32::try_from(count).unwrap_or(0);
    // SAFETY: every pNext points into `video`, which cannot move during the call.
    unsafe { get_properties(device, &raw mut output_count, properties.as_mut_ptr()) };
    let encode = video.iter().enumerate().find_map(|(index, capability)| {
        let queue = &properties[index].queue_family_properties;
        (queue.queue_count > 0
            && queue.queue_flags & VK_QUEUE_VIDEO_ENCODE_BIT_KHR != 0
            && capability.video_codec_operations
                & u32::try_from(VK_VIDEO_CODEC_OPERATION_ENCODE_H264_BIT_KHR).unwrap_or(0)
                != 0)
            .then(|| u32::try_from(index).ok())
            .flatten()
    })?;
    let compute = properties
        .iter()
        .enumerate()
        .filter(|(_, property)| {
            property.queue_family_properties.queue_count > 0
                && property.queue_family_properties.queue_flags & VK_QUEUE_COMPUTE_BIT != 0
        })
        .min_by_key(|(index, _)| u32::from(u32::try_from(*index).ok() != Some(encode)))
        .and_then(|(index, _)| u32::try_from(index).ok())?;
    Some((encode, compute))
}

unsafe fn h264_video_format(
    device: VkPhysicalDevice,
    get_formats: GetPhysicalDeviceVideoFormatProperties,
    image_usage: u32,
) -> Result<i32, VulkanVideoProbeBlocker> {
    let h264_profile = VkVideoEncodeH264ProfileInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_PROFILE_INFO_KHR,
        p_next: ptr::null(),
        std_profile_idc: STD_VIDEO_H264_PROFILE_IDC_HIGH,
    };
    let profile = VkVideoProfileInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_PROFILE_INFO_KHR,
        p_next: (&raw const h264_profile).cast(),
        video_codec_operation: VK_VIDEO_CODEC_OPERATION_ENCODE_H264_BIT_KHR,
        chroma_subsampling: VK_VIDEO_CHROMA_SUBSAMPLING_420_BIT_KHR,
        luma_bit_depth: VK_VIDEO_COMPONENT_BIT_DEPTH_8_BIT_KHR,
        chroma_bit_depth: VK_VIDEO_COMPONENT_BIT_DEPTH_8_BIT_KHR,
    };
    let profile_list = VkVideoProfileListInfoKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_PROFILE_LIST_INFO_KHR,
        p_next: ptr::null(),
        profile_count: 1,
        profiles: &raw const profile,
    };
    let format_info = VkPhysicalDeviceVideoFormatInfoKhr {
        s_type: VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VIDEO_FORMAT_INFO_KHR,
        p_next: (&raw const profile_list).cast(),
        image_usage,
    };
    let mut count = 0_u32;
    // SAFETY: the complete profile chain and count output live for the query.
    let result = unsafe {
        get_formats(
            device,
            &raw const format_info,
            &raw mut count,
            ptr::null_mut(),
        )
    };
    if result != VK_SUCCESS {
        return Err(VulkanVideoProbeBlocker::VideoFormatQueryFailed(result));
    }
    let count = usize::try_from(count).unwrap_or(0).min(32);
    let mut formats: Vec<VkVideoFormatPropertiesKhr> = (0..count)
        .map(|_| VkVideoFormatPropertiesKhr {
            s_type: VK_STRUCTURE_TYPE_VIDEO_FORMAT_PROPERTIES_KHR,
            p_next: ptr::null_mut(),
            format: 0,
            component_mapping: VkComponentMapping {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
            image_create_flags: 0,
            image_type: 0,
            image_tiling: 0,
            image_usage_flags: 0,
        })
        .collect();
    let mut output_count = u32::try_from(count).unwrap_or(0);
    // SAFETY: the bounded vector provides output storage for every requested format.
    let result = unsafe {
        get_formats(
            device,
            &raw const format_info,
            &raw mut output_count,
            formats.as_mut_ptr(),
        )
    };
    if result != VK_SUCCESS {
        return Err(VulkanVideoProbeBlocker::VideoFormatQueryFailed(result));
    }
    formats
        .into_iter()
        .take(usize::try_from(output_count).unwrap_or(0))
        .find(|format| format.format == VK_FORMAT_G8_B8R8_2PLANE_420_UNORM)
        .map(|format| format.format)
        .ok_or(VulkanVideoProbeBlocker::Nv12VideoFormatUnsupported)
}

struct H264CapabilityChain {
    video: VkVideoCapabilitiesKhr,
    encode: Box<VkVideoEncodeCapabilitiesKhr>,
    h264: Box<VkVideoEncodeH264CapabilitiesKhr>,
}

unsafe fn h264_capabilities(
    device: VkPhysicalDevice,
    request: VulkanVideoH264Request,
    get_capabilities: GetPhysicalDeviceVideoCapabilities,
) -> Result<H264CapabilityChain, VulkanVideoProbeBlocker> {
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
    let mut encode = Box::new(VkVideoEncodeCapabilitiesKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_CAPABILITIES_KHR,
        p_next: ptr::null_mut(),
        flags: 0,
        rate_control_modes: 0,
        max_rate_control_layers: 0,
        max_bitrate: 0,
        max_quality_levels: 0,
        encode_input_picture_granularity: VkExtent2d {
            width: 0,
            height: 0,
        },
        supported_encode_feedback_flags: 0,
    });
    let mut h264 = Box::new(VkVideoEncodeH264CapabilitiesKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_ENCODE_H264_CAPABILITIES_KHR,
        p_next: (&raw mut *encode).cast(),
        flags: 0,
        max_level_idc: 0,
        max_slice_count: 0,
        max_p_picture_l0_reference_count: 0,
        max_b_picture_l0_reference_count: 0,
        max_l1_reference_count: 0,
        max_temporal_layer_count: 0,
        expect_dyadic_temporal_layer_pattern: 0,
        min_qp: 0,
        max_qp: 0,
        prefers_gop_remaining_frames: 0,
        requires_gop_remaining_frames: 0,
        std_syntax_flags: 0,
    });
    let mut video = VkVideoCapabilitiesKhr {
        s_type: VK_STRUCTURE_TYPE_VIDEO_CAPABILITIES_KHR,
        p_next: (&raw mut *h264).cast(),
        flags: 0,
        min_bitstream_buffer_offset_alignment: 0,
        min_bitstream_buffer_size_alignment: 0,
        picture_access_granularity: VkExtent2d {
            width: 0,
            height: 0,
        },
        min_coded_extent: VkExtent2d {
            width: 0,
            height: 0,
        },
        max_coded_extent: VkExtent2d {
            width: 0,
            height: 0,
        },
        max_dpb_slots: 0,
        max_active_reference_pictures: 0,
        std_header_version: VkExtensionProperties::default(),
    };
    // SAFETY: the profile and complete output pNext chain live for the call.
    let result = unsafe { get_capabilities(device, &raw const profile, &raw mut video) };
    if result != VK_SUCCESS {
        return Err(VulkanVideoProbeBlocker::H264ProfileUnsupported(result));
    }
    if request.width < video.min_coded_extent.width
        || request.height < video.min_coded_extent.height
        || request.width > video.max_coded_extent.width
        || request.height > video.max_coded_extent.height
    {
        return Err(VulkanVideoProbeBlocker::CodedExtentUnsupported);
    }
    Ok(H264CapabilityChain {
        video,
        encode,
        h264,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send<T: Send>() {}

    #[test]
    fn request_limits_match_replay_product_limits() {
        assert!(
            VulkanVideoH264Request {
                width: 3_840,
                height: 2_160,
                frames_per_second: 60,
                variable_rate: false,
                target_megabits_per_second: 24,
            }
            .is_valid()
        );
        assert!(
            !VulkanVideoH264Request {
                width: 7_680,
                height: 4_320,
                frames_per_second: 60,
                variable_rate: false,
                target_megabits_per_second: 24,
            }
            .is_valid()
        );
        assert!(
            VulkanVideoH264Request {
                width: 1_920,
                height: 1_080,
                frames_per_second: 120,
                variable_rate: false,
                target_megabits_per_second: 24,
            }
            .is_valid()
        );
        assert!(
            !VulkanVideoH264Request {
                width: 3_840,
                height: 2_160,
                frames_per_second: 120,
                variable_rate: false,
                target_megabits_per_second: 24,
            }
            .is_valid()
        );
        assert!(
            !VulkanVideoH264Request {
                width: 1_919,
                height: 1_080,
                frames_per_second: 60,
                variable_rate: false,
                target_megabits_per_second: 24,
            }
            .is_valid()
        );
    }

    #[test]
    fn replay_presets_select_distinct_available_encoder_quality_levels() {
        let request = |target_megabits_per_second| VulkanVideoH264Request {
            width: 1_920,
            height: 1_080,
            frames_per_second: 60,
            variable_rate: false,
            target_megabits_per_second,
        };
        assert_eq!(request(12).quality_level(4), 0);
        assert_eq!(request(24).quality_level(4), 1);
        assert_eq!(request(40).quality_level(4), 3);
        assert_eq!(request(40).quality_level(1), 0);
    }

    #[test]
    fn level_selection_covers_the_supported_resolution_and_rate_ceiling() {
        assert_eq!(
            required_h264_level(VulkanVideoH264Request {
                width: 1_280,
                height: 720,
                frames_per_second: 30,
                variable_rate: false,
                target_megabits_per_second: 12,
            }),
            8
        );
        assert_eq!(
            required_h264_level(VulkanVideoH264Request {
                width: 1_280,
                height: 720,
                frames_per_second: 30,
                variable_rate: false,
                target_megabits_per_second: 24,
            }),
            9
        );
        assert_eq!(
            required_h264_level(VulkanVideoH264Request {
                width: 1_280,
                height: 720,
                frames_per_second: 30,
                variable_rate: false,
                target_megabits_per_second: 40,
            }),
            11
        );
        assert_eq!(
            required_h264_level(VulkanVideoH264Request {
                width: 1_920,
                height: 1_080,
                frames_per_second: 60,
                variable_rate: false,
                target_megabits_per_second: 24,
            }),
            12
        );
        assert_eq!(
            required_h264_level(VulkanVideoH264Request {
                width: 2_560,
                height: 1_440,
                frames_per_second: 60,
                variable_rate: false,
                target_megabits_per_second: 24,
            }),
            14
        );
        assert_eq!(
            required_h264_level(VulkanVideoH264Request {
                width: 3_840,
                height: 2_160,
                frames_per_second: 60,
                variable_rate: false,
                target_megabits_per_second: 24,
            }),
            15
        );
    }

    #[test]
    fn daemon_owned_native_resources_are_movable_to_one_encoder_worker() {
        assert_send::<VulkanVideoH264Device>();
        assert_send::<VulkanVideoH264Session>();
        assert_send::<VulkanVideoH264Parameters>();
    }

    #[test]
    fn invalid_request_does_not_touch_the_local_vulkan_loader() {
        let probe = VulkanVideoH264Probe::local(VulkanVideoH264Request {
            width: 0,
            height: 1_080,
            frames_per_second: 60,
            variable_rate: false,
            target_megabits_per_second: 24,
        });
        assert_eq!(probe.candidate, None);
        assert_eq!(
            probe.blockers,
            vec![VulkanVideoProbeBlocker::InvalidRequest]
        );
        assert!(!probe.production_ready());
    }

    #[test]
    fn dma_buf_storage_import_requires_importable_and_compatible_flags() {
        let importable = VkExternalMemoryProperties {
            external_memory_features: VK_EXTERNAL_MEMORY_FEATURE_IMPORTABLE_BIT,
            export_from_imported_handle_types: 0,
            compatible_handle_types: VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT,
        };
        assert_eq!(dma_buf_import_requirement(importable), Some(false));

        assert_eq!(
            dma_buf_import_requirement(VkExternalMemoryProperties {
                external_memory_features: 0,
                ..importable
            }),
            None
        );
        assert_eq!(
            dma_buf_import_requirement(VkExternalMemoryProperties {
                compatible_handle_types: 0,
                ..importable
            }),
            None
        );
    }

    #[test]
    fn dma_buf_storage_import_preserves_dedicated_only_requirement() {
        assert_eq!(
            dma_buf_import_requirement(VkExternalMemoryProperties {
                external_memory_features: VK_EXTERNAL_MEMORY_FEATURE_IMPORTABLE_BIT
                    | VK_EXTERNAL_MEMORY_FEATURE_DEDICATED_ONLY_BIT,
                export_from_imported_handle_types: 0,
                compatible_handle_types: VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT,
            }),
            Some(true)
        );
    }

    #[test]
    fn synchronization2_must_be_explicitly_reported() {
        assert!(!synchronization2_feature_enabled(0));
        assert!(synchronization2_feature_enabled(1));
    }
}
