use super::*;
use std::fs;

pub(super) struct Temp(pub PathBuf);
impl Temp {
    pub(super) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "redunar-log-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn shutdown_drains_and_repeated_shutdown_is_safe() {
    let temp = Temp::new();
    let path = log_path(&temp.0);
    let logger = Logger::start(path.clone());
    for i in 0..20 {
        logger.log(&format!("event={i}"));
    }
    logger.shutdown();
    logger.shutdown();
    assert_eq!(logger.status(), Status::Stopped);
    let contents = fs::read_to_string(path).unwrap();
    assert_eq!(contents.lines().count(), 20);
    assert!(contents.contains("event=19"));
}
#[test]
fn startup_failure_is_honest_and_logging_stays_safe() {
    let temp = Temp::new();
    let logger = Logger::start(temp.0.join("missing/diagnostics.log"));
    assert_eq!(logger.status(), Status::Unavailable);
    logger.log("ignored");
    logger.shutdown();
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 0);
}
#[test]
fn full_queue_drops_without_blocking() {
    let (sender, _receiver) = mpsc::sync_channel(1);
    let logger = Logger {
        sender,
        shared: Arc::new(Shared {
            status: AtomicU8::new(Status::Active as u8),
            dropped: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        }),
        worker: Mutex::new(None),
        started: Instant::now(),
    };
    logger.log("first");
    for _ in 0..1000 {
        logger.log("dropped");
    }
    assert_eq!(logger.shared.dropped.load(Ordering::Relaxed), 1000);
}
#[test]
fn filters_paths_newlines_and_oversized_utf8() {
    assert_eq!(
        sanitize("error '/home/user/secret' path=/tmp/private https://private/token\nnext"),
        "error [private]"
    );
    let bounded = sanitize(&"é".repeat(MAX_MESSAGE_BYTES));
    assert!(bounded.len() <= MAX_MESSAGE_BYTES);
    assert!(!bounded.contains('\n'));
}
#[test]
fn path_free_save_failure_causes_survive_conservative_redaction() {
    for stage in ["directory flush", "quota cleanup", "temporary link removal"] {
        let message = format!(
            "replay clip committed but {stage} failed: Permission denied (os error 13); clip=/home/private user/capture.mkv"
        );
        let sanitized = sanitize(&message);
        assert!(sanitized.contains(stage));
        assert!(sanitized.contains("Permission denied (os error 13)"));
        assert!(!sanitized.contains("private user"));
        assert!(sanitized.ends_with("[private]"));
        let error = std::io::Error::from_raw_os_error(5);
        let sanitized = sanitize(&format!(
            "{stage} failed: kind={:?} errno={:?} detail={error}; clip=/home/private user/capture.mkv",
            error.kind(),
            error.raw_os_error(),
        ));
        assert!(sanitized.contains(stage));
        assert!(sanitized.contains("errno=Some(5)"));
        assert!(!sanitized.contains("private user"));
    }
}
#[test]
fn disabled_logger_does_not_create_files() {
    // Global state is never initialized by these tests; instance tests isolate
    // their worker and directory so parallel tests cannot activate logging.
    assert_eq!(status(), Status::Off);
    log("ignored while off");
    assert_eq!(status(), Status::Off);
}
