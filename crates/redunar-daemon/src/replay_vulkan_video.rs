use crate::replay_encoder::MAX_HARDWARE_INPUTS_IN_FLIGHT;
use crate::{
    DmaBufReplayFrame, EncodedPacketBatch, H264ParameterSets, HardwareEncodeOutput,
    HardwareEncoderApi, HardwareEncoderBackend, ReplayEncoderError, ReplayPacketFormat,
    ReplayVideoCodec, ReplayVideoStream, h264_annex_b_access_unit,
};
use redunar_capture_vulkan::replay_video::{
    VulkanPackedPixelFormat, VulkanVideoDeviceError, VulkanVideoDmaBufFrame,
    VulkanVideoEncodedAccessUnit, VulkanVideoH264Encoder,
};
use std::collections::VecDeque;

const DRM_FORMAT_R8G8B8A8: u32 = u32::from_le_bytes(*b"RA24");
const DRM_FORMAT_B8G8R8A8: u32 = u32::from_le_bytes(*b"BA24");
const DRM_FORMAT_A2B10G10R10: u32 = u32::from_le_bytes(*b"AB30");
const DRM_FORMAT_A2R10G10B10: u32 = u32::from_le_bytes(*b"AR30");
const DRM_FORMAT_XRGB8888: u32 = u32::from_le_bytes(*b"XR24");
const DRM_FORMAT_ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
const DRM_FORMAT_XBGR8888: u32 = u32::from_le_bytes(*b"XB24");
const DRM_FORMAT_ABGR8888: u32 = u32::from_le_bytes(*b"AB24");

/// Daemon-owned adapter for Redunar's hardware-only Vulkan Video encoder.
///
/// Construction validates the driver-produced SPS/PPS and establishes one
/// immutable codec epoch. It deliberately does not advertise production
/// readiness or attach itself to [`crate::ProductionReplayRuntime`]; the
/// separate live-transfer, decoded-output, and performance gates remain the
/// authority for that transition.
pub struct VulkanVideoH264Backend {
    encoder: Option<VulkanVideoH264Encoder>,
    stream: ReplayVideoStream,
    failed: bool,
    pending_inputs: VecDeque<(u64, u64)>,
}

impl VulkanVideoH264Backend {
    /// Bridge one initialized Vulkan Video encoder into the daemon packet
    /// contract after validating its hardware-produced stream headers.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayEncoderError`] when the driver parameter bytes cannot
    /// initialize a bounded, playable H.264 Matroska stream.
    pub fn new(encoder: VulkanVideoH264Encoder) -> Result<Self, ReplayEncoderError> {
        let request = encoder.request();
        let parameters = H264ParameterSets::from_annex_b(encoder.encoded_parameters())?;
        let stream = ReplayVideoStream::new(
            ReplayVideoCodec::H264,
            ReplayPacketFormat::H264LengthPrefixed4,
            request.width,
            request.height,
            request.frames_per_second,
            parameters.avc_decoder_configuration(),
        )?;
        Ok(Self {
            encoder: Some(encoder),
            stream,
            failed: false,
            pending_inputs: VecDeque::with_capacity(MAX_HARDWARE_INPUTS_IN_FLIGHT),
        })
    }

    fn active_encoder(&mut self) -> Result<&mut VulkanVideoH264Encoder, ReplayEncoderError> {
        if self.failed {
            return Err(ReplayEncoderError::BackendFailed(
                "Vulkan Video encoder is in a failed state".to_owned(),
            ));
        }
        self.encoder.as_mut().ok_or_else(|| {
            ReplayEncoderError::BackendFailed("Vulkan Video encoder is shut down".to_owned())
        })
    }

    fn fail(
        &mut self,
        operation: &'static str,
        error: &VulkanVideoDeviceError,
    ) -> ReplayEncoderError {
        self.failed = true;
        // Dropping the sole encoder owner waits for its private logical device
        // and releases imported DMA-BUFs. No fallback backend is selected.
        drop(self.encoder.take());
        ReplayEncoderError::BackendFailed(format!("Vulkan Video {operation} failed: {error:?}"))
    }
}

impl HardwareEncoderBackend for VulkanVideoH264Backend {
    fn api(&self) -> HardwareEncoderApi {
        HardwareEncoderApi::VulkanVideo
    }

    fn stream(&self) -> &ReplayVideoStream {
        &self.stream
    }

    fn encode(
        &mut self,
        input_sequence: u64,
        frame: DmaBufReplayFrame,
        force_keyframe: bool,
    ) -> Result<HardwareEncodeOutput, ReplayEncoderError> {
        let timestamp_ns = frame.timestamp_ns;
        let frame = match vulkan_frame(&frame) {
            Ok(frame) => frame,
            Err(error) => {
                self.failed = true;
                drop(self.encoder.take());
                return Err(error);
            }
        };
        let access_units = match self.active_encoder()?.encode_frame(frame, force_keyframe) {
            Ok(access_units) => access_units,
            Err(error) => return Err(self.fail("frame submission", &error)),
        };
        let completed_inputs = match self.resolve_completed_inputs(&access_units) {
            Ok(completed) => completed,
            Err(error) => {
                self.failed = true;
                drop(self.encoder.take());
                return Err(error);
            }
        };
        self.pending_inputs
            .push_back((timestamp_ns, input_sequence));
        if self.pending_inputs.len() > MAX_HARDWARE_INPUTS_IN_FLIGHT {
            self.failed = true;
            drop(self.encoder.take());
            return Err(ReplayEncoderError::InvalidInputCompletion);
        }
        match encoded_batch(access_units)
            .and_then(|batch| HardwareEncodeOutput::new(batch, completed_inputs))
        {
            Ok(output) => Ok(output),
            Err(error) => {
                self.failed = true;
                drop(self.encoder.take());
                Err(error)
            }
        }
    }

    fn drain(&mut self) -> Result<HardwareEncodeOutput, ReplayEncoderError> {
        let access_units = match self.active_encoder()?.drain() {
            Ok(access_units) => access_units,
            Err(error) => return Err(self.fail("drain", &error)),
        };
        let completed_inputs = match self.resolve_completed_inputs(&access_units) {
            Ok(completed) if self.pending_inputs.is_empty() => completed,
            Ok(_) => {
                self.failed = true;
                drop(self.encoder.take());
                return Err(ReplayEncoderError::InvalidInputCompletion);
            }
            Err(error) => {
                self.failed = true;
                drop(self.encoder.take());
                return Err(error);
            }
        };
        match encoded_batch(access_units)
            .and_then(|batch| HardwareEncodeOutput::new(batch, completed_inputs))
        {
            Ok(output) => Ok(output),
            Err(error) => {
                self.failed = true;
                drop(self.encoder.take());
                Err(error)
            }
        }
    }

    fn shutdown(&mut self) -> Result<(), ReplayEncoderError> {
        if self.encoder.is_none() {
            return Ok(());
        }
        let drain_result = self
            .encoder
            .as_mut()
            .expect("checked Vulkan Video encoder owner")
            .drain()
            .map_err(|error| {
                ReplayEncoderError::BackendFailed(format!(
                    "Vulkan Video shutdown drain failed: {error:?}"
                ))
            })
            .and_then(encoded_batch);
        drop(self.encoder.take());
        match drain_result {
            Ok(_) => Ok(()),
            Err(error) => {
                self.failed = true;
                Err(error)
            }
        }
    }
}

impl VulkanVideoH264Backend {
    fn resolve_completed_inputs(
        &mut self,
        access_units: &[VulkanVideoEncodedAccessUnit],
    ) -> Result<Vec<u64>, ReplayEncoderError> {
        let mut completed = Vec::with_capacity(access_units.len());
        for unit in access_units {
            let Some(index) = self
                .pending_inputs
                .iter()
                .position(|(timestamp, _)| *timestamp == unit.timestamp_ns)
            else {
                return Err(ReplayEncoderError::InvalidInputCompletion);
            };
            let (_, sequence) = self
                .pending_inputs
                .remove(index)
                .expect("located pending Vulkan input");
            completed.push(sequence);
        }
        Ok(completed)
    }
}

fn vulkan_frame(frame: &DmaBufReplayFrame) -> Result<VulkanVideoDmaBufFrame, ReplayEncoderError> {
    if matches!(
        frame.drm_fourcc,
        DRM_FORMAT_XRGB8888 | DRM_FORMAT_ARGB8888 | DRM_FORMAT_XBGR8888 | DRM_FORMAT_ABGR8888
    ) {
        if frame.image_planes().len() != 1 || frame.objects().len() != 1 {
            return Err(ReplayEncoderError::InvalidDmaBufFrame);
        }
        let plane = frame.image_planes()[0];
        if plane.object_index != 0 {
            return Err(ReplayEncoderError::InvalidDmaBufFrame);
        }
        let fd = frame.objects()[0]
            .try_clone()
            .map_err(|_| ReplayEncoderError::InvalidDmaBufFrame)?;
        return VulkanVideoDmaBufFrame::new_drm_image(
            fd,
            frame.width,
            frame.height,
            frame.drm_fourcc,
            frame.modifier,
            plane.offset,
            plane.stride,
            frame.timestamp_ns,
            frame.duration_ns,
        )
        .map_err(|_| ReplayEncoderError::InvalidDmaBufFrame);
    }
    let format = match frame.drm_fourcc {
        DRM_FORMAT_R8G8B8A8 => VulkanPackedPixelFormat::Rgba8,
        DRM_FORMAT_B8G8R8A8 => VulkanPackedPixelFormat::Bgra8,
        DRM_FORMAT_A2B10G10R10 => VulkanPackedPixelFormat::A2b10g10r10,
        DRM_FORMAT_A2R10G10B10 => VulkanPackedPixelFormat::A2r10g10b10,
        _ => return Err(ReplayEncoderError::InvalidDmaBufFrame),
    };
    // The capture transfer is a Vulkan buffer export, not a DRM image. It has
    // exactly one packed plane and therefore no image modifier interpretation.
    if frame.modifier != 0 || frame.image_planes().len() != 1 || frame.objects().len() != 1 {
        return Err(ReplayEncoderError::InvalidDmaBufFrame);
    }
    let plane = frame.image_planes()[0];
    let fd = frame
        .fd()
        .try_clone()
        .map_err(|_| ReplayEncoderError::InvalidDmaBufFrame)?;
    VulkanVideoDmaBufFrame::new(
        fd,
        frame.width,
        frame.height,
        format,
        plane.offset,
        plane.stride,
        frame.timestamp_ns,
        frame.duration_ns,
    )
    .map_err(|_| ReplayEncoderError::InvalidDmaBufFrame)
}

fn encoded_batch(
    access_units: Vec<VulkanVideoEncodedAccessUnit>,
) -> Result<EncodedPacketBatch, ReplayEncoderError> {
    let packets = access_units
        .into_iter()
        .map(|unit| {
            packet_from_annex_b(
                unit.timestamp_ns,
                unit.duration_ns,
                unit.requested_keyframe,
                &unit.annex_b,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    EncodedPacketBatch::new(packets)
}

fn packet_from_annex_b(
    timestamp_ns: u64,
    duration_ns: u64,
    requested_keyframe: bool,
    annex_b: &[u8],
) -> Result<crate::EncodedReplayPacket, ReplayEncoderError> {
    let packet = h264_annex_b_access_unit(timestamp_ns, duration_ns, annex_b)?;
    if requested_keyframe && !packet.is_keyframe() {
        return Err(ReplayEncoderError::BackendFailed(
            "Vulkan Video did not produce an IDR for a requested keyframe".to_owned(),
        ));
    }
    Ok(packet)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DmaBufImagePlane;
    use std::fs::{File, OpenOptions};
    use std::path::PathBuf;

    fn packed_fixture(width: u32, height: u32) -> (PathBuf, File) {
        let path = std::env::temp_dir().join(format!(
            "redunar-kms-frame-{}-{}",
            std::process::id(),
            width
        ));
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)
            .expect("frame fixture");
        file.set_len(u64::from(width) * u64::from(height) * 4)
            .expect("fixture size");
        (path, file)
    }

    #[test]
    fn kms_frame_keeps_modifier_image_import_instead_of_buffer_import() {
        let (path, file) = packed_fixture(320, 240);
        let frame = DmaBufReplayFrame::new_multi_object(
            vec![file.into()],
            320,
            240,
            DRM_FORMAT_XRGB8888,
            0x0200_0000_0000_0001,
            1,
            16_666_667,
            vec![DmaBufImagePlane {
                object_index: 0,
                offset: 0,
                stride: 1_280,
            }],
        )
        .expect("KMS frame");
        let imported = vulkan_frame(&frame).expect("Vulkan KMS frame");
        assert_eq!(
            imported.input_mode(),
            redunar_capture_vulkan::replay_video::VulkanVideoInputMode::DrmImage
        );
        drop(imported);
        drop(frame);
        std::fs::remove_file(path).expect("cleanup fixture");
    }

    #[test]
    fn access_unit_is_converted_and_keyframe_is_derived_from_idr() {
        let packet = packet_from_annex_b(42, 16_666_667, true, &[0, 0, 0, 1, 0x65, 0x88, 0x84])
            .expect("IDR access unit");
        assert!(packet.is_keyframe());
        assert_eq!(packet.timestamp_ns(), 42);
        assert_eq!(packet.bytes(), &[0, 0, 0, 3, 0x65, 0x88, 0x84]);
    }

    #[test]
    fn requested_keyframe_without_idr_fails_closed() {
        let error = packet_from_annex_b(42, 16_666_667, true, &[0, 0, 0, 1, 0x41, 0x9a, 0x20])
            .expect_err("non-IDR response");
        assert!(matches!(error, ReplayEncoderError::BackendFailed(_)));
    }

    #[test]
    fn ordinary_p_picture_remains_valid_when_no_keyframe_was_requested() {
        let packet = packet_from_annex_b(58, 16_666_667, false, &[0, 0, 1, 0x41, 0x9a, 0x20])
            .expect("P-picture access unit");
        assert!(!packet.is_keyframe());
    }
}
