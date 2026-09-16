use crate::CapturePhase;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::time::Duration;

pub const ARMED_STARTUP_GRACE: Duration = Duration::from_secs(5);
pub const PRODUCER_EXIT_GRACE: Duration = Duration::from_millis(500);
const MAX_PROC_STAT_BYTES: u64 = 4 * 1024;

/// State of the process Redunar launched directly. Launchers and wrappers may
/// exit before the capture producer, so this is only one lifecycle input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaptureLaunchProcessState {
    Running,
    Exited(String),
    MonitorFailed(String),
}

/// Whether all processes and private capture resources owned by this launch
/// have ended, allowing the caller to accept another game launch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureLaunchDisposition {
    KeepActive,
    Release,
}

#[derive(Debug)]
pub(crate) struct CaptureLaunchLifecycle {
    producer: Option<ProcessIdentity>,
    producer_unavailable_since: Option<Duration>,
}

impl CaptureLaunchLifecycle {
    pub(crate) const fn new() -> Self {
        Self {
            producer: None,
            producer_unavailable_since: None,
        }
    }

    pub(crate) fn evaluate(
        &mut self,
        elapsed: Duration,
        phase: CapturePhase,
        producer_process_id: Option<u32>,
        launched_process: &CaptureLaunchProcessState,
    ) -> LifecycleDecision {
        self.evaluate_with_proc(
            elapsed,
            phase,
            producer_process_id,
            launched_process,
            Path::new("/proc"),
        )
    }

    fn evaluate_with_proc(
        &mut self,
        elapsed: Duration,
        phase: CapturePhase,
        producer_process_id: Option<u32>,
        launched_process: &CaptureLaunchProcessState,
        proc_root: &Path,
    ) -> LifecycleDecision {
        if let CaptureLaunchProcessState::MonitorFailed(error) = launched_process {
            return LifecycleDecision::fail(
                format!("Could not monitor the launched game process: {error}"),
                false,
            );
        }
        if phase == CapturePhase::Failed && producer_process_id.is_none() {
            return LifecycleDecision::finish(launched_process.has_exited());
        }

        let Some(producer_process_id) = producer_process_id else {
            if elapsed < ARMED_STARTUP_GRACE {
                return LifecycleDecision::keep();
            }
            let message = match launched_process {
                CaptureLaunchProcessState::Exited(status) => format!(
                    "The launcher exited with {status}, but no capture producer connected within {} seconds.",
                    ARMED_STARTUP_GRACE.as_secs()
                ),
                CaptureLaunchProcessState::Running => format!(
                    "No capture producer connected within {} seconds.",
                    ARMED_STARTUP_GRACE.as_secs()
                ),
                CaptureLaunchProcessState::MonitorFailed(_) => unreachable!(),
            };
            return LifecycleDecision::fail(message, launched_process.has_exited());
        };

        let liveness = self.producer_liveness(producer_process_id, proc_root);
        if phase == CapturePhase::Failed {
            return LifecycleDecision::finish(
                liveness == ProcessLiveness::Dead && launched_process.has_exited(),
            );
        }
        if liveness == ProcessLiveness::Alive {
            self.producer_unavailable_since = None;
            return LifecycleDecision::keep();
        }

        let unavailable_since = *self.producer_unavailable_since.get_or_insert(elapsed);
        if phase == CapturePhase::Completed {
            return LifecycleDecision::finish(
                liveness == ProcessLiveness::Dead && launched_process.has_exited(),
            );
        }
        if elapsed.saturating_sub(unavailable_since) < PRODUCER_EXIT_GRACE {
            return LifecycleDecision::keep();
        }

        let message = match liveness {
            ProcessLiveness::Dead => {
                "The capture producer exited without completing capture.".to_owned()
            }
            ProcessLiveness::Unknown => {
                "Redunar could not verify that the capture producer is still running.".to_owned()
            }
            ProcessLiveness::Alive => unreachable!(),
        };
        LifecycleDecision::fail(
            message,
            liveness == ProcessLiveness::Dead && launched_process.has_exited(),
        )
    }

    fn producer_liveness(&mut self, process_id: u32, proc_root: &Path) -> ProcessLiveness {
        if self
            .producer
            .as_ref()
            .is_some_and(|producer| producer.process_id != process_id)
        {
            return ProcessLiveness::Dead;
        }
        if self.producer.is_none() {
            match ProcessIdentity::capture(proc_root, process_id) {
                Ok(identity) => self.producer = Some(identity),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return ProcessLiveness::Dead;
                }
                Err(_) => return ProcessLiveness::Unknown,
            }
        }
        match self
            .producer
            .as_ref()
            .expect("producer was captured")
            .is_alive()
        {
            Ok(true) => ProcessLiveness::Alive,
            Ok(false) => ProcessLiveness::Dead,
            Err(error) if error.kind() == io::ErrorKind::NotFound => ProcessLiveness::Dead,
            Err(_) => ProcessLiveness::Unknown,
        }
    }
}

impl CaptureLaunchProcessState {
    fn has_exited(&self) -> bool {
        matches!(self, Self::Exited(_))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProcessLiveness {
    Alive,
    Dead,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LifecycleDecision {
    pub(crate) action: LifecycleAction,
    pub(crate) disposition: CaptureLaunchDisposition,
}

impl LifecycleDecision {
    const fn keep() -> Self {
        Self {
            action: LifecycleAction::Continue,
            disposition: CaptureLaunchDisposition::KeepActive,
        }
    }

    const fn finish(release: bool) -> Self {
        Self {
            action: LifecycleAction::Finish,
            disposition: if release {
                CaptureLaunchDisposition::Release
            } else {
                CaptureLaunchDisposition::KeepActive
            },
        }
    }

    fn fail(message: String, release: bool) -> Self {
        Self {
            action: LifecycleAction::Fail(message),
            disposition: if release {
                CaptureLaunchDisposition::Release
            } else {
                CaptureLaunchDisposition::KeepActive
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LifecycleAction {
    Continue,
    Finish,
    Fail(String),
}

#[derive(Debug)]
struct ProcessIdentity {
    process_id: u32,
    start_time_ticks: u64,
    proc_root: std::path::PathBuf,
}

impl ProcessIdentity {
    fn capture(proc_root: &Path, process_id: u32) -> io::Result<Self> {
        let stat = read_process_stat(proc_root, process_id)?;
        if matches!(stat.state, 'Z' | 'X' | 'x') {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "capture producer is no longer running",
            ));
        }
        Ok(Self {
            process_id,
            start_time_ticks: stat.start_time_ticks,
            proc_root: proc_root.to_path_buf(),
        })
    }

    fn is_alive(&self) -> io::Result<bool> {
        let stat = read_process_stat(&self.proc_root, self.process_id)?;
        Ok(
            stat.start_time_ticks == self.start_time_ticks
                && !matches!(stat.state, 'Z' | 'X' | 'x'),
        )
    }
}

struct ProcessStat {
    state: char,
    start_time_ticks: u64,
}

fn read_process_stat(proc_root: &Path, process_id: u32) -> io::Result<ProcessStat> {
    let file = File::open(proc_root.join(process_id.to_string()).join("stat"))?;
    let mut contents = Vec::new();
    file.take(MAX_PROC_STAT_BYTES + 1)
        .read_to_end(&mut contents)?;
    if contents.len() as u64 > MAX_PROC_STAT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "process stat exceeds the bounded read size",
        ));
    }
    parse_process_stat(&contents)
}

fn parse_process_stat(contents: &[u8]) -> io::Result<ProcessStat> {
    // The comm field is parenthesized and may itself contain spaces or `)`.
    // Linux's fields after the final `)` are whitespace-separated; starttime
    // is field 22, or item 19 after the state field (field 3).
    let suffix = contents
        .iter()
        .rposition(|byte| *byte == b')')
        .and_then(|index| contents.get(index.saturating_add(1)..))
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "process stat has no comm field")
        })?;
    let mut fields = suffix
        .split(u8::is_ascii_whitespace)
        .filter(|field| !field.is_empty());
    let state = fields
        .next()
        .and_then(|field| field.first())
        .map(|byte| char::from(*byte))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "process stat has no state"))?;
    let start_time_bytes = fields.nth(18).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "process stat has no start time")
    })?;
    let start_time_ticks = std::str::from_utf8(start_time_bytes)
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "process stat start time is invalid",
            )
        })?
        .parse::<u64>()
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "process stat start time is invalid",
            )
        })?;
    Ok(ProcessStat {
        state,
        start_time_ticks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct ProcFixture {
        root: std::path::PathBuf,
    }

    impl ProcFixture {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir()
                .join(format!("redunar-capture-proc-{}-{id}", std::process::id()));
            fs::create_dir_all(&root).expect("create proc fixture");
            Self { root }
        }

        fn write_stat(&self, process_id: u32, state: char, start_time_ticks: u64) {
            let directory = self.root.join(process_id.to_string());
            fs::create_dir_all(&directory).expect("create fake process");
            let mut fields = vec!["0".to_owned(); 20];
            fields[0] = state.to_string();
            fields[19] = start_time_ticks.to_string();
            fs::write(
                directory.join("stat"),
                format!("{process_id} (game ) wrapper) {}\n", fields.join(" ")),
            )
            .expect("write fake stat");
        }
    }

    impl Drop for ProcFixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).expect("remove proc fixture");
        }
    }

    #[test]
    fn launcher_exit_does_not_end_a_live_registered_producer() {
        let fixture = ProcFixture::new();
        fixture.write_stat(42, 'R', 900);
        let mut lifecycle = CaptureLaunchLifecycle::new();
        let decision = lifecycle.evaluate_with_proc(
            Duration::from_secs(1),
            CapturePhase::Capturing,
            Some(42),
            &CaptureLaunchProcessState::Exited("exit status: 0".to_owned()),
            &fixture.root,
        );
        assert_eq!(decision, LifecycleDecision::keep());
    }

    #[test]
    fn producer_can_register_after_its_launcher_child_exits() {
        let fixture = ProcFixture::new();
        let mut lifecycle = CaptureLaunchLifecycle::new();
        let exited = CaptureLaunchProcessState::Exited("exit status: 0".to_owned());

        assert_eq!(
            lifecycle.evaluate_with_proc(
                Duration::from_secs(1),
                CapturePhase::Armed,
                None,
                &exited,
                &fixture.root,
            ),
            LifecycleDecision::keep(),
            "wrapper exit alone must not close the producer registration window"
        );

        fixture.write_stat(42, 'R', 900);
        assert_eq!(
            lifecycle.evaluate_with_proc(
                ARMED_STARTUP_GRACE
                    .checked_sub(Duration::from_millis(1))
                    .expect("grace exceeds one millisecond"),
                CapturePhase::Capturing,
                Some(42),
                &exited,
                &fixture.root,
            ),
            LifecycleDecision::keep(),
            "the late producer, rather than the exited wrapper, owns capture lifetime"
        );
    }

    #[test]
    fn armed_wrapper_gets_a_bounded_window_for_descendant_startup() {
        let fixture = ProcFixture::new();
        let mut lifecycle = CaptureLaunchLifecycle::new();
        let exited = CaptureLaunchProcessState::Exited("exit status: 0".to_owned());
        assert_eq!(
            lifecycle.evaluate_with_proc(
                ARMED_STARTUP_GRACE
                    .checked_sub(Duration::from_millis(1))
                    .expect("grace exceeds one millisecond"),
                CapturePhase::Armed,
                None,
                &exited,
                &fixture.root,
            ),
            LifecycleDecision::keep()
        );
        let decision = lifecycle.evaluate_with_proc(
            ARMED_STARTUP_GRACE,
            CapturePhase::Armed,
            None,
            &exited,
            &fixture.root,
        );
        assert!(matches!(decision.action, LifecycleAction::Fail(_)));
        assert_eq!(decision.disposition, CaptureLaunchDisposition::Release);
    }

    #[test]
    fn missing_producer_waits_for_queued_goodbye_then_fails() {
        let fixture = ProcFixture::new();
        fixture.write_stat(42, 'R', 900);
        let mut lifecycle = CaptureLaunchLifecycle::new();
        let exited = CaptureLaunchProcessState::Exited("exit status: 0".to_owned());
        assert_eq!(
            lifecycle.evaluate_with_proc(
                Duration::from_secs(1),
                CapturePhase::Capturing,
                Some(42),
                &exited,
                &fixture.root,
            ),
            LifecycleDecision::keep()
        );
        fixture.write_stat(42, 'Z', 900);
        assert_eq!(
            lifecycle.evaluate_with_proc(
                Duration::from_secs(2),
                CapturePhase::Capturing,
                Some(42),
                &exited,
                &fixture.root,
            ),
            LifecycleDecision::keep()
        );
        let decision = lifecycle.evaluate_with_proc(
            Duration::from_secs(2) + PRODUCER_EXIT_GRACE,
            CapturePhase::Capturing,
            Some(42),
            &exited,
            &fixture.root,
        );
        assert!(matches!(decision.action, LifecycleAction::Fail(_)));
        assert_eq!(decision.disposition, CaptureLaunchDisposition::Release);
    }

    #[test]
    fn failed_capture_keeps_launch_owned_while_child_is_running() {
        let mut lifecycle = CaptureLaunchLifecycle::new();
        let decision = lifecycle.evaluate_with_proc(
            Duration::from_secs(10),
            CapturePhase::Failed,
            None,
            &CaptureLaunchProcessState::Running,
            Path::new("/unused"),
        );
        assert_eq!(decision.action, LifecycleAction::Finish);
        assert_eq!(decision.disposition, CaptureLaunchDisposition::KeepActive);
    }

    #[test]
    fn failed_capture_keeps_live_producer_owned_after_wrapper_exit() {
        let fixture = ProcFixture::new();
        fixture.write_stat(42, 'S', 900);
        let mut lifecycle = CaptureLaunchLifecycle::new();
        let exited = CaptureLaunchProcessState::Exited("exit status: 1".to_owned());

        let live_decision = lifecycle.evaluate_with_proc(
            Duration::from_secs(2),
            CapturePhase::Failed,
            Some(42),
            &exited,
            &fixture.root,
        );
        assert_eq!(live_decision.action, LifecycleAction::Finish);
        assert_eq!(
            live_decision.disposition,
            CaptureLaunchDisposition::KeepActive
        );
        fixture.write_stat(42, 'Z', 900);
        let decision = lifecycle.evaluate_with_proc(
            Duration::from_secs(3),
            CapturePhase::Failed,
            Some(42),
            &exited,
            &fixture.root,
        );
        assert_eq!(decision.action, LifecycleAction::Finish);
        assert_eq!(decision.disposition, CaptureLaunchDisposition::Release);
    }

    #[test]
    fn proc_identity_rejects_pid_reuse_and_zombies() {
        let fixture = ProcFixture::new();
        fixture.write_stat(42, 'S', 900);
        let identity = ProcessIdentity::capture(&fixture.root, 42).expect("capture identity");
        assert!(identity.is_alive().expect("live process"));
        fixture.write_stat(42, 'S', 901);
        assert!(!identity.is_alive().expect("reused pid"));
        fixture.write_stat(42, 'Z', 900);
        assert!(!identity.is_alive().expect("zombie process"));
    }

    #[test]
    fn unverifiable_producer_never_unlocks_a_second_launch() {
        let fixture = ProcFixture::new();
        let directory = fixture.root.join("42");
        fs::create_dir_all(&directory).expect("create fake process");
        fs::write(directory.join("stat"), b"malformed").expect("write malformed stat");
        let mut lifecycle = CaptureLaunchLifecycle::new();
        let exited = CaptureLaunchProcessState::Exited("exit status: 0".to_owned());

        let completed = lifecycle.evaluate_with_proc(
            Duration::from_secs(2),
            CapturePhase::Completed,
            Some(42),
            &exited,
            &fixture.root,
        );
        assert_eq!(completed.action, LifecycleAction::Finish);
        assert_eq!(completed.disposition, CaptureLaunchDisposition::KeepActive);
    }

    #[test]
    fn proc_identity_recognizes_the_current_process() {
        let identity = ProcessIdentity::capture(Path::new("/proc"), std::process::id())
            .expect("capture current process identity");
        assert!(identity.is_alive().expect("current process is alive"));
    }
}
