//! Opt-in bounded local diagnostics. Producers never wait for disk or queue
//! capacity. One worker owns rotation and exposes failures as runtime health.
mod writer;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const QUEUE_CAPACITY: usize = 256;
const MAX_MESSAGE_BYTES: usize = 2048;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(500);
static LOGGER: OnceLock<Logger> = OnceLock::new();

/// Runtime health is independent of the saved, restart-required preference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Status {
    Off = 0,
    Active = 1,
    Unavailable = 2,
    Stopped = 3,
}
impl Status {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Active => "active",
            Self::Unavailable => "unavailable",
            Self::Stopped => "stopped",
        }
    }
}
struct Shared {
    status: AtomicU8,
    dropped: AtomicU64,
    stop: AtomicBool,
}
struct Record {
    unix_ms: u128,
    elapsed_ms: u128,
    message: String,
}
struct Logger {
    sender: mpsc::SyncSender<Record>,
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
    started: Instant,
}
#[must_use]
pub fn log_path(state_directory: &Path) -> PathBuf {
    state_directory.join("diagnostics.log")
}
/// Initialize once. An unavailable log never prevents application startup.
pub fn start(state_directory: &Path) {
    LOGGER.get_or_init(|| Logger::start(log_path(state_directory)));
}
#[must_use]
pub fn status() -> Status {
    LOGGER.get().map_or(Status::Off, Logger::status)
}
#[must_use]
pub fn enabled() -> bool {
    status() == Status::Active
}
/// Bound message size before enqueueing. Never call from game presentation.
pub fn log(message: &str) {
    if let Some(logger) = LOGGER.get() {
        logger.log(message);
    }
}
/// Call after session/monitor cleanup. A blocked filesystem cannot hold app
/// exit indefinitely; the worker retains its resources until it returns.
pub fn shutdown() {
    if let Some(logger) = LOGGER.get() {
        logger.shutdown();
    }
}
impl Logger {
    fn start(path: PathBuf) -> Self {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let shared = Arc::new(Shared {
            status: AtomicU8::new(Status::Unavailable as u8),
            dropped: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        });
        let started = Instant::now();
        let worker = writer::Sink::open(path).ok().and_then(|sink| {
            let state = Arc::clone(&shared);
            state.status.store(Status::Active as u8, Ordering::Release);
            if let Ok(worker) = thread::Builder::new()
                .name("redunar-log".into())
                .spawn(move || writer::run(sink, &receiver, &state, started))
            {
                Some(worker)
            } else {
                shared
                    .status
                    .store(Status::Unavailable as u8, Ordering::Release);
                None
            }
        });
        Self {
            sender,
            shared,
            worker: Mutex::new(worker),
            started,
        }
    }
    fn status(&self) -> Status {
        match self.shared.status.load(Ordering::Acquire) {
            1 => Status::Active,
            3 => Status::Stopped,
            _ => Status::Unavailable,
        }
    }
    fn log(&self, message: &str) {
        if self.status() != Status::Active || self.shared.stop.load(Ordering::Acquire) {
            return;
        }
        let record = Record {
            unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            elapsed_ms: self.started.elapsed().as_millis(),
            message: sanitize(message),
        };
        match self.sender.try_send(record) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                self.shared.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.shared
                    .status
                    .store(Status::Unavailable as u8, Ordering::Release);
            }
        }
    }
    fn shutdown(&self) {
        self.shared.stop.store(true, Ordering::Release);
        let Ok(mut slot) = self.worker.lock() else {
            return;
        };
        if let Some(worker) = slot.take() {
            let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
            while !worker.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            if worker.is_finished() {
                if worker.join().is_err() {
                    self.shared
                        .status
                        .store(Status::Unavailable as u8, Ordering::Release);
                }
            } else {
                self.shared
                    .status
                    .store(Status::Unavailable as u8, Ordering::Release);
                // Dropping JoinHandle detaches; never release worker-owned data.
            }
        }
    }
}
impl Drop for Logger {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn sanitize(message: &str) -> String {
    // Typed events contain allowlisted values. This guard bounds legacy
    // messages and removes absolute paths/URLs from their raw IO errors.
    let end = message.floor_char_boundary(message.len().min(MAX_MESSAGE_BYTES));
    let mut text = String::with_capacity(end);
    for token in message[..end].split_whitespace() {
        let path_like = token.contains('/');
        if !text.is_empty() {
            text.push(' ');
        }
        if path_like {
            text.push_str("[private]");
            // A raw path may contain spaces without quotes. Suppress its
            // entire tail; actionable new events use typed reason codes.
            break;
        }
        text.extend(token.chars().filter(|c| !c.is_control()));
    }
    text.truncate(text.floor_char_boundary(text.len().min(MAX_MESSAGE_BYTES)));
    text
}
/// Existing stderr behavior is retained; the file copy is opt-in.
#[macro_export]
macro_rules! log_op {
    ($($argument:tt)*) => {{
        eprintln!($($argument)*);
        if $crate::diagnostic_log::enabled() { $crate::diagnostic_log::log(&format!($($argument)*)); }
    }};
}
#[cfg(test)]
mod tests;
