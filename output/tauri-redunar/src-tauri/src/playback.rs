//! Stream only the selected replay over loopback. WebKit's media pipeline can
//! request the whole file or seek by range without allocating the clip in RAM.
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    os::unix::fs::{DirBuilderExt, FileExt, MetadataExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

const BUFFER_BYTES: usize = 64 * 1024;
const MAX_HEADER_BYTES: usize = 8192;
const MAX_CONNECTIONS: usize = 4;
const IO_TIMEOUT: Duration = Duration::from_secs(2);
const PREPARE_TIMEOUT: Duration = Duration::from_secs(30);
const PREPARE_POLL_INTERVAL: Duration = Duration::from_millis(50);
const MAX_PREPARE_DIAGNOSTICS: usize = 8192;
const PREPARE_SIZE_ALLOWANCE: u64 = 4 * 1024 * 1024;
const PLAYER_TEMP_PREFIX: &str = "redunar-player-";
// Fresh preparations are actively written by this process; a copy is only
// swept when it predates this window, which also protects a second
// instance that is mid-startup before the control socket exists.
const STALE_PLAYER_COPY: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Default)]
pub struct Playback(Arc<Mutex<PlaybackState>>);

#[derive(Default)]
struct PlaybackState {
    closed: bool,
    server: Option<Server>,
    prepared: Option<PreparedPlayback>,
}

impl Playback {
    pub fn select(&self, file: File, _kind: &'static str) -> Result<String, String> {
        let mut guard = self.0.lock().map_err(|_| "Clip player unavailable")?;
        if guard.closed {
            return Err("Clip player has shut down".into());
        }
        let identity = SourceIdentity::read(&file)
            .map_err(|e| format!("Clip metadata could not be read: {e}"))?;
        // Normalize every recording through the same MP4 path. Some captured MP4s
        // contain an audio track that starts several seconds after video; passing
        // those files directly to WebKit makes its media clock jump to that offset.
        let selected = match guard
            .prepared
            .as_ref()
            .filter(|clip| clip.source == identity)
        {
            Some(prepared) => File::open(&prepared.temporary.path)
                .map_err(|e| format!("Prepared clip could not be reopened: {e}"))?,
            None => {
                let prepared = prepare_playback_mp4(&file, identity)
                    .map_err(|e| format!("Clip could not be prepared for seeking: {e}"))?;
                let selected = File::open(&prepared.temporary.path)
                    .map_err(|e| format!("Prepared clip could not be opened: {e}"))?;
                guard.prepared = Some(prepared);
                selected
            }
        };
        if guard.server.is_none() {
            guard.server =
                Some(Server::start().map_err(|e| format!("Local playback could not start: {e}"))?);
        }
        guard
            .server
            .as_ref()
            .unwrap()
            .select(selected, "video/mp4")
            .map_err(|e| format!("Clip could not be opened: {e}"))
    }
    pub fn shutdown(&self) {
        if let Ok(mut guard) = self.0.lock() {
            // A queued preparation worker must not restart playback after Quit.
            guard.closed = true;
            guard.server.take();
            guard.prepared.take();
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceIdentity {
    device: u64,
    inode: u64,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
}

impl SourceIdentity {
    fn read(file: &File) -> io::Result<Self> {
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "The clip is empty or not a regular file",
            ));
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            size: metadata.len(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
        })
    }
}

struct PreparedPlayback {
    source: SourceIdentity,
    temporary: TemporaryPlayback,
}

struct TemporaryPlayback {
    directory: PathBuf,
    path: PathBuf,
}

impl Drop for TemporaryPlayback {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_dir(&self.directory);
    }
}

fn prepare_playback_mp4(file: &File, source: SourceIdentity) -> io::Result<PreparedPlayback> {
    let directory = private_temporary_directory()?;
    let path = directory.join("clip.mp4");
    let temporary = TemporaryPlayback { directory, path };
    let max_output_bytes = source
        .size
        .checked_add(PREPARE_SIZE_ALLOWANCE.max(source.size / 50))
        .ok_or_else(|| io::Error::other("Clip size is unsupported"))?;
    let input = file.try_clone()?;
    let mut command = crate::media_tools::ffmpeg(None).map_err(io::Error::other)?;
    command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-i",
            "pipe:0",
            "-map",
            "0:v:0",
            "-map",
            "0:a?",
            "-c:v",
            "copy",
            "-af",
            "aresample=async=1:first_pts=0",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-map_metadata",
            "-1",
            "-fs",
            &max_output_bytes.to_string(),
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
        ])
        .arg(&temporary.path)
        .stdin(Stdio::from(input));
    run_preparation(&mut command)?;
    let metadata = fs::symlink_metadata(&temporary.path)?;
    if !metadata.file_type().is_file()
        || metadata.len() == 0
        || metadata.len() > max_output_bytes
        || !has_mp4_header(&temporary.path)?
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The prepared recording is invalid",
        ));
    }
    Ok(PreparedPlayback { source, temporary })
}

fn run_preparation(command: &mut Command) -> io::Result<()> {
    let mut child = command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "FFmpeg is required to prepare this recording for the native player",
                )
            } else {
                error
            }
        })?;
    // Drain concurrently to avoid a full stderr pipe stalling conversion. Keep
    // only a bounded tail and expose classified errors, never raw private paths.
    let stderr = child.stderr.take().expect("piped conversion diagnostics");
    let diagnostics = std::thread::spawn(move || preparation_diagnostics(stderr));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = diagnostics.join();
                return Err(error);
            }
        }
        if started.elapsed() >= PREPARE_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            let _ = diagnostics.join();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Preparing the recording timed out",
            ));
        }
        std::thread::sleep(PREPARE_POLL_INTERVAL);
    };
    let details = diagnostics.join().unwrap_or_default();
    if !status.success() {
        let storage_full =
            details.contains("Disk quota exceeded") || details.contains("No space left on device");
        return Err(if storage_full {
            io::Error::new(io::ErrorKind::StorageFull,
                "Not enough temporary storage to prepare this clip. Free space in /var/tmp and try again. The original recording is unchanged.")
        } else {
            io::Error::new(
                io::ErrorKind::InvalidData,
                crate::media_tools::capability_error(&details).unwrap_or_else(|| {
                    "The recording could not be converted for native playback".into()
                }),
            )
        });
    }
    Ok(())
}

fn preparation_diagnostics(mut output: impl Read) -> String {
    let mut tail = Vec::with_capacity(MAX_PREPARE_DIAGNOSTICS);
    let mut buffer = [0u8; 4096];
    while let Ok(length) = output.read(&mut buffer) {
        if length == 0 {
            break;
        }
        tail.extend_from_slice(&buffer[..length]);
        if tail.len() > MAX_PREPARE_DIAGNOSTICS {
            tail.drain(..tail.len() - MAX_PREPARE_DIAGNOSTICS);
        }
    }
    String::from_utf8_lossy(&tail).into_owned()
}

fn private_temporary_directory() -> io::Result<PathBuf> {
    for _ in 0..8 {
        let mut random = [0u8; 16];
        File::open("/dev/urandom")?.read_exact(&mut random)?;
        let token = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        // Playback copies can be hundreds of MiB. /tmp is commonly a bounded
        // RAM filesystem with per-user quotas, even while it reports free space.
        // Keep only our private, removable copy in Linux's large-file temp area.
        let path = Path::new("/var/tmp").join(format!("redunar-player-{token}"));
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&path) {
            Ok(()) => {
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Could not create a private playback directory",
    ))
}

/// Remove playback copies that a previous Redunar process owned but never
/// deleted because it crashed or was killed mid-playback. Only directories
/// matching our exact private naming, owned by this uid, and untouched for
/// longer than STALE_PLAYER_COPY are removed, so a copy this process or a
/// starting second instance is actively writing is never swept. Failures are
/// ignored: the copy is bounded, disposable, and startup must not fail
/// because a foreign directory cannot be removed.
pub(crate) fn sweep_stale_playback_copies() {
    sweep_stale_playback_copies_in(Path::new("/var/tmp"));
}

fn sweep_stale_playback_copies_in(root: &Path) {
    use std::os::unix::fs::MetadataExt;
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let current_uid = match fs::metadata("/proc/self") {
        Ok(metadata) => metadata.uid(),
        Err(_) => return,
    };
    let now = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(age) => age,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(suffix) = name.to_str().and_then(|value| value.strip_prefix(PLAYER_TEMP_PREFIX))
        else {
            continue;
        };
        // Our tokens are exactly 32 lowercase hex characters from /dev/urandom.
        if suffix.len() != 32
            || !suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            continue;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_dir() || metadata.uid() != current_uid {
            continue;
        }
        let modified = match metadata.modified() {
            Ok(modified) => modified,
            Err(_) => continue,
        };
        let age = now
            .checked_sub(
                modified
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default(),
            )
            .unwrap_or_default();
        if age < STALE_PLAYER_COPY {
            continue;
        }
        // The only content is clip.mp4. Unlinking a descriptor that some
        // survivor still holds open only removes the path, so an in-flight
        // read keeps working; a fresh directory is protected by its mtime.
        let _ = fs::remove_file(path.join("clip.mp4"));
        let _ = fs::remove_dir(&path);
    }
}

fn has_mp4_header(path: &Path) -> io::Result<bool> {
    let mut header = [0u8; 8];
    File::open(path)?.read_exact(&mut header)?;
    Ok(&header[4..] == b"ftyp")
}

struct Clip {
    token: String,
    file: File,
    size: u64,
    kind: &'static str,
    cancelled: AtomicBool,
}
struct Shared {
    selected: Mutex<Option<Arc<Clip>>>,
    stopping: AtomicBool,
    address: SocketAddr,
}
struct Server {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}
impl Server {
    fn start() -> io::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let shared = Arc::new(Shared {
            selected: Mutex::new(None),
            stopping: AtomicBool::new(false),
            address: listener.local_addr()?,
        });
        let state = shared.clone();
        let worker = std::thread::Builder::new()
            .name("redunar-player".into())
            .spawn(move || {
                let mut connections: Vec<JoinHandle<()>> = Vec::new();
                for incoming in listener.incoming() {
                    if state.stopping.load(Ordering::Acquire) {
                        break;
                    }
                    let Ok(mut stream) = incoming else {
                        break;
                    };
                    let mut i = 0;
                    while i < connections.len() {
                        if connections[i].is_finished() {
                            let _ = connections.swap_remove(i).join();
                        } else {
                            i += 1;
                        }
                    }
                    // No queue or unbounded thread creation while a decoder seeks.
                    if connections.len() >= MAX_CONNECTIONS {
                        continue;
                    }
                    let state = state.clone();
                    if let Ok(worker) = std::thread::Builder::new()
                        .name("redunar-video-read".into())
                        .spawn(move || {
                            let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                            let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                            let _ = serve(&mut stream, &state);
                        })
                    {
                        connections.push(worker);
                    }
                }
                for connection in connections {
                    let _ = connection.join();
                }
            })?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }
    fn select(&self, file: File, kind: &'static str) -> io::Result<String> {
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "The clip is empty or not a regular file",
            ));
        }
        // A new private capability for each selection: no paths or predictable
        // identifiers are exposed to other local web pages/processes.
        let mut random = [0u8; 32];
        File::open("/dev/urandom")?.read_exact(&mut random)?;
        let token = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let url = format!("http://{}/{token}", self.shared.address);
        let clip = Arc::new(Clip {
            token,
            file,
            size: metadata.len(),
            kind,
            cancelled: AtomicBool::new(false),
        });
        let mut selected = self
            .shared
            .selected
            .lock()
            .map_err(|_| io::Error::other("Player unavailable"))?;
        if let Some(old) = selected.replace(clip) {
            old.cancelled.store(true, Ordering::Release);
        }
        Ok(url)
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::Release);
        // Wake blocking accept; idle playback otherwise does no polling.
        let _ = TcpStream::connect_timeout(&self.shared.address, IO_TIMEOUT);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn reject(stream: &mut TcpStream, status: &str, extra: &str) -> io::Result<()> {
    write!(stream, "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: 0\r\nCache-Control: no-store\r\n{extra}\r\n")
}
fn serve(stream: &mut TcpStream, shared: &Shared) -> io::Result<()> {
    let started = Instant::now();
    let mut header = Vec::with_capacity(1024);
    while !header.ends_with(b"\r\n\r\n") {
        if shared.stopping.load(Ordering::Acquire) {
            return Ok(());
        }
        if header.len() >= MAX_HEADER_BYTES || started.elapsed() > IO_TIMEOUT {
            return reject(stream, "431 Request Header Fields Too Large", "");
        }
        let mut byte = [0];
        stream.read_exact(&mut byte)?;
        header.push(byte[0]);
    }
    let Ok(header) = std::str::from_utf8(&header) else {
        return reject(stream, "400 Bad Request", "");
    };
    let mut lines = header.split("\r\n");
    let parts = lines
        .next()
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>();
    if parts.len() != 3 || parts[2] != "HTTP/1.1" {
        return reject(stream, "400 Bad Request", "");
    }
    let (method, path) = (parts[0], parts[1]);
    if method != "GET" && method != "HEAD" {
        return reject(stream, "405 Method Not Allowed", "Allow: GET, HEAD\r\n");
    }
    let (mut host, mut range, mut origin) = (None, None, None);
    for line in lines.take_while(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            return reject(stream, "400 Bad Request", "");
        };
        let slot = match name.to_ascii_lowercase().as_str() {
            "host" => &mut host,
            "range" => &mut range,
            "origin" => &mut origin,
            _ => continue,
        };
        if slot.replace(value.trim()).is_some() {
            return reject(stream, "400 Bad Request", "");
        }
    }
    if host != Some(shared.address.to_string().as_str()) {
        return reject(stream, "403 Forbidden", "");
    }
    if origin.is_some_and(|value| !matches!(value, "tauri://localhost" | "http://tauri.localhost"))
    {
        return reject(stream, "403 Forbidden", "");
    }
    let clip = shared
        .selected
        .lock()
        .ok()
        .and_then(|selected| selected.clone());
    let Some(clip) = clip.filter(|clip| path == format!("/{}", clip.token)) else {
        return reject(stream, "404 Not Found", "");
    };
    // Range has no meaning for HEAD: return the selected representation's size.
    let range = if method == "HEAD" { None } else { range };
    let Some((start, end)) = byte_range(range, clip.size) else {
        return reject(
            stream,
            "416 Range Not Satisfiable",
            &format!("Content-Range: bytes */{}\r\n", clip.size),
        );
    };
    let status = if range.is_some() {
        "206 Partial Content"
    } else {
        "200 OK"
    };
    write!(stream, "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n", clip.kind, end - start + 1)?;
    if let Some(origin) = origin {
        write!(stream, "Access-Control-Allow-Origin: {origin}\r\n")?;
    }
    if range.is_some() {
        write!(
            stream,
            "Content-Range: bytes {start}-{end}/{}\r\n",
            clip.size
        )?;
    }
    write!(stream, "\r\n")?;
    if method == "HEAD" {
        return Ok(());
    }
    let mut buffer = [0; BUFFER_BYTES];
    let mut offset = start;
    while offset <= end
        && !shared.stopping.load(Ordering::Acquire)
        && !clip.cancelled.load(Ordering::Acquire)
    {
        let length = (end - offset + 1).min(BUFFER_BYTES as u64) as usize;
        clip.file.read_exact_at(&mut buffer[..length], offset)?;
        stream.write_all(&buffer[..length])?;
        offset += length as u64;
    }
    Ok(())
}

fn byte_range(header: Option<&str>, size: u64) -> Option<(u64, u64)> {
    if size == 0 {
        return None;
    }
    let Some(header) = header else {
        return Some((0, size - 1));
    };
    let (start, end) = header.strip_prefix("bytes=")?.split_once('-')?;
    let decimal = |s: &str| -> Option<u64> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            None
        } else {
            s.parse().ok()
        }
    };
    if start.is_empty() {
        let length = decimal(end)?.min(size);
        return (length > 0).then_some((size - length, size - 1));
    }
    let start = decimal(start)?;
    let end = if end.is_empty() {
        size - 1
    } else {
        decimal(end)?.min(size - 1)
    };
    (start < size && end >= start).then_some((start, end))
}

#[cfg(test)]
#[path = "playback_tests.rs"]
mod tests;
