//! Identify the actual running executable without logging its private path.
use sha2::{Digest, Sha256};
use std::io::Read;
use std::time::{Duration, Instant};

pub fn start() {
    if !redunar_daemon::diagnostic_log::enabled() {
        return;
    }
    // One opt-in startup task; no executable hashing on UI or monitor ticks.
    let _ = std::thread::Builder::new()
        .name("redunar-build-log".into())
        .spawn(|| {
            let result = std::fs::File::open("/proc/self/exe")
                .ok()
                .and_then(|file| fingerprint(file, Instant::now() + Duration::from_secs(2)));
            if let Some(digest) = result {
                redunar_daemon::diagnostic_log::log(&format!("app executable_sha256={digest}"));
            } else {
                redunar_daemon::diagnostic_log::log("app executable_sha256=unavailable");
            }
        });
}
fn fingerprint(mut input: impl Read, deadline: Instant) -> Option<String> {
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_usize;
    loop {
        if Instant::now() >= deadline {
            return None;
        }
        let count = input.read(&mut buffer).ok()?;
        if count == 0 {
            return Some(format!("{:x}", digest.finalize()));
        }
        total = total.checked_add(count)?;
        if total > 256 * 1024 * 1024 {
            return None;
        }
        digest.update(&buffer[..count]);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn build_fingerprint_matches_exact_bytes_and_respects_deadline() {
        assert_eq!(
            fingerprint(&b"abc"[..], Instant::now() + Duration::from_secs(1)).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(fingerprint(&b"abc"[..], Instant::now()).is_none());
    }
}
