use super::{CaptureSessionError, lock_unpoisoned};
use redunar_capture::{CaptureMessage, CaptureSessionId, MAX_MESSAGE_BYTES};
use std::collections::BTreeMap;
use std::io;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

// Includes queued inputs, GPU inputs, and failed ACK retries. At capacity,
// capture stops accepting exports rather than losing an existing owner's route.
pub(super) const REPLAY_RELEASE_TARGET_CAPACITY: usize = 64;

#[derive(Clone)]
pub(super) struct ReplayExportOrigin {
    reply_path: PathBuf,
    wire_sequence: u64,
}

#[derive(Clone)]
pub(super) struct ReplayReleaseTransport {
    pub(super) session_id: CaptureSessionId,
    pub(super) reply_socket_path: PathBuf,
    socket: Option<Arc<UnixDatagram>>,
    next_token: Arc<AtomicU64>,
    pub(super) targets: Arc<Mutex<BTreeMap<u64, ReplayExportOrigin>>>,
    #[cfg(test)]
    acknowledgements: Option<Arc<Mutex<Vec<u64>>>>,
}

impl ReplayReleaseTransport {
    pub(super) fn at_capacity(&self) -> bool {
        lock_unpoisoned(&self.targets).len() >= REPLAY_RELEASE_TARGET_CAPACITY
    }

    pub(super) fn new(
        session_id: CaptureSessionId,
        reply_socket_path: PathBuf,
        socket: Option<Arc<UnixDatagram>>,
    ) -> Self {
        Self {
            session_id,
            reply_socket_path,
            socket,
            next_token: Arc::new(AtomicU64::new(1)),
            targets: Arc::new(Mutex::new(BTreeMap::new())),
            #[cfg(test)]
            acknowledgements: None,
        }
    }

    #[cfg(test)]
    pub(super) fn set_socket_for_test(&mut self, socket: Arc<UnixDatagram>) {
        self.socket = Some(socket);
    }

    #[cfg(test)]
    pub(super) fn with_acknowledgements(
        session_id: CaptureSessionId,
        acknowledgements: Arc<Mutex<Vec<u64>>>,
    ) -> Self {
        let mut transport = Self::new(session_id, PathBuf::new(), None);
        transport.acknowledgements = Some(acknowledgements);
        transport
    }

    /// Allocate a session-unique encoder token without changing the wire
    /// protocol. Process-local sequences are meaningful only with their owner.
    pub(super) fn register(
        &self,
        wire_sequence: u64,
        sender_path: Option<&Path>,
    ) -> Result<u64, CaptureSessionError> {
        let target = sender_path.unwrap_or(&self.reply_socket_path);
        let expected_parent = self
            .reply_socket_path
            .parent()
            .ok_or_else(|| CaptureSessionError::new("replay reply socket has no private parent"))?;
        if !target.is_absolute() || target.parent() != Some(expected_parent) {
            return Err(CaptureSessionError::new(
                "replay producer reply socket is outside the private session",
            ));
        }
        if wire_sequence == 0 {
            return Err(CaptureSessionError::new("replay export sequence is zero"));
        }
        let mut targets = lock_unpoisoned(&self.targets);
        if targets
            .values()
            .any(|origin| origin.reply_path == target && origin.wire_sequence == wire_sequence)
        {
            // A duplicate of a GPU-owned input must never trigger an early ACK.
            return Err(CaptureSessionError::new(
                "replay producer reused an in-flight export sequence",
            ));
        }
        if targets.len() >= REPLAY_RELEASE_TARGET_CAPACITY {
            return Err(CaptureSessionError::new(
                "replay release route limit reached",
            ));
        }
        let token = self
            .next_token
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .map_err(|_| CaptureSessionError::new("replay export token exhausted"))?;
        targets.insert(
            token,
            ReplayExportOrigin {
                reply_path: target.to_path_buf(),
                wire_sequence,
            },
        );
        Ok(token)
    }

    pub(super) fn release(&self, token: u64) -> Result<(), CaptureSessionError> {
        #[cfg(test)]
        if let Some(acknowledgements) = &self.acknowledgements {
            lock_unpoisoned(acknowledgements).push(token);
            lock_unpoisoned(&self.targets).remove(&token);
            return Ok(());
        }
        let origin = lock_unpoisoned(&self.targets)
            .get(&token)
            .cloned()
            .ok_or_else(|| CaptureSessionError::new("replay export has no reply route"))?;
        let message = CaptureMessage::ReplayFrameReleased {
            session_id: self.session_id,
            sequence: origin.wire_sequence,
        };
        let mut buffer = [0; MAX_MESSAGE_BYTES];
        let length = redunar_capture::encode_message(&message, &mut buffer)
            .map_err(|error| CaptureSessionError::owned(error.to_string()))?;
        let socket = self.socket.as_ref().ok_or_else(|| {
            CaptureSessionError::new("replay release transport has no session socket")
        })?;
        match redunar_capture_vulkan::fd_transport::send_datagram_to_nonblocking(
            socket,
            &buffer[..length],
            &origin.reply_path,
        )
        .and_then(|written| {
            if written == length {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "incomplete replay acknowledgement",
                ))
            }
        }) {
            Ok(()) => {
                lock_unpoisoned(&self.targets).remove(&token);
                Ok(())
            }
            // Producer destruction ends reuse of its buffers. A missing socket
            // completes only this token; another process's equal wire sequence
            // keeps its independent route and GPU ownership.
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                ) =>
            {
                lock_unpoisoned(&self.targets).remove(&token);
                Ok(())
            }
            Err(error) => Err(CaptureSessionError::owned(format!(
                "could not release replay export: {error}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        CAPTURE_LIBRARY_FILE, CaptureModuleGates, CaptureSessionConfig, CaptureSessionHandle,
    };
    use super::*;
    use std::fs;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn actual_handle_shutdown_returns_with_routes_and_ingress_saturated() {
        let root = std::env::temp_dir().join(format!("rd-stop-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let library = root.join(CAPTURE_LIBRARY_FILE);
        fs::write(&library, b"private layer fixture").unwrap();
        let mut handle = CaptureSessionHandle::start(
            &CaptureSessionConfig::new(root.join("runtime"), &library),
            CaptureModuleGates::default(),
        )
        .unwrap();
        for sequence in 1..=REPLAY_RELEASE_TARGET_CAPACITY as u64 {
            handle
                .shared
                .replay_release
                .register(sequence, None)
                .unwrap();
        }
        let socket = handle
            .shared
            .replay_release
            .socket
            .as_ref()
            .unwrap()
            .clone();
        let mut senders = Vec::new();
        let mut saturated = false;
        for _ in 0..16 {
            let sender = UnixDatagram::unbound().unwrap();
            let mut sent = 0;
            loop {
                match redunar_capture_vulkan::fd_transport::send_datagram_to_nonblocking(
                    &sender,
                    b"full",
                    &handle.socket_path,
                ) {
                    Ok(_) => {
                        sent += 1;
                        assert!(sent < 1024);
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) => panic!("fixture saturation failed: {error}"),
                }
            }
            senders.push(sender);
            if sent == 0 {
                saturated = true;
                break;
            }
        }
        assert!(saturated, "fixture did not fill ingress queue");
        let (finished, completion) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            handle.shutdown();
            finished.send(()).unwrap();
        });
        let result = completion.recv_timeout(Duration::from_secs(2));
        if result.is_err() {
            // Unblock a regressed wake send before failing, so the test never
            // leaves a detached shutdown thread or a private fixture behind.
            socket
                .set_read_timeout(Some(Duration::from_millis(10)))
                .unwrap();
            while socket.recv(&mut [0; 32]).is_ok() {}
        }
        worker.join().unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(result.is_ok(), "handle shutdown blocked on its wake send");
    }
}
