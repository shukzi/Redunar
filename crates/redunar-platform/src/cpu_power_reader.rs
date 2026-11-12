//! Nonblocking, bounded client for the installed CPU package power helper.
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub(super) struct CpuPowerReader {
    connection: Option<Connection>,
    retry_after: Option<Instant>,
    retired: Option<Child>,
}
#[derive(Debug)]
struct Connection {
    socket: UnixStream,
    child: Child,
    pending: Option<Instant>,
    bytes: Vec<u8>,
}
impl CpuPowerReader {
    pub(super) fn sample(&mut self) -> Option<f64> {
        let now = Instant::now();
        if let Some(child) = self.retired.as_mut() {
            child.try_wait().ok().flatten()?;
            self.retired = None;
        }
        if self.connection.is_none() {
            if self.retry_after.is_some_and(|deadline| now < deadline) {
                return None;
            }
            self.retry_after = Some(now + Duration::from_secs(30));
            self.connection = Connection::start().ok();
        }
        let connection = self.connection.as_mut()?;
        if let Ok(power) = connection.sample(now) {
            power
        } else {
            self.retired = self.connection.take().map(Connection::stop);
            self.retry_after = Some(now + Duration::from_secs(30));
            None
        }
    }
}
impl Connection {
    fn start() -> std::io::Result<Self> {
        let (socket, peer) = UnixStream::pair()?;
        socket.set_nonblocking(true)?;
        let child = Command::new("/usr/bin/pkexec")
            .args([
                "--disable-internal-agent",
                "/usr/libexec/redunar-cpu-power-helper",
            ])
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::from(OwnedFd::from(peer.try_clone()?)))
            .stdout(Stdio::from(OwnedFd::from(peer)))
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Self {
            socket,
            child,
            pending: None,
            bytes: Vec::with_capacity(32),
        })
    }
    fn sample(&mut self, now: Instant) -> std::io::Result<Option<f64>> {
        let mut value = None;
        if let Some(sent) = self.pending {
            if now.duration_since(sent) > Duration::from_secs(3) {
                return Err(std::io::Error::other("CPU power helper timed out"));
            }
            let mut bytes = [0; 32];
            match self.socket.read(&mut bytes) {
                Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
                Ok(length) => {
                    self.bytes.extend_from_slice(&bytes[..length]);
                    if self.bytes.len() > 24 {
                        return Err(std::io::Error::other("oversized CPU power response"));
                    }
                    if self.bytes.ends_with(b"\n") {
                        value = parse_response(&self.bytes)?;
                        self.bytes.clear();
                        self.pending = None;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
                Err(error) => return Err(error),
            }
        }
        if self.pending.is_none() {
            self.socket.write_all(b"S")?;
            self.pending = Some(now);
        }
        Ok(value)
    }
}
impl Connection {
    fn stop(self) -> Child {
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
        self.child
    }
}
impl Drop for CpuPowerReader {
    fn drop(&mut self) {
        let child = self
            .connection
            .take()
            .map(Connection::stop)
            .or_else(|| self.retired.take());
        if let Some(mut child) = child {
            // One teardown waiter keeps shutdown nonblocking. Failed helpers are
            // retained until reaped, so retries cannot accumulate waiters.
            let _ = std::thread::Builder::new()
                .name("cpu-power-reaper".into())
                .spawn(move || {
                    let _ = child.wait();
                });
        }
    }
}
fn parse_response(bytes: &[u8]) -> std::io::Result<Option<f64>> {
    let text = std::str::from_utf8(bytes).map_err(std::io::Error::other)?;
    let value = text
        .strip_prefix("RCPU1 ")
        .and_then(|text| text.strip_suffix('\n'))
        .ok_or_else(|| std::io::Error::other("invalid CPU power protocol"))?;
    if value == "-" {
        return Ok(None);
    }
    let watts = value
        .parse::<u32>()
        .ok()
        .filter(|value| *value <= 2_000_000)
        .ok_or_else(|| std::io::Error::other("invalid CPU power value"))?;
    Ok(Some(f64::from(watts) / 1000.0))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Connection, UnixStream) {
        let (socket, peer) = UnixStream::pair().unwrap();
        socket.set_nonblocking(true).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let child = Command::new("/usr/bin/true").spawn().unwrap();
        (
            Connection {
                socket,
                child,
                pending: None,
                bytes: Vec::with_capacity(32),
            },
            peer,
        )
    }
    #[test]
    fn delayed_partial_stale_and_closed_responses_are_bounded() {
        let (mut client, mut peer) = fixture();
        let now = Instant::now();
        assert_eq!(client.sample(now).unwrap(), None);
        let mut request = [0];
        peer.read_exact(&mut request).unwrap();
        assert_eq!(request, [b'S']);
        assert_eq!(client.sample(now).unwrap(), None);
        peer.write_all(b"RCPU1 65").unwrap();
        assert_eq!(client.sample(now).unwrap(), None);
        peer.write_all(b"500\n").unwrap();
        assert_eq!(client.sample(now).unwrap(), Some(65.5));
        peer.read_exact(&mut request).unwrap();
        assert!(client.sample(now + Duration::from_secs(4)).is_err());
        client.stop().wait().unwrap();
        let (mut client, peer) = fixture();
        client.sample(now).unwrap();
        drop(peer);
        assert!(client.sample(now).is_err());
        client.stop().wait().unwrap();
    }
    #[test]
    fn malformed_response_retires_child_and_backoff_prevents_respawn() {
        let (client, mut peer) = fixture();
        let mut reader = CpuPowerReader {
            connection: Some(client),
            retry_after: None,
            retired: None,
        };
        assert_eq!(reader.sample(), None);
        peer.write_all(&[b'x'; 32]).unwrap();
        assert_eq!(reader.sample(), None);
        assert!(reader.connection.is_none());
        assert!(reader.retired.is_some());
        assert_eq!(reader.sample(), None);
        assert!(reader.connection.is_none());
        assert!(reader.retry_after.unwrap() > Instant::now());
    }
    #[test]
    #[ignore = "synthetic nonblocking IPC measurement; run explicitly in release mode"]
    fn cpu_power_ipc_latency() {
        let (mut client, mut peer) = fixture();
        let mut durations = Vec::new();
        let mut request = [0];
        client.sample(Instant::now()).unwrap();
        for _ in 0..1000 {
            peer.read_exact(&mut request).unwrap();
            peer.write_all(b"RCPU1 65500\n").unwrap();
            let start = Instant::now();
            assert_eq!(client.sample(start).unwrap(), Some(65.5));
            durations.push(start.elapsed().as_nanos());
        }
        durations.sort_unstable();
        println!(
            "CPU power IPC p50={} ns p95={} ns p99={} ns",
            durations[499], durations[949], durations[989]
        );
        client.stop().wait().unwrap();
    }
    #[test]
    fn bounded_power_protocol_rejects_invalid_or_incomplete_values() {
        assert_eq!(parse_response(b"RCPU1 65500\n").unwrap(), Some(65.5));
        assert_eq!(parse_response(b"RCPU1 0\n").unwrap(), Some(0.0));
        assert_eq!(parse_response(b"RCPU1 -\n").unwrap(), None);
        for invalid in [
            b"RCPU1 2000001\n".as_slice(),
            b"RCPU1 NaN\n",
            b"RCPU1 1\nRCPU1 2\n",
            b"RCPU1 1",
            b"RCPU2 1\n",
        ] {
            assert!(parse_response(invalid).is_err());
        }
    }
}
