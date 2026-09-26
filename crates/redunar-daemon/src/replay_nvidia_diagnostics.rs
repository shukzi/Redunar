//! Privacy-minimal NVIDIA beta encoder startup failures only.
use redunar_capture_vulkan::replay_video::{VulkanVideoDeviceError, VulkanVideoProbeBlocker};
use std::fmt::Write;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub(super) enum Stage {
    Device,
    Session,
    Parameters,
    Encoder,
    Backend,
}
impl Stage {
    const fn code(self) -> &'static str {
        match self {
            Self::Device => "device",
            Self::Session => "session",
            Self::Parameters => "parameters",
            Self::Encoder => "encoder",
            Self::Backend => "backend",
        }
    }
}

#[derive(Default)]
struct Budget {
    emitted: u8,
    last: Option<Instant>,
}
impl Budget {
    fn admit(&mut self, selected: bool, now: Instant) -> bool {
        if !selected
            || self.emitted >= 32
            || self
                .last
                .is_some_and(|last| now.saturating_duration_since(last) < Duration::from_secs(5))
        {
            return false;
        }
        self.emitted += 1;
        self.last = Some(now);
        true
    }
}
static BUDGET: Mutex<Budget> = Mutex::new(Budget {
    emitted: 0,
    last: None,
});

pub(super) fn startup_result<T>(
    selected: bool,
    stage: Stage,
    result: Result<T, VulkanVideoDeviceError>,
) -> Result<T, String> {
    result.map_err(|error| {
        // Preserve the established non-NVIDIA error contract.
        if !selected {
            return error.to_string();
        }
        let (reason, result) = device_reason(&error);
        emit(selected, stage, reason, result);
        "Replay hardware encoder startup failed".to_owned()
    })
}
pub(super) fn backend_failed(selected: bool) {
    emit(selected, Stage::Backend, "backend_validation_failed", None);
}
fn emit(selected: bool, stage: Stage, reason: &'static str, result: Option<i32>) {
    let Ok(mut budget) = BUDGET.try_lock() else {
        return;
    };
    if !budget.admit(selected, Instant::now()) {
        return;
    }
    drop(budget);
    // This sink is a no-op until the saved logging opt-in starts it. Never use
    // log_op!: that would also expose diagnostics on unconditional stderr.
    crate::diagnostic_log::log(&line(stage, reason, result));
}
fn line(stage: Stage, reason: &'static str, result: Option<i32>) -> String {
    let mut text = format!("NVIDIA Replay stage={} reason={reason}", stage.code());
    if let Some(result) = result {
        let _ = write!(text, " vk_result={result}");
    }
    text
}
// Exhaustive matches deliberately discard all string and non-result payloads.
// New upstream variants require an explicit privacy-reviewed classification.
fn device_reason(error: &VulkanVideoDeviceError) -> (&'static str, Option<i32>) {
    use VulkanVideoDeviceError as E;
    match error {
        E::Probe(blockers) => blockers
            .first()
            .map_or(("probe_failed", None), probe_reason),
        E::LoaderUnavailable => ("loader_unavailable", None),
        E::LoaderSymbolUnavailable(_) => ("loader_symbol_unavailable", None),
        E::InstanceCreationFailed(result) => ("instance_creation_failed", Some(*result)),
        E::PhysicalDeviceEnumerationFailed(result) => {
            ("physical_device_enumeration_failed", Some(*result))
        }
        E::SelectedPhysicalDeviceUnavailable => ("selected_physical_device_unavailable", None),
        E::LogicalDeviceCreationFailed(result) => ("logical_device_creation_failed", Some(*result)),
        E::QueueUnavailable => ("queue_unavailable", None),
        E::DeviceSymbolUnavailable(_) => ("device_symbol_unavailable", None),
        E::VideoSessionCreationFailed(result) => ("video_session_creation_failed", Some(*result)),
        E::VideoSessionMemoryQueryFailed(result) => {
            ("video_session_memory_query_failed", Some(*result))
        }
        E::VideoSessionMemoryBoundExceeded => ("video_session_memory_bound_exceeded", None),
        E::DeviceLocalMemoryUnavailable => ("device_local_memory_unavailable", None),
        E::VideoSessionMemoryAllocationFailed(result) => {
            ("video_session_memory_allocation_failed", Some(*result))
        }
        E::VideoSessionMemoryBindFailed(result) => {
            ("video_session_memory_bind_failed", Some(*result))
        }
        E::SessionParameterCreationFailed(result) => {
            ("session_parameter_creation_failed", Some(*result))
        }
        E::SessionParameterReadFailed(result) => ("session_parameter_read_failed", Some(*result)),
        E::SessionParameterBytesExceeded => ("session_parameter_bytes_exceeded", None),
        E::InvalidDmaBufFrame => ("invalid_dma_buf_frame", None),
        E::DmaBufSizeUnavailable => ("dma_buf_size_unavailable", None),
        E::DmaBufPropertiesFailed(result) => ("dma_buf_properties_failed", Some(*result)),
        E::ImportedBufferCreationFailed(result) => {
            ("imported_buffer_creation_failed", Some(*result))
        }
        E::ImportedMemoryTypeUnavailable => ("imported_memory_type_unavailable", None),
        E::ImportedMemoryAllocationFailed(result) => {
            ("imported_memory_allocation_failed", Some(*result))
        }
        E::ImportedBufferBindFailed(result) => ("imported_buffer_bind_failed", Some(*result)),
        E::InvalidComputeShader => ("invalid_compute_shader", None),
        E::ImageCreationFailed(result) => ("image_creation_failed", Some(*result)),
        E::ImageMemoryTypeUnavailable => ("image_memory_type_unavailable", None),
        E::ImageMemoryAllocationFailed(result) => ("image_memory_allocation_failed", Some(*result)),
        E::ImageMemoryBindFailed(result) => ("image_memory_bind_failed", Some(*result)),
        E::ImageViewCreationFailed(result) => ("image_view_creation_failed", Some(*result)),
        E::SamplerCreationFailed(result) => ("sampler_creation_failed", Some(*result)),
        E::DescriptorSetLayoutCreationFailed(result) => {
            ("descriptor_set_layout_creation_failed", Some(*result))
        }
        E::DescriptorPoolCreationFailed(result) => {
            ("descriptor_pool_creation_failed", Some(*result))
        }
        E::DescriptorSetAllocationFailed(result) => {
            ("descriptor_set_allocation_failed", Some(*result))
        }
        E::PipelineLayoutCreationFailed(result) => {
            ("pipeline_layout_creation_failed", Some(*result))
        }
        E::ShaderModuleCreationFailed(result) => ("shader_module_creation_failed", Some(*result)),
        E::ComputePipelineCreationFailed(result) => {
            ("compute_pipeline_creation_failed", Some(*result))
        }
        E::CommandPoolCreationFailed(result) => ("command_pool_creation_failed", Some(*result)),
        E::CommandBufferAllocationFailed(result) => {
            ("command_buffer_allocation_failed", Some(*result))
        }
        E::SemaphoreCreationFailed(result) => ("semaphore_creation_failed", Some(*result)),
        E::FenceCreationFailed(result) => ("fence_creation_failed", Some(*result)),
        E::DeviceWaitFailed(result) => ("device_wait_failed", Some(*result)),
        E::BitstreamBufferCreationFailed(result) => {
            ("bitstream_buffer_creation_failed", Some(*result))
        }
        E::BitstreamMemoryTypeUnavailable => ("bitstream_memory_type_unavailable", None),
        E::BitstreamMemoryAllocationFailed(result) => {
            ("bitstream_memory_allocation_failed", Some(*result))
        }
        E::BitstreamBufferBindFailed(result) => ("bitstream_buffer_bind_failed", Some(*result)),
        E::BitstreamMapFailed(result) => ("bitstream_map_failed", Some(*result)),
        E::EncodeQueryPoolCreationFailed(result) => {
            ("encode_query_pool_creation_failed", Some(*result))
        }
        E::EncodeFeedbackUnsupported => ("encode_feedback_unsupported", None),
        E::FenceWaitFailed(result) => ("fence_wait_failed", Some(*result)),
        E::FenceResetFailed(result) => ("fence_reset_failed", Some(*result)),
        E::CommandBufferResetFailed(result) => ("command_buffer_reset_failed", Some(*result)),
        E::CommandBufferBeginFailed(result) => ("command_buffer_begin_failed", Some(*result)),
        E::CommandBufferEndFailed(result) => ("command_buffer_end_failed", Some(*result)),
        E::QueueSubmissionFailed(result) => ("queue_submission_failed", Some(*result)),
        E::EncodeFeedbackReadFailed(result) => ("encode_feedback_read_failed", Some(*result)),
        E::EncodedBytesExceeded => ("encoded_bytes_exceeded", None),
        E::EncoderPoisoned => ("encoder_poisoned", None),
    }
}
fn probe_reason(error: &VulkanVideoProbeBlocker) -> (&'static str, Option<i32>) {
    use VulkanVideoProbeBlocker as E;
    match error {
        E::InvalidRequest => ("invalid_request", None),
        E::LoaderUnavailable => ("loader_unavailable", None),
        E::LoaderSymbolUnavailable(_) => ("loader_symbol_unavailable", None),
        E::InstanceCreationFailed(result) => ("instance_creation_failed", Some(*result)),
        E::PhysicalDeviceEnumerationFailed(result) => {
            ("physical_device_enumeration_failed", Some(*result))
        }
        E::NoSupportedPhysicalDevice => ("no_supported_physical_device", None),
        E::Vulkan13Unsupported => ("vulkan13_unsupported", None),
        E::Synchronization2Unsupported => ("synchronization2_unsupported", None),
        E::DeviceExtensionEnumerationFailed(result) => {
            ("device_extension_enumeration_failed", Some(*result))
        }
        E::MissingDeviceExtension(_) => ("missing_device_extension", None),
        E::HardwareH264CodecUnavailable => ("hardware_h264_codec_unavailable", None),
        E::DmaBufStorageBufferImportUnsupported => {
            ("dma_buf_storage_buffer_import_unsupported", None)
        }
        E::QueueFamilyEnumerationFailed => ("queue_family_enumeration_failed", None),
        E::NoH264EncodeQueue => ("no_h264_encode_queue", None),
        E::NoComputeQueue => ("no_compute_queue", None),
        E::H264ProfileUnsupported(result) => ("h264_profile_unsupported", Some(*result)),
        E::H264LevelUnsupported { .. } => ("h264_level_unsupported", None),
        E::CodedExtentUnsupported => ("coded_extent_unsupported", None),
        E::VideoFormatQueryFailed(result) => ("video_format_query_failed", Some(*result)),
        E::Nv12VideoFormatUnsupported => ("nv12_video_format_unsupported", None),
        E::ReferencePicturesUnsupported => ("reference_pictures_unsupported", None),
        E::InvalidStdHeaderVersion => ("invalid_std_header_version", None),
        E::EncodeFeedbackUnsupported => ("encode_feedback_unsupported", None),
        E::RequestedBitrateUnsupported => ("requested_bitrate_unsupported", None),
        E::RateControlUnsupported => ("rate_control_unsupported", None),
        E::PPictureReferenceUnsupported => ("p_picture_reference_unsupported", None),
        E::GopRemainingFramesRequired => ("gop_remaining_frames_required", None),
        E::SeparateReferenceImagesUnsupported => ("separate_reference_images_unsupported", None),
        E::SliceUnsupported => ("slice_unsupported", None),
        E::TemporalLayerUnsupported => ("temporal_layer_unsupported", None),
    }
}
