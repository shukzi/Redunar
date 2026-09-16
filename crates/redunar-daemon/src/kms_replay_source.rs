use crate::game_session::ReplayFrameSource;
use crate::{
    DmaBufImagePlane, DmaBufReplayFrame, HardwareEncoderBackend, ReplayRuntimeError,
    vulkan_replay_backend,
};
use redunar_capture_kms::{KmsCapturePlan, KmsDrmCapture, discover_outputs_at};
use redunar_core::ReplaySettings;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

const DIAGNOSTIC_MINIMUM_FRAMES: u16 = 2;
const DIAGNOSTIC_MAXIMUM_FRAMES: u16 = 120;

/// Result of a bounded live KMS-to-Vulkan-Video verification run.
///
/// The diagnostic never maps or saves captured pixels. It only confirms that
/// the current user can export the primary framebuffer and
/// that Redunar's independent GPU pipeline can import, convert, and encode it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KmsReplayLiveDiagnostic {
    pub width: u32,
    pub height: u32,
    pub requested_frames: u16,
    pub completed_frames: u16,
    pub encoded_packets: u32,
    pub encoded_bytes: u64,
    pub keyframes: u16,
    pub elapsed_milliseconds: u64,
}

pub(crate) struct KmsReplaySource {
    settings: ReplaySettings,
    stop: AtomicBool,
    state: Mutex<KmsSourceState>,
}

enum KmsSourceState {
    Uninitialized,
    Active(Box<ActiveKmsSource>),
    Stopped,
}

struct ActiveKmsSource {
    capture: KmsDrmCapture,
    started_at: Instant,
    next_capture_at: Instant,
    frame_duration: Duration,
    sequence: u64,
}

impl KmsReplaySource {
    #[must_use]
    pub(crate) fn new(settings: ReplaySettings) -> Arc<Self> {
        Arc::new(Self {
            settings,
            stop: AtomicBool::new(false),
            state: Mutex::new(KmsSourceState::Uninitialized),
        })
    }

    fn ensure_active(&self) -> Result<MutexGuard<'_, KmsSourceState>, ReplayRuntimeError> {
        let mut state = lock_unpoisoned(&self.state);
        if matches!(*state, KmsSourceState::Uninitialized) {
            if self.stop.load(Ordering::Acquire) {
                *state = KmsSourceState::Stopped;
                return Ok(state);
            }
            *state = KmsSourceState::Active(Box::new(start_capture(self.settings)?));
        }
        Ok(state)
    }

    fn stop_active(&self) {
        let mut state = lock_unpoisoned(&self.state);
        let _previous = std::mem::replace(&mut *state, KmsSourceState::Stopped);
    }
}

impl ReplayFrameSource for KmsReplaySource {
    fn wait_next(
        &self,
        timeout: Duration,
    ) -> Result<Option<(u64, DmaBufReplayFrame)>, ReplayRuntimeError> {
        if self.stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        let deadline = {
            let state = self.ensure_active()?;
            match &*state {
                KmsSourceState::Active(active) => active.next_capture_at,
                KmsSourceState::Uninitialized | KmsSourceState::Stopped => return Ok(None),
            }
        };
        let now = Instant::now();
        if deadline > now {
            let remaining = deadline.duration_since(now);
            thread::sleep(remaining.min(timeout));
            if remaining > timeout || self.stop.load(Ordering::Acquire) {
                return Ok(None);
            }
        }
        let mut state = self.ensure_active()?;
        let KmsSourceState::Active(active) = &mut *state else {
            return Ok(None);
        };
        let exported = active.capture.acquire_frame().map_err(|error| {
            ReplayRuntimeError::new(format!("KMS framebuffer export failed: {error}"))
        })?;
        let width = exported.width;
        let height = exported.height;
        if !self.settings.frame_rate.supports_dimensions(width, height) {
            return Err(ReplayRuntimeError::new(
                "the active KMS framebuffer no longer supports the selected Replay frame rate",
            ));
        }
        let timestamp_ns =
            u64::try_from(active.started_at.elapsed().as_nanos()).unwrap_or(u64::MAX);
        let duration_ns = u64::try_from(active.frame_duration.as_nanos()).unwrap_or(u64::MAX);
        if active.sequence == u64::MAX {
            return Err(ReplayRuntimeError::new("KMS Replay sequence overflowed"));
        }
        active.sequence += 1;
        let sequence = active.sequence;
        advance_deadline(active);
        let planes = vec![DmaBufImagePlane {
            object_index: 0,
            offset: exported.offset,
            stride: exported.stride,
        }];
        let frame = DmaBufReplayFrame::new_multi_object(
            exported.objects,
            width,
            height,
            exported.drm_fourcc,
            exported.modifier,
            timestamp_ns,
            duration_ns,
            planes,
        )
        .map_err(|error| ReplayRuntimeError::new(format!("invalid KMS DMA-BUF frame: {error}")))?;
        Ok(Some((sequence, frame)))
    }

    fn release(&self, _sequence: u64) -> Result<(), ReplayRuntimeError> {
        // Each capture exports duplicate DMA-BUF ownership directly to the
        // encoder. Dropping the completed frame is the release edge.
        Ok(())
    }

    fn drain_and_release(&self) {
        self.stop_active();
    }

    fn wake(&self) {
        self.stop.store(true, Ordering::Release);
    }
}

impl Drop for KmsReplaySource {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.stop_active();
    }
}

/// Exercise direct KMS capture and the Vulkan Video encoder with a bounded
/// number of live frames without creating a replay store or writing a clip.
///
/// # Errors
///
/// Returns [`ReplayRuntimeError`] when framebuffer export,
/// explicit DRM-modifier import, conversion, encoding, or completion tracking
/// fails. The DRM session and all imported DMA-BUFs are released on every path.
#[allow(
    clippy::too_many_lines,
    reason = "the diagnostic keeps its bounded capture, encode, drain, and completion checks together"
)]
pub(crate) fn run_live_diagnostic(
    settings: ReplaySettings,
    requested_frames: u16,
) -> Result<KmsReplayLiveDiagnostic, ReplayRuntimeError> {
    if !(DIAGNOSTIC_MINIMUM_FRAMES..=DIAGNOSTIC_MAXIMUM_FRAMES).contains(&requested_frames) {
        return Err(ReplayRuntimeError::new(
            "KMS Replay diagnostic frame count must be between 2 and 120",
        ));
    }

    let source = KmsReplaySource::new(settings);
    let started_at = Instant::now();
    let frame_wait = Duration::from_secs(2);
    let overall_deadline =
        started_at + Duration::from_secs(u64::from(requested_frames).saturating_mul(2));
    let mut backend: Option<Box<dyn HardwareEncoderBackend>> = None;
    let mut dimensions = None;
    let mut submitted = 0_u16;
    let mut completed = Vec::with_capacity(usize::from(requested_frames));
    let mut encoded_packets = 0_u32;
    let mut encoded_bytes = 0_u64;
    let mut keyframes = 0_u16;

    let result = (|| {
        while submitted < requested_frames {
            if Instant::now() >= overall_deadline {
                return Err(ReplayRuntimeError::new(
                    "KMS Replay diagnostic timed out while waiting for live frames",
                ));
            }
            let Some((sequence, frame)) = source.wait_next(frame_wait)? else {
                continue;
            };
            let frame_dimensions = (frame.width, frame.height);
            if dimensions.is_some_and(|current| current != frame_dimensions) {
                return Err(ReplayRuntimeError::new(
                    "the KMS framebuffer resized during the bounded diagnostic",
                ));
            }
            dimensions = Some(frame_dimensions);
            if backend.is_none() {
                backend = Some(vulkan_replay_backend(&frame, settings).map_err(|error| {
                    ReplayRuntimeError::new(format!(
                        "could not initialize the KMS Vulkan Video encoder: {error}"
                    ))
                })?);
            }
            let output = backend
                .as_mut()
                .expect("initialized diagnostic backend")
                .encode(sequence, frame, submitted == 0)
                .map_err(|error| {
                    ReplayRuntimeError::new(format!(
                        "KMS Vulkan Video frame submission failed: {error}"
                    ))
                })?;
            accumulate_output(
                &output,
                &mut completed,
                &mut encoded_packets,
                &mut encoded_bytes,
                &mut keyframes,
            )?;
            submitted += 1;
        }

        let output = backend
            .as_mut()
            .ok_or_else(|| ReplayRuntimeError::new("KMS diagnostic acquired no frames"))?
            .drain()
            .map_err(|error| {
                ReplayRuntimeError::new(format!("KMS Vulkan Video drain failed: {error}"))
            })?;
        accumulate_output(
            &output,
            &mut completed,
            &mut encoded_packets,
            &mut encoded_bytes,
            &mut keyframes,
        )?;

        completed.sort_unstable();
        let expected = (1..=u64::from(requested_frames)).collect::<Vec<_>>();
        if completed != expected {
            return Err(ReplayRuntimeError::new(
                "KMS Vulkan Video did not complete every submitted DMA-BUF exactly once",
            ));
        }
        if encoded_packets == 0 || encoded_bytes == 0 || keyframes == 0 {
            return Err(ReplayRuntimeError::new(
                "KMS Vulkan Video produced no complete decodable access unit",
            ));
        }

        let (width, height) = dimensions.expect("submitted frames have dimensions");
        Ok(KmsReplayLiveDiagnostic {
            width,
            height,
            requested_frames,
            completed_frames: u16::try_from(completed.len()).unwrap_or(u16::MAX),
            encoded_packets,
            encoded_bytes,
            keyframes,
            elapsed_milliseconds: u64::try_from(started_at.elapsed().as_millis())
                .unwrap_or(u64::MAX),
        })
    })();

    if let Some(backend) = backend.as_mut() {
        let _ = backend.shutdown();
    }
    source.drain_and_release();
    result
}

fn accumulate_output(
    output: &crate::HardwareEncodeOutput,
    completed: &mut Vec<u64>,
    encoded_packets: &mut u32,
    encoded_bytes: &mut u64,
    keyframes: &mut u16,
) -> Result<(), ReplayRuntimeError> {
    for packet in output.batch().packets() {
        *encoded_packets = encoded_packets
            .checked_add(1)
            .ok_or_else(|| ReplayRuntimeError::new("diagnostic packet count overflowed"))?;
        *encoded_bytes = encoded_bytes
            .checked_add(u64::try_from(packet.bytes().len()).unwrap_or(u64::MAX))
            .ok_or_else(|| ReplayRuntimeError::new("diagnostic byte count overflowed"))?;
        if packet.is_keyframe() {
            *keyframes = keyframes
                .checked_add(1)
                .ok_or_else(|| ReplayRuntimeError::new("diagnostic keyframe count overflowed"))?;
        }
    }
    completed.extend_from_slice(output.completed_inputs());
    Ok(())
}

fn start_capture(settings: ReplaySettings) -> Result<ActiveKmsSource, ReplayRuntimeError> {
    let outputs = discover_outputs_at(Path::new("/"))
        .map_err(|error| ReplayRuntimeError::new(error.to_string()))?;
    let mut plans = outputs
        .iter()
        .filter_map(|output| KmsCapturePlan::new(output, settings.frame_rate).ok())
        .collect::<Vec<_>>();
    if plans.len() != 1 {
        return Err(ReplayRuntimeError::new(if plans.is_empty() {
            "Instant Replay could not find one active output compatible with the selected frame rate"
        } else {
            "Instant Replay requires exactly one connected output until automatic game-monitor selection is verified"
        }));
    }
    let plan = plans.pop().expect("one checked KMS plan");
    let capture = KmsDrmCapture::open(plan.card_index, &plan.connector)
        .map_err(|error| ReplayRuntimeError::new(format!("KMS capture unavailable: {error}")))?;
    let (width, height) = capture.dimensions();
    if !settings.frame_rate.supports_dimensions(width, height) {
        return Err(ReplayRuntimeError::new(
            "the active KMS framebuffer does not support the selected Replay frame rate",
        ));
    }
    let frame_duration = Duration::from_nanos(
        1_000_000_000_u64 / u64::from(settings.frame_rate.frames_per_second()),
    );
    let started_at = Instant::now();
    Ok(ActiveKmsSource {
        capture,
        started_at,
        next_capture_at: started_at,
        frame_duration,
        sequence: 0,
    })
}

fn advance_deadline(active: &mut ActiveKmsSource) {
    advance_deadline_values(&mut active.next_capture_at, active.frame_duration);
}

fn advance_deadline_values(next_capture_at: &mut Instant, frame_duration: Duration) {
    *next_capture_at += frame_duration;
    let now = Instant::now();
    if *next_capture_at < now {
        // Skip missed sample points instead of bursting and increasing load.
        *next_capture_at = now + frame_duration;
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_diagnostic_rejects_unbounded_work_before_device_access() {
        let low = run_live_diagnostic(ReplaySettings::default(), 1).expect_err("too few frames");
        let high =
            run_live_diagnostic(ReplaySettings::default(), 121).expect_err("too many frames");
        assert!(low.to_string().contains("between 2 and 120"));
        assert!(high.to_string().contains("between 2 and 120"));
    }

    #[test]
    fn missed_deadline_skips_ahead_without_a_capture_burst() {
        let mut next_capture_at = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("one second before a fresh instant");
        advance_deadline_values(&mut next_capture_at, Duration::from_millis(16));
        assert!(next_capture_at > Instant::now());
    }
}
