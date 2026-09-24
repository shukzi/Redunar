use redunar_capture::{
    CaptureApi, CaptureMessage, CaptureSessionId, GoodbyeReason, MAX_FRAME_INTERVALS,
    MAX_MESSAGE_BYTES, OverlayFailureReason, OverlayRuntimeStatus, PROTOCOL_VERSION,
    ReplaySourceCandidate, ReplaySourceRejection, encode_frame_batch, encode_message,
};
use std::env;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::UnixDatagram;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};

const BATCH_INTERVALS: usize = 64;
const CLOCK_MONOTONIC: i32 = 1;

#[repr(C)]
struct Timespec {
    seconds: i64,
    nanoseconds: i64,
}

unsafe extern "C" {
    fn clock_gettime(clock_id: i32, time: *mut Timespec) -> i32;
}

struct Producer {
    initialized: bool,
    socket: Option<UnixDatagram>,
    session_id: Option<CaptureSessionId>,
    intervals: [u64; BATCH_INTERVALS],
    interval_count: usize,
    first_sequence: u64,
    previous_sequence: Option<u64>,
    active_devices: u32,
    overlay_status: Option<OverlayRuntimeStatus>,
    reply_path: Option<std::path::PathBuf>,
}

impl Default for Producer {
    fn default() -> Self {
        Self {
            initialized: false,
            socket: None,
            session_id: None,
            intervals: [0; BATCH_INTERVALS],
            interval_count: 0,
            first_sequence: 0,
            previous_sequence: None,
            active_devices: 0,
            overlay_status: None,
            reply_path: None,
        }
    }
}

static PRODUCER: LazyLock<Mutex<Producer>> = LazyLock::new(|| Mutex::new(Producer::default()));
static LAST_PRESENT_NS: AtomicU64 = AtomicU64::new(0);
static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static NEXT_REPLAY_COPY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static OVERLAY_ACTIVE_REPORTED: AtomicBool = AtomicBool::new(false);

pub(crate) fn device_created() {
    let started_ns = monotonic_ns().unwrap_or(1);
    let mut producer = lock(&PRODUCER);
    producer.initialize(started_ns);
    producer.active_devices = producer.active_devices.saturating_add(1);
}

pub(crate) fn device_destroyed() {
    let mut producer = lock(&PRODUCER);
    producer.active_devices = producer.active_devices.saturating_sub(1);
    if producer.active_devices == 0 {
        producer.finish();
        LAST_PRESENT_NS.store(0, Ordering::Relaxed);
        NEXT_SEQUENCE.store(0, Ordering::Relaxed);
        NEXT_REPLAY_COPY_SEQUENCE.store(0, Ordering::Relaxed);
        crate::replay_copy::reset_export_sequence();
    }
}

pub(crate) fn record_present_at(now_ns: u64) {
    poll_replay_releases();
    let previous_ns = LAST_PRESENT_NS.swap(now_ns, Ordering::Relaxed);
    if previous_ns == 0 || now_ns <= previous_ns {
        return;
    }
    let interval_ns = now_ns - previous_ns;
    let sequence = NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let Ok(mut producer) = PRODUCER.try_lock() else {
        return;
    };
    producer.push(sequence, interval_ns);
}

fn poll_replay_releases() {
    let Ok(producer) = PRODUCER.try_lock() else {
        return;
    };
    let Some(socket) = producer.socket.as_ref() else {
        return;
    };
    let mut bytes = [0; MAX_MESSAGE_BYTES];
    loop {
        match socket.recv(&mut bytes) {
            Ok(length) => {
                if let Ok(CaptureMessage::ReplayFrameReleased { sequence, .. }) =
                    redunar_capture::decode_message(&bytes[..length])
                {
                    crate::replay_copy::release_sequence(sequence);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => break,
        }
    }
}

pub(crate) fn record_replay_source_candidate(candidate: ReplaySourceCandidate) {
    record_replay_source_assessment(Ok(candidate));
}

pub(crate) fn record_overlay_active() {
    if OVERLAY_ACTIVE_REPORTED.load(Ordering::Relaxed) {
        return;
    }
    record_overlay_status(OverlayRuntimeStatus::Active);
}

pub(crate) fn record_overlay_error(reason: OverlayFailureReason) {
    // A renderer can become unavailable after an earlier successful present,
    // for example during swapchain recreation. Permit the next successful
    // submission to publish Active again after this error transition.
    OVERLAY_ACTIVE_REPORTED.store(false, Ordering::Relaxed);
    record_overlay_status(OverlayRuntimeStatus::Error(reason));
}

fn record_overlay_status(status: OverlayRuntimeStatus) {
    let Ok(mut producer) = PRODUCER.try_lock() else {
        return;
    };
    producer.send_overlay_status(status);
    if producer.overlay_status == Some(OverlayRuntimeStatus::Active) {
        OVERLAY_ACTIVE_REPORTED.store(true, Ordering::Relaxed);
    }
}

pub(crate) fn record_replay_source_rejected(reason: ReplaySourceRejection) {
    record_replay_source_assessment(Err(reason));
}

fn record_replay_source_assessment(
    assessment: Result<ReplaySourceCandidate, ReplaySourceRejection>,
) {
    let Ok(producer) = PRODUCER.try_lock() else {
        return;
    };
    let (Some(socket), Some(session_id)) = (&producer.socket, producer.session_id) else {
        return;
    };
    let message = match assessment {
        Ok(candidate) => CaptureMessage::ReplaySourceCandidate {
            session_id,
            candidate,
        },
        Err(reason) => CaptureMessage::ReplaySourceRejected { session_id, reason },
    };
    let mut buffer = [0; MAX_MESSAGE_BYTES];
    if let Ok(length) = encode_message(&message, &mut buffer) {
        let _ = socket.send(&buffer[..length]);
    }
}

pub(crate) fn record_replay_frame_copied(
    source: ReplaySourceCandidate,
    copied_bytes: u32,
    sample_checksum: u64,
) {
    let Ok(producer) = PRODUCER.try_lock() else {
        return;
    };
    let (Some(socket), Some(session_id)) = (&producer.socket, producer.session_id) else {
        return;
    };
    let message = CaptureMessage::ReplayFrameCopied {
        session_id,
        sequence: NEXT_REPLAY_COPY_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        source,
        copied_bytes,
        sample_checksum,
    };
    let mut buffer = [0; MAX_MESSAGE_BYTES];
    if let Ok(length) = encode_message(&message, &mut buffer) {
        let _ = socket.send(&buffer[..length]);
    }
}

/// Publish one bounded GPU-export record with the DMA-BUF transferred through
/// `SCM_RIGHTS`. The producer retains its own descriptor until the daemon sends
/// the matching release acknowledgement after encoder ownership ends.
#[allow(
    dead_code,
    reason = "called when the production DMA-BUF route is connected"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "the fixed protocol event keeps each bounded wire field explicit"
)]
pub(crate) fn record_replay_frame_exported(
    sequence: u64,
    fd_number: u32,
    source: ReplaySourceCandidate,
    offset: u32,
    stride: u32,
    modifier: u64,
    timestamp_ns: u64,
    duration_ns: u64,
) {
    let Ok(producer) = PRODUCER.try_lock() else {
        return;
    };
    let (Some(socket), Some(session_id)) = (&producer.socket, producer.session_id) else {
        return;
    };
    let message = CaptureMessage::ReplayFrameExported {
        session_id,
        sequence,
        fd_number,
        source,
        offset,
        stride,
        modifier,
        timestamp_ns,
        duration_ns,
    };
    let mut buffer = [0; MAX_MESSAGE_BYTES];
    if let Ok(length) = encode_message(&message, &mut buffer)
        && let Ok(descriptor) = i32::try_from(fd_number)
    {
        let _ = crate::fd_transport::send_datagram_fd(socket, &buffer[..length], descriptor);
    }
}

impl Producer {
    fn initialize(&mut self, started_ns: u64) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        if env::var("REDUNAR_CAPTURE_PROTOCOL").ok().as_deref()
            != Some(&PROTOCOL_VERSION.to_string())
        {
            return;
        }
        let Some(session_id) = env::var("REDUNAR_CAPTURE_SESSION")
            .ok()
            .and_then(|value| CaptureSessionId::from_hex(&value).ok())
        else {
            return;
        };
        let Some(socket_path) = env::var_os("REDUNAR_CAPTURE_SOCKET") else {
            return;
        };
        if !Path::new(&socket_path).is_absolute() {
            return;
        }
        let reply_path = env::var_os("REDUNAR_CAPTURE_REPLY_SOCKET")
            .or_else(|| {
                Path::new(&socket_path)
                    .parent()
                    .map(|parent| parent.join("capture-reply.sock").into_os_string())
            })
            .and_then(|base| process_reply_path(Path::new(&base), std::process::id()));
        let Some(reply_path) = reply_path else {
            return;
        };
        if let Ok(metadata) = std::fs::symlink_metadata(&reply_path)
            && (!metadata.file_type().is_socket() || std::fs::remove_file(&reply_path).is_err())
        {
            return;
        }
        let Ok(socket) = UnixDatagram::bind(&reply_path) else {
            return;
        };
        if std::fs::set_permissions(&reply_path, std::fs::Permissions::from_mode(0o600)).is_err()
            || socket.set_nonblocking(true).is_err()
            || socket.connect(&socket_path).is_err()
        {
            let _ = std::fs::remove_file(&reply_path);
            return;
        }

        let hello = CaptureMessage::Hello {
            session_id,
            process_id: std::process::id(),
            api: CaptureApi::Vulkan,
            producer_started_monotonic_ns: started_ns.max(1),
        };
        let mut buffer = [0; MAX_MESSAGE_BYTES];
        let Ok(length) = encode_message(&hello, &mut buffer) else {
            return;
        };
        if socket.send(&buffer[..length]).is_err() {
            let _ = std::fs::remove_file(&reply_path);
            return;
        }
        self.session_id = Some(session_id);
        self.socket = Some(socket);
        self.reply_path = Some(reply_path);
        if crate::overlay::renderer_requested_from_environment() {
            self.send_overlay_status(OverlayRuntimeStatus::Requested);
        }
    }

    fn send_overlay_status(&mut self, status: OverlayRuntimeStatus) {
        if self.overlay_status == Some(status) {
            return;
        }
        if status != OverlayRuntimeStatus::Requested && self.overlay_status.is_none() {
            self.send_overlay_status(OverlayRuntimeStatus::Requested);
            if self.overlay_status.is_none() {
                return;
            }
        }
        let (Some(socket), Some(session_id)) = (&self.socket, self.session_id) else {
            return;
        };
        let message = CaptureMessage::OverlayStatus { session_id, status };
        let mut buffer = [0; MAX_MESSAGE_BYTES];
        if let Ok(length) = encode_message(&message, &mut buffer)
            && socket.send(&buffer[..length]).is_ok()
        {
            self.overlay_status = Some(status);
        }
    }

    fn push(&mut self, sequence: u64, interval_ns: u64) {
        if self.socket.is_none() {
            return;
        }
        if self
            .previous_sequence
            .is_some_and(|previous| sequence != previous.saturating_add(1))
        {
            self.flush();
        }
        if self.interval_count == 0 {
            self.first_sequence = sequence;
        }
        self.intervals[self.interval_count] = interval_ns;
        self.interval_count += 1;
        self.previous_sequence = Some(sequence);
        if self.interval_count == BATCH_INTERVALS {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.interval_count == 0 {
            return;
        }
        let (Some(socket), Some(session_id)) = (&self.socket, self.session_id) else {
            self.interval_count = 0;
            return;
        };
        let mut buffer = [0; MAX_MESSAGE_BYTES];
        if let Ok(length) = encode_frame_batch(
            session_id,
            CaptureApi::Vulkan,
            self.first_sequence,
            &self.intervals[..self.interval_count],
            &mut buffer,
        ) {
            let _ = socket.send(&buffer[..length]);
        }
        self.interval_count = 0;
    }

    fn finish(&mut self) {
        self.flush();
        if let (Some(socket), Some(session_id)) = (&self.socket, self.session_id) {
            let goodbye = CaptureMessage::Goodbye {
                session_id,
                api: CaptureApi::Vulkan,
                last_sequence: self.previous_sequence.unwrap_or(0),
                reason: GoodbyeReason::Normal,
            };
            let mut buffer = [0; MAX_MESSAGE_BYTES];
            if let Ok(length) = encode_message(&goodbye, &mut buffer) {
                let _ = socket.send(&buffer[..length]);
            }
        }
        self.socket = None;
        if let Some(path) = self.reply_path.take() {
            let _ = std::fs::remove_file(path);
        }
        self.session_id = None;
        self.interval_count = 0;
        self.previous_sequence = None;
        self.overlay_status = None;
        OVERLAY_ACTIVE_REPORTED.store(false, Ordering::Relaxed);
        self.initialized = false;
    }
}

fn process_reply_path(base: &Path, process_id: u32) -> Option<std::path::PathBuf> {
    let parent = base.parent()?;
    if !base.is_absolute() || process_id == 0 {
        return None;
    }
    Some(parent.join(format!("r-{process_id}.sock")))
}

pub(crate) fn monotonic_ns() -> Option<u64> {
    let mut time = Timespec {
        seconds: 0,
        nanoseconds: 0,
    };
    // SAFETY: `time` points to valid writable storage for one `timespec`, and
    // `CLOCK_MONOTONIC` does not require any additional lifetime contract.
    if unsafe { clock_gettime(CLOCK_MONOTONIC, &raw mut time) } != 0
        || time.seconds < 0
        || time.nanoseconds < 0
    {
        return None;
    }
    u64::try_from(time.seconds)
        .ok()?
        .checked_mul(1_000_000_000)?
        .checked_add(u64::try_from(time.nanoseconds).ok()?)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

const _: () = assert!(BATCH_INTERVALS <= MAX_FRAME_INTERVALS);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_process_gets_a_distinct_reply_socket_in_the_private_session() {
        let base = Path::new("/run/user/1000/redunar/session/capture-reply.sock");
        assert_eq!(
            process_reply_path(base, 101),
            Some(Path::new("/run/user/1000/redunar/session/r-101.sock").to_path_buf())
        );
        assert_eq!(
            process_reply_path(base, 202),
            Some(Path::new("/run/user/1000/redunar/session/r-202.sock").to_path_buf())
        );
        assert!(process_reply_path(base, 0).is_none());
        assert!(process_reply_path(Path::new("capture-reply.sock"), 101).is_none());
    }

    #[test]
    fn monotonic_clock_returns_a_positive_value() {
        assert!(monotonic_ns().is_some_and(|value| value > 0));
    }

    #[test]
    fn disabled_producer_drops_frames_without_growing_storage() {
        let mut producer = Producer::default();
        for sequence in 0..10_000 {
            producer.push(sequence, 16_000_000);
        }
        assert_eq!(producer.interval_count, 0);
        assert!(producer.socket.is_none());
    }
}
