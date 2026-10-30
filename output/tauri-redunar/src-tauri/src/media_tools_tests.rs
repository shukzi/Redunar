use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        let path = std::env::temp_dir().join(format!(
            "redunar-media-tools-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn tool(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        // Identity fixture only. Never execute a freshly written script while
        // parallel tests are forking: inherited writable FDs can cause ETXTBSY.
        fs::write(&path, b"fixture").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn accepts_encoders_without_using_package_or_library_branding() {
    for description in [
        "Fedora ffmpeg-free + freeworld",
        "RPM Fusion ffmpeg",
        "custom FFmpeg",
    ] {
        let fixture = Fixture::new();
        let executable = fixture.tool("ffmpeg");
        assert_eq!(
            resolve("ffmpeg", Some(fixture.0.as_os_str())).unwrap(),
            executable
        );
        let cache = Mutex::new(None);
        for encoder in ["libx264", "aac", "mjpeg"] {
            require_encoder(&executable, encoder, &cache, || Ok(format!(
                "{description}\n V....D libx264 H.264 encoder\n A....D aac AAC encoder\n VFS..D mjpeg JPEG encoder\n"
            ).into_bytes())).unwrap();
        }
    }
}

#[test]
fn checks_exact_encoder_and_retries_after_codec_libraries_are_added() {
    let fixture = Fixture::new();
    let executable = fixture.tool("ffmpeg");
    let cache = Mutex::new(None);
    let message = require_encoder(&executable, "libx264", &cache, || {
        Ok(b" V....D libx264rgb not-the-required-encoder\n VFS..D mjpeg JPEG\n".to_vec())
    })
    .unwrap_err();
    assert!(message.contains("libx264 encoder"), "{message}");
    assert!(message.contains("RPM Fusion"));
    // Same binary identity, but installed shared libraries now supply the codec.
    require_encoder(&executable, "libx264", &cache, || {
        Ok(b" V....D libx264 H264\n VFS..D mjpeg JPEG\n".to_vec())
    })
    .unwrap();
}

#[test]
fn successful_capability_checks_are_cached_between_thumbnails() {
    let fixture = Fixture::new();
    let executable = fixture.tool("ffmpeg");
    let cache = Mutex::new(None);
    require_encoder(&executable, "mjpeg", &cache, || {
        Ok(b" VFS..D mjpeg JPEG\n".to_vec())
    })
    .unwrap();
    require_encoder(&executable, "mjpeg", &cache, || {
        panic!("unchanged successful lookup must not spawn a process")
    })
    .unwrap();
}

#[test]
fn missing_or_nonexecutable_tools_report_requirement_without_package_actions() {
    let fixture = Fixture::new();
    assert!(resolve("ffprobe", Some(fixture.0.as_os_str()))
        .unwrap_err()
        .contains("ffprobe"));
    fs::write(fixture.0.join("ffmpeg"), "not executable").unwrap();
    assert!(resolve("ffmpeg", Some(fixture.0.as_os_str())).is_err());
    assert!(resolve("ffmpeg", None).is_err());
}

#[test]
fn capability_probe_bounds_time_output_and_rejects_broken_tools() {
    let started = Instant::now();
    let error = run_probe(
        Command::new("/bin/sleep").arg("2"),
        Duration::from_millis(30),
        1024,
    )
    .unwrap_err();
    assert!(error.contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(1));
    let large_output = " VFS..D mjpeg JPEG\n".repeat(1000);
    assert!(run_probe(
        Command::new("/usr/bin/printf").args(["%s", &large_output]),
        PROBE_TIMEOUT,
        1024
    )
    .is_err());
    assert!(run_probe(&mut Command::new("/bin/false"), PROBE_TIMEOUT, PROBE_LIMIT).is_err());
    assert!(run_probe(
        Command::new("/usr/bin/printf").arg("not encoder information"),
        PROBE_TIMEOUT,
        PROBE_LIMIT
    )
    .is_err());
    let bytes = run_probe(
        Command::new("/usr/bin/printf").arg(" V....D libx264 H264\n"),
        PROBE_TIMEOUT,
        PROBE_LIMIT,
    )
    .unwrap();
    assert!(encoder_names(&bytes).contains("libx264"));
}

#[test]
fn clip_specific_missing_codecs_are_distinct_from_bad_media_or_storage() {
    for details in [
        "Unknown encoder 'aac'",
        "Decoding requested, but no decoder found for: h264",
        "No such filter: aresample",
        "error while loading shared libraries: libavcodec.so",
    ] {
        let message = capability_error(details).unwrap();
        assert!(message.contains("codec or format support"));
        assert!(!message.contains("libavcodec.so"));
    }
    assert!(capability_error("Disk quota exceeded").is_none());
    assert!(capability_error("Invalid data found when processing input").is_none());
}
