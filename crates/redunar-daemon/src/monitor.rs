use redunar_core::{GameProcess, SystemSnapshot};
use redunar_platform::{LinuxGameProcessDetector, LinuxTelemetrySampler};
use std::error::Error;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

const DEFAULT_HARDWARE_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_GAME_SCAN_INTERVAL: Duration = Duration::from_secs(5);
const MINIMUM_INTERVAL: Duration = Duration::from_millis(250);

/// Sampling cadence for the daemon-owned live monitor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MonitorConfig {
    hardware_interval: Duration,
    game_scan_interval: Duration,
}

impl MonitorConfig {
    /// Create a validated monitor configuration.
    ///
    /// # Errors
    ///
    /// Returns [`MonitorConfigError`] when either interval is below 250 ms.
    pub fn new(
        hardware_interval: Duration,
        game_scan_interval: Duration,
    ) -> Result<Self, MonitorConfigError> {
        if hardware_interval < MINIMUM_INTERVAL {
            return Err(MonitorConfigError::new("hardware", hardware_interval));
        }
        if game_scan_interval < MINIMUM_INTERVAL {
            return Err(MonitorConfigError::new("game scan", game_scan_interval));
        }
        Ok(Self {
            hardware_interval,
            game_scan_interval,
        })
    }

    #[must_use]
    pub const fn hardware_interval(self) -> Duration {
        self.hardware_interval
    }

    #[must_use]
    pub const fn game_scan_interval(self) -> Duration {
        self.game_scan_interval
    }
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            hardware_interval: DEFAULT_HARDWARE_INTERVAL,
            game_scan_interval: DEFAULT_GAME_SCAN_INTERVAL,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonitorConfigError {
    message: String,
}

impl MonitorConfigError {
    fn new(interval_name: &str, value: Duration) -> Self {
        Self {
            message: format!(
                "{interval_name} interval must be at least {} ms, got {} ms",
                MINIMUM_INTERVAL.as_millis(),
                value.as_millis()
            ),
        }
    }
}

impl fmt::Display for MonitorConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for MonitorConfigError {}

/// Lightweight counters for diagnosing monitor overhead without per-tick logs.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MonitorDiagnostics {
    pub hardware_samples: u64,
    pub game_scans: u64,
    pub hardware_overruns: u64,
    pub game_scan_overruns: u64,
    pub last_hardware_sample_duration: Option<Duration>,
    pub last_game_scan_duration: Option<Duration>,
    pub telemetry_error: Option<String>,
    pub game_detection_error: Option<String>,
}

/// The newest complete monitor state. Older states are never queued.
#[derive(Clone, Debug)]
pub struct MonitorSnapshot {
    pub revision: u64,
    pub captured_at: SystemTime,
    pub hardware: Option<Arc<SystemSnapshot>>,
    pub games: Arc<[GameProcess]>,
    pub diagnostics: MonitorDiagnostics,
}

impl Default for MonitorSnapshot {
    fn default() -> Self {
        Self {
            revision: 0,
            captured_at: SystemTime::now(),
            hardware: None,
            games: Arc::from([]),
            diagnostics: MonitorDiagnostics::default(),
        }
    }
}

/// Owns the monitor worker and provides cheap reads of its newest state.
pub struct MonitorHandle {
    shared: Arc<MonitorShared>,
    worker: Option<JoinHandle<()>>,
}

impl fmt::Debug for MonitorHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MonitorHandle")
            .field("snapshot", &self.snapshot())
            .finish_non_exhaustive()
    }
}

impl MonitorHandle {
    pub(crate) fn start(config: MonitorConfig, allow_nvidia_beta: bool) -> Self {
        Self::start_worker(
            config,
            Box::new(move || {
                LinuxTelemetrySampler::with_nvidia_beta("/proc", "/sys", allow_nvidia_beta)
                    .map(|sampler| Box::new(sampler) as Box<dyn TelemetrySource>)
                    .map_err(|error| error.to_string())
            }),
            Box::<LinuxGameProcessDetector>::default(),
        )
    }

    fn start_worker(
        config: MonitorConfig,
        telemetry_factory: TelemetryFactory,
        detector: Box<dyn GameSource>,
    ) -> Self {
        let shared = Arc::new(MonitorShared::default());
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("redunar-monitor".into())
            .spawn(move || monitor_loop(&worker_shared, config, telemetry_factory, &*detector))
            .expect("failed to start Redunar monitor worker");
        Self {
            shared,
            worker: Some(worker),
        }
    }

    /// Return the current immutable state with only an atomic reference-count
    /// increment and a short mutex hold. Pollers can skip work when `revision`
    /// has not changed.
    #[must_use]
    pub fn snapshot(&self) -> Arc<MonitorSnapshot> {
        self.shared.latest.load()
    }

    /// Create a lightweight read-only view without starting another worker.
    #[must_use]
    pub fn reader(&self) -> MonitorReader {
        MonitorReader {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Interrupt monitoring and wait for its one worker to finish.
    pub fn shutdown(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.shared.request_stop();
            let _ = worker.join();
        }
    }
}

/// Cloneable read-only access to a running monitor's latest-value slot.
#[derive(Clone)]
pub struct MonitorReader {
    shared: Arc<MonitorShared>,
}

impl fmt::Debug for MonitorReader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MonitorReader")
            .field("snapshot", &self.snapshot())
            .finish_non_exhaustive()
    }
}

impl MonitorReader {
    /// Return the newest immutable snapshot.
    #[must_use]
    pub fn snapshot(&self) -> Arc<MonitorSnapshot> {
        self.shared.latest.load()
    }
}

impl Drop for MonitorHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct LatestSlot {
    value: Mutex<Arc<MonitorSnapshot>>,
}

impl Default for LatestSlot {
    fn default() -> Self {
        Self {
            value: Mutex::new(Arc::new(MonitorSnapshot::default())),
        }
    }
}

impl LatestSlot {
    fn load(&self) -> Arc<MonitorSnapshot> {
        Arc::clone(&lock_unpoisoned(&self.value))
    }

    fn publish(&self, mut snapshot: MonitorSnapshot) {
        let mut current = lock_unpoisoned(&self.value);
        snapshot.revision = current.revision.saturating_add(1);
        snapshot.captured_at = SystemTime::now();
        *current = Arc::new(snapshot);
    }
}

#[derive(Default)]
struct MonitorShared {
    latest: LatestSlot,
    stop: Mutex<bool>,
    wake: Condvar,
}

impl MonitorShared {
    fn request_stop(&self) {
        *lock_unpoisoned(&self.stop) = true;
        self.wake.notify_one();
    }

    fn wait_until(&self, deadline: Instant) -> bool {
        let mut stopped = lock_unpoisoned(&self.stop);
        while !*stopped {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let (guard, _) = self
                .wake
                .wait_timeout(stopped, deadline.saturating_duration_since(now))
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            stopped = guard;
        }
        *stopped
    }

    fn is_stopped(&self) -> bool {
        *lock_unpoisoned(&self.stop)
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct WorkerState {
    hardware: Option<Arc<SystemSnapshot>>,
    games: Arc<[GameProcess]>,
    diagnostics: MonitorDiagnostics,
}

impl WorkerState {
    fn snapshot(&self) -> MonitorSnapshot {
        MonitorSnapshot {
            revision: 0,
            captured_at: SystemTime::now(),
            hardware: self.hardware.clone(),
            games: Arc::clone(&self.games),
            diagnostics: self.diagnostics.clone(),
        }
    }
}

trait TelemetrySource: Send {
    fn sample(&mut self) -> SystemSnapshot;
}

impl TelemetrySource for LinuxTelemetrySampler {
    fn sample(&mut self) -> SystemSnapshot {
        Self::sample(self)
    }
}

trait GameSource: Send {
    fn detect(&self) -> Result<Vec<GameProcess>, String>;
}

impl GameSource for LinuxGameProcessDetector {
    fn detect(&self) -> Result<Vec<GameProcess>, String> {
        Self::detect(self).map_err(|error| error.to_string())
    }
}

type TelemetryFactory = Box<dyn FnOnce() -> Result<Box<dyn TelemetrySource>, String> + Send>;

fn monitor_loop(
    shared: &MonitorShared,
    config: MonitorConfig,
    telemetry_factory: TelemetryFactory,
    detector: &dyn GameSource,
) {
    let (mut sampler, telemetry_error) = match telemetry_factory() {
        Ok(sampler) => (Some(sampler), None),
        Err(error) => (None, Some(error)),
    };
    let mut state = WorkerState {
        hardware: None,
        games: Arc::from([]),
        diagnostics: MonitorDiagnostics {
            telemetry_error,
            ..MonitorDiagnostics::default()
        },
    };

    let started = Instant::now();
    let mut next_hardware = started;
    let mut next_game_scan = started;

    loop {
        if shared.is_stopped() {
            break;
        }
        let now = Instant::now();
        let mut changed = false;

        if now >= next_hardware {
            if let Some(sampler) = sampler.as_mut() {
                let sample_started = Instant::now();
                state.hardware = Some(Arc::new(sampler.sample()));
                let elapsed = sample_started.elapsed();
                state.diagnostics.hardware_samples =
                    state.diagnostics.hardware_samples.saturating_add(1);
                state.diagnostics.last_hardware_sample_duration = Some(elapsed);
                if elapsed > config.hardware_interval {
                    state.diagnostics.hardware_overruns =
                        state.diagnostics.hardware_overruns.saturating_add(1);
                }
                changed = true;
            }
            next_hardware =
                advance_deadline(next_hardware, config.hardware_interval, Instant::now());
        }

        if now >= next_game_scan {
            let scan_started = Instant::now();
            match detector.detect() {
                Ok(games) => {
                    state.games = Arc::from(games);
                    state.diagnostics.game_detection_error = None;
                }
                Err(error) => state.diagnostics.game_detection_error = Some(error),
            }
            let elapsed = scan_started.elapsed();
            state.diagnostics.game_scans = state.diagnostics.game_scans.saturating_add(1);
            state.diagnostics.last_game_scan_duration = Some(elapsed);
            if elapsed > config.game_scan_interval {
                state.diagnostics.game_scan_overruns =
                    state.diagnostics.game_scan_overruns.saturating_add(1);
            }
            next_game_scan =
                advance_deadline(next_game_scan, config.game_scan_interval, Instant::now());
            changed = true;
        }

        if changed {
            shared.latest.publish(state.snapshot());
        }

        let next_deadline = if sampler.is_some() {
            next_hardware.min(next_game_scan)
        } else {
            next_game_scan
        };
        if shared.wait_until(next_deadline) {
            break;
        }
    }
}

fn advance_deadline(previous: Instant, interval: Duration, now: Instant) -> Instant {
    let next = previous + interval;
    if next > now { next } else { now + interval }
}

#[cfg(test)]
mod tests {
    use super::*;
    use redunar_core::CpuSnapshot;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    fn config(interval: Duration) -> MonitorConfig {
        MonitorConfig::new(interval, interval).expect("valid monitor interval")
    }

    struct FakeTelemetry {
        samples: Arc<AtomicU64>,
        used_named_thread: Arc<AtomicBool>,
    }

    impl TelemetrySource for FakeTelemetry {
        fn sample(&mut self) -> SystemSnapshot {
            self.samples.fetch_add(1, Ordering::Relaxed);
            self.used_named_thread.store(
                thread::current().name() == Some("redunar-monitor"),
                Ordering::Relaxed,
            );
            SystemSnapshot {
                memory: None,
                cpu: CpuSnapshot {
                    vendor: "AuthenticAMD".into(),
                    model: "Fixture CPU".into(),
                    logical_cpus: 8,
                    temperature_celsius: Some(50.0),
                    utilization_percent: Some(10.0),
                    scaling_driver: Some("amd-pstate".into()),
                    governor: Some("performance".into()),
                    energy_performance_preference: Some("performance".into()),
                },
                gpus: Vec::new(),
            }
        }
    }

    struct FakeGameDetector {
        scans: Arc<AtomicU64>,
    }

    impl GameSource for FakeGameDetector {
        fn detect(&self) -> Result<Vec<GameProcess>, String> {
            self.scans.fetch_add(1, Ordering::Relaxed);
            Ok(Vec::new())
        }
    }

    fn fake_monitor(
        interval: Duration,
    ) -> (
        MonitorHandle,
        Arc<AtomicU64>,
        Arc<AtomicU64>,
        Arc<AtomicBool>,
    ) {
        let samples = Arc::new(AtomicU64::new(0));
        let scans = Arc::new(AtomicU64::new(0));
        let used_named_thread = Arc::new(AtomicBool::new(false));
        let telemetry = FakeTelemetry {
            samples: Arc::clone(&samples),
            used_named_thread: Arc::clone(&used_named_thread),
        };
        let detector = FakeGameDetector {
            scans: Arc::clone(&scans),
        };
        let monitor = MonitorHandle::start_worker(
            config(interval),
            Box::new(|| Ok(Box::new(telemetry))),
            Box::new(detector),
        );
        (monitor, samples, scans, used_named_thread)
    }

    fn wait_for_first_snapshot(monitor: &MonitorHandle) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while monitor.snapshot().revision == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(monitor.snapshot().revision > 0, "monitor did not publish");
    }

    #[test]
    fn default_cadence_matches_performance_budget() {
        let config = MonitorConfig::default();
        assert_eq!(config.hardware_interval(), Duration::from_secs(1));
        assert_eq!(config.game_scan_interval(), Duration::from_secs(5));
    }

    #[test]
    fn rejects_intervals_below_minimum() {
        assert!(MonitorConfig::new(Duration::from_millis(249), Duration::from_secs(5)).is_err());
        assert!(MonitorConfig::new(Duration::from_secs(1), Duration::ZERO).is_err());
    }

    #[test]
    fn latest_slot_overwrites_instead_of_queueing() {
        let slot = LatestSlot::default();
        for samples in 1..=100 {
            let mut snapshot = MonitorSnapshot::default();
            snapshot.diagnostics.hardware_samples = samples;
            slot.publish(snapshot);
        }
        let latest = slot.load();
        assert_eq!(latest.revision, 100);
        assert_eq!(latest.diagnostics.hardware_samples, 100);
    }

    #[test]
    fn revisions_are_monotonic_and_old_snapshots_remain_immutable() {
        let slot = LatestSlot::default();
        let original = slot.load();
        slot.publish(MonitorSnapshot::default());
        let first = slot.load();
        slot.publish(MonitorSnapshot::default());
        let second = slot.load();

        assert_eq!(original.revision, 0);
        assert_eq!(first.revision, 1);
        assert_eq!(second.revision, 2);
        assert_eq!(first.revision, 1);
    }

    #[test]
    fn shutdown_interrupts_a_long_wait_promptly() {
        let (mut monitor, _, _, _) = fake_monitor(Duration::from_secs(30));
        wait_for_first_snapshot(&monitor);
        let started = Instant::now();
        monitor.shutdown();
        assert!(started.elapsed() < Duration::from_millis(200));
    }

    #[test]
    fn monitor_does_not_busy_loop_before_next_deadline() {
        let (monitor, samples, scans, used_named_thread) = fake_monitor(MINIMUM_INTERVAL);
        wait_for_first_snapshot(&monitor);
        let initial_samples = samples.load(Ordering::Relaxed);
        let initial_scans = scans.load(Ordering::Relaxed);
        thread::sleep(Duration::from_millis(100));
        assert_eq!(samples.load(Ordering::Relaxed), initial_samples);
        assert_eq!(scans.load(Ordering::Relaxed), initial_scans);
        assert!(used_named_thread.load(Ordering::Relaxed));
    }

    #[test]
    fn monitor_can_be_started_and_stopped_repeatedly() {
        for _ in 0..3 {
            let (mut monitor, _, _, _) = fake_monitor(Duration::from_secs(1));
            monitor.shutdown();
            monitor.shutdown();
        }
    }

    #[test]
    fn telemetry_initialization_failure_is_published_without_retrying() {
        let scans = Arc::new(AtomicU64::new(0));
        let detector = FakeGameDetector {
            scans: Arc::clone(&scans),
        };
        let monitor = MonitorHandle::start_worker(
            config(MINIMUM_INTERVAL),
            Box::new(|| Err("fixture discovery failure".into())),
            Box::new(detector),
        );
        wait_for_first_snapshot(&monitor);
        let snapshot = monitor.snapshot();
        assert_eq!(
            snapshot.diagnostics.telemetry_error.as_deref(),
            Some("fixture discovery failure")
        );
        assert_eq!(snapshot.diagnostics.hardware_samples, 0);
        assert_eq!(scans.load(Ordering::Relaxed), 1);
    }
}
