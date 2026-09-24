use redunar_capture::{
    CaptureApi, CaptureMessage, CaptureSessionId, OverlayRuntimeStatus, ReplaySourceCandidate,
    ReplaySourceRejection,
};
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::time::Duration;

pub const LIVE_FRAME_HISTORY_CAPACITY: usize = 240;
const REPLAY_SOURCE_HISTORY_CAPACITY: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapturePhase {
    Armed,
    Capturing,
    Completed,
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaptureFrameMetrics {
    pub average_fps: f64,
    pub one_percent_low_fps: f64,
    pub point_one_percent_low_fps: f64,
    pub newest_frame_time_ms: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaptureSnapshot {
    pub revision: u64,
    pub session_id: CaptureSessionId,
    pub phase: CapturePhase,
    pub producer_process_id: Option<u32>,
    pub capture_api: Option<CaptureApi>,
    pub received_frame_count: u64,
    pub dropped_frame_count: u64,
    pub rejected_message_count: u64,
    pub recent_frame_intervals_ns: Vec<u64>,
    pub metrics: Option<CaptureFrameMetrics>,
    pub replay_source_candidate: Option<ReplaySourceCandidate>,
    pub replay_source_rejection: Option<ReplaySourceRejection>,
    pub replay_copied_frame_count: u64,
    pub replay_latest_copy_source: Option<ReplaySourceCandidate>,
    pub replay_latest_copied_bytes: Option<u32>,
    pub replay_latest_sample_checksum: Option<u64>,
    pub replay_exported_frame_count: u64,
    pub replay_latest_export_fd: Option<u32>,
    pub replay_latest_export_sequence: Option<u64>,
    pub replay_latest_export_source: Option<ReplaySourceCandidate>,
    pub replay_latest_export_offset: Option<u32>,
    pub replay_latest_export_stride: Option<u32>,
    pub replay_latest_export_modifier: Option<u64>,
    pub replay_latest_export_timestamp_ns: Option<u64>,
    pub replay_latest_export_duration_ns: Option<u64>,
    /// Runtime-reported overlay state. `None` means no overlay signal was
    /// received; a saved/requested profile is not enough to populate it.
    pub overlay_status: Option<OverlayRuntimeStatus>,
    pub failure: Option<String>,
}

/// Daemon-owned validation and bounded history for one capture producer.
/// Transport code passes only successfully decoded protocol messages here.
pub struct CaptureSessionModel {
    session_id: CaptureSessionId,
    phase: CapturePhase,
    producer_process_id: Option<u32>,
    capture_api: Option<CaptureApi>,
    next_sequence: Option<u64>,
    received_frame_count: u64,
    dropped_frame_count: u64,
    rejected_message_count: u64,
    history: VecDeque<u64>,
    replay_source_candidate: Option<ReplaySourceCandidate>,
    replay_announced_sources: VecDeque<ReplaySourceCandidate>,
    replay_source_rejection: Option<ReplaySourceRejection>,
    replay_last_copy_sequence: Option<u64>,
    replay_copied_frame_count: u64,
    replay_latest_copy_source: Option<ReplaySourceCandidate>,
    replay_latest_copied_bytes: Option<u32>,
    replay_latest_sample_checksum: Option<u64>,
    replay_exported_frame_count: u64,
    replay_latest_export_fd: Option<u32>,
    replay_latest_export_sequence: Option<u64>,
    replay_latest_export_source: Option<ReplaySourceCandidate>,
    replay_latest_export_offset: Option<u32>,
    replay_latest_export_stride: Option<u32>,
    replay_latest_export_modifier: Option<u64>,
    replay_latest_export_timestamp_ns: Option<u64>,
    replay_latest_export_duration_ns: Option<u64>,
    overlay_status: Option<OverlayRuntimeStatus>,
    revision: u64,
    failure: Option<String>,
}

impl CaptureSessionModel {
    #[must_use]
    pub fn new(session_id: CaptureSessionId) -> Self {
        Self {
            session_id,
            phase: CapturePhase::Armed,
            producer_process_id: None,
            capture_api: None,
            next_sequence: None,
            received_frame_count: 0,
            dropped_frame_count: 0,
            rejected_message_count: 0,
            history: VecDeque::with_capacity(LIVE_FRAME_HISTORY_CAPACITY),
            replay_source_candidate: None,
            replay_announced_sources: VecDeque::with_capacity(REPLAY_SOURCE_HISTORY_CAPACITY),
            replay_source_rejection: None,
            replay_last_copy_sequence: None,
            replay_copied_frame_count: 0,
            replay_latest_copy_source: None,
            replay_latest_copied_bytes: None,
            replay_latest_sample_checksum: None,
            replay_exported_frame_count: 0,
            replay_latest_export_fd: None,
            replay_latest_export_sequence: None,
            replay_latest_export_source: None,
            replay_latest_export_offset: None,
            replay_latest_export_stride: None,
            replay_latest_export_modifier: None,
            replay_latest_export_timestamp_ns: None,
            replay_latest_export_duration_ns: None,
            overlay_status: None,
            revision: 0,
            failure: None,
        }
    }

    /// Apply one decoded producer message.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureStateError`] for a different session, an invalid phase
    /// transition, duplicate/out-of-order data, or impossible sequence values.
    pub fn accept(&mut self, message: CaptureMessage) -> Result<(), CaptureStateError> {
        if message.session_id() != self.session_id {
            return self.reject("capture message belongs to a different session");
        }

        match message {
            CaptureMessage::Hello {
                process_id,
                api,
                producer_started_monotonic_ns,
                ..
            } => self.accept_hello(process_id, api, producer_started_monotonic_ns),
            CaptureMessage::FrameBatch {
                api,
                first_sequence,
                frame_intervals_ns,
                ..
            } => self.accept_frames(api, first_sequence, &frame_intervals_ns),
            CaptureMessage::Goodbye {
                api, last_sequence, ..
            } => self.accept_goodbye(api, last_sequence),
            CaptureMessage::ReplaySourceCandidate { candidate, .. } => {
                self.accept_replay_source_candidate(candidate)
            }
            CaptureMessage::ReplaySourceRejected { reason, .. } => {
                self.accept_replay_source_rejection(reason)
            }
            CaptureMessage::ReplayFrameCopied {
                sequence,
                source,
                copied_bytes,
                sample_checksum,
                ..
            } => self.accept_replay_frame_copied(sequence, source, copied_bytes, sample_checksum),
            CaptureMessage::ReplayFrameExported {
                sequence,
                fd_number,
                source,
                offset,
                stride,
                modifier,
                timestamp_ns,
                duration_ns,
                ..
            } => self.accept_replay_frame_exported(
                sequence,
                fd_number,
                source,
                offset,
                stride,
                modifier,
                timestamp_ns,
                duration_ns,
            ),
            CaptureMessage::ReplayFrameReleased { .. } => {
                self.reject("replay frame release is daemon-to-producer only")
            }
            CaptureMessage::OverlayStatus { status, .. } => self.accept_overlay_status(status),
        }
    }

    pub(crate) fn accept_frame_batch(
        &mut self,
        session_id: CaptureSessionId,
        api: CaptureApi,
        first_sequence: u64,
        frame_intervals_ns: &[u64],
    ) -> Result<(), CaptureStateError> {
        if session_id != self.session_id {
            return self.reject("capture message belongs to a different session");
        }
        self.accept_frames(api, first_sequence, frame_intervals_ns)
    }

    /// Mark an active session failed after a transport or producer error.
    pub fn fail(&mut self, message: impl Into<String>) {
        if !matches!(self.phase, CapturePhase::Completed | CapturePhase::Failed) {
            self.phase = CapturePhase::Failed;
            self.failure = Some(message.into());
            self.bump_revision();
        }
    }

    /// Count a malformed or oversized transport message without changing the
    /// active capture phase. One bad datagram must not stop a valid producer.
    pub fn reject_transport_message(&mut self) {
        self.rejected_message_count = self.rejected_message_count.saturating_add(1);
        self.bump_revision();
    }

    #[must_use]
    pub fn snapshot(&self) -> CaptureSnapshot {
        let recent_frame_intervals_ns = self.history.iter().copied().collect::<Vec<_>>();
        CaptureSnapshot {
            revision: self.revision,
            session_id: self.session_id,
            phase: self.phase,
            producer_process_id: self.producer_process_id,
            capture_api: self.capture_api,
            received_frame_count: self.received_frame_count,
            dropped_frame_count: self.dropped_frame_count,
            rejected_message_count: self.rejected_message_count,
            metrics: summarize(&recent_frame_intervals_ns),
            recent_frame_intervals_ns,
            replay_source_candidate: self.replay_source_candidate,
            replay_source_rejection: self.replay_source_rejection,
            replay_copied_frame_count: self.replay_copied_frame_count,
            replay_latest_copy_source: self.replay_latest_copy_source,
            replay_latest_copied_bytes: self.replay_latest_copied_bytes,
            replay_latest_sample_checksum: self.replay_latest_sample_checksum,
            replay_exported_frame_count: self.replay_exported_frame_count,
            replay_latest_export_fd: self.replay_latest_export_fd,
            replay_latest_export_sequence: self.replay_latest_export_sequence,
            replay_latest_export_source: self.replay_latest_export_source,
            replay_latest_export_offset: self.replay_latest_export_offset,
            replay_latest_export_stride: self.replay_latest_export_stride,
            replay_latest_export_modifier: self.replay_latest_export_modifier,
            replay_latest_export_timestamp_ns: self.replay_latest_export_timestamp_ns,
            replay_latest_export_duration_ns: self.replay_latest_export_duration_ns,
            overlay_status: self.overlay_status,
            failure: self.failure.clone(),
        }
    }

    fn accept_hello(
        &mut self,
        process_id: u32,
        api: CaptureApi,
        producer_started_monotonic_ns: u64,
    ) -> Result<(), CaptureStateError> {
        if !matches!(self.phase, CapturePhase::Armed | CapturePhase::Completed) {
            return self.reject("capture hello is only valid while armed");
        }
        if process_id == 0 || producer_started_monotonic_ns == 0 {
            return self.reject("capture hello contains invalid producer identity");
        }
        if self
            .producer_process_id
            .is_some_and(|existing| existing != process_id)
        {
            return self.reject("capture session cannot change producer process");
        }
        self.phase = CapturePhase::Capturing;
        self.producer_process_id = Some(process_id);
        self.capture_api = Some(api);
        self.next_sequence = None;
        self.replay_source_candidate = None;
        self.replay_announced_sources.clear();
        self.replay_source_rejection = None;
        self.replay_last_copy_sequence = None;
        self.replay_copied_frame_count = 0;
        self.replay_latest_copy_source = None;
        self.replay_latest_copied_bytes = None;
        self.replay_latest_sample_checksum = None;
        self.replay_exported_frame_count = 0;
        self.replay_latest_export_fd = None;
        self.replay_latest_export_sequence = None;
        self.replay_latest_export_source = None;
        self.replay_latest_export_offset = None;
        self.replay_latest_export_stride = None;
        self.replay_latest_export_modifier = None;
        self.replay_latest_export_timestamp_ns = None;
        self.replay_latest_export_duration_ns = None;
        self.overlay_status = None;
        self.bump_revision();
        Ok(())
    }

    fn accept_frames(
        &mut self,
        api: CaptureApi,
        first_sequence: u64,
        frame_intervals_ns: &[u64],
    ) -> Result<(), CaptureStateError> {
        if self.phase != CapturePhase::Capturing {
            return self.reject("capture frames require an active producer");
        }
        if self.capture_api != Some(api) {
            return self.reject("capture frames do not match the active graphics API");
        }
        if frame_intervals_ns.is_empty() || frame_intervals_ns.contains(&0) {
            return self.reject("capture frames must contain positive intervals");
        }
        if let Some(expected) = self.next_sequence {
            if first_sequence < expected {
                return self.reject("capture frames are duplicate or out of order");
            }
            self.dropped_frame_count = self
                .dropped_frame_count
                .saturating_add(first_sequence.saturating_sub(expected));
        } else {
            self.dropped_frame_count = self.dropped_frame_count.saturating_add(first_sequence);
        }
        let frame_count = u64::try_from(frame_intervals_ns.len())
            .map_err(|_| CaptureStateError::new("capture frame count cannot fit sequence"))?;
        let next_sequence = first_sequence
            .checked_add(frame_count)
            .ok_or_else(|| CaptureStateError::new("capture frame sequence overflowed"))?;

        for interval in frame_intervals_ns {
            if self.history.len() == LIVE_FRAME_HISTORY_CAPACITY {
                self.history.pop_front();
            }
            self.history.push_back(*interval);
        }
        self.received_frame_count = self.received_frame_count.saturating_add(frame_count);
        self.next_sequence = Some(next_sequence);
        self.bump_revision();
        Ok(())
    }

    fn accept_goodbye(
        &mut self,
        api: CaptureApi,
        last_sequence: u64,
    ) -> Result<(), CaptureStateError> {
        if self.phase != CapturePhase::Capturing {
            return self.reject("capture goodbye requires an active producer");
        }
        if self.capture_api != Some(api) {
            return self.reject("capture goodbye does not match the active graphics API");
        }
        if let Some(expected) = self.next_sequence {
            let accepted_last = expected.saturating_sub(1);
            if last_sequence < accepted_last {
                return self.reject("capture goodbye precedes accepted frame data");
            }
            self.dropped_frame_count = self
                .dropped_frame_count
                .saturating_add(last_sequence.saturating_sub(accepted_last));
        } else if last_sequence > 0 {
            self.dropped_frame_count = self
                .dropped_frame_count
                .saturating_add(last_sequence.saturating_add(1));
        }
        self.phase = CapturePhase::Completed;
        self.bump_revision();
        Ok(())
    }

    fn accept_replay_source_candidate(
        &mut self,
        candidate: ReplaySourceCandidate,
    ) -> Result<(), CaptureStateError> {
        if self.phase != CapturePhase::Capturing {
            return self.reject("replay source candidate requires an active producer");
        }
        if !self.replay_announced_sources.contains(&candidate) {
            if self.replay_announced_sources.len() == REPLAY_SOURCE_HISTORY_CAPACITY {
                self.replay_announced_sources.pop_front();
            }
            self.replay_announced_sources.push_back(candidate);
        }
        let new_area = u64::from(candidate.width).saturating_mul(u64::from(candidate.height));
        let current_area = self.replay_source_candidate.map_or(0, |current| {
            u64::from(current.width).saturating_mul(u64::from(current.height))
        });
        if new_area > current_area {
            self.replay_source_candidate = Some(candidate);
            self.replay_source_rejection = None;
            self.bump_revision();
        }
        Ok(())
    }

    fn accept_replay_source_rejection(
        &mut self,
        reason: ReplaySourceRejection,
    ) -> Result<(), CaptureStateError> {
        if self.phase != CapturePhase::Capturing {
            return self.reject("replay source rejection requires an active producer");
        }
        if self.replay_source_candidate.is_none() && self.replay_source_rejection != Some(reason) {
            self.replay_source_rejection = Some(reason);
            self.bump_revision();
        }
        Ok(())
    }

    fn accept_replay_frame_copied(
        &mut self,
        sequence: u64,
        source: ReplaySourceCandidate,
        copied_bytes: u32,
        sample_checksum: u64,
    ) -> Result<(), CaptureStateError> {
        if self.phase != CapturePhase::Capturing || !self.replay_announced_sources.contains(&source)
        {
            return self.reject("replay frame copy requires an active eligible source");
        }
        let expected_bytes = source
            .width
            .checked_mul(source.height)
            .and_then(|pixels| pixels.checked_mul(4));
        if expected_bytes != Some(copied_bytes)
            || self
                .replay_last_copy_sequence
                .is_some_and(|previous| sequence <= previous)
        {
            return self.reject("replay frame copy metadata is invalid or out of order");
        }
        self.replay_last_copy_sequence = Some(sequence);
        self.replay_copied_frame_count = self.replay_copied_frame_count.saturating_add(1);
        self.replay_latest_copy_source = Some(source);
        self.replay_latest_copied_bytes = Some(copied_bytes);
        self.replay_latest_sample_checksum = Some(sample_checksum);
        self.bump_revision();
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the fixed capture protocol keeps every bounded export field explicit"
    )]
    fn accept_replay_frame_exported(
        &mut self,
        sequence: u64,
        fd_number: u32,
        source: ReplaySourceCandidate,
        offset: u32,
        stride: u32,
        modifier: u64,
        timestamp_ns: u64,
        duration_ns: u64,
    ) -> Result<(), CaptureStateError> {
        // Both native producers export DMA-BUF descriptors through the same
        // SCM_RIGHTS contract; the GL producer uses GBM RA24 (LINEAR) and the
        // Vulkan producer uses its LinearBuffer allocation. The metadata
        // checks below, not the backend identity, guard the route.
        if !matches!(
            self.capture_api,
            Some(CaptureApi::Vulkan | CaptureApi::OpenGl)
        ) {
            return self.reject("replay frame export requires a native capture backend");
        }
        if self.phase != CapturePhase::Capturing
            || !self.replay_announced_sources.contains(&source)
            || fd_number < 3
            || stride < source.width.saturating_mul(4)
            || offset > source.height.saturating_mul(stride)
            || duration_ns == 0
            || timestamp_ns.checked_add(duration_ns).is_none()
            || self
                .replay_latest_export_sequence
                .is_some_and(|previous| sequence <= previous)
        {
            return self.reject("replay frame export metadata is invalid or out of order");
        }
        self.replay_latest_export_sequence = Some(sequence);
        self.replay_latest_export_fd = Some(fd_number);
        self.replay_latest_export_source = Some(source);
        self.replay_latest_export_offset = Some(offset);
        self.replay_latest_export_stride = Some(stride);
        self.replay_latest_export_modifier = Some(modifier);
        self.replay_latest_export_timestamp_ns = Some(timestamp_ns);
        self.replay_latest_export_duration_ns = Some(duration_ns);
        self.replay_exported_frame_count = self.replay_exported_frame_count.saturating_add(1);
        self.bump_revision();
        Ok(())
    }

    fn accept_overlay_status(
        &mut self,
        status: OverlayRuntimeStatus,
    ) -> Result<(), CaptureStateError> {
        if self.capture_api.is_none() {
            return self.reject("overlay status requires a capture backend");
        }
        if self.phase != CapturePhase::Capturing {
            return self.reject("overlay status requires an active producer");
        }
        let transition_is_valid = match (self.overlay_status, status) {
            (None, OverlayRuntimeStatus::Requested)
            | (Some(OverlayRuntimeStatus::Requested | OverlayRuntimeStatus::Error(_)), _)
            | (
                Some(OverlayRuntimeStatus::Active),
                OverlayRuntimeStatus::Active | OverlayRuntimeStatus::Error(_),
            ) => true,
            (None, OverlayRuntimeStatus::Active | OverlayRuntimeStatus::Error(_))
            | (Some(OverlayRuntimeStatus::Active), OverlayRuntimeStatus::Requested) => false,
        };
        if !transition_is_valid {
            return self.reject("overlay status transition is invalid");
        }
        if self.overlay_status != Some(status) {
            self.overlay_status = Some(status);
            self.bump_revision();
        }
        Ok(())
    }

    fn reject<T>(&mut self, message: &'static str) -> Result<T, CaptureStateError> {
        self.rejected_message_count = self.rejected_message_count.saturating_add(1);
        self.bump_revision();
        Err(CaptureStateError::new(message))
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

fn summarize(intervals_ns: &[u64]) -> Option<CaptureFrameMetrics> {
    let newest = *intervals_ns.last()?;
    let mut slowest_first = intervals_ns.to_vec();
    slowest_first.sort_unstable_by(|left, right| right.cmp(left));
    Some(CaptureFrameMetrics {
        average_fps: fps_from_intervals(intervals_ns),
        one_percent_low_fps: slowest_fps(&slowest_first, 100),
        point_one_percent_low_fps: slowest_fps(&slowest_first, 1_000),
        newest_frame_time_ms: Duration::from_nanos(newest).as_secs_f64() * 1_000.0,
    })
}

fn slowest_fps(slowest_first: &[u64], denominator: usize) -> f64 {
    let count = slowest_first.len().div_ceil(denominator).max(1);
    fps_from_intervals(&slowest_first[..count])
}

fn fps_from_intervals(intervals_ns: &[u64]) -> f64 {
    let (seconds, count) = intervals_ns
        .iter()
        .fold((0.0, 0.0), |(seconds, count), value| {
            (
                seconds + Duration::from_nanos(*value).as_secs_f64(),
                count + 1.0,
            )
        });
    count / seconds
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureStateError {
    message: &'static str,
}

impl CaptureStateError {
    const fn new(message: &'static str) -> Self {
        Self { message }
    }
}

impl fmt::Display for CaptureStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl Error for CaptureStateError {}

#[cfg(test)]
mod tests {
    use super::*;
    use redunar_capture::{CaptureApi, GoodbyeReason, ReplayPixelFormat, ReplaySourceRejection};

    fn session_id() -> CaptureSessionId {
        CaptureSessionId::new([3; 16]).expect("session id")
    }

    fn hello() -> CaptureMessage {
        CaptureMessage::Hello {
            session_id: session_id(),
            process_id: 42,
            api: CaptureApi::Vulkan,
            producer_started_monotonic_ns: 1,
        }
    }

    #[test]
    fn session_follows_guarded_lifecycle_and_calculates_metrics() {
        let mut session = CaptureSessionModel::new(session_id());
        assert_eq!(session.snapshot().phase, CapturePhase::Armed);
        session.accept(hello()).expect("accept hello");
        session
            .accept(CaptureMessage::FrameBatch {
                session_id: session_id(),
                api: CaptureApi::Vulkan,
                first_sequence: 10,
                frame_intervals_ns: vec![10_000_000; 100],
            })
            .expect("accept frames");
        session
            .accept(CaptureMessage::Goodbye {
                session_id: session_id(),
                api: CaptureApi::Vulkan,
                last_sequence: 109,
                reason: GoodbyeReason::Normal,
            })
            .expect("accept goodbye");

        let snapshot = session.snapshot();
        assert_eq!(snapshot.phase, CapturePhase::Completed);
        assert_eq!(snapshot.capture_api, Some(CaptureApi::Vulkan));
        assert_eq!(snapshot.received_frame_count, 100);
        assert_eq!(snapshot.recent_frame_intervals_ns.len(), 100);
        let metrics = snapshot.metrics.expect("frame metrics");
        assert!((metrics.average_fps - 100.0).abs() < 0.001);
        assert!((metrics.one_percent_low_fps - 100.0).abs() < 0.001);
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the protocol fixture checks the full OpenGL copy and exported-frame sequence"
    )]
    fn opengl_producer_uses_diagnostic_copies_and_the_shared_dma_buf_export_route() {
        let mut session = CaptureSessionModel::new(session_id());
        session
            .accept(CaptureMessage::Hello {
                session_id: session_id(),
                process_id: 42,
                api: CaptureApi::OpenGl,
                producer_started_monotonic_ns: 1,
            })
            .expect("OpenGL hello");
        session
            .accept(CaptureMessage::FrameBatch {
                session_id: session_id(),
                api: CaptureApi::OpenGl,
                first_sequence: 0,
                frame_intervals_ns: vec![16_000_000; 16],
            })
            .expect("OpenGL frame metrics");
        assert!(
            session
                .accept(CaptureMessage::FrameBatch {
                    session_id: session_id(),
                    api: CaptureApi::Vulkan,
                    first_sequence: 16,
                    frame_intervals_ns: vec![16_000_000],
                })
                .is_err(),
            "a second injected API must not corrupt the selected producer timeline"
        );
        assert!(
            session
                .accept(CaptureMessage::Goodbye {
                    session_id: session_id(),
                    api: CaptureApi::Vulkan,
                    last_sequence: 16,
                    reason: GoodbyeReason::Normal,
                })
                .is_err(),
            "a second injected API must not complete the selected producer"
        );
        let source = ReplaySourceCandidate {
            width: 640,
            height: 480,
            pixel_format: redunar_capture::ReplayPixelFormat::Rgba8Unorm,
            target_frames_per_second: 60,
        };
        session
            .accept(CaptureMessage::ReplaySourceCandidate {
                session_id: session_id(),
                candidate: source,
            })
            .expect("OpenGL diagnostic source");
        session
            .accept(CaptureMessage::ReplayFrameCopied {
                session_id: session_id(),
                sequence: 0,
                source,
                copied_bytes: 640 * 480 * 4,
                sample_checksum: 0x1234,
            })
            .expect("OpenGL diagnostic copy");
        session
            .accept(CaptureMessage::ReplayFrameExported {
                session_id: session_id(),
                sequence: 1,
                fd_number: 7,
                source,
                offset: 0,
                stride: 640 * 4,
                modifier: 0,
                timestamp_ns: 1,
                duration_ns: 16_666_667,
            })
            .expect("OpenGL DMA-BUF export accepted");
        // A descriptor below the protocol floor is still rejected regardless
        // of backend, so the shared route keeps its ownership guard.
        assert!(
            session
                .accept(CaptureMessage::ReplayFrameExported {
                    session_id: session_id(),
                    sequence: 2,
                    fd_number: 2,
                    source,
                    offset: 0,
                    stride: 640 * 4,
                    modifier: 0,
                    timestamp_ns: 2,
                    duration_ns: 16_666_667,
                })
                .is_err(),
            "invalid descriptors are rejected on every backend"
        );
        session
            .accept(CaptureMessage::OverlayStatus {
                session_id: session_id(),
                status: OverlayRuntimeStatus::Requested,
            })
            .expect("OpenGL overlay requested");
        session
            .accept(CaptureMessage::OverlayStatus {
                session_id: session_id(),
                status: OverlayRuntimeStatus::Active,
            })
            .expect("OpenGL overlay active");

        let snapshot = session.snapshot();
        assert_eq!(snapshot.capture_api, Some(CaptureApi::OpenGl));
        assert_eq!(snapshot.received_frame_count, 16);
        assert!(snapshot.metrics.is_some());
        assert_eq!(snapshot.overlay_status, Some(OverlayRuntimeStatus::Active));
        assert_eq!(snapshot.replay_source_candidate, Some(source));
        assert_eq!(snapshot.replay_copied_frame_count, 1);
        // The fd_number=2 probe above was rejected, so only the first export
        // is counted.
        assert_eq!(snapshot.replay_exported_frame_count, 1);
    }

    #[test]
    fn history_is_fixed_capacity_and_sequence_gaps_are_counted() {
        let mut session = CaptureSessionModel::new(session_id());
        session.accept(hello()).expect("accept hello");
        session
            .accept(CaptureMessage::FrameBatch {
                session_id: session_id(),
                api: CaptureApi::Vulkan,
                first_sequence: 0,
                frame_intervals_ns: vec![10_000_000; 128],
            })
            .expect("first batch");
        session
            .accept(CaptureMessage::FrameBatch {
                session_id: session_id(),
                api: CaptureApi::Vulkan,
                first_sequence: 130,
                frame_intervals_ns: vec![20_000_000; 128],
            })
            .expect("second batch");

        let snapshot = session.snapshot();
        assert_eq!(snapshot.received_frame_count, 256);
        assert_eq!(snapshot.dropped_frame_count, 2);
        assert_eq!(
            snapshot.recent_frame_intervals_ns.len(),
            LIVE_FRAME_HISTORY_CAPACITY
        );
        assert_eq!(snapshot.recent_frame_intervals_ns[0], 10_000_000);
        assert_eq!(snapshot.recent_frame_intervals_ns[112], 20_000_000);
    }

    #[test]
    fn invalid_transitions_and_other_sessions_are_rejected() {
        let mut session = CaptureSessionModel::new(session_id());
        let other = CaptureSessionId::new([4; 16]).expect("other session");
        assert!(
            session
                .accept(CaptureMessage::FrameBatch {
                    session_id: session_id(),
                    api: CaptureApi::Vulkan,
                    first_sequence: 0,
                    frame_intervals_ns: vec![1],
                })
                .is_err()
        );
        assert!(
            session
                .accept(CaptureMessage::Hello {
                    session_id: other,
                    process_id: 42,
                    api: CaptureApi::Vulkan,
                    producer_started_monotonic_ns: 1,
                })
                .is_err()
        );
        assert_eq!(session.snapshot().rejected_message_count, 2);
    }

    #[test]
    fn explicit_failure_is_terminal() {
        let mut session = CaptureSessionModel::new(session_id());
        session.fail("producer did not connect");
        session.fail("ignored later failure");
        let snapshot = session.snapshot();
        assert_eq!(snapshot.phase, CapturePhase::Failed);
        assert_eq!(
            snapshot.failure.as_deref(),
            Some("producer did not connect")
        );
    }

    #[test]
    fn overlay_status_requires_runtime_evidence_and_active_follows_request() {
        let mut session = CaptureSessionModel::new(session_id());
        assert_eq!(session.snapshot().overlay_status, None);
        assert!(
            session
                .accept(CaptureMessage::OverlayStatus {
                    session_id: session_id(),
                    status: OverlayRuntimeStatus::Requested,
                })
                .is_err()
        );

        session.accept(hello()).expect("hello");
        assert!(
            session
                .accept(CaptureMessage::OverlayStatus {
                    session_id: session_id(),
                    status: OverlayRuntimeStatus::Active,
                })
                .is_err()
        );
        session
            .accept(CaptureMessage::OverlayStatus {
                session_id: session_id(),
                status: OverlayRuntimeStatus::Requested,
            })
            .expect("runtime request");
        session
            .accept(CaptureMessage::OverlayStatus {
                session_id: session_id(),
                status: OverlayRuntimeStatus::Active,
            })
            .expect("submitted overlay");
        assert_eq!(
            session.snapshot().overlay_status,
            Some(OverlayRuntimeStatus::Active)
        );
    }

    #[test]
    fn overlay_error_is_bounded_and_can_recover_across_recreation() {
        let mut session = CaptureSessionModel::new(session_id());
        session.accept(hello()).expect("hello");
        session
            .accept(CaptureMessage::OverlayStatus {
                session_id: session_id(),
                status: OverlayRuntimeStatus::Requested,
            })
            .expect("runtime request");
        session
            .accept(CaptureMessage::OverlayStatus {
                session_id: session_id(),
                status: OverlayRuntimeStatus::Error(
                    redunar_capture::OverlayFailureReason::GraphicsQueueUnavailable,
                ),
            })
            .expect("observable renderer error");
        session
            .accept(CaptureMessage::OverlayStatus {
                session_id: session_id(),
                status: OverlayRuntimeStatus::Active,
            })
            .expect("later device can recover");
        assert_eq!(
            session.snapshot().overlay_status,
            Some(OverlayRuntimeStatus::Active)
        );
        session
            .accept(CaptureMessage::OverlayStatus {
                session_id: session_id(),
                status: OverlayRuntimeStatus::Error(
                    redunar_capture::OverlayFailureReason::RendererCapacityReached,
                ),
            })
            .expect("later renderer failure is observable");
        assert_eq!(
            session.snapshot().overlay_status,
            Some(OverlayRuntimeStatus::Error(
                redunar_capture::OverlayFailureReason::RendererCapacityReached,
            ))
        );
        session
            .accept(CaptureMessage::OverlayStatus {
                session_id: session_id(),
                status: OverlayRuntimeStatus::Active,
            })
            .expect("renderer can recover after recreation");
    }

    #[test]
    fn same_process_can_recreate_its_last_vulkan_device() {
        let mut session = CaptureSessionModel::new(session_id());
        session.accept(hello()).expect("first hello");
        session
            .accept(CaptureMessage::FrameBatch {
                session_id: session_id(),
                api: CaptureApi::Vulkan,
                first_sequence: 0,
                frame_intervals_ns: vec![10_000_000; 2],
            })
            .expect("first device frames");
        session
            .accept(CaptureMessage::Goodbye {
                session_id: session_id(),
                api: CaptureApi::Vulkan,
                last_sequence: 1,
                reason: GoodbyeReason::Normal,
            })
            .expect("first device goodbye");

        session.accept(hello()).expect("second hello");
        session
            .accept(CaptureMessage::FrameBatch {
                session_id: session_id(),
                api: CaptureApi::Vulkan,
                first_sequence: 0,
                frame_intervals_ns: vec![20_000_000; 2],
            })
            .expect("second device frames");

        let snapshot = session.snapshot();
        assert_eq!(snapshot.phase, CapturePhase::Capturing);
        assert_eq!(snapshot.received_frame_count, 4);
        assert_eq!(snapshot.dropped_frame_count, 0);
    }

    #[test]
    fn initial_transport_gaps_and_invalid_direct_messages_are_visible() {
        let mut session = CaptureSessionModel::new(session_id());
        session.accept(hello()).expect("hello");
        assert!(
            session
                .accept(CaptureMessage::FrameBatch {
                    session_id: session_id(),
                    api: CaptureApi::Vulkan,
                    first_sequence: 0,
                    frame_intervals_ns: vec![0],
                })
                .is_err()
        );
        session
            .accept(CaptureMessage::FrameBatch {
                session_id: session_id(),
                api: CaptureApi::Vulkan,
                first_sequence: 64,
                frame_intervals_ns: vec![10_000_000; 2],
            })
            .expect("later batch");

        let snapshot = session.snapshot();
        assert_eq!(snapshot.rejected_message_count, 1);
        assert_eq!(snapshot.dropped_frame_count, 64);
    }

    #[test]
    fn replay_source_candidate_requires_a_producer_and_keeps_the_largest_surface() {
        let mut session = CaptureSessionModel::new(session_id());
        let smaller = ReplaySourceCandidate {
            width: 1_280,
            height: 720,
            pixel_format: ReplayPixelFormat::Bgra8Unorm,
            target_frames_per_second: 60,
        };
        assert!(
            session
                .accept(CaptureMessage::ReplaySourceCandidate {
                    session_id: session_id(),
                    candidate: smaller,
                })
                .is_err()
        );
        session.accept(hello()).expect("hello");
        session
            .accept(CaptureMessage::ReplaySourceCandidate {
                session_id: session_id(),
                candidate: smaller,
            })
            .expect("small candidate");
        let larger = ReplaySourceCandidate {
            width: 2_560,
            height: 1_440,
            pixel_format: ReplayPixelFormat::Rgba8Srgb,
            target_frames_per_second: 30,
        };
        session
            .accept(CaptureMessage::ReplaySourceCandidate {
                session_id: session_id(),
                candidate: larger,
            })
            .expect("larger candidate");
        session
            .accept(CaptureMessage::ReplaySourceCandidate {
                session_id: session_id(),
                candidate: smaller,
            })
            .expect("smaller candidate is harmless");
        assert_eq!(session.snapshot().replay_source_candidate, Some(larger));
    }

    #[test]
    fn replay_source_rejection_is_active_only_and_never_overwrites_a_candidate() {
        let mut session = CaptureSessionModel::new(session_id());
        let rejection = CaptureMessage::ReplaySourceRejected {
            session_id: session_id(),
            reason: ReplaySourceRejection::TransferSourceUsageMissing,
        };
        assert!(session.accept(rejection.clone()).is_err());

        session.accept(hello()).expect("hello");
        session.accept(rejection).expect("active rejection");
        assert_eq!(
            session.snapshot().replay_source_rejection,
            Some(ReplaySourceRejection::TransferSourceUsageMissing)
        );

        let candidate = ReplaySourceCandidate {
            width: 1_920,
            height: 1_080,
            pixel_format: ReplayPixelFormat::Bgra8Srgb,
            target_frames_per_second: 60,
        };
        session
            .accept(CaptureMessage::ReplaySourceCandidate {
                session_id: session_id(),
                candidate,
            })
            .expect("candidate");
        session
            .accept(CaptureMessage::ReplaySourceRejected {
                session_id: session_id(),
                reason: ReplaySourceRejection::PixelFormatUnsupported,
            })
            .expect("later rejection is harmless");
        let snapshot = session.snapshot();
        assert_eq!(snapshot.replay_source_candidate, Some(candidate));
        assert_eq!(snapshot.replay_source_rejection, None);
    }

    #[test]
    fn replay_copy_proof_requires_candidate_and_monotonic_sequence() {
        let mut session = CaptureSessionModel::new(session_id());
        session.accept(hello()).expect("hello");
        let source = ReplaySourceCandidate {
            width: 640,
            height: 480,
            pixel_format: ReplayPixelFormat::Bgra8Unorm,
            target_frames_per_second: 60,
        };
        let copied = CaptureMessage::ReplayFrameCopied {
            session_id: session_id(),
            sequence: 1,
            source,
            copied_bytes: 1_228_800,
            sample_checksum: 42,
        };
        assert!(session.accept(copied.clone()).is_err());
        session
            .accept(CaptureMessage::ReplaySourceCandidate {
                session_id: session_id(),
                candidate: source,
            })
            .expect("candidate");
        session.accept(copied.clone()).expect("first copy");
        assert!(session.accept(copied).is_err());
        assert!(
            session
                .accept(CaptureMessage::ReplayFrameCopied {
                    session_id: session_id(),
                    sequence: 2,
                    source: ReplaySourceCandidate {
                        width: 320,
                        height: 240,
                        pixel_format: ReplayPixelFormat::Bgra8Unorm,
                        target_frames_per_second: 60,
                    },
                    copied_bytes: 320 * 240 * 4,
                    sample_checksum: 43,
                })
                .is_err()
        );
        assert!(
            session
                .accept(CaptureMessage::ReplayFrameCopied {
                    session_id: session_id(),
                    sequence: 2,
                    source,
                    copied_bytes: 1,
                    sample_checksum: 43,
                })
                .is_err()
        );
        let snapshot = session.snapshot();
        assert_eq!(snapshot.replay_copied_frame_count, 1);
        assert_eq!(snapshot.replay_latest_copy_source, Some(source));
        assert_eq!(snapshot.replay_latest_copied_bytes, Some(1_228_800));
        assert_eq!(snapshot.replay_latest_sample_checksum, Some(42));

        assert!(
            session
                .accept(CaptureMessage::ReplayFrameExported {
                    session_id: session_id(),
                    sequence: 3,
                    fd_number: 17,
                    source,
                    offset: 0,
                    stride: 640 * 4,
                    modifier: 0,
                    timestamp_ns: 1_000_000_000,
                    duration_ns: 16_666_666,
                })
                .is_ok()
        );
        let snapshot = session.snapshot();
        assert_eq!(snapshot.replay_exported_frame_count, 1);
        assert_eq!(snapshot.replay_latest_export_fd, Some(17));
    }
}
