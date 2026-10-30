use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

// Linux sun_path has 108 bytes, including the terminating NUL. Reserve the
// whole producer filename before arming; capture.sock alone is shorter.
const MAX_SOCKET_PATH_BYTES: usize = 107;
const REPLY_FILENAME_TEMPLATE: &str = "v-0000000000000000.sock";

/// Check that a private session directory can hold every producer reply path.
///
/// # Errors
///
/// Returns an error for a relative directory or a path exceeding Linux's
/// pathname socket limit. Length is measured in bytes, including separators.
pub fn validate_replay_reply_directory(directory: &Path) -> io::Result<()> {
    if !directory.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "reply endpoint directory is not absolute",
        ));
    }
    if directory
        .join(REPLY_FILENAME_TEMPLATE)
        .as_os_str()
        .as_bytes()
        .len()
        > MAX_SOCKET_PATH_BYTES
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "capture runtime path is too long for producer reply endpoints",
        ));
    }
    Ok(())
}

/// Choose a fresh private reply endpoint for one producer incarnation.
///
/// A PID or a loaded-library counter can be reused while old DMA-BUF input
/// completions remain in the daemon. A fresh endpoint keeps those old ACKs
/// separate. The caller must bind without removing another endpoint first.
///
/// # Errors
///
/// Returns an error if the private parent or kernel entropy is unavailable,
/// or the full endpoint path exceeds Linux's pathname socket limit.
pub fn unique_replay_reply_path(base: &Path, api: crate::CaptureApi) -> io::Result<PathBuf> {
    let parent = base
        .parent()
        .filter(|_| base.is_absolute())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "reply endpoint has no absolute parent",
            )
        })?;
    validate_replay_reply_directory(parent)?;
    let mut nonce = [0; 8];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut nonce)?;
    let prefix = match api {
        crate::CaptureApi::Vulkan => 'v',
        crate::CaptureApi::OpenGl => 'g',
    };
    Ok(parent.join(format!("{prefix}-{:016x}.sock", u64::from_ne_bytes(nonce))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_path_reserves_the_full_linux_socket_limit_in_bytes() {
        // 83 directory bytes + separator + 23 filename bytes = 107.
        // Multibyte names must use their byte length, not their character count.
        let directory = PathBuf::from(format!("/{}", "é".repeat(41)));
        assert_eq!(directory.as_os_str().as_bytes().len(), 83);
        for api in [crate::CaptureApi::Vulkan, crate::CaptureApi::OpenGl] {
            let path = unique_replay_reply_path(&directory.join("capture-reply.sock"), api)
                .expect("107-byte reply path fits");
            assert_eq!(path.as_os_str().as_bytes().len(), 107);
            let too_long = PathBuf::from(format!("{}a", directory.display()));
            let error = unique_replay_reply_path(&too_long.join("capture-reply.sock"), api)
                .expect_err("108-byte reply path must be rejected before bind");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert!(error.to_string().contains("too long"));
        }
        assert!(validate_replay_reply_directory(Path::new("relative")).is_err());
    }
}
