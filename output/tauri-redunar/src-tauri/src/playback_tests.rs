use super::*;
use std::{
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::AtomicUsize,
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn queued_playback_clone_cannot_restart_after_shutdown() {
    let playback = Playback::default();
    let worker = playback.clone();
    playback.shutdown();
    let error = worker
        .select(File::open("/dev/null").unwrap(), "video/mp4")
        .unwrap_err();
    assert_eq!(error, "Clip player has shut down");
    assert!(worker.0.lock().unwrap().server.is_none());
}

struct Fixture {
    server: Server,
    file: PathBuf,
    body: Vec<u8>,
    url: String,
}
impl Fixture {
    fn new() -> Self {
        let file = std::env::temp_dir().join(format!(
            "redunar-player-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let body = (0..3 * 1024 * 1024)
            .map(|n| (n % 251) as u8)
            .collect::<Vec<_>>();
        std::fs::write(&file, &body).unwrap();
        let server = Server::start().unwrap();
        let url = server
            .select(File::open(&file).unwrap(), "video/mp4")
            .unwrap();
        Self {
            server,
            file,
            body,
            url,
        }
    }
    fn request(&self, method: &str, url: &str, headers: &str) -> (String, Vec<u8>) {
        let mut connection = TcpStream::connect(self.server.shared.address).unwrap();
        connection
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let path = url
            .strip_prefix(&format!("http://{}", self.server.shared.address))
            .unwrap();
        write!(
            connection,
            "{method} {path} HTTP/1.1\r\nHost: {}\r\n{headers}\r\n",
            self.server.shared.address
        )
        .unwrap();
        let mut response = Vec::new();
        connection.read_to_end(&mut response).unwrap();
        let end = response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap()
            + 4;
        (
            String::from_utf8(response[..end].to_vec()).unwrap(),
            response[end..].to_vec(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
    }
}

#[test]
fn large_clip_without_range_streams_every_byte_and_head_has_no_body() {
    let f = Fixture::new();
    let (headers, body) = f.request("GET", &f.url, "");
    assert!(headers.starts_with("HTTP/1.1 200"));
    assert!(headers.contains(&format!("Content-Length: {}", f.body.len())));
    assert_eq!(body, f.body);
    let (headers, body) = f.request("HEAD", &f.url, "Range: bytes=0-99\r\n");
    assert!(headers.starts_with("HTTP/1.1 200"));
    assert!(headers.contains(&format!("Content-Length: {}", f.body.len())));
    assert!(body.is_empty());
}
#[test]
fn seeking_returns_the_requested_bytes_and_invalid_ranges_are_rejected() {
    let f = Fixture::new();
    for (range, start, end) in [
        ("bytes=1048576-1048675", 1048576, 1048676),
        ("bytes=-37", f.body.len() - 37, f.body.len()),
        ("bytes=2097152-", 2097152, f.body.len()),
    ] {
        let (headers, body) = f.request("GET", &f.url, &format!("Range: {range}\r\n"));
        assert!(headers.starts_with("HTTP/1.1 206"));
        assert!(headers.contains(&format!(
            "Content-Range: bytes {}-{}/{}",
            start,
            end - 1,
            f.body.len()
        )));
        assert_eq!(body, f.body[start..end]);
    }
    for range in [
        "bytes=9999999-",
        "bytes=10-5",
        "bytes=-0",
        "bytes=0-1,3-4",
        "bytes=+1-2",
    ] {
        assert!(f
            .request("GET", &f.url, &format!("Range: {range}\r\n"))
            .0
            .starts_with("HTTP/1.1 416"));
    }
}
#[test]
fn only_current_private_selection_is_accessible() {
    let f = Fixture::new();
    assert!(f.request("POST", &f.url, "").0.starts_with("HTTP/1.1 405"));
    assert!(f
        .request(
            "GET",
            &format!("http://{}/wrong", f.server.shared.address),
            ""
        )
        .0
        .starts_with("HTTP/1.1 404"));
    assert!(f
        .request("GET", &f.url, "Origin: https://unrelated.example\r\n")
        .0
        .starts_with("HTTP/1.1 403"));
    let new_url = f
        .server
        .select(File::open(&f.file).unwrap(), "video/mp4")
        .unwrap();
    assert_ne!(new_url, f.url);
    assert!(f.request("HEAD", &f.url, "").0.starts_with("HTTP/1.1 404"));
    assert!(f
        .request("HEAD", &new_url, "")
        .0
        .starts_with("HTTP/1.1 200"));
}
#[test]
fn range_parser_preserves_full_and_suffix_requests() {
    assert_eq!(byte_range(None, 10_000_000), Some((0, 9_999_999)));
    assert_eq!(byte_range(Some("bytes=-200"), 100), Some((0, 99)));
    assert_eq!(byte_range(Some("bytes=1-999"), 100), Some((1, 99)));
    assert_eq!(byte_range(None, 0), None);
}

#[test]
fn matroska_preparation_creates_fast_start_mp4_and_removes_its_private_copy() {
    if !Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        return;
    }
    let source = std::env::temp_dir().join(format!(
        "redunar-seekless-fixture-{}-{}.mkv",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let output = File::create(&source).unwrap();
    let generated = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=32x32:r=2:d=1",
            "-c:v",
            "ffv1",
            "-f",
            "matroska",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::from(output))
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(generated.success());
    let source_bytes = std::fs::read(&source).unwrap();
    let cues = [0x1c, 0x53, 0xbb, 0x6b];
    assert!(!source_bytes.windows(cues.len()).any(|bytes| bytes == cues));

    let file = File::open(&source).unwrap();
    let prepared = prepare_playback_mp4(&file, SourceIdentity::read(&file).unwrap()).unwrap();
    let prepared_path = prepared.temporary.path.clone();
    let prepared_directory = prepared.temporary.directory.clone();
    assert_eq!(
        prepared_directory.parent(),
        Some(Path::new("/var/tmp")),
        "large playback copies must not consume /tmp quota"
    );
    let prepared_bytes = std::fs::read(&prepared_path).unwrap();
    assert_eq!(&prepared_bytes[4..8], b"ftyp");
    assert!(prepared_bytes
        .windows(4)
        .take(128 * 1024)
        .any(|bytes| bytes == b"moov"));
    assert_eq!(
        std::fs::metadata(&prepared_directory)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    drop(prepared);
    assert!(!prepared_path.exists());
    assert!(!prepared_directory.exists());
    std::fs::remove_file(source).unwrap();
}

#[test]
fn conversion_reports_storage_exhaustion_without_exposing_diagnostics() {
    for message in ["Disk quota exceeded", "No space left on device"] {
        let mut command = Command::new("/bin/sh");
        // Message is a process argument, not shell source. The fixture writes no
        // media and does not exhaust the host filesystem to reproduce the error.
        command.args([
            "-c",
            "printf '%s\\n' \"$1\" >&2; exit 1",
            "fixture",
            message,
        ]);
        let error = run_preparation(&mut command).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::StorageFull);
        assert!(error.to_string().contains("/var/tmp"));
        assert!(error
            .to_string()
            .contains("original recording is unchanged"));
    }
    let mut command = Command::new("/bin/sh");
    command.args([
        "-c",
        "printf '%s' '/private/recording corrupt packet' >&2; exit 1",
    ]);
    let error = run_preparation(&mut command).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(!error.to_string().contains("/private"));
}

#[test]
fn conversion_reports_missing_codecs_without_requesting_a_provider_swap() {
    let mut command = Command::new("/bin/sh");
    command.args([
        "-c",
        "printf '%s\\n' \"$1\" >&2; exit 1",
        "fixture",
        "Unknown encoder 'aac' for /private/recording.mkv",
    ]);
    let error = run_preparation(&mut command).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    let message = error.to_string();
    assert!(message.contains("codec or format support"));
    assert!(message.contains("RPM Fusion"));
    assert!(!message.contains("/private"));
    assert!(!message.contains("swap"));
}

#[test]
fn conversion_diagnostics_drain_large_output_but_retain_only_a_bounded_tail() {
    let mut output = vec![b'x'; 100_000];
    output.extend_from_slice(b"Disk quota exceeded");
    let mut reader = io::Cursor::new(output);
    let details = preparation_diagnostics(&mut reader);
    assert_eq!(reader.position(), reader.get_ref().len() as u64);
    assert_eq!(details.len(), MAX_PREPARE_DIAGNOSTICS);
    assert!(details.ends_with("Disk quota exceeded"));
    assert!(run_preparation(&mut Command::new("/bin/true")).is_ok());
}

fn make_player_dir(root: &Path, name: &str, age: Option<Duration>) -> PathBuf {
    let dir = root.join(name);
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("clip.mp4"), b"ftyp-stub").unwrap();
    if let Some(age) = age {
        let handle = File::open(&dir).unwrap();
        handle
            .set_modified(std::time::SystemTime::now() - age)
            .unwrap();
    }
    dir
}

#[test]
fn playback_sweep_removes_only_old_private_copies() {
    let root = std::env::temp_dir().join(format!(
        "redunar-player-sweep-{}",
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    let stale = make_player_dir(
        &root,
        "redunar-player-0123456789abcdef0123456789abcdef",
        Some(STALE_PLAYER_COPY * 2),
    );
    let fresh = make_player_dir(
        &root,
        "redunar-player-fedcba9876543210fedcba9876543210",
        None,
    );
    // Look-alikes that must survive: foreign prefix, uppercase token, short
    // token, and a matching-name file rather than a directory.
    let foreign = make_player_dir(
        &root,
        "other-player-0123456789abcdef0123456789abcdef",
        Some(STALE_PLAYER_COPY * 2),
    );
    let uppercase = make_player_dir(
        &root,
        "redunar-player-0123456789ABCDEF0123456789abcdef",
        Some(STALE_PLAYER_COPY * 2),
    );
    let short = make_player_dir(
        &root,
        "redunar-player-0123456789abcdef0123456789abcde",
        Some(STALE_PLAYER_COPY * 2),
    );
    fs::write(
        root.join("redunar-player-1123456789abcdef0123456789abcdef"),
        b"",
    )
    .unwrap();
    sweep_stale_playback_copies_in(&root);
    assert!(!stale.exists(), "old private copy must be removed");
    assert!(fresh.exists(), "in-progress copy must survive");
    assert!(foreign.exists());
    assert!(uppercase.exists());
    assert!(short.exists());
    fs::remove_dir_all(&root).unwrap();
}
