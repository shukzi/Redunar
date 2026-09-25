use crate::{
    EncodedReplayPacket, ReplayPacketError, ReplayPushOutcome, ReplayRing, ReplayRingError,
};
use redunar_core::ReplaySettings;
use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

const MAX_RENDER_NODES: usize = 16;
const MAX_DMABUF_PLANES: usize = 4;
const MAX_CODEC_PRIVATE_BYTES: usize = 64 * 1024;
const MAX_PACKETS_PER_FRAME: usize = 8;
const MAX_ENCODED_BYTES_PER_FRAME: usize = 8 * 1024 * 1024;
/// Maximum hardware submissions the daemon permits before completion.
///
/// This must match the capture/export and Vulkan conversion slot bound so a
/// normal four-slot drain is accepted without weakening the bounded contract.
pub(crate) const MAX_HARDWARE_INPUTS_IN_FLIGHT: usize =
    redunar_capture::REPLAY_ENCODER_PIPELINE_DEPTH;
const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;
const DRM_FORMAT_R8G8B8A8: u32 = u32::from_le_bytes(*b"RA24");
const DRM_FORMAT_B8G8R8A8: u32 = u32::from_le_bytes(*b"BA24");
const DRM_FORMAT_A2B10G10R10: u32 = u32::from_le_bytes(*b"AB30");
const DRM_FORMAT_A2R10G10B10: u32 = u32::from_le_bytes(*b"AR30");
const DRM_FORMAT_XRGB8888: u32 = u32::from_le_bytes(*b"XR24");
const DRM_FORMAT_ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
const DRM_FORMAT_XBGR8888: u32 = u32::from_le_bytes(*b"XB24");
const DRM_FORMAT_ABGR8888: u32 = u32::from_le_bytes(*b"AB24");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayVideoCodec {
    H264,
    Av1,
}

impl ReplayVideoCodec {
    pub(crate) const fn matroska_codec_id(self) -> &'static [u8] {
        match self {
            Self::H264 => b"V_MPEG4/ISO/AVC",
            Self::Av1 => b"V_AV1",
        }
    }
}

/// Packet framing expected by the video-only Matroska writer.
///
/// H.264 is length-prefixed AVC, not Annex B. Keeping this explicit prevents
/// a future backend from writing a syntactically valid but undecodable clip.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayPacketFormat {
    H264LengthPrefixed4,
    Av1Obu,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayVideoStream {
    codec: ReplayVideoCodec,
    packet_format: ReplayPacketFormat,
    width: u32,
    height: u32,
    frames_per_second: u8,
    codec_private: Box<[u8]>,
}

impl ReplayVideoStream {
    /// Describe one encoder output stream after its headers are available.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayEncoderError`] when dimensions, frame rate, packet
    /// framing, or codec initialization data cannot make a playable stream.
    pub fn new(
        codec: ReplayVideoCodec,
        packet_format: ReplayPacketFormat,
        width: u32,
        height: u32,
        frames_per_second: u8,
        codec_private: Vec<u8>,
    ) -> Result<Self, ReplayEncoderError> {
        if width == 0 || height == 0 || width > 3_840 || height > 2_160 {
            return Err(ReplayEncoderError::InvalidStream);
        }
        if !matches!(frames_per_second, 30 | 60 | 120) {
            return Err(ReplayEncoderError::InvalidStream);
        }
        if codec_private.len() > MAX_CODEC_PRIVATE_BYTES {
            return Err(ReplayEncoderError::InvalidStream);
        }
        match (codec, packet_format) {
            (ReplayVideoCodec::H264, ReplayPacketFormat::H264LengthPrefixed4) => {
                validate_avc_decoder_configuration(&codec_private)?;
            }
            (ReplayVideoCodec::Av1, ReplayPacketFormat::Av1Obu) => {
                validate_av1_codec_configuration(&codec_private)?;
            }
            _ => return Err(ReplayEncoderError::InvalidStream),
        }
        Ok(Self {
            codec,
            packet_format,
            width,
            height,
            frames_per_second,
            codec_private: codec_private.into_boxed_slice(),
        })
    }

    #[must_use]
    pub const fn codec(&self) -> ReplayVideoCodec {
        self.codec
    }

    #[must_use]
    pub const fn packet_format(&self) -> ReplayPacketFormat {
        self.packet_format
    }

    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub const fn frames_per_second(&self) -> u8 {
        self.frames_per_second
    }

    #[must_use]
    pub const fn codec_private(&self) -> &[u8] {
        &self.codec_private
    }

    pub(crate) fn validate_packet(&self, packet: &[u8]) -> Result<(), ReplayEncoderError> {
        match self.packet_format {
            ReplayPacketFormat::H264LengthPrefixed4 => validate_h264_packet(packet),
            ReplayPacketFormat::Av1Obu if packet.is_empty() => {
                Err(ReplayEncoderError::InvalidPacketFraming)
            }
            ReplayPacketFormat::Av1Obu => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DmaBufPlane {
    pub offset: u32,
    pub stride: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DmaBufImagePlane {
    pub object_index: u8,
    pub offset: u32,
    pub stride: u32,
}

/// A hardware-encoder input frame backed by a GPU-shareable DMA-BUF.
///
/// There is deliberately no host-pixel or byte-buffer alternative. Backends
/// that cannot import this handle must reject the session instead of silently
/// selecting a CPU conversion or software encoder.
#[derive(Debug)]
pub struct DmaBufReplayFrame {
    objects: Box<[OwnedFd]>,
    pub width: u32,
    pub height: u32,
    pub drm_fourcc: u32,
    pub modifier: u64,
    pub timestamp_ns: u64,
    pub duration_ns: u64,
    planes: Box<[DmaBufImagePlane]>,
}

impl DmaBufReplayFrame {
    /// The accepted layouts are packed 8-bit RGBA/BGRA and packed 10-bit
    /// A2B10G10R10/A2R10G10B10, matching the Vulkan source whitelist. Other
    /// formats are rejected before an encoder can claim ownership of the frame.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayEncoderError`] for an unsafe or unbounded frame shape.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fd: OwnedFd,
        width: u32,
        height: u32,
        drm_fourcc: u32,
        modifier: u64,
        timestamp_ns: u64,
        duration_ns: u64,
        planes: Vec<DmaBufPlane>,
    ) -> Result<Self, ReplayEncoderError> {
        if width == 0
            || height == 0
            || width > 3_840
            || height > 2_160
            || drm_fourcc == 0
            || duration_ns == 0
            || duration_ns > NANOSECONDS_PER_SECOND
            || timestamp_ns.checked_add(duration_ns).is_none()
            || planes.is_empty()
            || planes.len() > MAX_DMABUF_PLANES
            || !matches!(
                drm_fourcc,
                DRM_FORMAT_R8G8B8A8
                    | DRM_FORMAT_B8G8R8A8
                    | DRM_FORMAT_A2B10G10R10
                    | DRM_FORMAT_A2R10G10B10
            )
            || planes.iter().any(|plane| {
                plane.stride < width.saturating_mul(4)
                    || plane.offset > width.saturating_mul(height).saturating_mul(4)
            })
        {
            return Err(ReplayEncoderError::InvalidDmaBufFrame);
        }
        Self::new_multi_object(
            vec![fd],
            width,
            height,
            drm_fourcc,
            modifier,
            timestamp_ns,
            duration_ns,
            planes
                .into_iter()
                .map(|plane| DmaBufImagePlane {
                    object_index: 0,
                    offset: plane.offset,
                    stride: plane.stride,
                })
                .collect(),
        )
    }

    /// Construct a DRM-image frame backed by one to four DMA-BUF objects.
    /// Plane object indices preserve the exact `drmModeFB2` layout.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayEncoderError`] for inconsistent object/plane metadata,
    /// unsupported packed formats, or unbounded dimensions and timings.
    #[allow(clippy::too_many_arguments)]
    pub fn new_multi_object(
        objects: Vec<OwnedFd>,
        width: u32,
        height: u32,
        drm_fourcc: u32,
        modifier: u64,
        timestamp_ns: u64,
        duration_ns: u64,
        planes: Vec<DmaBufImagePlane>,
    ) -> Result<Self, ReplayEncoderError> {
        const MAX_DMABUF_OBJECTS: usize = 4;
        if width == 0
            || height == 0
            || width > 3_840
            || height > 2_160
            || drm_fourcc == 0
            || duration_ns == 0
            || duration_ns > NANOSECONDS_PER_SECOND
            || timestamp_ns.checked_add(duration_ns).is_none()
            || objects.is_empty()
            || objects.len() > MAX_DMABUF_OBJECTS
            || planes.is_empty()
            || planes.len() > MAX_DMABUF_PLANES
            || !matches!(
                drm_fourcc,
                DRM_FORMAT_R8G8B8A8
                    | DRM_FORMAT_B8G8R8A8
                    | DRM_FORMAT_A2B10G10R10
                    | DRM_FORMAT_A2R10G10B10
                    | DRM_FORMAT_XRGB8888
                    | DRM_FORMAT_ARGB8888
                    | DRM_FORMAT_XBGR8888
                    | DRM_FORMAT_ABGR8888
            )
            || planes.iter().any(|plane| {
                usize::from(plane.object_index) >= objects.len()
                    || plane.stride < width.saturating_mul(4)
                    || plane.offset > width.saturating_mul(height).saturating_mul(4)
            })
        {
            return Err(ReplayEncoderError::InvalidDmaBufFrame);
        }
        Ok(Self {
            objects: objects.into_boxed_slice(),
            width,
            height,
            drm_fourcc,
            modifier,
            timestamp_ns,
            duration_ns,
            planes: planes.into_boxed_slice(),
        })
    }

    #[must_use]
    pub const fn fd(&self) -> &OwnedFd {
        &self.objects[0]
    }

    #[must_use]
    pub const fn objects(&self) -> &[OwnedFd] {
        &self.objects
    }

    #[must_use]
    pub const fn image_planes(&self) -> &[DmaBufImagePlane] {
        &self.planes
    }

    #[must_use]
    pub fn planes(&self) -> Vec<DmaBufPlane> {
        self.planes
            .iter()
            .map(|plane| DmaBufPlane {
                offset: plane.offset,
                stride: plane.stride,
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HardwareEncoderApi {
    VaApi,
    VulkanVideo,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HardwareEncoderProbeBlocker {
    NoSupportedRenderNode,
    RenderNodeNotAccessible,
    EncodeProfileNotVerified,
    DmaBufImportNotVerified,
    SustainedFramePacingNotVerified,
    BackendNotIntegrated,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HardwareEncoderDeviceCandidate {
    render_node: PathBuf,
    driver: String,
    accessible: bool,
}

impl HardwareEncoderDeviceCandidate {
    #[must_use]
    pub fn render_node(&self) -> &Path {
        &self.render_node
    }

    #[must_use]
    pub fn driver(&self) -> &str {
        &self.driver
    }

    #[must_use]
    pub const fn is_accessible(&self) -> bool {
        self.accessible
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HardwareEncoderProbe {
    candidates: Vec<HardwareEncoderDeviceCandidate>,
    blockers: Vec<HardwareEncoderProbeBlocker>,
}

impl HardwareEncoderProbe {
    /// Inspect local DRM render nodes without starting an encoder or reading
    /// desktop/game pixels. Device presence is only a candidate signal.
    #[must_use]
    pub fn local() -> Self {
        Self::from_roots(Path::new("/dev/dri"), Path::new("/sys/class/drm"))
    }

    /// Admit a single NVIDIA render node as a beta candidate. Multi-GPU
    /// systems remain unavailable until the game and encoder can be matched.
    #[must_use]
    pub fn local_with_nvidia_beta(allow_nvidia_beta: bool) -> Self {
        Self::from_roots_with_nvidia_beta(
            Path::new("/dev/dri"),
            Path::new("/sys/class/drm"),
            allow_nvidia_beta,
        )
    }

    #[must_use]
    pub fn candidates(&self) -> &[HardwareEncoderDeviceCandidate] {
        &self.candidates
    }

    #[must_use]
    pub fn blockers(&self) -> &[HardwareEncoderProbeBlocker] {
        &self.blockers
    }

    /// A filesystem probe alone can never satisfy the production gate.
    #[must_use]
    pub const fn production_ready(&self) -> bool {
        false
    }

    fn from_roots(device_root: &Path, sysfs_root: &Path) -> Self {
        Self::from_roots_with_nvidia_beta(device_root, sysfs_root, false)
    }

    fn from_roots_with_nvidia_beta(
        device_root: &Path,
        sysfs_root: &Path,
        allow_nvidia_beta: bool,
    ) -> Self {
        let mut candidates = Vec::new();
        if let Ok(entries) = fs::read_dir(device_root) {
            let nodes: Vec<_> = entries
                .flatten()
                .filter(|entry| entry.file_name().to_str().is_some_and(is_render_node_name))
                .take(MAX_RENDER_NODES + 1)
                .collect();
            let single_gpu = nodes.len() == 1;
            let has_nvidia = nodes.iter().any(|entry| {
                fs::read_link(sysfs_root.join(entry.file_name()).join("device/driver"))
                    .ok()
                    .and_then(|path| path.file_name().map(|name| name == "nvidia"))
                    .unwrap_or(false)
            });
            // A hybrid NVIDIA system cannot safely associate the exported
            // game frame with an encoder until device identity is carried.
            if allow_nvidia_beta && has_nvidia && !single_gpu {
                return Self {
                    candidates,
                    blockers: vec![HardwareEncoderProbeBlocker::NoSupportedRenderNode],
                };
            }
            for entry in nodes.into_iter().take(MAX_RENDER_NODES) {
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                let driver_link = sysfs_root.join(name).join("device/driver");
                let Ok(driver_target) = fs::read_link(driver_link) else {
                    continue;
                };
                let Some(driver) = driver_target.file_name().and_then(|value| value.to_str())
                else {
                    continue;
                };
                if driver != "amdgpu" && !(allow_nvidia_beta && single_gpu && driver == "nvidia") {
                    continue;
                }
                let render_node = entry.path();
                let accessible = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&render_node)
                    .is_ok();
                candidates.push(HardwareEncoderDeviceCandidate {
                    render_node,
                    driver: driver.to_owned(),
                    accessible,
                });
            }
        }
        candidates.sort_by(|left, right| left.render_node.cmp(&right.render_node));
        let mut blockers = Vec::new();
        if candidates.is_empty() {
            blockers.push(HardwareEncoderProbeBlocker::NoSupportedRenderNode);
        } else if candidates.iter().all(|candidate| !candidate.accessible) {
            blockers.push(HardwareEncoderProbeBlocker::RenderNodeNotAccessible);
        }
        blockers.extend([
            HardwareEncoderProbeBlocker::EncodeProfileNotVerified,
            HardwareEncoderProbeBlocker::DmaBufImportNotVerified,
            HardwareEncoderProbeBlocker::SustainedFramePacingNotVerified,
            HardwareEncoderProbeBlocker::BackendNotIntegrated,
        ]);
        Self {
            candidates,
            blockers,
        }
    }
}

/// The only encoder backend contract accepted by Replay.
///
/// Implementations must be hardware-only and import the supplied DMA-BUF.
/// Returning packets produced from a host-memory fallback violates this
/// contract and must be treated as an encoder failure by the owner.
pub trait HardwareEncoderBackend: Send {
    fn api(&self) -> HardwareEncoderApi;
    fn stream(&self) -> &ReplayVideoStream;
    /// Import and encode one DMA-BUF without a host-memory fallback.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayEncoderError`] when import, encoding, or bounded packet
    /// collection fails.
    fn encode(
        &mut self,
        input_sequence: u64,
        frame: DmaBufReplayFrame,
        force_keyframe: bool,
    ) -> Result<HardwareEncodeOutput, ReplayEncoderError>;
    /// Drain only hardware-encoder output already submitted by `encode`.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayEncoderError`] for a backend or packet-bound failure.
    fn drain(&mut self) -> Result<HardwareEncodeOutput, ReplayEncoderError>;
    /// Release and join all backend-owned resources.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayEncoderError`] when orderly release fails.
    fn shutdown(&mut self) -> Result<(), ReplayEncoderError>;
}

/// One bounded hardware result plus the exact input exports whose completion
/// fences have signaled. A producer may reuse only these sequences.
#[derive(Debug)]
pub struct HardwareEncodeOutput {
    batch: EncodedPacketBatch,
    completed_inputs: Vec<u64>,
}

impl HardwareEncodeOutput {
    /// # Errors
    ///
    /// Returns [`ReplayEncoderError`] for zero, duplicate, or unbounded input
    /// completion tokens.
    pub fn new(
        batch: EncodedPacketBatch,
        completed_inputs: Vec<u64>,
    ) -> Result<Self, ReplayEncoderError> {
        if completed_inputs.len() > MAX_HARDWARE_INPUTS_IN_FLIGHT
            || completed_inputs.contains(&0)
            || completed_inputs
                .iter()
                .enumerate()
                .any(|(index, sequence)| completed_inputs[..index].contains(sequence))
        {
            return Err(ReplayEncoderError::InvalidInputCompletion);
        }
        Ok(Self {
            batch,
            completed_inputs,
        })
    }

    #[must_use]
    pub fn batch(&self) -> &EncodedPacketBatch {
        &self.batch
    }

    #[must_use]
    pub fn completed_inputs(&self) -> &[u64] {
        &self.completed_inputs
    }

    pub(crate) fn into_parts(self) -> (EncodedPacketBatch, Vec<u64>) {
        (self.batch, self.completed_inputs)
    }
}

#[derive(Debug, Default)]
pub struct EncodedPacketBatch {
    packets: Vec<EncodedReplayPacket>,
}

impl EncodedPacketBatch {
    /// # Errors
    ///
    /// Returns [`ReplayEncoderError`] when one backend result could make the
    /// daemon perform unbounded work or when timestamps are not monotonic.
    pub fn new(packets: Vec<EncodedReplayPacket>) -> Result<Self, ReplayEncoderError> {
        if packets.len() > MAX_PACKETS_PER_FRAME {
            return Err(ReplayEncoderError::PacketBatchExceeded);
        }
        let mut total = 0_usize;
        let mut last_timestamp = None;
        for packet in &packets {
            total = total
                .checked_add(packet.bytes().len())
                .ok_or(ReplayEncoderError::PacketBatchExceeded)?;
            if total > MAX_ENCODED_BYTES_PER_FRAME
                || last_timestamp.is_some_and(|timestamp| packet.timestamp_ns() <= timestamp)
            {
                return Err(ReplayEncoderError::PacketBatchExceeded);
            }
            last_timestamp = Some(packet.timestamp_ns());
        }
        Ok(Self { packets })
    }

    #[must_use]
    pub fn packets(&self) -> &[EncodedReplayPacket] {
        &self.packets
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayPacketFlowPhase {
    Running,
    ResetRequired,
    Failed,
    Shutdown,
}

/// Daemon-owned bridge from bounded hardware-encoder batches into the ring.
/// Resize/reconfigure, failure, and shutdown discard the old codec epoch so a
/// saved clip can never combine incompatible stream parameters.
pub struct ReplayPacketFlow {
    stream: ReplayVideoStream,
    ring: ReplayRing,
    phase: ReplayPacketFlowPhase,
}

impl ReplayPacketFlow {
    #[must_use]
    pub fn new(settings: ReplaySettings, stream: ReplayVideoStream) -> Self {
        Self {
            stream,
            ring: ReplayRing::new(settings),
            phase: ReplayPacketFlowPhase::Running,
        }
    }

    #[must_use]
    pub const fn stream(&self) -> &ReplayVideoStream {
        &self.stream
    }

    #[must_use]
    pub const fn ring(&self) -> &ReplayRing {
        &self.ring
    }

    #[must_use]
    pub const fn phase(&self) -> ReplayPacketFlowPhase {
        self.phase
    }

    /// # Errors
    ///
    /// Returns [`ReplayEncoderError`] and fails closed on malformed output,
    /// ring rejection, or an invalid lifecycle state.
    pub fn push_batch(
        &mut self,
        batch: EncodedPacketBatch,
    ) -> Result<Vec<ReplayPushOutcome>, ReplayEncoderError> {
        if self.phase != ReplayPacketFlowPhase::Running {
            return Err(ReplayEncoderError::InvalidLifecycle);
        }
        for packet in batch.packets() {
            if let Err(error) = self.stream.validate_packet(packet.bytes()) {
                self.fail_closed();
                return Err(error);
            }
        }
        let mut outcomes = Vec::with_capacity(batch.packets.len());
        for packet in batch.packets {
            match self.ring.push(packet) {
                Ok(outcome) => outcomes.push(outcome),
                Err(error) => {
                    self.fail_closed();
                    return Err(ReplayEncoderError::Ring(error));
                }
            }
        }
        Ok(outcomes)
    }

    /// Begin a resize/reset. All packets from the old coded size are removed.
    pub fn begin_reset(&mut self) {
        if self.phase == ReplayPacketFlowPhase::Running {
            self.ring.reset_epoch();
            self.phase = ReplayPacketFlowPhase::ResetRequired;
        }
    }

    /// # Errors
    ///
    /// Returns [`ReplayEncoderError`] unless a reset is pending.
    pub fn complete_reset(&mut self, stream: ReplayVideoStream) -> Result<(), ReplayEncoderError> {
        if self.phase != ReplayPacketFlowPhase::ResetRequired {
            return Err(ReplayEncoderError::InvalidLifecycle);
        }
        self.stream = stream;
        self.phase = ReplayPacketFlowPhase::Running;
        Ok(())
    }

    pub fn fail(&mut self) {
        if matches!(
            self.phase,
            ReplayPacketFlowPhase::Running | ReplayPacketFlowPhase::ResetRequired
        ) {
            self.fail_closed();
        }
    }

    pub fn shutdown(&mut self) {
        self.ring.reset_epoch();
        self.phase = ReplayPacketFlowPhase::Shutdown;
    }

    fn fail_closed(&mut self) {
        self.ring.reset_epoch();
        self.phase = ReplayPacketFlowPhase::Failed;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplayEncoderError {
    InvalidStream,
    InvalidCodecHeaders,
    InvalidDmaBufFrame,
    InvalidPacketFraming,
    PacketBatchExceeded,
    Packet(ReplayPacketError),
    Ring(ReplayRingError),
    InvalidLifecycle,
    InvalidInputCompletion,
    BackendFailed(String),
}

impl fmt::Display for ReplayEncoderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStream => formatter.write_str("replay video stream is invalid"),
            Self::InvalidCodecHeaders => {
                formatter.write_str("hardware encoder codec headers are invalid")
            }
            Self::InvalidDmaBufFrame => formatter.write_str("replay DMA-BUF frame is invalid"),
            Self::InvalidPacketFraming => {
                formatter.write_str("hardware encoder packet framing is invalid")
            }
            Self::PacketBatchExceeded => {
                formatter.write_str("hardware encoder packet batch exceeds its fixed bound")
            }
            Self::Packet(error) => write!(formatter, "encoded replay packet is invalid: {error}"),
            Self::Ring(error) => write!(formatter, "encoded replay ring rejected output: {error}"),
            Self::InvalidLifecycle => formatter.write_str("replay encoder lifecycle is invalid"),
            Self::InvalidInputCompletion => {
                formatter.write_str("hardware encoder input completion is invalid")
            }
            Self::BackendFailed(message) => write!(formatter, "hardware encoder failed: {message}"),
        }
    }
}

impl Error for ReplayEncoderError {}

fn is_render_node_name(name: &str) -> bool {
    name.strip_prefix("renderD").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn validate_h264_packet(packet: &[u8]) -> Result<(), ReplayEncoderError> {
    let mut remaining = packet;
    let mut units = 0_u8;
    while !remaining.is_empty() {
        let length_bytes: [u8; 4] = remaining
            .get(..4)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(ReplayEncoderError::InvalidPacketFraming)?;
        let length = usize::try_from(u32::from_be_bytes(length_bytes))
            .map_err(|_| ReplayEncoderError::InvalidPacketFraming)?;
        if length == 0 || remaining.len() < 4 + length {
            return Err(ReplayEncoderError::InvalidPacketFraming);
        }
        remaining = &remaining[4 + length..];
        units = units.saturating_add(1);
    }
    if units == 0 {
        return Err(ReplayEncoderError::InvalidPacketFraming);
    }
    Ok(())
}

fn validate_avc_decoder_configuration(data: &[u8]) -> Result<(), ReplayEncoderError> {
    if data.len() < 7 || data[0] != 1 || data[4] & 0b11 != 3 {
        return Err(ReplayEncoderError::InvalidStream);
    }
    let sequence_count = usize::from(data[5] & 0x1f);
    if sequence_count == 0 {
        return Err(ReplayEncoderError::InvalidStream);
    }
    let mut offset = 6_usize;
    for _ in 0..sequence_count {
        offset = skip_avc_parameter_set(data, offset)?;
    }
    let picture_count = usize::from(*data.get(offset).ok_or(ReplayEncoderError::InvalidStream)?);
    offset += 1;
    if picture_count == 0 {
        return Err(ReplayEncoderError::InvalidStream);
    }
    for _ in 0..picture_count {
        offset = skip_avc_parameter_set(data, offset)?;
    }
    if offset > data.len() {
        return Err(ReplayEncoderError::InvalidStream);
    }
    Ok(())
}

fn validate_av1_codec_configuration(data: &[u8]) -> Result<(), ReplayEncoderError> {
    // ISO/IEC 14496-15 AV1CodecConfigurationRecord: marker=1, version=1.
    // The remaining fields are produced by the hardware backend and carried
    // verbatim as Matroska CodecPrivate after this bounded structural check.
    if data.len() < 4 || data[0] != 0x81 {
        return Err(ReplayEncoderError::InvalidStream);
    }
    Ok(())
}

fn skip_avc_parameter_set(data: &[u8], offset: usize) -> Result<usize, ReplayEncoderError> {
    let length_bytes: [u8; 2] = data
        .get(offset..offset.saturating_add(2))
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(ReplayEncoderError::InvalidStream)?;
    let length = usize::from(u16::from_be_bytes(length_bytes));
    if length == 0 {
        return Err(ReplayEncoderError::InvalidStream);
    }
    offset
        .checked_add(2)
        .and_then(|start| start.checked_add(length))
        .filter(|end| *end <= data.len())
        .ok_or(ReplayEncoderError::InvalidStream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    fn h264_stream(width: u32, height: u32) -> ReplayVideoStream {
        ReplayVideoStream::new(
            ReplayVideoCodec::H264,
            ReplayPacketFormat::H264LengthPrefixed4,
            width,
            height,
            60,
            vec![
                1, 66, 0, 30, 0xff, 0xe1, 0, 2, 0x67, 0x42, 1, 0, 2, 0x68, 0xce,
            ],
        )
        .expect("valid stream")
    }

    fn packet(timestamp_ns: u64, keyframe: bool, payload: Vec<u8>) -> EncodedReplayPacket {
        EncodedReplayPacket::new(timestamp_ns, 16_666_667, keyframe, payload).expect("valid packet")
    }

    fn h264_payload(nal: u8) -> Vec<u8> {
        vec![0, 0, 0, 2, nal, 0x88]
    }

    #[test]
    fn hardware_completion_batch_matches_the_four_slot_pipeline_bound() {
        assert!(
            HardwareEncodeOutput::new(
                EncodedPacketBatch::new(Vec::new()).expect("empty packet batch"),
                vec![1, 2, 3, 4],
            )
            .is_ok()
        );
        assert!(
            HardwareEncodeOutput::new(
                EncodedPacketBatch::new(Vec::new()).expect("empty packet batch"),
                vec![1, 2, 3, 4, 5],
            )
            .is_err()
        );
    }

    #[test]
    fn stream_requires_matching_framing_and_h264_parameter_sets() {
        assert!(
            ReplayVideoStream::new(
                ReplayVideoCodec::H264,
                ReplayPacketFormat::Av1Obu,
                1_920,
                1_080,
                60,
                Vec::new(),
            )
            .is_err()
        );
        assert!(
            ReplayVideoStream::new(
                ReplayVideoCodec::H264,
                ReplayPacketFormat::H264LengthPrefixed4,
                1_920,
                1_080,
                60,
                vec![1, 2, 3],
            )
            .is_err()
        );
        assert_eq!(h264_stream(1_920, 1_080).codec(), ReplayVideoCodec::H264);
        assert!(
            ReplayVideoStream::new(
                ReplayVideoCodec::Av1,
                ReplayPacketFormat::Av1Obu,
                1_920,
                1_080,
                60,
                Vec::new(),
            )
            .is_err()
        );
        assert!(
            ReplayVideoStream::new(
                ReplayVideoCodec::Av1,
                ReplayPacketFormat::Av1Obu,
                1_920,
                1_080,
                60,
                vec![0x81, 0, 0, 0],
            )
            .is_ok()
        );
    }

    #[test]
    fn packet_flow_is_bounded_and_fails_closed_on_bad_framing() {
        let mut flow = ReplayPacketFlow::new(ReplaySettings::default(), h264_stream(1_920, 1_080));
        flow.push_batch(
            EncodedPacketBatch::new(vec![packet(1, true, h264_payload(0x65))])
                .expect("bounded batch"),
        )
        .expect("store packet");
        assert_eq!(flow.ring().stats().stored_packets, 1);
        let error = flow
            .push_batch(
                EncodedPacketBatch::new(vec![packet(2, false, vec![0, 0, 0, 9, 1])])
                    .expect("bounded malformed batch"),
            )
            .expect_err("bad packet must fail");
        assert_eq!(error, ReplayEncoderError::InvalidPacketFraming);
        assert_eq!(flow.phase(), ReplayPacketFlowPhase::Failed);
        assert_eq!(flow.ring().stats().stored_packets, 0);
    }

    #[test]
    fn resize_discards_the_old_codec_epoch_before_resuming() {
        let mut flow = ReplayPacketFlow::new(ReplaySettings::default(), h264_stream(1_920, 1_080));
        flow.push_batch(
            EncodedPacketBatch::new(vec![packet(10, true, h264_payload(0x65))]).expect("batch"),
        )
        .expect("packet");
        flow.begin_reset();
        assert_eq!(flow.phase(), ReplayPacketFlowPhase::ResetRequired);
        assert_eq!(flow.ring().stats().stored_packets, 0);
        flow.complete_reset(h264_stream(1_280, 720))
            .expect("complete reset");
        flow.push_batch(
            EncodedPacketBatch::new(vec![packet(1, true, h264_payload(0x65))])
                .expect("new epoch batch"),
        )
        .expect("timestamps restart after reset");
        assert_eq!(flow.stream().width(), 1_280);
    }

    #[test]
    fn local_probe_reports_supported_candidates_without_claiming_readiness() {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "redunar-replay-encoder-probe-{}-{id}",
            std::process::id()
        ));
        let device_root = root.join("dev/dri");
        let sysfs_root = root.join("sys/class/drm");
        fs::create_dir_all(sysfs_root.join("renderD128/device")).expect("sysfs fixture");
        fs::create_dir_all(&device_root).expect("device fixture");
        fs::write(device_root.join("renderD128"), []).expect("render fixture");
        symlink(
            "/sys/bus/pci/drivers/amdgpu",
            sysfs_root.join("renderD128/device/driver"),
        )
        .expect("driver link");

        let probe = HardwareEncoderProbe::from_roots(&device_root, &sysfs_root);
        assert_eq!(probe.candidates().len(), 1);
        assert!(probe.candidates()[0].is_accessible());
        assert_eq!(probe.candidates()[0].driver(), "amdgpu");
        assert!(!probe.production_ready());
        assert!(
            probe
                .blockers()
                .contains(&HardwareEncoderProbeBlocker::DmaBufImportNotVerified)
        );
        assert!(
            probe
                .blockers()
                .contains(&HardwareEncoderProbeBlocker::BackendNotIntegrated)
        );
        assert!(
            probe
                .blockers()
                .contains(&HardwareEncoderProbeBlocker::SustainedFramePacingNotVerified)
        );

        fs::create_dir_all(sysfs_root.join("renderD129/device"))
            .expect("unsupported sysfs fixture");
        fs::write(device_root.join("renderD129"), []).expect("unsupported render fixture");
        symlink(
            "/sys/bus/pci/drivers/i915",
            sysfs_root.join("renderD129/device/driver"),
        )
        .expect("unsupported driver link");
        let probe = HardwareEncoderProbe::from_roots(&device_root, &sysfs_root);
        assert_eq!(probe.candidates().len(), 1);
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn packet_batch_rejects_non_monotonic_or_excessive_backend_output() {
        assert_eq!(
            EncodedPacketBatch::new(vec![
                packet(2, true, h264_payload(0x65)),
                packet(1, false, h264_payload(0x41)),
            ])
            .expect_err("backwards output"),
            ReplayEncoderError::PacketBatchExceeded
        );
        let packets = (0..=MAX_PACKETS_PER_FRAME)
            .map(|index| packet(index as u64 + 1, index == 0, h264_payload(0x41)))
            .collect();
        assert_eq!(
            EncodedPacketBatch::new(packets).expect_err("too many packets"),
            ReplayEncoderError::PacketBatchExceeded
        );
    }
}
