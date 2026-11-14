//! Session-scoped admission for initial Replay worker recovery.
use std::time::{Duration, Instant};

pub(super) struct ReplayStartup {
    generation: u64,
    in_flight: bool,
    next: Option<Instant>,
    backoff: Duration,
}
impl Default for ReplayStartup {
    fn default() -> Self {
        Self {
            generation: 0,
            in_flight: false,
            next: None,
            backoff: super::REPLAY_REARM_BACKOFF,
        }
    }
}
impl ReplayStartup {
    pub fn reset(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.in_flight = false;
        self.next = None;
        self.backoff = super::REPLAY_REARM_BACKOFF;
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn claim(&mut self, now: Instant) -> Option<u64> {
        if self.in_flight || self.next.is_some_and(|next| now < next) {
            return None;
        }
        self.in_flight = true;
        Some(self.generation)
    }
    pub fn finish(&mut self, generation: u64, now: Instant, success: bool) -> bool {
        if self.generation != generation || !self.in_flight {
            return false;
        }
        self.in_flight = false;
        self.next = (!success).then_some(now + self.backoff);
        self.backoff = if success {
            super::REPLAY_REARM_BACKOFF
        } else {
            (self.backoff * 2).min(super::REPLAY_REARM_BACKOFF_MAX)
        };
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn start_failures_back_off_without_overlapping_or_starving_retries() {
        let mut retry = ReplayStartup::default();
        let mut now = Instant::now();
        for seconds in [0.5, 1.0, 2.0, 4.0, 8.0, 10.0, 10.0] {
            let generation = retry.claim(now).unwrap();
            assert!(retry.claim(now + Duration::from_secs(30)).is_none());
            assert!(retry.finish(generation, now, false));
            let wait = Duration::from_secs_f64(seconds);
            assert!(
                retry
                    .claim((now + wait).checked_sub(Duration::from_nanos(1)).unwrap())
                    .is_none()
            );
            now += wait;
        }
        let generation = retry.claim(now).unwrap();
        retry.reset();
        assert!(!retry.finish(generation, now, false));
        assert!(retry.claim(now).is_some());
    }
}
