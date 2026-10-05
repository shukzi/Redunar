//! Launch-time requests from Steam to the native session owner. This listener
//! prepares a session; the existing one-shot bridge still supplies capture state.
use crate::SteamAppId;
use crate::steam_launch_bridge::{SocketIdentity, remove_socket_if_same, validate_socket_parent};
use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAGIC: &[u8; 8] = b"RDSTMS01";
const REQUEST_BYTES: usize = 16;
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const START_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
pub const STEAM_BACKGROUND_ARGUMENT: &str = "--steam-background";

#[must_use]
pub fn steam_session_socket_path(runtime: &Path) -> PathBuf {
    runtime.join("redunar/steam-session-v1.sock")
}

/// App-lifetime listener. Only the backend owner may bind it. The callback
/// receives a same-user, kernel-verified PID and a bounded app ID, never argv.
pub struct SteamSessionBroker {
    socket: PathBuf,
    identity: SocketIdentity,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl SteamSessionBroker {
    /// Bind in the owner's private runtime directory.
    ///
    /// # Errors
    /// Returns an error for unsafe paths, live owners, or listener/worker failure.
    pub fn bind(
        socket: PathBuf,
        prepare: impl Fn(SteamAppId, u32) -> bool + Send + 'static,
    ) -> io::Result<Self> {
        validate_socket_parent(&socket).map_err(io::Error::other)?;
        let uid = fs::metadata("/proc/self")?.uid();
        if let Ok(metadata) = fs::symlink_metadata(&socket) {
            if !metadata.file_type().is_socket() || metadata.uid() != uid {
                return Err(io::Error::other("unsafe Steam session socket"));
            }
            if UnixStream::connect(&socket).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "Steam session owner is live",
                ));
            }
            remove_socket_if_same(&socket, SocketIdentity::from_metadata(&metadata));
        }
        let listener = UnixListener::bind(&socket)?;
        let identity = SocketIdentity::from_metadata(&fs::symlink_metadata(&socket)?);
        if let Err(error) = fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))
            .and_then(|()| listener.set_nonblocking(true))
        {
            remove_socket_if_same(&socket, identity);
            return Err(error);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("redunar-steam-sessions".into())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            // Input is fixed and bounded. A stalled peer cannot hold
                            // shutdown for the longer session-preparation timeout.
                            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                            let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
                            let accepted =
                                read_request(&mut stream, uid).is_ok_and(|(app, pid)| {
                                    !worker_stop.load(Ordering::Acquire) && prepare(app, pid)
                                });
                            let _ = stream.write_all(&[u8::from(accepted)]);
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::park_timeout(Duration::from_millis(250));
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(_) => break,
                    }
                }
            });
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                remove_socket_if_same(&socket, identity);
                return Err(error);
            }
        };
        Ok(Self {
            socket,
            identity,
            stop,
            worker: Some(worker),
        })
    }

    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
        remove_socket_if_same(&self.socket, self.identity);
    }
}

impl Drop for SteamSessionBroker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn read_request(stream: &mut UnixStream, uid: u32) -> io::Result<(SteamAppId, u32)> {
    let peer = getsockopt(&*stream, PeerCredentials).map_err(io::Error::other)?;
    let mut bytes = Vec::new();
    (&mut *stream)
        .take((REQUEST_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    decode_request(&bytes, peer.uid(), peer.pid(), uid)
}

fn decode_request(
    bytes: &[u8],
    peer_uid: u32,
    peer_process_id: i32,
    owner_uid: u32,
) -> io::Result<(SteamAppId, u32)> {
    if bytes.len() != REQUEST_BYTES
        || &bytes[..8] != MAGIC
        || peer_uid != owner_uid
        || peer_process_id <= 0
    {
        return Err(io::Error::other("invalid Steam session request"));
    }
    let app = u32::from_le_bytes(bytes[8..12].try_into().map_err(io::Error::other)?);
    let pid = u32::from_le_bytes(bytes[12..16].try_into().map_err(io::Error::other)?);
    if u32::try_from(peer_process_id).ok() != Some(pid) {
        return Err(io::Error::other("Steam session PID mismatch"));
    }
    Ok((
        SteamAppId::new(app).ok_or_else(|| io::Error::other("invalid Steam app ID"))?,
        pid,
    ))
}

/// Ask the existing owner to prepare this Steam game before exec.
///
/// # Errors
/// Returns an error for missing/unsafe owners, malformed replies or bounded I/O failure.
pub fn request_steam_session(socket: &Path, app: SteamAppId) -> io::Result<bool> {
    let mut stream = UnixStream::connect(socket)?;
    let peer = getsockopt(&stream, PeerCredentials).map_err(io::Error::other)?;
    if peer.uid() != fs::metadata("/proc/self")?.uid() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Steam session owner UID mismatch",
        ));
    }
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut request = Vec::from(MAGIC.as_slice());
    request.extend_from_slice(&app.get().to_le_bytes());
    request.extend_from_slice(&std::process::id().to_le_bytes());
    stream.write_all(&request)?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut reply = Vec::new();
    stream.take(2).read_to_end(&mut reply)?;
    match reply.as_slice() {
        [0] => Ok(false),
        [1] => Ok(true),
        _ => Err(io::Error::other("invalid Steam session reply")),
    }
}

/// Start the packaged sibling app only when no session listener exists, then
/// request preparation. The caller always retains Steam's original argv.
///
/// # Errors
/// Returns an error for an unavailable app, unsafe runtime, or bounded startup/I/O failure.
pub fn ensure_steam_session(
    app: SteamAppId,
    runtime: &Path,
    executable: &Path,
) -> io::Result<bool> {
    let runtime_metadata = fs::symlink_metadata(runtime)?;
    if !runtime.is_absolute()
        || !runtime_metadata.is_dir()
        || runtime_metadata.file_type().is_symlink()
        || runtime_metadata.uid() != fs::metadata("/proc/self")?.uid()
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unsafe Steam runtime directory",
        ));
    }
    let socket = steam_session_socket_path(runtime);
    match request_steam_session(&socket, app) {
        Ok(result) => return Ok(result),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) => {}
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(executable)?;
    if !executable.is_absolute()
        || !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o111 == 0
    {
        return Err(io::Error::other("background Redunar app is unavailable"));
    }
    let mut command = Command::new(executable);
    command
        .arg(STEAM_BACKGROUND_ARGUMENT)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    // Steam's game libraries and identity must not leak into its host app.
    for key in [
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "VK_LAYER_PATH",
        "VK_INSTANCE_LAYERS",
        "SteamAppId",
        "SteamGameId",
        "SteamOverlayGameId",
        "STEAM_COMPAT_APP_ID",
    ] {
        command.env_remove(key);
    }
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|key| key.starts_with("REDUNAR_")) {
            command.env_remove(key);
        }
    }
    let mut child = command.spawn()?;
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        match request_steam_session(&socket, app) {
            Ok(result) => return Ok(result),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) => {}
            Err(error) => return Err(error),
        }
        // Reap a starter that lost ownership; the winning owner may still be
        // binding. Never start another app or terminate the existing one.
        let _ = child.try_wait()?;
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "background Redunar startup timed out",
            ));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(app: u32, pid: u32) -> Vec<u8> {
        [MAGIC.as_slice(), &app.to_le_bytes(), &pid.to_le_bytes()].concat()
    }

    #[test]
    fn rejects_forged_peer_identity_and_malformed_requests() {
        assert!(decode_request(&request(42, 10), 1000, 10, 1000).is_ok());
        for (bytes, uid, pid) in [
            (request(42, 10), 1001, 10),
            (request(42, 11), 1000, 10),
            (request(0, 10), 1000, 10),
            (vec![0; 17], 1000, 10),
            (vec![], 1000, 10),
        ] {
            assert!(decode_request(&bytes, uid, pid, 1000).is_err());
        }
    }

    #[test]
    fn listener_survives_denial_and_repeated_sessions_and_preserves_live_owner() {
        let root = std::env::temp_dir().join(format!("rdstm-{}", std::process::id()));
        fs::create_dir(&root).expect("private fixture");
        let socket = root.join("session.sock");
        let mut broker = SteamSessionBroker::bind(socket.clone(), |app, pid| {
            app.get() == 42 && pid == std::process::id()
        })
        .expect("bind");
        assert!(SteamSessionBroker::bind(socket.clone(), |_, _| false).is_err());
        assert!(!request_steam_session(&socket, SteamAppId::new(43).unwrap()).unwrap());
        for _ in 0..2 {
            assert!(request_steam_session(&socket, SteamAppId::new(42).unwrap()).unwrap());
        }
        assert_eq!(
            fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(fs::metadata(&socket).unwrap().file_type().is_socket());
        broker.shutdown();
        assert!(!socket.exists());
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn absent_owner_starts_background_app_once_and_existing_owner_skips_startup() {
        let root = std::env::temp_dir().join(format!("rdstbg-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("redunar")).unwrap();
        let executable = root.join("redunar-tauri");
        let marker = root.join("started");
        fs::write(
            &executable,
            format!("#!/bin/sh\nprintf '%s' \"$1\" > '{}'\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let socket = steam_session_socket_path(&root);
        let signal = marker.clone();
        let owner = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !signal.exists() {
                assert!(Instant::now() < deadline, "fixture app did not start");
                thread::sleep(Duration::from_millis(5));
            }
            SteamSessionBroker::bind(socket, |app, _| app.get() == 42).unwrap()
        });
        let app = SteamAppId::new(42).unwrap();
        assert!(ensure_steam_session(app, &root, &executable).unwrap());
        assert_eq!(
            fs::read_to_string(&marker).unwrap(),
            STEAM_BACKGROUND_ARGUMENT
        );
        let mut broker = owner.join().unwrap();
        fs::remove_file(&marker).unwrap();
        assert!(ensure_steam_session(app, &root, &root.join("missing-app")).unwrap());
        assert!(!ensure_steam_session(SteamAppId::new(43).unwrap(), &root, &executable).unwrap());
        assert!(!marker.exists(), "denial must not start another app");
        broker.shutdown();
        assert!(ensure_steam_session(app, &root, &root.join("missing-app")).is_err());
        let link = root.join("linked-runtime");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        assert!(ensure_steam_session(app, &link, &executable).is_err());
        assert!(!marker.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
