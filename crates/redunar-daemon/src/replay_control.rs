use crate::{ProductionGameSessionCoordinator, ReplayRuntimeError};
use redunar_core::ReplayDuration;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const CONTROL_FILE: &str = "replay-control-v1.sock";
const MAX_COMMAND_BYTES: usize = 64;

/// App-lifetime local command owner for every Replay save entry point.
///
/// The socket lives below the login session's private runtime directory and
/// is mode 0600. The protocol intentionally has one bounded command and never
/// accepts paths, shell text, or encoder settings.
pub(crate) struct ReplayControlServer {
    path: Option<PathBuf>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl ReplayControlServer {
    pub(crate) fn start(
        coordinator: ProductionGameSessionCoordinator,
        path: Option<PathBuf>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let Some(path) = path else {
            return Self {
                path: None,
                stop,
                worker: None,
            };
        };
        match bind_control_socket(&path) {
            Ok(socket) => {
                let worker_stop = Arc::clone(&stop);
                let worker_path = path.clone();
                let worker = thread::Builder::new()
                    .name("redunar-replay-control".to_owned())
                    .spawn(move || run_control(&socket, &worker_stop, &coordinator));
                match worker {
                    Ok(worker) => Self {
                        path: Some(path),
                        stop,
                        worker: Some(worker),
                    },
                    Err(error) => {
                        let _ = fs::remove_file(&worker_path);
                        eprintln!("Redunar Replay control: worker could not start: {error}");
                        Self {
                            path: None,
                            stop,
                            worker: None,
                        }
                    }
                }
            }
            Err(error) => {
                eprintln!("Redunar Replay control: socket could not start: {error}");
                Self {
                    path: None,
                    stop,
                    worker: None,
                }
            }
        }
    }
}

impl Drop for ReplayControlServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(path) = self.path.as_ref() {
            let _ = UnixStream::connect(path);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Some(path) = self.path.as_ref() {
            let _ = fs::remove_file(path);
        }
    }
}

pub(crate) fn replay_control_socket_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|path| path.join("redunar").join(CONTROL_FILE))
}

fn bind_control_socket(path: &Path) -> io::Result<UnixListener> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing socket parent"))?;
    fs::create_dir_all(directory)?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        let directory_uid = fs::metadata(directory)?.uid();
        if !metadata.file_type().is_socket() || metadata.uid() != directory_uid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "existing Replay control path is not an owned socket",
            ));
        }
        match UnixStream::connect(path) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "another Redunar Replay control server is active",
                ));
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                ) =>
            {
                fs::remove_file(path)?;
            }
            Err(error) => return Err(error),
        }
    }
    let socket = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(socket)
}

fn run_control(
    socket: &UnixListener,
    stop: &AtomicBool,
    coordinator: &ProductionGameSessionCoordinator,
) {
    while !stop.load(Ordering::Acquire) {
        match socket.accept() {
            Ok((mut stream, _)) => handle_connection(&mut stream, coordinator),
            Err(error) => {
                eprintln!("Redunar Replay control: receive failed: {error}");
                break;
            }
        }
    }
}

fn handle_connection(stream: &mut UnixStream, coordinator: &ProductionGameSessionCoordinator) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    loop {
        let bytes = match read_command(stream) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return,
            Err(error) => {
                let _ = stream.write_all(format!("ERROR {error}\n").as_bytes());
                return;
            }
        };
        let now = std::time::Instant::now();
        let response = if coordinator.replay_menu_watchdog(now) {
            Ok("OK RELEASE\n")
        } else {
            parse_command(&bytes).and_then(|command| apply_command(command, coordinator, now))
        };
        let release = response
            .as_ref()
            .is_ok_and(|value| *value == "OK RELEASE\n");
        let response = match response {
            Ok(response) => response.to_owned(),
            Err(error) => format!("ERROR {error}\n"),
        };
        if let Err(error) = stream.write_all(response.as_bytes()) {
            eprintln!("Redunar Replay control: response failed: {error}");
            return;
        }
        if release {
            return;
        }
    }
}

fn read_command(stream: &mut UnixStream) -> Result<Option<Vec<u8>>, ReplayRuntimeError> {
    let mut command = Vec::with_capacity(16);
    let mut byte = [0_u8; 1];
    while command.len() <= MAX_COMMAND_BYTES {
        let read = stream
            .read(&mut byte)
            .map_err(|error| ReplayRuntimeError::new(format!("command read failed: {error}")))?;
        if read == 0 {
            return if command.is_empty() {
                Ok(None)
            } else {
                Err(ReplayRuntimeError::new("incomplete Replay command"))
            };
        }
        command.push(byte[0]);
        if byte[0] == b'\n' {
            break;
        }
    }
    if command.is_empty() || command.len() > MAX_COMMAND_BYTES || command.last() != Some(&b'\n') {
        return Err(ReplayRuntimeError::new("invalid Replay command length"));
    }
    Ok(Some(command))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReplayControlCommand {
    Save(ReplayDuration),
    ToggleMenu,
    Move(i32, i32),
    Button(bool),
    Escape,
    Ping,
}

fn parse_command(bytes: &[u8]) -> Result<ReplayControlCommand, ReplayRuntimeError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| ReplayRuntimeError::new("command is not UTF-8"))?;
    let line = text
        .strip_suffix('\n')
        .ok_or_else(|| ReplayRuntimeError::new("invalid Replay command"))?;
    if line == "MENU TOGGLE" {
        return Ok(ReplayControlCommand::ToggleMenu);
    }
    if line == "MENU ESCAPE" {
        return Ok(ReplayControlCommand::Escape);
    }
    if line == "MENU PING" {
        return Ok(ReplayControlCommand::Ping);
    }
    if let Some(value) = line.strip_prefix("MENU BUTTON ") {
        return match value {
            "0" => Ok(ReplayControlCommand::Button(false)),
            "1" => Ok(ReplayControlCommand::Button(true)),
            _ => Err(ReplayRuntimeError::new("invalid Replay button state")),
        };
    }
    if let Some(value) = line.strip_prefix("MENU MOVE ") {
        let mut values = value.split(' ');
        let dx = values.next().and_then(|v| v.parse::<i32>().ok());
        let dy = values.next().and_then(|v| v.parse::<i32>().ok());
        if values.next().is_some()
            || dx.is_none_or(|v| !(-4096..=4096).contains(&v))
            || dy.is_none_or(|v| !(-4096..=4096).contains(&v))
        {
            return Err(ReplayRuntimeError::new("Replay pointer delta is invalid"));
        }
        return Ok(ReplayControlCommand::Move(dx.unwrap_or(0), dy.unwrap_or(0)));
    }
    let seconds = line
        .strip_prefix("SAVE ")
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| ReplayRuntimeError::new("invalid Replay command"))?;
    replay_duration(seconds)
        .map(ReplayControlCommand::Save)
        .ok_or_else(|| ReplayRuntimeError::new("unsupported Replay duration"))
}

fn apply_command(
    command: ReplayControlCommand,
    coordinator: &ProductionGameSessionCoordinator,
    now: std::time::Instant,
) -> Result<&'static str, ReplayRuntimeError> {
    match command {
        ReplayControlCommand::Save(duration) => coordinator.save_replay(duration).map(|()| "OK\n"),
        ReplayControlCommand::ToggleMenu => coordinator
            .toggle_replay_menu(now)
            .map(|visible| if visible { "OK GRAB\n" } else { "OK RELEASE\n" }),
        ReplayControlCommand::Move(dx, dy) => {
            coordinator.move_replay_menu_cursor(dx, dy, now);
            Ok("OK\n")
        }
        ReplayControlCommand::Button(pressed) => coordinator
            .replay_menu_button(pressed, now)
            .map(|closed| if closed { "OK RELEASE\n" } else { "OK\n" }),
        ReplayControlCommand::Escape => {
            coordinator.close_replay_menu();
            Ok("OK RELEASE\n")
        }
        ReplayControlCommand::Ping => {
            coordinator.replay_menu_heartbeat(now);
            Ok("OK\n")
        }
    }
}

fn replay_duration(seconds: u16) -> Option<ReplayDuration> {
    match seconds {
        15 => Some(ReplayDuration::Seconds15),
        30 => Some(ReplayDuration::Seconds30),
        60 => Some(ReplayDuration::Seconds60),
        120 => Some(ReplayDuration::Seconds120),
        180 => Some(ReplayDuration::Seconds180),
        300 => Some(ReplayDuration::Seconds300),
        600 => Some(ReplayDuration::Seconds600),
        900 => Some(ReplayDuration::Seconds900),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_service_does_not_start_a_login_session_control_worker() {
        let service = crate::RedunarService::with_state_directory(
            std::env::temp_dir().join("redunar-control-isolation-unused"),
        );
        assert!(service.replay_control_path.is_none());
        let _ = service.game_session_coordinator();
        let server = service.runtime.replay_control.get().unwrap();
        assert!(server.path.is_none());
        assert!(server.worker.is_none());
    }

    #[test]
    fn control_protocol_accepts_only_bounded_typed_save_commands() {
        assert_eq!(
            parse_command(b"SAVE 30\n").expect("valid command"),
            ReplayControlCommand::Save(ReplayDuration::Seconds30)
        );
        assert!(parse_command(b"SAVE 31\n").is_err());
        assert!(parse_command(b"SAVE 30;touch /tmp/no\n").is_err());
        assert!(parse_command(&[0xff]).is_err());
    }

    #[test]
    fn interactive_commands_are_typed_and_pointer_deltas_are_bounded() {
        assert_eq!(
            parse_command(b"MENU TOGGLE\n").unwrap(),
            ReplayControlCommand::ToggleMenu
        );
        assert_eq!(
            parse_command(b"MENU MOVE -4096 4096\n").unwrap(),
            ReplayControlCommand::Move(-4096, 4096)
        );
        assert_eq!(
            parse_command(b"MENU BUTTON 0\n").unwrap(),
            ReplayControlCommand::Button(false)
        );
        assert_eq!(
            parse_command(b"MENU ESCAPE\n").unwrap(),
            ReplayControlCommand::Escape
        );
        assert_eq!(
            parse_command(b"MENU PING\n").unwrap(),
            ReplayControlCommand::Ping
        );
        assert!(parse_command(b"MENU MOVE 4097 0\n").is_err());
        assert!(parse_command(b"MENU MOVE 1 2 3\n").is_err());
    }
}
