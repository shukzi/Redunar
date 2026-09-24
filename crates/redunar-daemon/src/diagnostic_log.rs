//! Opt-in diagnostic logging for debugging reports.
//!
//! When the owner enables it in Global settings, Redunar records its
//! operational messages into a private bounded file inside the state
//! directory. The file holds launch, capture, replay, and failure messages
//! with timestamps; it deliberately excludes clip contents and never includes
//! audio or video data. Rotation keeps at most two files of a bounded size so
//! a long session cannot grow the log without limit. The feature is off by
//! default and the file may contain game names and session identifiers, which
//! the settings copy states plainly.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum bytes per log file before rotation.
const MAX_LOG_BYTES: u64 = 4 * 1024 * 1024;
const LOG_FILE: &str = "diagnostics.log";

static LOG_FILE_HANDLE: OnceLock<Mutex<Option<File>>> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// The diagnostic log path inside a state directory.
#[must_use]
pub fn log_path(state_directory: &Path) -> PathBuf {
    state_directory.join(LOG_FILE)
}

/// Open the bounded log file for appending. Safe to call once per process;
/// later calls are ignored. Failures are silent: diagnostics must never break
/// the application.
pub fn start(state_directory: &Path) {
    let path = log_path(state_directory);
    let _ = LOG_PATH.set(path.clone());
    let Ok(file) = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&path)
    else {
        return;
    };
    let slot = LOG_FILE_HANDLE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(file);
    }
}

/// Record one operational message with a timestamp when logging is enabled.
pub fn log(message: &str) {
    let Some(path) = LOG_PATH.get() else { return };
    let Some(slot) = LOG_FILE_HANDLE.get() else {
        return;
    };
    let Ok(mut guard) = slot.lock() else { return };
    let Some(file) = guard.as_mut() else { return };
    if fs::metadata(path).map_or(0, |metadata| metadata.len()) > MAX_LOG_BYTES {
        let previous = path.with_extension("log.1");
        let _ = fs::rename(path, &previous);
        if let Ok(replacement) = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)
        {
            *file = replacement;
        } else {
            return;
        }
    }
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let _ = writeln!(file, "[{seconds}] {message}");
    let _ = file.flush();
}

/// Record an operational message both on stderr and in the diagnostic log.
#[macro_export]
macro_rules! log_op {
    ($($argument:tt)*) => {{
        eprintln!($($argument)*);
        $crate::diagnostic_log::log(&format!($($argument)*));
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_path_lives_in_the_state_directory() {
        let path = log_path(Path::new("/tmp/redunar-state"));
        assert_eq!(path, PathBuf::from("/tmp/redunar-state/diagnostics.log"));
    }

    #[test]
    fn logging_is_silent_until_started() {
        // No start call in this test: log must not panic or create files.
        log("no-op before start");
    }
}
