use super::{Record, Shared, Status};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::path::PathBuf;
use std::sync::{atomic::Ordering, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_LOG_BYTES: u64 = 4 * 1024 * 1024;
const REPEAT_WINDOW_MS: u128 = 5_000;

pub(super) struct Sink {
    path: PathBuf,
    file: File,
    bytes: u64,
    limit: u64,
    previous: Option<Record>,
    repeated: u64,
}
impl Sink {
    pub(super) fn open(path: PathBuf) -> io::Result<Self> {
        let previous = path.with_extension("log.1");
        match fs::symlink_metadata(&previous) {
            Ok(_) => {
                let retained = private_file(&previous)?;
                if retained.metadata()?.len() > MAX_LOG_BYTES {
                    retained.set_len(MAX_LOG_BYTES)?;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let file = private_file(&path)?;
        let bytes = file.metadata()?.len();
        // Logs from older builds may already exceed this version's bound.
        // Diagnostics are disposable; never retain an oversized rotation.
        if bytes > MAX_LOG_BYTES {
            file.set_len(MAX_LOG_BYTES)?;
        }
        Ok(Self {
            path,
            file,
            bytes: bytes.min(MAX_LOG_BYTES),
            limit: MAX_LOG_BYTES,
            previous: None,
            repeated: 0,
        })
    }
    pub(super) fn record(&mut self, record: Record) -> io::Result<()> {
        if self.previous.as_ref().is_some_and(|previous| {
            previous.message == record.message
                && record.elapsed_ms.saturating_sub(previous.elapsed_ms) < REPEAT_WINDOW_MS
        }) {
            self.repeated = self.repeated.saturating_add(1);
            return Ok(());
        }
        self.flush_repeats()?;
        self.write(&record)?;
        self.previous = Some(record);
        Ok(())
    }
    fn write(&mut self, record: &Record) -> io::Result<()> {
        let line = format!(
            "[{}.{:03}] +{}ms {}\n",
            record.unix_ms / 1000,
            record.unix_ms % 1000,
            record.elapsed_ms,
            record.message
        );
        let length = u64::try_from(line.len()).unwrap_or(u64::MAX);
        if self.bytes.saturating_add(length) > self.limit {
            // Failed rotation stops logging rather than breaking the bound.
            fs::rename(&self.path, self.path.with_extension("log.1"))?;
            self.file = private_file(&self.path)?;
            self.bytes = self.file.metadata()?.len();
        }
        self.file.write_all(line.as_bytes())?;
        self.bytes = self.bytes.saturating_add(length);
        Ok(())
    }
    fn flush_repeats(&mut self) -> io::Result<()> {
        if self.repeated != 0 {
            if let Some(previous) = &self.previous {
                let summary = Record {
                    unix_ms: previous.unix_ms,
                    elapsed_ms: previous.elapsed_ms,
                    message: format!(
                        "diagnostics repeated={} event={}",
                        self.repeated, previous.message
                    ),
                };
                self.write(&summary)?;
            }
            self.repeated = 0;
        }
        Ok(())
    }
    fn finish(&mut self) -> io::Result<()> {
        self.flush_repeats()?;
        self.file.flush()
    }
}
fn private_file(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    // Refuse symlinks, FIFOs, and shared hardlinks in private state.
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(io::Error::other(
            "diagnostic file is not private regular storage",
        ));
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}
pub(super) fn run(
    mut sink: Sink,
    receiver: &mpsc::Receiver<Record>,
    shared: &Shared,
    started: Instant,
) {
    let outcome = run_inner(&mut sink, receiver, shared, started).and_then(|()| sink.finish());
    shared.status.store(
        if outcome.is_ok() {
            Status::Stopped
        } else {
            Status::Unavailable
        } as u8,
        Ordering::Release,
    );
}
fn run_inner(
    sink: &mut Sink,
    receiver: &mpsc::Receiver<Record>,
    shared: &Shared,
    started: Instant,
) -> io::Result<()> {
    loop {
        let dropped = shared.dropped.swap(0, Ordering::Relaxed);
        if dropped != 0 {
            sink.record(Record {
                unix_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis(),
                elapsed_ms: started.elapsed().as_millis(),
                message: format!("diagnostics queue_dropped={dropped}"),
            })?;
        }
        if shared.stop.load(Ordering::Acquire) {
            for record in receiver.try_iter().take(super::QUEUE_CAPACITY) {
                sink.record(record)?;
            }
            return Ok(());
        }
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(record) => sink.record(record)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if sink.previous.as_ref().is_some_and(|record| {
                    started
                        .elapsed()
                        .as_millis()
                        .saturating_sub(record.elapsed_ms)
                        >= REPEAT_WINDOW_MS
                }) {
                    sink.flush_repeats()?;
                    sink.previous = None;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic_log::tests::Temp;
    fn record(message: &str, elapsed_ms: u128) -> Record {
        Record {
            unix_ms: 123_456,
            elapsed_ms,
            message: message.into(),
        }
    }
    #[test]
    fn rotation_preserves_two_bounded_private_files() {
        let temp = Temp::new();
        let path = temp.0.join("diagnostics.log");
        let mut sink = Sink::open(path.clone()).unwrap();
        sink.limit = 128;
        for i in 0..40 {
            sink.record(record(&format!("event={i}"), i * 1000))
                .unwrap();
        }
        sink.finish().unwrap();
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 2);
        for path in [path.clone(), path.with_extension("log.1")] {
            let meta = fs::metadata(path).unwrap();
            assert!(meta.len() <= 128);
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }
    }
    #[test]
    fn failed_rotation_does_not_append_or_truncate() {
        let temp = Temp::new();
        let path = temp.0.join("diagnostics.log");
        let mut sink = Sink::open(path.clone()).unwrap();
        sink.record(record("first", 0)).unwrap();
        let before = fs::read(&path).unwrap();
        fs::create_dir(path.with_extension("log.1")).unwrap();
        sink.limit = 1;
        assert!(sink.record(record("second", 1000)).is_err());
        assert_eq!(fs::read(path).unwrap(), before);
    }
    #[test]
    fn repeats_are_counted_and_distinct_failures_survive() {
        let temp = Temp::new();
        let path = temp.0.join("diagnostics.log");
        let mut sink = Sink::open(path.clone()).unwrap();
        for i in 0..50 {
            sink.record(record("reason=missing_extension", i)).unwrap();
        }
        sink.record(record("reason=device_lost", 51)).unwrap();
        sink.finish().unwrap();
        let contents = fs::read_to_string(path).unwrap();
        assert_eq!(contents.lines().count(), 3);
        assert!(contents.contains("repeated=49"));
        assert!(contents.contains("reason=device_lost"));
        assert!(contents.contains("[123.456] +51ms"));
    }
    #[test]
    fn symlink_and_hardlink_targets_are_rejected() {
        let temp = Temp::new();
        let target = temp.0.join("private");
        fs::write(&target, "preserve").unwrap();
        let path = temp.0.join("diagnostics.log");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(Sink::open(path.clone()).is_err());
        fs::remove_file(&path).unwrap();
        fs::hard_link(&target, &path).unwrap();
        assert!(Sink::open(path).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "preserve");
    }

    #[test]
    fn worker_reports_write_failure_and_lost_queue_records() {
        use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64};
        let temp = Temp::new();
        let path = temp.0.join("diagnostics.log");
        let sink = Sink::open(path.clone()).unwrap();
        let shared = Shared {
            status: AtomicU8::new(Status::Active as u8),
            dropped: AtomicU64::new(7),
            stop: AtomicBool::new(true),
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        sender.try_send(record("last_event", 1)).unwrap();
        run(sink, &receiver, &shared, Instant::now());
        assert_eq!(shared.status.load(Ordering::Acquire), Status::Stopped as u8);
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("queue_dropped=7"));
        assert!(contents.contains("last_event"));

        let mut sink = Sink::open(path.clone()).unwrap();
        sink.limit = 1;
        fs::create_dir(path.with_extension("log.1")).unwrap();
        sender.try_send(record("cannot_write", 2)).unwrap();
        run(sink, &receiver, &shared, Instant::now());
        assert_eq!(
            shared.status.load(Ordering::Acquire),
            Status::Unavailable as u8
        );
        assert_eq!(fs::read_to_string(path).unwrap(), contents);
    }
}
