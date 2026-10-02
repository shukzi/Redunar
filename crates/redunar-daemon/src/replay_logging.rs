//! Opt-in aggregate observations from daemon workers, never game callbacks.
use crate::{ProductionReplayRuntime, ReplayRuntimeStatus, diagnostic_log};
use redunar_core::ReplayFrameRate;
use std::time::{Duration, Instant};

const INTERVAL: Duration = Duration::from_secs(5);
#[derive(Default)]
struct Gaps {
    previous_end: Option<u64>,
    count: u64,
    largest_ns: u64,
    previous_timestamp: Option<u64>,
    largest_interval_ns: u64,
}
impl Gaps {
    fn interval(&mut self, timestamp: u64) {
        if let Some(previous) = self.previous_timestamp {
            self.largest_interval_ns = self
                .largest_interval_ns
                .max(timestamp.saturating_sub(previous));
        }
        self.previous_timestamp = Some(timestamp);
    }
    fn observe(&mut self, timestamp: u64, duration: u64) {
        self.interval(timestamp);
        if let Some(end) = self.previous_end {
            let gap = timestamp.saturating_sub(end);
            // Ignore tiny timestamp jitter. Report missing time, not an
            // inferred focus change or fabricated number of dropped frames.
            if gap > 5_000_000 {
                self.count = self.count.saturating_add(1);
                self.largest_ns = self.largest_ns.max(gap);
            }
        }
        self.previous_end = Some(timestamp.saturating_add(duration));
    }
    fn take_window(&mut self) -> (u64, u64) {
        let values = (self.count, self.largest_ns / 1_000_000);
        self.count = 0;
        self.largest_ns = 0;
        values
    }
}

pub(crate) struct VideoTimeline {
    enabled: bool,
    next: Instant,
    first_source: bool,
    first_encoded: bool,
    gaps: Gaps,
    submissions: u64,
    failed: u64,
    max_submit_us: u128,
    frame_rate: ReplayFrameRate,
}
impl VideoTimeline {
    pub(crate) fn new(frame_rate: ReplayFrameRate) -> Self {
        Self {
            enabled: diagnostic_log::enabled(),
            next: Instant::now() + INTERVAL,
            first_source: false,
            first_encoded: false,
            gaps: Gaps::default(),
            submissions: 0,
            failed: 0,
            max_submit_us: 0,
            frame_rate,
        }
    }
    pub(crate) fn source(&mut self, timestamp: u64, duration: u64) {
        if !self.enabled {
            return;
        }
        if !self.first_source {
            diagnostic_log::log("Replay event=first_source_frame");
            self.first_source = true;
        }
        if self.frame_rate == ReplayFrameRate::Variable {
            self.gaps.interval(timestamp);
        } else {
            self.gaps.observe(timestamp, duration);
        }
    }
    pub(crate) fn submitted(&mut self, elapsed: Duration, failed: bool) {
        if !self.enabled {
            return;
        }
        self.submissions = self.submissions.saturating_add(1);
        self.failed = self.failed.saturating_add(u64::from(failed));
        self.max_submit_us = self.max_submit_us.max(elapsed.as_micros());
    }
    pub(crate) fn tick(&mut self, replay: &ProductionReplayRuntime) {
        if !self.enabled || Instant::now() < self.next {
            return;
        }
        self.report(replay.status(), "progress");
        let (in_flight, spool) = replay.diagnostic_counters();
        if let Some(spool) = spool {
            diagnostic_log::log(&format!(
                "Replay event=queue_progress encoder_in_flight={in_flight} spool_queued={} spool_queue_drops={} spool_keyframe_drops={} spool_slow_writes={} spool_resyncs={}",
                spool.queued_packets,
                spool.dropped_queue_full,
                spool.dropped_awaiting_keyframe,
                spool.slow_writes,
                spool.segment_resyncs,
            ));
        }
        self.next = Instant::now() + INTERVAL;
    }
    pub(crate) fn finish(&mut self, replay: &ProductionReplayRuntime) {
        if self.enabled {
            self.report(replay.status(), "end");
        }
    }
    fn report(&mut self, status: ReplayRuntimeStatus, event: &str) {
        if !self.first_encoded && status.encoded_packet_count > 0 {
            diagnostic_log::log("Replay event=encoded_output_observed");
            self.first_encoded = true;
        }
        let (gaps, largest_ms) = self.gaps.take_window();
        diagnostic_log::log(&format!(
            "Replay event={event} phase={:?} received={} encoded={} buffered_ms={} audio_retained={} audio_active={} submitted_window={} submit_failed_window={} submit_max_us={} variable={} source_interval_max_ms={} fixed_source_gaps_window={gaps} fixed_source_gap_max_ms={largest_ms}",
            status.phase,
            status.received_frame_count,
            status.encoded_packet_count,
            status.buffered_duration_ns / 1_000_000,
            status.audio_packet_count,
            status.audio_active,
            self.submissions,
            self.failed,
            self.max_submit_us,
            self.frame_rate == ReplayFrameRate::Variable,
            self.gaps.largest_interval_ns / 1_000_000,
        ));
        self.submissions = 0;
        self.failed = 0;
        self.max_submit_us = 0;
        self.gaps.largest_interval_ns = 0;
    }
}

pub(crate) struct AudioTimeline {
    enabled: bool,
    next: Instant,
    packets: u64,
    gaps: Gaps,
}
impl AudioTimeline {
    pub(crate) fn new() -> Self {
        Self {
            enabled: diagnostic_log::enabled(),
            next: Instant::now() + INTERVAL,
            packets: 0,
            gaps: Gaps::default(),
        }
    }
    pub(crate) fn packet(&mut self, timestamp: u64, duration: u64) {
        if !self.enabled {
            return;
        }
        if self.gaps.previous_end.is_none() {
            diagnostic_log::log("Replay event=first_audio_packet");
        }
        self.gaps.observe(timestamp, duration);
        self.packets = self.packets.saturating_add(1);
        if Instant::now() >= self.next {
            self.report();
            self.next = Instant::now() + INTERVAL;
        }
    }
    fn report(&mut self) {
        let (gaps, largest_ms) = self.gaps.take_window();
        diagnostic_log::log(&format!(
            "Replay event=audio_progress packets_window={} gaps_window={gaps} gap_max_ms={largest_ms}",
            self.packets
        ));
        self.packets = 0;
    }
}
impl Drop for AudioTimeline {
    fn drop(&mut self) {
        if self.enabled && self.packets != 0 {
            self.report();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn variable_cadence_measures_intervals_without_inventing_gaps() {
        let mut timeline = VideoTimeline::new(ReplayFrameRate::Variable);
        timeline.enabled = true;
        for frame in 0..60 {
            timeline.source(frame * 16_666_667, 8_333_333);
        }
        assert_eq!(timeline.gaps.take_window(), (0, 0));
        assert_eq!(timeline.gaps.largest_interval_ns, 16_666_667);
    }
    #[test]
    fn gap_windows_preserve_previous_end_without_inventing_drops() {
        let mut gaps = Gaps::default();
        gaps.observe(0, 20_000_000);
        gaps.observe(20_000_000, 20_000_000);
        gaps.observe(140_000_000, 20_000_000);
        assert_eq!(gaps.take_window(), (1, 100));
        gaps.observe(160_000_000, 20_000_000);
        assert_eq!(gaps.take_window(), (0, 0));
        gaps.observe(1, 20_000_000); // reset is not an enormous unsigned gap
        assert_eq!(gaps.take_window(), (0, 0));
    }
}
