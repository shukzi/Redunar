use crate::{
    EncodedPacketBatch, EncodedReplayPacket, ReplayBackendReadiness, ReplayBudget, ReplayClipStore,
    ReplayPacketFlow, ReplayPacketFormat, ReplayStorageStatus, ReplayVideoCodec, ReplayVideoStream,
    StoredReplayClip,
};
use redunar_core::ReplaySettings;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;
const SELF_TEST_SECONDS: u16 = 2;
const MATROSKA_MAGIC: [u8; 4] = [0x1a, 0x45, 0xdf, 0xa3];
const AVC_DECODER_CONFIGURATION: [u8; 15] = [
    1, 66, 0, 30, 0xff, 0xe1, 0, 2, 0x67, 0x42, 1, 0, 2, 0x68, 0xce,
];

/// Stages exercised by the generated-packet replay foundation self-test.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayFoundationSelfTestStage {
    Settings,
    PacketFlow,
    ClipStore,
    ContainerCommit,
    Readback,
}

impl fmt::Display for ReplayFoundationSelfTestStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Settings => "settings",
            Self::PacketFlow => "encoded packet flow",
            Self::ClipStore => "Replay folder",
            Self::ContainerCommit => "container commit",
            Self::Readback => "bounded readback",
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayFoundationSelfTestError {
    pub stage: ReplayFoundationSelfTestStage,
    message: String,
}

impl ReplayFoundationSelfTestError {
    pub(crate) fn settings(error: &impl fmt::Display) -> Self {
        Self::new(ReplayFoundationSelfTestStage::Settings, error)
    }

    fn new(stage: ReplayFoundationSelfTestStage, error: &impl fmt::Display) -> Self {
        Self {
            stage,
            message: error.to_string(),
        }
    }
}

impl fmt::Display for ReplayFoundationSelfTestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "replay foundation self-test failed during {}: {}",
            self.stage, self.message
        )
    }
}

impl std::error::Error for ReplayFoundationSelfTestError {}

/// Result of one bounded generated-packet foundation exercise.
///
/// The report intentionally carries the closed production readiness state.
/// Structurally valid packet framing and a durable Matroska commit do not prove
/// hardware encoding, decoded playback, live game transfer, or performance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayFoundationSelfTestReport {
    pub settings: ReplaySettings,
    pub budget: ReplayBudget,
    pub packet_count: u32,
    pub encoded_bytes: u64,
    pub synthetic_duration_ns: u64,
    pub stored_clip: StoredReplayClip,
    pub storage: ReplayStorageStatus,
    pub backend_readiness: ReplayBackendReadiness,
}

/// Exercise the daemon's already-encoded packet path through a durable private
/// clip commit using a tiny generated H.264-shaped packet sequence.
///
/// No frame capture, GPU access, encoder process, audio source, software codec,
/// or unbounded payload is used. `directory` is initialized as a real
/// Redunar-owned replay store and therefore should be a dedicated empty
/// diagnostic path, normally below `/tmp`.
///
/// # Errors
///
/// Returns [`ReplayFoundationSelfTestError`] if packet validation, bounded ring
/// insertion, store initialization, streaming mux/commit, or four-byte
/// Matroska signature readback fails.
pub fn run_replay_foundation_self_test(
    directory: impl Into<PathBuf>,
    settings: ReplaySettings,
) -> Result<ReplayFoundationSelfTestReport, ReplayFoundationSelfTestError> {
    let budget = ReplayBudget::from_settings(settings);
    let frames_per_second = settings.frame_rate.frames_per_second();
    let frame_duration_ns = NANOSECONDS_PER_SECOND / u64::from(frames_per_second);
    let packet_count = u32::from(frames_per_second) * u32::from(SELF_TEST_SECONDS);
    let stream = ReplayVideoStream::new(
        ReplayVideoCodec::H264,
        ReplayPacketFormat::H264LengthPrefixed4,
        320,
        180,
        u8::try_from(frames_per_second).unwrap_or(60),
        AVC_DECODER_CONFIGURATION.to_vec(),
    )
    .map_err(|error| {
        ReplayFoundationSelfTestError::new(ReplayFoundationSelfTestStage::PacketFlow, &error)
    })?;
    let mut flow = ReplayPacketFlow::new(settings, stream);

    for frame in 0..packet_count {
        let keyframe = frame.is_multiple_of(u32::from(frames_per_second));
        let nal_type = if keyframe { 0x65 } else { 0x41 };
        let packet = EncodedReplayPacket::new(
            u64::from(frame).saturating_mul(frame_duration_ns),
            frame_duration_ns,
            keyframe,
            vec![0, 0, 0, 2, nal_type, 0x88],
        )
        .map_err(|error| {
            ReplayFoundationSelfTestError::new(ReplayFoundationSelfTestStage::PacketFlow, &error)
        })?;
        let batch = EncodedPacketBatch::new(vec![packet]).map_err(|error| {
            ReplayFoundationSelfTestError::new(ReplayFoundationSelfTestStage::PacketFlow, &error)
        })?;
        flow.push_batch(batch).map_err(|error| {
            ReplayFoundationSelfTestError::new(ReplayFoundationSelfTestStage::PacketFlow, &error)
        })?;
    }

    let ring_stats = flow.ring().stats();
    let directory = directory.into();
    let store = ReplayClipStore::open(&directory, budget).map_err(|error| {
        ReplayFoundationSelfTestError::new(ReplayFoundationSelfTestStage::ClipStore, &error)
    })?;
    let stored_clip = store
        .save_video_only(flow.stream(), flow.ring())
        .map_err(|error| {
            ReplayFoundationSelfTestError::new(
                ReplayFoundationSelfTestStage::ContainerCommit,
                &error,
            )
        })?;

    let mut signature = [0_u8; MATROSKA_MAGIC.len()];
    File::open(&stored_clip.path)
        .and_then(|mut clip| clip.read_exact(&mut signature))
        .map_err(|error| {
            ReplayFoundationSelfTestError::new(ReplayFoundationSelfTestStage::Readback, &error)
        })?;
    if signature != MATROSKA_MAGIC {
        return Err(ReplayFoundationSelfTestError::new(
            ReplayFoundationSelfTestStage::Readback,
            &"committed clip has an invalid Matroska signature",
        ));
    }
    let storage = ReplayClipStore::inspect(&directory, budget).map_err(|error| {
        ReplayFoundationSelfTestError::new(ReplayFoundationSelfTestStage::Readback, &error)
    })?;
    if !storage.initialized
        || storage.owned_clips != 1
        || storage.used_bytes != stored_clip.bytes
        || storage.bytes_over_limit != 0
    {
        return Err(ReplayFoundationSelfTestError::new(
            ReplayFoundationSelfTestStage::Readback,
            &"committed clip inventory does not match the self-test output",
        ));
    }

    Ok(ReplayFoundationSelfTestReport {
        settings,
        budget,
        packet_count: ring_stats.stored_packets,
        encoded_bytes: ring_stats.stored_bytes,
        synthetic_duration_ns: u64::from(SELF_TEST_SECONDS) * NANOSECONDS_PER_SECOND,
        stored_clip,
        storage,
        backend_readiness: ReplayBackendReadiness::headless_foundation(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use redunar_core::{ReplayDuration, ReplayFrameRate, ReplayQuality, ReplayStorageLimit};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "redunar-replay-self-test-{}-{id}",
                std::process::id()
            )))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn generated_packets_reach_one_private_bounded_clip_without_opening_gates() {
        let fixture = Fixture::new();
        let settings = ReplaySettings {
            duration: ReplayDuration::Seconds15,
            frame_rate: ReplayFrameRate::Fps30,
            quality: ReplayQuality::Efficient,
            storage_limit: ReplayStorageLimit::GiB5,
        };
        let report = run_replay_foundation_self_test(&fixture.0, settings).expect("self-test");
        assert_eq!(report.packet_count, 60);
        assert_eq!(report.encoded_bytes, 360);
        assert_eq!(report.synthetic_duration_ns, 2_000_000_000);
        assert!(report.stored_clip.path.is_file());
        assert_eq!(report.storage.owned_clips, 1);
        assert_eq!(report.storage.used_bytes, report.stored_clip.bytes);
        assert!(report.stored_clip.bytes < report.budget.maximum_ring_bytes);
        assert!(!report.backend_readiness.activation_allowed());
    }

    #[test]
    fn self_test_refuses_to_claim_an_unowned_output_directory() {
        let fixture = Fixture::new();
        fs::create_dir_all(&fixture.0).expect("fixture directory");
        fs::write(fixture.0.join("keep"), b"user").expect("user file");
        let error = run_replay_foundation_self_test(&fixture.0, ReplaySettings::default())
            .expect_err("unowned directory must be rejected");
        assert_eq!(error.stage, ReplayFoundationSelfTestStage::ClipStore);
        assert_eq!(fs::read(fixture.0.join("keep")).unwrap(), b"user");
    }
}
