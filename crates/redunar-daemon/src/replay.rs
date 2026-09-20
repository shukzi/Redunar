use redunar_core::ReplaySettings;
use std::fmt;

const BYTES_PER_MEGABIT: u64 = 1_000_000 / 8;
const CONTAINER_HEADROOM_PERCENT: u64 = 10;

/// Daemon-owned replay lifecycle. No state beyond `Unavailable` is entered
/// until a capture/encoder backend passes Redunar's safety and performance
/// gates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayPhase {
    Unavailable,
    Inactive,
    Buffering,
    Saving,
    Failed,
}

/// Live evidence for the recorder, independent from its requested lifecycle.
/// A Buffering phase alone is not proof that encoded history is advancing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayRecorderHealth {
    Inactive,
    Starting,
    Healthy,
    Stalled,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayFailure {
    FrameSourceLost,
    EncoderFailed,
    ContainerFailed,
    StorageFailed,
    ResourceBudgetExceeded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayUnavailableReason {
    BackendNotVerified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayCapability {
    Unavailable(ReplayUnavailableReason),
    /// Hardware transfer/encode/playback passed controlled validation. The
    /// owner may run a real-game acceptance session, but release performance
    /// verification is still pending.
    ValidationCandidate,
    Available,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayBackendComponent {
    EncodedPacketRing,
    AtomicClipStore,
    LiveFrameTransfer,
    HardwareEncoder,
    PlayableContainer,
    PerformanceBudget,
}

/// Verification bits for every component required before Replay can start.
///
/// The first two bits describe the headless foundation implemented in the
/// daemon. They do not imply that game frames can be recorded yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplayBackendReadiness(u8);

impl ReplayBackendReadiness {
    const ENCODED_PACKET_RING: u8 = 1 << 0;
    const ATOMIC_CLIP_STORE: u8 = 1 << 1;
    const LIVE_FRAME_TRANSFER: u8 = 1 << 2;
    const HARDWARE_ENCODER: u8 = 1 << 3;
    const PLAYABLE_CONTAINER: u8 = 1 << 4;
    const PERFORMANCE_BUDGET: u8 = 1 << 5;
    const REQUIRED: u8 = Self::ENCODED_PACKET_RING
        | Self::ATOMIC_CLIP_STORE
        | Self::LIVE_FRAME_TRANSFER
        | Self::HARDWARE_ENCODER
        | Self::PLAYABLE_CONTAINER
        | Self::PERFORMANCE_BUDGET;
    const VALIDATION_REQUIRED: u8 = Self::REQUIRED & !Self::PERFORMANCE_BUDGET;

    #[must_use]
    pub const fn headless_foundation() -> Self {
        Self(Self::ENCODED_PACKET_RING | Self::ATOMIC_CLIP_STORE)
    }

    #[must_use]
    pub(crate) const fn hardware_validation_candidate() -> Self {
        Self(Self::VALIDATION_REQUIRED)
    }

    #[must_use]
    pub const fn validation_allowed(self) -> bool {
        self.0 & Self::VALIDATION_REQUIRED == Self::VALIDATION_REQUIRED
    }

    /// Construct the closed all-components attestation after an integrated
    /// backend has passed live transfer, decode, and performance validation.
    /// Kept crate-private so a UI preference or device-name probe can never
    /// open the production gate.
    #[must_use]
    #[allow(
        dead_code,
        reason = "reserved for the integrated AMD backend after its live validation gate"
    )]
    pub(crate) const fn fully_verified() -> Self {
        Self(Self::REQUIRED)
    }

    #[must_use]
    pub const fn is_verified(self, component: ReplayBackendComponent) -> bool {
        let bit = match component {
            ReplayBackendComponent::EncodedPacketRing => Self::ENCODED_PACKET_RING,
            ReplayBackendComponent::AtomicClipStore => Self::ATOMIC_CLIP_STORE,
            ReplayBackendComponent::LiveFrameTransfer => Self::LIVE_FRAME_TRANSFER,
            ReplayBackendComponent::HardwareEncoder => Self::HARDWARE_ENCODER,
            ReplayBackendComponent::PlayableContainer => Self::PLAYABLE_CONTAINER,
            ReplayBackendComponent::PerformanceBudget => Self::PERFORMANCE_BUDGET,
        };
        self.0 & bit != 0
    }

    #[must_use]
    pub const fn activation_allowed(self) -> bool {
        self.0 & Self::REQUIRED == Self::REQUIRED
    }

    const fn capability(self) -> ReplayCapability {
        if self.activation_allowed() {
            ReplayCapability::Available
        } else if self.validation_allowed() {
            ReplayCapability::ValidationCandidate
        } else {
            ReplayCapability::Unavailable(ReplayUnavailableReason::BackendNotVerified)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplayBudget {
    pub frame_capacity: u32,
    pub target_bitrate_megabits_per_second: u16,
    /// Upper target for one encoded rolling clip including conservative
    /// container/audio headroom. A backend may use less, never more.
    pub maximum_ring_bytes: u64,
    pub storage_limit_bytes: u64,
    pub minimum_full_clip_capacity: u32,
}

impl ReplayBudget {
    #[must_use]
    pub fn from_settings(settings: ReplaySettings) -> Self {
        let seconds = u64::from(settings.duration.seconds());
        let frames_per_second = u64::from(settings.frame_rate.frames_per_second());
        let bitrate = settings.quality.target_megabits_per_second();
        let video_bytes = seconds
            .saturating_mul(u64::from(bitrate))
            .saturating_mul(BYTES_PER_MEGABIT);
        let maximum_ring_bytes = video_bytes
            .saturating_mul(100 + CONTAINER_HEADROOM_PERCENT)
            .div_ceil(100);
        // Saved clips have no configured total quota. The replay store still
        // applies its filesystem safety reserve before writing another clip.
        let storage_limit_bytes = u64::MAX;
        let full_clips = storage_limit_bytes / maximum_ring_bytes.max(1);

        Self {
            frame_capacity: u32::try_from(seconds.saturating_mul(frames_per_second))
                .unwrap_or(u32::MAX),
            target_bitrate_megabits_per_second: bitrate,
            maximum_ring_bytes,
            storage_limit_bytes,
            minimum_full_clip_capacity: u32::try_from(full_clips).unwrap_or(u32::MAX),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplayRuntimeStatus {
    pub phase: ReplayPhase,
    pub capability: ReplayCapability,
    pub backend_readiness: ReplayBackendReadiness,
    pub last_failure: Option<ReplayFailure>,
    /// Monotonic count of clips durably committed in this process. UI and
    /// overlay presentation use this event edge; it is not storage authority.
    pub completed_save_revision: u64,
    /// Actual keyframe-safe encoded history observed by the asynchronous
    /// spool, measured from packet timestamps rather than wall-clock guesses.
    pub buffered_duration_ns: u64,
    /// Game images handed to the hardware encoder during this session.
    pub received_frame_count: u64,
    /// Encoded packets accepted by the bounded rolling spool in this epoch.
    pub encoded_packet_count: u64,
    /// Game-audio packets accepted by the bounded audio timeline in this epoch.
    pub audio_packet_count: u64,
    /// Bytes retained by the bounded audio timeline in this epoch.
    pub audio_byte_count: u64,
    /// True only while new audio packets have arrived recently.
    pub audio_active: bool,
    pub recorder_health: ReplayRecorderHealth,
    pub settings: ReplaySettings,
    pub budget: ReplayBudget,
}

impl ReplayRuntimeStatus {
    #[must_use]
    pub fn unavailable(settings: ReplaySettings) -> Self {
        let backend_readiness = ReplayBackendReadiness::headless_foundation();
        Self {
            phase: ReplayPhase::Unavailable,
            capability: backend_readiness.capability(),
            backend_readiness,
            last_failure: None,
            completed_save_revision: 0,
            buffered_duration_ns: 0,
            received_frame_count: 0,
            encoded_packet_count: 0,
            audio_packet_count: 0,
            audio_byte_count: 0,
            audio_active: false,
            recorder_health: ReplayRecorderHealth::Inactive,
            settings,
            budget: ReplayBudget::from_settings(settings),
        }
    }

    #[must_use]
    pub fn validation_candidate(settings: ReplaySettings) -> Self {
        let backend_readiness = ReplayBackendReadiness::hardware_validation_candidate();
        Self {
            phase: ReplayPhase::Inactive,
            capability: backend_readiness.capability(),
            backend_readiness,
            last_failure: None,
            completed_save_revision: 0,
            buffered_duration_ns: 0,
            received_frame_count: 0,
            encoded_packet_count: 0,
            audio_packet_count: 0,
            audio_byte_count: 0,
            audio_active: false,
            recorder_health: ReplayRecorderHealth::Inactive,
            settings,
            budget: ReplayBudget::from_settings(settings),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplayStartError;

impl fmt::Display for ReplayStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "instant replay remains unavailable until a capture and encoder backend is verified",
        )
    }
}

impl std::error::Error for ReplayStartError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplayTransitionError;

impl fmt::Display for ReplayTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("instant replay lifecycle transition is invalid")
    }
}

impl std::error::Error for ReplayTransitionError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayStorageAccessError {
    message: String,
}

impl ReplayStorageAccessError {
    pub(crate) fn catalog(error: &impl fmt::Display) -> Self {
        Self {
            message: format!("could not load replay settings: {error}"),
        }
    }

    pub(crate) fn store(error: &impl fmt::Display) -> Self {
        Self {
            message: format!("could not prepare replay storage: {error}"),
        }
    }

    pub(crate) fn preferences(error: &impl fmt::Display) -> Self {
        Self {
            message: format!("could not load Replay preferences: {error}"),
        }
    }

    pub(crate) fn inventory(error: &impl fmt::Display) -> Self {
        Self {
            message: format!("could not inspect saved replay clips: {error}"),
        }
    }

    pub(crate) fn delete(error: &impl fmt::Display) -> Self {
        Self {
            message: format!("could not delete the saved replay clip: {error}"),
        }
    }

    pub(crate) fn archive(error: &impl fmt::Display) -> Self {
        Self {
            message: format!("could not archive the original replay clip: {error}"),
        }
    }
}

impl fmt::Display for ReplayStorageAccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ReplayStorageAccessError {}

/// Refuse activation explicitly rather than allowing the UI or a future caller
/// to infer readiness from installed encoder names alone.
///
/// # Errors
///
/// Returns [`ReplayStartError`] while the backend capability is unavailable.
pub const fn request_start(
    status: ReplayRuntimeStatus,
) -> Result<ReplayRuntimeStatus, ReplayStartError> {
    match (
        status.capability,
        status.backend_readiness.activation_allowed(),
        status.phase,
    ) {
        (ReplayCapability::Available, true, ReplayPhase::Inactive) => Ok(ReplayRuntimeStatus {
            phase: ReplayPhase::Buffering,
            last_failure: None,
            ..status
        }),
        (ReplayCapability::ValidationCandidate, false, ReplayPhase::Inactive)
            if status.backend_readiness.validation_allowed() =>
        {
            Ok(ReplayRuntimeStatus {
                phase: ReplayPhase::Buffering,
                last_failure: None,
                ..status
            })
        }
        _ => Err(ReplayStartError),
    }
}

/// Enter clip finalization without stopping the bounded rolling buffer.
///
/// # Errors
///
/// Returns [`ReplayTransitionError`] unless Replay is currently buffering.
pub const fn begin_save(
    status: ReplayRuntimeStatus,
) -> Result<ReplayRuntimeStatus, ReplayTransitionError> {
    if matches!(status.phase, ReplayPhase::Buffering) {
        Ok(ReplayRuntimeStatus {
            phase: ReplayPhase::Saving,
            ..status
        })
    } else {
        Err(ReplayTransitionError)
    }
}

/// Return to rolling buffering after one durable clip commit.
///
/// # Errors
///
/// Returns [`ReplayTransitionError`] unless Replay is currently saving.
pub const fn complete_save(
    status: ReplayRuntimeStatus,
) -> Result<ReplayRuntimeStatus, ReplayTransitionError> {
    if matches!(status.phase, ReplayPhase::Saving) {
        Ok(ReplayRuntimeStatus {
            phase: ReplayPhase::Buffering,
            completed_save_revision: status.completed_save_revision.saturating_add(1),
            ..status
        })
    } else {
        Err(ReplayTransitionError)
    }
}

/// Record a terminal backend failure. Production callers must release capture
/// and encoder resources before publishing this state.
///
/// # Errors
///
/// Returns [`ReplayTransitionError`] unless Replay was buffering or saving.
pub const fn fail(
    status: ReplayRuntimeStatus,
    failure: ReplayFailure,
) -> Result<ReplayRuntimeStatus, ReplayTransitionError> {
    if matches!(status.phase, ReplayPhase::Buffering | ReplayPhase::Saving) {
        Ok(ReplayRuntimeStatus {
            phase: ReplayPhase::Failed,
            last_failure: Some(failure),
            ..status
        })
    } else {
        Err(ReplayTransitionError)
    }
}

/// Return an active or failed session to inactive after all owned resources
/// have been joined and released.
///
/// # Errors
///
/// Returns [`ReplayTransitionError`] for unavailable or already inactive state.
pub const fn stop(
    status: ReplayRuntimeStatus,
) -> Result<ReplayRuntimeStatus, ReplayTransitionError> {
    if matches!(
        status.phase,
        ReplayPhase::Buffering | ReplayPhase::Saving | ReplayPhase::Failed
    ) {
        Ok(ReplayRuntimeStatus {
            phase: ReplayPhase::Inactive,
            last_failure: None,
            ..status
        })
    } else {
        Err(ReplayTransitionError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use redunar_core::{ReplayDuration, ReplayFrameRate, ReplayQuality, ReplayStorageLimit};

    #[test]
    fn default_budget_is_bounded_and_has_multiple_clip_capacity() {
        let budget = ReplayBudget::from_settings(ReplaySettings::default());
        assert_eq!(budget.frame_capacity, 1_800);
        assert_eq!(budget.target_bitrate_megabits_per_second, 24);
        assert_eq!(budget.maximum_ring_bytes, 99_000_000);
        assert_eq!(budget.storage_limit_bytes, u64::MAX);
        assert!(budget.minimum_full_clip_capacity >= 100);
    }

    #[test]
    fn largest_supported_budget_remains_finite_and_storage_bounded() {
        let budget = ReplayBudget::from_settings(ReplaySettings {
            duration: ReplayDuration::Seconds900,
            frame_rate: ReplayFrameRate::Fps60,
            quality: ReplayQuality::High,
            storage_limit: ReplayStorageLimit::Unlimited,
        });
        assert_eq!(budget.frame_capacity, 54_000);
        assert_eq!(budget.maximum_ring_bytes, 4_950_000_000);
        assert!(budget.maximum_ring_bytes < budget.storage_limit_bytes);
        assert!(budget.minimum_full_clip_capacity > 3_000_000_000);
    }

    #[test]
    fn unavailable_backend_never_enters_buffering() {
        let status = ReplayRuntimeStatus::unavailable(ReplaySettings::default());
        assert_eq!(status.phase, ReplayPhase::Unavailable);
        assert_eq!(request_start(status), Err(ReplayStartError));
    }

    #[test]
    fn foundation_reports_only_the_components_verified_headlessly() {
        let readiness = ReplayBackendReadiness::headless_foundation();
        assert!(readiness.is_verified(ReplayBackendComponent::EncodedPacketRing));
        assert!(readiness.is_verified(ReplayBackendComponent::AtomicClipStore));
        assert!(!readiness.is_verified(ReplayBackendComponent::LiveFrameTransfer));
        assert!(!readiness.is_verified(ReplayBackendComponent::HardwareEncoder));
        assert!(!readiness.is_verified(ReplayBackendComponent::PlayableContainer));
        assert!(!readiness.is_verified(ReplayBackendComponent::PerformanceBudget));
        assert!(!readiness.activation_allowed());
    }

    #[test]
    fn forged_available_flag_cannot_bypass_component_gate() {
        let status = ReplayRuntimeStatus {
            capability: ReplayCapability::Available,
            ..ReplayRuntimeStatus::unavailable(ReplaySettings::default())
        };
        assert_eq!(request_start(status), Err(ReplayStartError));
    }

    #[test]
    fn verified_backend_lifecycle_has_guarded_save_failure_and_stop_transitions() {
        let readiness = ReplayBackendReadiness(ReplayBackendReadiness::REQUIRED);
        let inactive = ReplayRuntimeStatus {
            phase: ReplayPhase::Inactive,
            capability: readiness.capability(),
            backend_readiness: readiness,
            last_failure: None,
            completed_save_revision: 0,
            buffered_duration_ns: 0,
            received_frame_count: 0,
            encoded_packet_count: 0,
            audio_packet_count: 0,
            audio_byte_count: 0,
            audio_active: false,
            recorder_health: ReplayRecorderHealth::Inactive,
            settings: ReplaySettings::default(),
            budget: ReplayBudget::from_settings(ReplaySettings::default()),
        };
        let buffering = request_start(inactive).expect("start verified backend");
        let saving = begin_save(buffering).expect("begin save");
        assert_eq!(begin_save(saving), Err(ReplayTransitionError));
        let resumed = complete_save(saving).expect("complete save");
        assert_eq!(resumed.completed_save_revision, 1);
        let failed = fail(resumed, ReplayFailure::EncoderFailed).expect("record failure");
        assert_eq!(failed.phase, ReplayPhase::Failed);
        assert_eq!(failed.last_failure, Some(ReplayFailure::EncoderFailed));
        let stopped = stop(failed).expect("stop failed session");
        assert_eq!(stopped.phase, ReplayPhase::Inactive);
        assert_eq!(stopped.last_failure, None);
    }
}
