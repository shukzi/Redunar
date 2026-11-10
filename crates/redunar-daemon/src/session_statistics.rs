//! Exact whole-capture sums and bounded exact slow-frame retention.
use std::cmp::Reverse;
use std::collections::BinaryHeap;

// Retain the slowest 1% of at most ten million accepted frame intervals.
// Beyond that horizon lows are unavailable, never estimated. The full-session
// count and duration continue to accumulate independently.
const SLOW_FRAME_CAPACITY: usize = 100_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionFrameSummary {
    pub frame_count: u64,
    pub total_frame_time_ns: u128,
    pub dropped_frame_count: u64,
    pub rejected_message_count: u64,
    pub slowest_one_percent_ns: Option<u128>,
    pub slowest_point_one_percent_ns: Option<u128>,
    pub average_fps: Option<f64>,
    pub one_percent_low_fps: Option<f64>,
    pub point_one_percent_low_fps: Option<f64>,
}

#[derive(Default)]
pub(crate) struct SessionFrameAccumulator {
    count: u64,
    total_ns: u128,
    slowest: BinaryHeap<Reverse<u64>>,
}
impl SessionFrameAccumulator {
    pub fn record(&mut self, intervals: &[u64]) {
        for &interval in intervals {
            self.count += 1;
            self.total_ns += u128::from(interval);
            if self.slowest.len() < SLOW_FRAME_CAPACITY {
                self.slowest.push(Reverse(interval));
            } else if self
                .slowest
                .peek()
                .is_some_and(|smallest| interval > smallest.0)
            {
                *self.slowest.peek_mut().expect("full heap") = Reverse(interval);
            }
        }
    }
    pub fn summary(&self) -> SessionFrameSummary {
        let mut slowest: Vec<u64> = self.slowest.iter().map(|value| value.0).collect();
        slowest.sort_unstable_by(|a, b| b.cmp(a));
        let low = |denominator| {
            let count = self.count.div_ceil(denominator);
            if count == 0 || count > SLOW_FRAME_CAPACITY as u64 {
                return None;
            }
            Some(
                slowest[..usize::try_from(count).expect("bounded tail length")]
                    .iter()
                    .map(|&n| u128::from(n))
                    .sum(),
            )
        };
        SessionFrameSummary {
            frame_count: self.count,
            total_frame_time_ns: self.total_ns,
            average_fps: fps(self.count, self.total_ns),
            slowest_one_percent_ns: low(100),
            slowest_point_one_percent_ns: low(1000),
            one_percent_low_fps: low(100).and_then(|total| fps(self.count.div_ceil(100), total)),
            point_one_percent_low_fps: low(1000)
                .and_then(|total| fps(self.count.div_ceil(1000), total)),
            ..Default::default()
        }
    }
}
#[allow(clippy::cast_precision_loss)] // Convert exact integer totals only at presentation.
pub(crate) fn fps(count: u64, total_ns: u128) -> Option<f64> {
    (count > 0 && total_ns > 0).then(|| count as f64 * 1_000_000_000.0 / total_ns as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_session_retains_early_stalls_after_live_window_rollover() {
        let mut stats = SessionFrameAccumulator::default();
        stats.record(&[100_000_000; 100]);
        stats.record(&vec![10_000_000; 9900]);
        let summary = stats.summary();
        assert_eq!(summary.frame_count, 10_000);
        assert_eq!(summary.total_frame_time_ns, 109_000_000_000);
        assert_eq!(summary.average_fps, Some(10_000.0 / 109.0));
        assert_eq!(summary.one_percent_low_fps, Some(10.0));
        assert_eq!(summary.point_one_percent_low_fps, Some(10.0));
    }
    #[test]
    fn bounded_tail_matches_exact_sort_of_all_inputs() {
        let all: Vec<u64> = (0..300_001).map(|i| 1 + (i * 7919) % 99_991).collect();
        let mut stats = SessionFrameAccumulator::default();
        for chunk in all.chunks(128) {
            stats.record(chunk);
        }
        assert_eq!(stats.slowest.len(), SLOW_FRAME_CAPACITY);
        let mut sorted = all.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        let expected = |denominator: usize| {
            let count = all.len().div_ceil(denominator);
            fps(
                count as u64,
                sorted[..count].iter().map(|&n| u128::from(n)).sum(),
            )
        };
        assert_eq!(stats.summary().one_percent_low_fps, expected(100));
        assert_eq!(stats.summary().point_one_percent_low_fps, expected(1000));
    }
    #[test]
    fn empty_and_exhausted_exact_tail_never_invent_lows() {
        let mut stats = SessionFrameAccumulator::default();
        assert_eq!(stats.summary(), SessionFrameSummary::default());
        stats.record(&vec![10_000_000; SLOW_FRAME_CAPACITY]);
        stats.count = 10_000_001;
        stats.total_ns = u128::from(stats.count) * 10_000_000;
        let summary = stats.summary();
        assert_eq!(summary.average_fps, Some(100.0));
        assert_eq!(summary.one_percent_low_fps, None);
        assert_eq!(summary.point_one_percent_low_fps, Some(100.0));
    }
}
