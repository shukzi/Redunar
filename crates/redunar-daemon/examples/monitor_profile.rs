use redunar_daemon::RedunarService;
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_SECONDS: u64 = 60;
const POLL_INTERVAL: Duration = Duration::from_millis(50);

fn main() {
    let seconds = std::env::args().nth(1).map_or(DEFAULT_SECONDS, |value| {
        value.parse::<u64>().expect("duration must be seconds")
    });
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let service = RedunarService::default();
    let mut monitor = service.start_monitor();
    let mut hardware_durations = Vec::new();
    let mut last_hardware_samples = 0;

    while Instant::now() < deadline {
        let snapshot = monitor.snapshot();
        let diagnostics = &snapshot.diagnostics;
        if diagnostics.hardware_samples != last_hardware_samples {
            if let Some(duration) = diagnostics.last_hardware_sample_duration {
                hardware_durations.push(duration);
            }
            last_hardware_samples = diagnostics.hardware_samples;
        }
        thread::sleep(POLL_INTERVAL);
    }

    let snapshot = monitor.snapshot();
    monitor.shutdown();
    hardware_durations.sort_unstable();

    println!("duration_seconds={seconds}");
    println!("hardware_samples={}", snapshot.diagnostics.hardware_samples);
    println!("game_scans={}", snapshot.diagnostics.game_scans);
    println!(
        "hardware_sample_p95_us={}",
        percentile(&hardware_durations, 95).map_or(0, |duration| duration.as_micros())
    );
    println!(
        "hardware_sample_p99_us={}",
        percentile(&hardware_durations, 99).map_or(0, |duration| duration.as_micros())
    );
    println!(
        "hardware_sample_max_us={}",
        hardware_durations.last().map_or(0, Duration::as_micros)
    );
    println!(
        "hardware_overruns={}",
        snapshot.diagnostics.hardware_overruns
    );
    println!(
        "game_scan_overruns={}",
        snapshot.diagnostics.game_scan_overruns
    );
    println!(
        "telemetry_error={}",
        snapshot
            .diagnostics
            .telemetry_error
            .as_deref()
            .unwrap_or("none")
    );
    println!(
        "game_detection_error={}",
        snapshot
            .diagnostics
            .game_detection_error
            .as_deref()
            .unwrap_or("none")
    );
}

fn percentile(sorted: &[Duration], percentage: usize) -> Option<Duration> {
    let rank = sorted.len().checked_mul(percentage)?.div_ceil(100);
    sorted.get(rank.saturating_sub(1)).copied()
}

#[cfg(test)]
mod tests {
    use super::percentile;
    use std::time::Duration;

    #[test]
    fn percentile_uses_nearest_rank() {
        let values = (1..=100).map(Duration::from_micros).collect::<Vec<_>>();
        assert_eq!(percentile(&values, 95), Some(Duration::from_micros(95)));
        assert_eq!(percentile(&values, 99), Some(Duration::from_micros(99)));
        assert_eq!(percentile(&[], 95), None);
    }
}
