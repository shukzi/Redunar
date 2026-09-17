use crate::{
    EncodedReplayPacket, ReplayBudget, ReplayClipStore, ReplayOutputFormat, ReplayRing,
    ReplayVideoStream, StoredReplayClip,
};
use redunar_core::{ReplayDuration, ReplayQuality, ReplaySettings};
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const MARKER: &str = ".redunar-replay-spool-v1";
const MARKER_CONTENTS: &[u8] = b"redunar-replay-spool-v1\n";
const SEGMENT_PREFIX: &str = "redunar-segment-";
const SEGMENT_SUFFIX: &str = ".rseg";
const TEMP_SUFFIX: &str = ".tmp";
const SEGMENT_MAGIC: &[u8; 8] = b"RDNSEG01";
const MIN_SAVABLE_HISTORY_NS: u64 = NANOSECONDS_PER_SECOND;
const TARGET_SEGMENT_NS: u64 = 2_000_000_000;
const MAX_SEGMENT_NS: u64 = 4_000_000_000;
const MAX_SEGMENT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_SEGMENTS: usize = 512;
const COMMAND_CAPACITY: usize = 4;
const MAX_WRITE_LATENCY: Duration = Duration::from_millis(500);
const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;
const BYTES_PER_MEGABIT: u64 = 1_000_000 / 8;
const RETAINED_DURATION: ReplayDuration = ReplayDuration::Seconds900;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplaySpoolPhase {
    Running,
    SlowStorage,
    Failed,
    Shutdown,
}

impl ReplaySpoolPhase {
    const fn code(self) -> u8 {
        match self {
            Self::Running => 0,
            Self::SlowStorage => 1,
            Self::Failed => 2,
            Self::Shutdown => 3,
        }
    }

    const fn from_code(value: u8) -> Self {
        match value {
            0 => Self::Running,
            1 => Self::SlowStorage,
            2 => Self::Failed,
            _ => Self::Shutdown,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaySpoolSegment {
    pub path: PathBuf,
    pub start_timestamp_ns: u64,
    pub end_timestamp_ns: u64,
    pub bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaySpoolSnapshotPlan {
    pub segments: Vec<ReplaySpoolSegment>,
    pub requested_duration: ReplayDuration,
    pub available_duration_ns: u64,
    pub generation: u64,
}

#[derive(Debug)]
pub struct ReplayAssemblyJob {
    worker: JoinHandle<Result<StoredReplayClip, ReplaySpoolError>>,
}

impl ReplayAssemblyJob {
    /// Wait for the isolated clip assembler. Recording and spool rotation may
    /// continue while this worker owns open handles to the selected segments.
    ///
    /// # Errors
    ///
    /// Returns an assembly, validation, storage, or worker failure.
    pub fn join(self) -> Result<StoredReplayClip, ReplaySpoolError> {
        self.worker
            .join()
            .map_err(|_| ReplaySpoolError::new("Replay clip assembler panicked"))?
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReplaySpoolStats {
    pub active_segments: u32,
    pub active_bytes: u64,
    pub buffered_duration_ns: u64,
    /// Total encoded packets durably accepted by this spool epoch.
    pub accepted_packets: u64,
    pub queued_packets: u32,
    pub dropped_queue_full: u64,
    pub dropped_awaiting_keyframe: u64,
    pub recovered_segments: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaySpoolError {
    message: String,
    kind: ReplaySpoolErrorKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReplaySpoolErrorKind {
    General,
    InsufficientHistory,
}

impl ReplaySpoolError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: ReplaySpoolErrorKind::General,
        }
    }

    fn insufficient_history() -> Self {
        Self {
            message: "Replay just started; try saving again in a moment".to_owned(),
            kind: ReplaySpoolErrorKind::InsufficientHistory,
        }
    }

    pub(crate) const fn is_insufficient_history(&self) -> bool {
        matches!(self.kind, ReplaySpoolErrorKind::InsufficientHistory)
    }
}

impl fmt::Display for ReplaySpoolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ReplaySpoolError {}

#[derive(Debug)]
pub enum ReplaySpoolSubmitError {
    QueueFull(EncodedReplayPacket),
    NotRunning(EncodedReplayPacket),
}

impl fmt::Display for ReplaySpoolSubmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::QueueFull(_) => "Replay spool queue is full",
            Self::NotRunning(_) => "Replay spool is not running",
        })
    }
}

impl Error for ReplaySpoolSubmitError {}

enum SpoolCommand {
    Packet(EncodedReplayPacket),
    SealSnapshot(mpsc::Sender<Result<(), ReplaySpoolError>>),
    ResetEpoch,
    Shutdown,
}

#[derive(Clone, Copy)]
struct SpoolLimits {
    duration_ns: u64,
    bytes: u64,
}

impl SpoolLimits {
    fn new(duration: ReplayDuration, quality: ReplayQuality) -> Self {
        let seconds = u64::from(duration.seconds());
        let video_bytes = seconds
            .saturating_mul(u64::from(quality.target_megabits_per_second()))
            .saturating_mul(BYTES_PER_MEGABIT);
        Self {
            duration_ns: seconds.saturating_mul(NANOSECONDS_PER_SECOND),
            bytes: video_bytes.saturating_mul(110).div_ceil(100),
        }
    }
}

#[derive(Default)]
struct SpoolIndex {
    segments: VecDeque<ReplaySpoolSegment>,
    active_bytes: u64,
    generation: u64,
    recovered_segments: u32,
    dropped_awaiting_keyframe: u64,
}

struct SharedStats {
    phase: AtomicU8,
    queued_packets: AtomicU32,
    dropped_queue_full: AtomicU64,
    discontinuity: AtomicU64,
    first_packet_timestamp_ns: AtomicU64,
    latest_packet_end_ns: AtomicU64,
    accepted_packets: AtomicU64,
}

/// Asynchronous, disk-segment rolling history for already-encoded packets.
///
/// The presentation/encoder caller performs one bounded `try_send`; all file
/// creation, writes, flushes, renames, scans, and cleanup happen on the worker.
/// The queue retains at most four packet payloads (32 MiB worst case under the
/// existing 8 MiB packet contract), independent of the selected 15-second to
/// 15-minute interval. This type has no raw-frame or CPU-encoder input.
pub struct ReplaySegmentSpool {
    sender: SyncSender<SpoolCommand>,
    worker: Option<JoinHandle<()>>,
    index: Arc<Mutex<SpoolIndex>>,
    shared: Arc<SharedStats>,
}

impl ReplaySegmentSpool {
    /// Open an exact owned spool, recover complete segments, remove abandoned
    /// temporary files, enforce current limits, and start the I/O worker.
    ///
    /// # Errors
    ///
    /// Returns [`ReplaySpoolError`] for unsafe paths, ownership mismatch,
    /// excessive entries, malformed owned segments, I/O, or worker startup.
    pub fn open(
        directory: impl Into<PathBuf>,
        settings: ReplaySettings,
    ) -> Result<Self, ReplaySpoolError> {
        let directory = directory.into();
        if !directory.is_absolute() {
            return Err(ReplaySpoolError::new("Replay spool path must be absolute"));
        }
        initialize_spool(&directory)?;
        let limits = SpoolLimits::new(RETAINED_DURATION, settings.quality);
        let mut index = recover_index(&directory)?;
        enforce_limits(&directory, &mut index, limits)?;
        let index = Arc::new(Mutex::new(index));
        let shared = Arc::new(SharedStats {
            phase: AtomicU8::new(ReplaySpoolPhase::Running.code()),
            queued_packets: AtomicU32::new(0),
            dropped_queue_full: AtomicU64::new(0),
            discontinuity: AtomicU64::new(0),
            first_packet_timestamp_ns: AtomicU64::new(u64::MAX),
            latest_packet_end_ns: AtomicU64::new(0),
            accepted_packets: AtomicU64::new(0),
        });
        let (sender, receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
        let worker_index = Arc::clone(&index);
        let worker_shared = Arc::clone(&shared);
        let worker_directory = directory.clone();
        let worker = thread::Builder::new()
            .name("redunar-replay-spool".to_owned())
            .spawn(move || {
                run_worker(
                    &receiver,
                    &worker_directory,
                    limits,
                    &worker_index,
                    &worker_shared,
                );
            })
            .map_err(|error| {
                ReplaySpoolError::new(format!("could not start Replay spool: {error}"))
            })?;
        Ok(Self {
            sender,
            worker: Some(worker),
            index,
            shared,
        })
    }

    /// Queue one already-encoded packet without waiting for disk I/O.
    ///
    /// # Errors
    ///
    /// Returns the packet to the caller when the fixed queue is full or the
    /// worker is no longer running. Dropping is preferred over game blocking.
    pub fn try_submit(&self, packet: EncodedReplayPacket) -> Result<(), ReplaySpoolSubmitError> {
        if self.phase() != ReplaySpoolPhase::Running {
            return Err(ReplaySpoolSubmitError::NotRunning(packet));
        }
        match self.sender.try_send(SpoolCommand::Packet(packet)) {
            Ok(()) => {
                self.shared.queued_packets.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(TrySendError::Full(SpoolCommand::Packet(packet))) => {
                self.shared
                    .dropped_queue_full
                    .fetch_add(1, Ordering::Relaxed);
                self.shared.discontinuity.fetch_add(1, Ordering::Release);
                Err(ReplaySpoolSubmitError::QueueFull(packet))
            }
            Err(TrySendError::Disconnected(SpoolCommand::Packet(packet))) => {
                Err(ReplaySpoolSubmitError::NotRunning(packet))
            }
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                unreachable!("try_submit sends only packet commands")
            }
        }
    }

    /// Create one immutable keyframe-safe manifest plan from complete owned
    /// segments. This performs no file reads or writes. The assembler must
    /// revalidate every exact path before opening it because rolling cleanup
    /// may advance after this snapshot generation.
    ///
    /// # Errors
    ///
    /// Returns [`ReplaySpoolError`] when the requested interval exceeds the
    /// currently configured history or no complete segment is available.
    pub fn snapshot_plan(
        &self,
        requested: ReplayDuration,
    ) -> Result<ReplaySpoolSnapshotPlan, ReplaySpoolError> {
        if requested.seconds() > RETAINED_DURATION.seconds() {
            return Err(ReplaySpoolError::new(
                "requested Replay interval exceeds retained history",
            ));
        }
        let index = lock_index(&self.index);
        let newest = index
            .segments
            .back()
            .ok_or_else(|| ReplaySpoolError::new("no complete Replay segment is available"))?;
        let cutoff = newest
            .end_timestamp_ns
            .saturating_sub(u64::from(requested.seconds()) * NANOSECONDS_PER_SECOND);
        let mut segments = index
            .segments
            .iter()
            .rev()
            .take_while(|segment| segment.end_timestamp_ns > cutoff)
            .cloned()
            .collect::<Vec<_>>();
        segments.reverse();
        let available_duration_ns = segments.first().map_or(0, |first| {
            newest
                .end_timestamp_ns
                .saturating_sub(first.start_timestamp_ns)
        });
        if available_duration_ns < MIN_SAVABLE_HISTORY_NS {
            return Err(ReplaySpoolError::insufficient_history());
        }
        Ok(ReplaySpoolSnapshotPlan {
            segments,
            requested_duration: requested,
            available_duration_ns,
            generation: index.generation,
        })
    }

    /// Start assembling one keyframe-safe suffix without pausing recording.
    ///
    /// The selected segment files are opened before the worker starts. Their
    /// file descriptors remain valid if rolling retention unlinks the original
    /// names, which makes the save an immutable snapshot without copying
    /// encoded payloads on the caller thread. Segment contents are parsed with
    /// fixed record and payload bounds before being muxed into the clip store.
    ///
    /// # Errors
    ///
    /// Returns an error when no complete history exists, an exact selected
    /// segment was replaced, or the bounded assembler worker cannot start.
    pub fn save_async(
        &self,
        requested: ReplayDuration,
        settings: ReplaySettings,
        stream: ReplayVideoStream,
        store: ReplayClipStore,
    ) -> Result<ReplayAssemblyJob, ReplaySpoolError> {
        self.save_async_as(
            requested,
            settings,
            stream,
            store,
            ReplayOutputFormat::Matroska,
            crate::ReplayAudioSnapshot {
                stream: redunar_capture_audio::OpusStreamDescription::default(),
                packets: Vec::new(),
            },
        )
    }

    /// Assemble a snapshot into the selected supported output container.
    ///
    /// # Errors
    ///
    /// Returns an error under the same bounded snapshot, ownership, and worker
    /// startup conditions as [`Self::save_async`].
    pub fn save_async_as(
        &self,
        requested: ReplayDuration,
        mut settings: ReplaySettings,
        stream: ReplayVideoStream,
        store: ReplayClipStore,
        output_format: ReplayOutputFormat,
        audio: crate::ReplayAudioSnapshot,
    ) -> Result<ReplayAssemblyJob, ReplaySpoolError> {
        if requested.seconds() > RETAINED_DURATION.seconds() {
            return Err(ReplaySpoolError::new(
                "requested Replay interval exceeds retained history",
            ));
        }
        self.seal_active_tail()?;
        let plan = self.snapshot_plan(requested)?;
        let mut opened = Vec::with_capacity(plan.segments.len());
        for segment in &plan.segments {
            let file = open_snapshot_segment(segment)?;
            opened.push((segment.clone(), file));
        }
        settings.duration = requested;
        let worker = thread::Builder::new()
            .name("redunar-replay-assembler".to_owned())
            .spawn(move || {
                assemble_snapshot(opened, settings, &stream, &store, output_format, &audio)
            })
            .map_err(|error| {
                ReplaySpoolError::new(format!("could not start Replay clip assembler: {error}"))
            })?;
        Ok(ReplayAssemblyJob { worker })
    }

    /// Commit the currently writable segment after every packet already
    /// accepted by the bounded queue. This control-path wait does not perform
    /// file I/O on the encoder thread; it only establishes an ordered boundary
    /// so a save includes frames immediately preceding the request.
    fn seal_active_tail(&self) -> Result<(), ReplaySpoolError> {
        if self.phase() != ReplaySpoolPhase::Running {
            return Err(ReplaySpoolError::new("Replay spool is not running"));
        }
        let (response_sender, response_receiver) = mpsc::channel();
        self.sender
            .send(SpoolCommand::SealSnapshot(response_sender))
            .map_err(|_| ReplaySpoolError::new("Replay spool stopped before snapshot"))?;
        response_receiver
            .recv()
            .map_err(|_| ReplaySpoolError::new("Replay spool stopped during snapshot"))?
    }

    #[must_use]
    pub fn phase(&self) -> ReplaySpoolPhase {
        ReplaySpoolPhase::from_code(self.shared.phase.load(Ordering::Acquire))
    }

    #[must_use]
    pub fn stats(&self) -> ReplaySpoolStats {
        let index = lock_index(&self.index);
        ReplaySpoolStats {
            active_segments: u32::try_from(index.segments.len()).unwrap_or(u32::MAX),
            active_bytes: index.active_bytes,
            buffered_duration_ns: buffered_duration_ns(&self.shared),
            accepted_packets: self.shared.accepted_packets.load(Ordering::Relaxed),
            queued_packets: self.shared.queued_packets.load(Ordering::Relaxed),
            dropped_queue_full: self.shared.dropped_queue_full.load(Ordering::Relaxed),
            dropped_awaiting_keyframe: index.dropped_awaiting_keyframe,
            recovered_segments: index.recovered_segments,
        }
    }

    /// Discard the current codec epoch without pausing the packet producer.
    ///
    /// The command is ordered after packets already accepted by the queue and
    /// before packets accepted afterwards. This prevents a source resize or
    /// encoder reset from producing a clip that joins incompatible streams.
    ///
    /// # Errors
    ///
    /// Returns an error if the worker has already stopped. Reset is a rare
    /// control-path operation, so it may wait for the fixed queue to accept
    /// the ordered command; packet submission never waits this way.
    pub fn reset_epoch(&self) -> Result<(), ReplaySpoolError> {
        if self.phase() != ReplaySpoolPhase::Running {
            return Err(ReplaySpoolError::new("Replay spool is not running"));
        }
        self.sender
            .send(SpoolCommand::ResetEpoch)
            .map_err(|_| ReplaySpoolError::new("Replay spool stopped before reset"))
    }

    /// Stop and join the I/O worker. The current partial segment is discarded;
    /// only atomically committed complete segments survive recovery.
    pub fn shutdown(&mut self) {
        if self.worker.is_none() {
            return;
        }
        let _ = self.sender.send(SpoolCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.shared
            .phase
            .store(ReplaySpoolPhase::Shutdown.code(), Ordering::Release);
    }
}

fn open_snapshot_segment(segment: &ReplaySpoolSegment) -> Result<File, ReplaySpoolError> {
    let metadata = fs::symlink_metadata(&segment.path)
        .map_err(|error| io_error("could not inspect Replay snapshot segment", &error))?;
    if !metadata.file_type().is_file() || metadata.len() != segment.bytes {
        return Err(ReplaySpoolError::new(
            "Replay snapshot segment changed before assembly",
        ));
    }
    File::open(&segment.path)
        .map_err(|error| io_error("could not open Replay snapshot segment", &error))
}

fn assemble_snapshot(
    segments: Vec<(ReplaySpoolSegment, File)>,
    settings: ReplaySettings,
    stream: &ReplayVideoStream,
    store: &ReplayClipStore,
    output_format: ReplayOutputFormat,
    audio: &crate::ReplayAudioSnapshot,
) -> Result<StoredReplayClip, ReplaySpoolError> {
    let mut ring = ReplayRing::new(settings);
    for (segment, mut file) in segments {
        read_segment_packets(&segment, &mut file, &mut ring)?;
    }
    if ring.stats().stored_packets == 0 {
        return Err(ReplaySpoolError::new(
            "Replay snapshot contains no decodable encoded packets",
        ));
    }
    // Clip duration is selected at save time and can be longer than the
    // legacy duration retained in the recording profile. Re-open the owned
    // store with the requested interval's byte budget so long saves are not
    // capped by that stale profile value.
    let requested_store =
        ReplayClipStore::open_existing(store.directory(), ReplayBudget::from_settings(settings))
            .map_err(|error| {
                ReplaySpoolError::new(format!(
                    "could not prepare Replay storage for the requested interval: {error}"
                ))
            })?;
    requested_store
        .ensure_free_space_for_clip()
        .map_err(|error| {
            ReplaySpoolError::new(format!(
                "could not reserve space for assembled Replay: {error}"
            ))
        })?;
    requested_store
        .save_with_audio_as(stream, &ring, audio, output_format)
        .map_err(|error| {
            ReplaySpoolError::new(format!("could not store assembled Replay: {error}"))
        })
}

fn read_segment_packets(
    segment: &ReplaySpoolSegment,
    file: &mut File,
    ring: &mut ReplayRing,
) -> Result<(), ReplaySpoolError> {
    let mut magic = [0_u8; SEGMENT_MAGIC.len()];
    file.read_exact(&mut magic)
        .map_err(|error| io_error("could not read Replay snapshot header", &error))?;
    if &magic != SEGMENT_MAGIC {
        return Err(ReplaySpoolError::new(
            "Replay snapshot segment header is invalid",
        ));
    }
    let mut consumed = u64::try_from(SEGMENT_MAGIC.len()).unwrap_or(u64::MAX);
    while consumed < segment.bytes {
        let timestamp_ns = read_u64(file, "timestamp")?;
        let duration_ns = read_u64(file, "duration")?;
        let mut keyframe = [0_u8; 1];
        file.read_exact(&mut keyframe)
            .map_err(|error| io_error("could not read Replay snapshot keyframe flag", &error))?;
        if keyframe[0] > 1 {
            return Err(ReplaySpoolError::new(
                "Replay snapshot keyframe flag is invalid",
            ));
        }
        let mut length = [0_u8; 4];
        file.read_exact(&mut length)
            .map_err(|error| io_error("could not read Replay snapshot packet length", &error))?;
        let length = u32::from_le_bytes(length);
        let record_bytes = 8_u64 + 8 + 1 + 4 + u64::from(length);
        consumed = consumed
            .checked_add(record_bytes)
            .ok_or_else(|| ReplaySpoolError::new("Replay snapshot size overflows"))?;
        if length == 0 || length > 8 * 1024 * 1024 || consumed > segment.bytes {
            return Err(ReplaySpoolError::new(
                "Replay snapshot packet length is invalid",
            ));
        }
        let mut bytes = vec![0_u8; usize::try_from(length).unwrap_or(usize::MAX)];
        file.read_exact(&mut bytes)
            .map_err(|error| io_error("could not read Replay snapshot packet", &error))?;
        let packet = EncodedReplayPacket::new(timestamp_ns, duration_ns, keyframe[0] == 1, bytes)
            .map_err(|error| {
            ReplaySpoolError::new(format!("invalid Replay snapshot packet: {error}"))
        })?;
        ring.push(packet).map_err(|error| {
            ReplaySpoolError::new(format!("invalid Replay snapshot order: {error}"))
        })?;
    }
    if consumed != segment.bytes {
        return Err(ReplaySpoolError::new(
            "Replay snapshot segment is truncated",
        ));
    }
    Ok(())
}

fn read_u64(file: &mut File, field: &str) -> Result<u64, ReplaySpoolError> {
    let mut bytes = [0_u8; 8];
    file.read_exact(&mut bytes)
        .map_err(|error| io_error(&format!("could not read Replay snapshot {field}"), &error))?;
    Ok(u64::from_le_bytes(bytes))
}

impl Drop for ReplaySegmentSpool {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct SegmentWriter {
    temporary_path: PathBuf,
    file: File,
    start_timestamp_ns: u64,
    end_timestamp_ns: u64,
    bytes: u64,
    sequence: u64,
}

fn run_worker(
    receiver: &Receiver<SpoolCommand>,
    directory: &Path,
    limits: SpoolLimits,
    index: &Arc<Mutex<SpoolIndex>>,
    shared: &SharedStats,
) {
    let mut current: Option<SegmentWriter> = None;
    let mut next_sequence = lock_index(index).generation.saturating_add(1);
    let mut observed_discontinuity = shared.discontinuity.load(Ordering::Acquire);
    while let Ok(command) = receiver.recv() {
        match command {
            SpoolCommand::Packet(packet) => {
                shared.queued_packets.fetch_sub(1, Ordering::Relaxed);
                let discontinuity = shared.discontinuity.load(Ordering::Acquire);
                if discontinuity != observed_discontinuity {
                    if let Some(writer) = current.take() {
                        let _ = fs::remove_file(writer.temporary_path);
                    }
                    observed_discontinuity = discontinuity;
                    reset_buffered_duration(shared);
                }
                let started = Instant::now();
                if let Err(error) = process_packet(
                    directory,
                    &packet,
                    &mut current,
                    &mut next_sequence,
                    limits,
                    index,
                ) {
                    eprintln!("Redunar Replay: disk spool failed: {error}");
                    shared
                        .phase
                        .store(ReplaySpoolPhase::Failed.code(), Ordering::Release);
                    break;
                }
                record_buffered_packet(shared, &packet);
                if started.elapsed() > MAX_WRITE_LATENCY {
                    eprintln!("Redunar Replay: disk spool exceeded the bounded write latency");
                    shared
                        .phase
                        .store(ReplaySpoolPhase::SlowStorage.code(), Ordering::Release);
                    break;
                }
            }
            SpoolCommand::ResetEpoch => {
                if let Some(writer) = current.take()
                    && let Err(error) = fs::remove_file(writer.temporary_path)
                    && error.kind() != io::ErrorKind::NotFound
                {
                    eprintln!("Redunar Replay: disk spool epoch reset failed: {error}");
                    shared
                        .phase
                        .store(ReplaySpoolPhase::Failed.code(), Ordering::Release);
                    break;
                }
                if let Err(error) = clear_completed_segments(directory, index) {
                    eprintln!("Redunar Replay: completed spool cleanup failed: {error}");
                    shared
                        .phase
                        .store(ReplaySpoolPhase::Failed.code(), Ordering::Release);
                    break;
                }
                reset_buffered_duration(shared);
            }
            SpoolCommand::SealSnapshot(response) => {
                let result = current.take().map_or(Ok(()), |writer| {
                    commit_segment(directory, writer, limits, index)
                });
                let failed = result.is_err();
                let _ = response.send(result);
                if failed {
                    shared
                        .phase
                        .store(ReplaySpoolPhase::Failed.code(), Ordering::Release);
                    break;
                }
            }
            SpoolCommand::Shutdown => break,
        }
    }
    if let Some(writer) = current.take() {
        let _ = fs::remove_file(writer.temporary_path);
    }
    shared.queued_packets.store(0, Ordering::Relaxed);
}

fn record_buffered_packet(shared: &SharedStats, packet: &EncodedReplayPacket) {
    shared.accepted_packets.fetch_add(1, Ordering::Relaxed);
    let timestamp_ns = packet.timestamp_ns();
    let _ = shared.first_packet_timestamp_ns.compare_exchange(
        u64::MAX,
        timestamp_ns,
        Ordering::AcqRel,
        Ordering::Acquire,
    );
    shared.latest_packet_end_ns.fetch_max(
        timestamp_ns.saturating_add(packet.duration_ns()),
        Ordering::Release,
    );
}

fn reset_buffered_duration(shared: &SharedStats) {
    shared
        .first_packet_timestamp_ns
        .store(u64::MAX, Ordering::Release);
    shared.latest_packet_end_ns.store(0, Ordering::Release);
    shared.accepted_packets.store(0, Ordering::Release);
}

fn buffered_duration_ns(shared: &SharedStats) -> u64 {
    let first = shared.first_packet_timestamp_ns.load(Ordering::Acquire);
    if first == u64::MAX {
        return 0;
    }
    shared
        .latest_packet_end_ns
        .load(Ordering::Acquire)
        .saturating_sub(first)
}

fn process_packet(
    directory: &Path,
    packet: &EncodedReplayPacket,
    current: &mut Option<SegmentWriter>,
    next_sequence: &mut u64,
    limits: SpoolLimits,
    index: &Arc<Mutex<SpoolIndex>>,
) -> Result<(), ReplaySpoolError> {
    let packet_bytes = u64::try_from(packet.bytes().len()).unwrap_or(u64::MAX);
    let record_bytes = 8_u64 + 8 + 1 + 4 + packet_bytes;
    let should_rotate = current.as_ref().is_some_and(|writer| {
        packet.is_keyframe()
            && (packet
                .timestamp_ns()
                .saturating_sub(writer.start_timestamp_ns)
                >= TARGET_SEGMENT_NS
                || writer.bytes.saturating_add(record_bytes) > MAX_SEGMENT_BYTES)
    });
    if should_rotate {
        let writer = current.take().expect("rotation requires a current segment");
        commit_segment(directory, writer, limits, index)?;
    }
    if current.is_none() {
        if !packet.is_keyframe() {
            let mut index = lock_index(index);
            index.dropped_awaiting_keyframe = index.dropped_awaiting_keyframe.saturating_add(1);
            return Ok(());
        }
        *current = Some(create_segment(
            directory,
            *next_sequence,
            packet.timestamp_ns(),
        )?);
        *next_sequence = next_sequence.saturating_add(1);
    }
    let writer = current.as_mut().expect("segment was created");
    let elapsed = packet
        .timestamp_ns()
        .saturating_add(packet.duration_ns())
        .saturating_sub(writer.start_timestamp_ns);
    if elapsed > MAX_SEGMENT_NS || writer.bytes.saturating_add(record_bytes) > MAX_SEGMENT_BYTES {
        return Err(ReplaySpoolError::new(
            "encoder did not provide a keyframe within the segment bound",
        ));
    }
    writer
        .file
        .write_all(&packet.timestamp_ns().to_le_bytes())
        .and_then(|()| writer.file.write_all(&packet.duration_ns().to_le_bytes()))
        .and_then(|()| writer.file.write_all(&[u8::from(packet.is_keyframe())]))
        .and_then(|()| {
            writer.file.write_all(
                &u32::try_from(packet.bytes().len())
                    .unwrap_or(u32::MAX)
                    .to_le_bytes(),
            )
        })
        .and_then(|()| writer.file.write_all(packet.bytes()))
        .map_err(|error| io_error("could not write Replay segment", &error))?;
    writer.end_timestamp_ns = packet.timestamp_ns().saturating_add(packet.duration_ns());
    writer.bytes = writer.bytes.saturating_add(record_bytes);
    Ok(())
}

fn clear_completed_segments(
    directory: &Path,
    index: &Arc<Mutex<SpoolIndex>>,
) -> Result<(), ReplaySpoolError> {
    let mut index = lock_index(index);
    let mut removed = false;
    while let Some(segment) = index.segments.pop_front() {
        fs::remove_file(&segment.path)
            .map_err(|error| io_error("could not remove Replay segment from old epoch", &error))?;
        removed = true;
    }
    index.active_bytes = 0;
    index.generation = index.generation.saturating_add(1);
    if removed {
        sync_directory(directory)?;
    }
    Ok(())
}

fn create_segment(
    directory: &Path,
    sequence: u64,
    start_timestamp_ns: u64,
) -> Result<SegmentWriter, ReplaySpoolError> {
    let temporary_path = directory.join(format!(".{SEGMENT_PREFIX}{sequence:016x}{TEMP_SUFFIX}"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary_path)
        .map_err(|error| io_error("could not create Replay segment", &error))?;
    file.write_all(SEGMENT_MAGIC)
        .map_err(|error| io_error("could not write Replay segment header", &error))?;
    Ok(SegmentWriter {
        temporary_path,
        file,
        start_timestamp_ns,
        end_timestamp_ns: start_timestamp_ns,
        bytes: u64::try_from(SEGMENT_MAGIC.len()).unwrap_or(u64::MAX),
        sequence,
    })
}

fn commit_segment(
    directory: &Path,
    writer: SegmentWriter,
    limits: SpoolLimits,
    index: &Arc<Mutex<SpoolIndex>>,
) -> Result<(), ReplaySpoolError> {
    writer
        .file
        .sync_data()
        .map_err(|error| io_error("could not flush Replay segment", &error))?;
    drop(writer.file);
    let destination = directory.join(format!(
        "{SEGMENT_PREFIX}{:016x}-{:016x}-{:016x}{SEGMENT_SUFFIX}",
        writer.sequence, writer.start_timestamp_ns, writer.end_timestamp_ns
    ));
    fs::rename(&writer.temporary_path, &destination)
        .map_err(|error| io_error("could not commit Replay segment", &error))?;
    sync_directory(directory)?;
    let metadata = fs::symlink_metadata(&destination)
        .map_err(|error| io_error("could not inspect Replay segment", &error))?;
    let mut index = lock_index(index);
    index.active_bytes = index.active_bytes.saturating_add(metadata.len());
    index.generation = index.generation.saturating_add(1);
    index.segments.push_back(ReplaySpoolSegment {
        path: destination,
        start_timestamp_ns: writer.start_timestamp_ns,
        end_timestamp_ns: writer.end_timestamp_ns,
        bytes: metadata.len(),
    });
    enforce_limits(directory, &mut index, limits)
}

fn enforce_limits(
    directory: &Path,
    index: &mut SpoolIndex,
    limits: SpoolLimits,
) -> Result<(), ReplaySpoolError> {
    let mut removed = false;
    while index.segments.len() > MAX_SEGMENTS
        || index.active_bytes > limits.bytes
        || index
            .segments
            .front()
            .zip(index.segments.back())
            .is_some_and(|(oldest, newest)| {
                newest
                    .end_timestamp_ns
                    .saturating_sub(oldest.start_timestamp_ns)
                    > limits.duration_ns
            })
    {
        let segment = index
            .segments
            .pop_front()
            .ok_or_else(|| ReplaySpoolError::new("Replay spool limits are invalid"))?;
        fs::remove_file(&segment.path)
            .map_err(|error| io_error("could not remove expired Replay segment", &error))?;
        index.active_bytes = index.active_bytes.saturating_sub(segment.bytes);
        removed = true;
    }
    if removed {
        sync_directory(directory)?;
    }
    Ok(())
}

fn initialize_spool(directory: &Path) -> Result<(), ReplaySpoolError> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Err(ReplaySpoolError::new("Replay spool is not a directory")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir_all(directory)
            .map_err(|error| io_error("could not create Replay spool", &error))?,
        Err(error) => return Err(io_error("could not inspect Replay spool", &error)),
    }
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| io_error("could not secure Replay spool", &error))?;
    let marker = directory.join(MARKER);
    match fs::symlink_metadata(&marker) {
        Ok(_) => validate_marker(&marker),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if fs::read_dir(directory)
                .map_err(|error| io_error("could not inspect Replay spool", &error))?
                .next()
                .transpose()
                .map_err(|error| io_error("could not inspect Replay spool", &error))?
                .is_some()
            {
                return Err(ReplaySpoolError::new(
                    "refusing to claim a non-empty Replay spool",
                ));
            }
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&marker)
                .map_err(|error| io_error("could not create Replay spool marker", &error))?;
            file.write_all(MARKER_CONTENTS)
                .and_then(|()| file.sync_all())
                .map_err(|error| io_error("could not write Replay spool marker", &error))?;
            sync_directory(directory)
        }
        Err(error) => Err(io_error("could not inspect Replay spool marker", &error)),
    }
}

fn validate_marker(marker: &Path) -> Result<(), ReplaySpoolError> {
    let metadata = fs::symlink_metadata(marker)
        .map_err(|error| io_error("could not inspect Replay spool marker", &error))?;
    if !metadata.file_type().is_file() || metadata.len() != MARKER_CONTENTS.len() as u64 {
        return Err(ReplaySpoolError::new("Replay spool marker is invalid"));
    }
    let mut contents = Vec::new();
    File::open(marker)
        .and_then(|mut file| file.read_to_end(&mut contents))
        .map_err(|error| io_error("could not read Replay spool marker", &error))?;
    if contents != MARKER_CONTENTS {
        return Err(ReplaySpoolError::new("Replay spool marker is invalid"));
    }
    Ok(())
}

fn recover_index(directory: &Path) -> Result<SpoolIndex, ReplaySpoolError> {
    let mut index = SpoolIndex::default();
    let mut entry_count = 0_usize;
    for entry in
        fs::read_dir(directory).map_err(|error| io_error("could not scan Replay spool", &error))?
    {
        let entry = entry.map_err(|error| io_error("could not scan Replay spool", &error))?;
        entry_count = entry_count.saturating_add(1);
        if entry_count > MAX_SEGMENTS + 64 {
            return Err(ReplaySpoolError::new(
                "Replay spool contains too many entries",
            ));
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(&format!(".{SEGMENT_PREFIX}")) && name.ends_with(TEMP_SUFFIX) {
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|error| io_error("could not inspect temporary Replay segment", &error))?;
            if metadata.file_type().is_file() {
                fs::remove_file(entry.path()).map_err(|error| {
                    io_error("could not remove abandoned Replay segment", &error)
                })?;
            }
            continue;
        }
        let Some((sequence, start, end)) = parse_segment_name(name) else {
            continue;
        };
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| io_error("could not inspect Replay segment", &error))?;
        if !metadata.file_type().is_file() || start >= end || metadata.len() > MAX_SEGMENT_BYTES {
            return Err(ReplaySpoolError::new("owned Replay segment is invalid"));
        }
        let mut magic = [0_u8; 8];
        File::open(entry.path())
            .and_then(|mut file| file.read_exact(&mut magic))
            .map_err(|error| io_error("could not read Replay segment", &error))?;
        if &magic != SEGMENT_MAGIC {
            return Err(ReplaySpoolError::new(
                "owned Replay segment header is invalid",
            ));
        }
        index.active_bytes = index.active_bytes.saturating_add(metadata.len());
        index.generation = index.generation.max(sequence);
        index.recovered_segments = index.recovered_segments.saturating_add(1);
        index.segments.push_back(ReplaySpoolSegment {
            path: entry.path(),
            start_timestamp_ns: start,
            end_timestamp_ns: end,
            bytes: metadata.len(),
        });
    }
    index
        .segments
        .make_contiguous()
        .sort_by_key(|segment| segment.start_timestamp_ns);
    Ok(index)
}

fn parse_segment_name(name: &str) -> Option<(u64, u64, u64)> {
    let body = name
        .strip_prefix(SEGMENT_PREFIX)?
        .strip_suffix(SEGMENT_SUFFIX)?;
    let mut parts = body.split('-');
    let sequence = u64::from_str_radix(parts.next()?, 16).ok()?;
    let start = u64::from_str_radix(parts.next()?, 16).ok()?;
    let end = u64::from_str_radix(parts.next()?, 16).ok()?;
    (parts.next().is_none()).then_some((sequence, start, end))
}

fn lock_index(index: &Mutex<SpoolIndex>) -> MutexGuard<'_, SpoolIndex> {
    index
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn sync_directory(directory: &Path) -> Result<(), ReplaySpoolError> {
    File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|error| io_error("could not flush Replay spool directory", &error))
}

fn io_error(context: &str, error: &io::Error) -> ReplaySpoolError {
    ReplaySpoolError::new(format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReplayBudget, ReplayPacketFormat, ReplayVideoCodec};
    use redunar_core::{ReplayFrameRate, ReplayStorageLimit};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    fn fixture() -> PathBuf {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("redunar-replay-spool-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        path
    }

    fn settings(duration: ReplayDuration) -> ReplaySettings {
        ReplaySettings {
            duration,
            frame_rate: ReplayFrameRate::Fps60,
            quality: ReplayQuality::Efficient,
            storage_limit: ReplayStorageLimit::GiB5,
        }
    }

    fn packet(timestamp_ns: u64, keyframe: bool) -> EncodedReplayPacket {
        let nal = if keyframe { 0x65 } else { 0x41 };
        EncodedReplayPacket::new(
            timestamp_ns,
            1_000_000_000,
            keyframe,
            vec![0, 0, 0, 2, nal, 0x88],
        )
        .expect("packet")
    }

    fn short_packet(timestamp_ns: u64, keyframe: bool) -> EncodedReplayPacket {
        let nal = if keyframe { 0x65 } else { 0x41 };
        EncodedReplayPacket::new(
            timestamp_ns,
            16_666_667,
            keyframe,
            vec![0, 0, 0, 2, nal, 0x88],
        )
        .expect("short packet")
    }

    fn target_bitrate_packet(timestamp_ns: u64, keyframe: bool) -> EncodedReplayPacket {
        let packet_bytes = usize::try_from(
            u64::from(ReplayQuality::Efficient.target_megabits_per_second()) * BYTES_PER_MEGABIT,
        )
        .expect("one second target payload fits usize");
        packet_with_payload_bytes(timestamp_ns, keyframe, packet_bytes)
    }

    fn packet_with_payload_bytes(
        timestamp_ns: u64,
        keyframe: bool,
        packet_bytes: usize,
    ) -> EncodedReplayPacket {
        let mut payload = vec![0_u8; packet_bytes];
        payload[..4].copy_from_slice(
            &u32::try_from(packet_bytes - 4)
                .expect("bounded payload")
                .to_be_bytes(),
        );
        payload[4] = if keyframe { 0x65 } else { 0x41 };
        EncodedReplayPacket::new(timestamp_ns, NANOSECONDS_PER_SECOND, keyframe, payload)
            .expect("target bitrate packet")
    }

    fn stream() -> ReplayVideoStream {
        ReplayVideoStream::new(
            ReplayVideoCodec::H264,
            ReplayPacketFormat::H264LengthPrefixed4,
            1_920,
            1_080,
            60,
            vec![
                1, 66, 0, 30, 0xff, 0xe1, 0, 2, 0x67, 0x42, 1, 0, 2, 0x68, 0xce,
            ],
        )
        .expect("stream")
    }

    fn wait_for_segments(spool: &ReplaySegmentSpool, count: u32) {
        for _ in 0..100 {
            if spool.stats().active_segments >= count {
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        panic!("Replay spool did not commit {count} segments");
    }

    #[test]
    fn async_spool_keeps_present_path_queue_and_disk_history_bounded() {
        let root = fixture();
        let mut spool = ReplaySegmentSpool::open(&root, settings(ReplayDuration::Seconds15))
            .expect("open spool");
        for second in 0..40_u64 {
            loop {
                match spool.try_submit(packet(second * NANOSECONDS_PER_SECOND, second % 2 == 0)) {
                    Ok(()) => {
                        thread::sleep(Duration::from_millis(1));
                        break;
                    }
                    Err(ReplaySpoolSubmitError::QueueFull(_)) => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("submit failed: {error}"),
                }
            }
        }
        wait_for_segments(&spool, 1);
        spool.shutdown();
        let stats = spool.stats();
        assert!(stats.active_segments <= u32::try_from(MAX_SEGMENTS).unwrap_or(u32::MAX));
        assert!(
            stats.active_bytes
                <= SpoolLimits::new(RETAINED_DURATION, ReplayQuality::Efficient).bytes
        );
        assert_eq!(stats.buffered_duration_ns, 40 * NANOSECONDS_PER_SECOND);
        assert_eq!(COMMAND_CAPACITY, 4);
        assert_eq!(MAX_SEGMENT_BYTES, 32 * 1024 * 1024);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fifteen_minute_spool_budget_is_disk_bounded_without_growing_the_queue() {
        let efficient = SpoolLimits::new(ReplayDuration::Seconds900, ReplayQuality::Efficient);
        let balanced = SpoolLimits::new(ReplayDuration::Seconds900, ReplayQuality::Balanced);
        let high = SpoolLimits::new(ReplayDuration::Seconds900, ReplayQuality::High);
        assert_eq!(efficient.bytes, 1_485_000_000);
        assert_eq!(balanced.bytes, 2_970_000_000);
        assert_eq!(high.bytes, 4_950_000_000);
        assert_eq!(COMMAND_CAPACITY, 4);
        assert_eq!(MAX_SEGMENTS, 512);
        assert_eq!(MAX_SEGMENT_BYTES, 32 * 1024 * 1024);
    }

    #[test]
    fn keyframe_safe_snapshot_can_request_any_retained_duration() {
        let root = fixture();
        let mut spool = ReplaySegmentSpool::open(&root, settings(ReplayDuration::Seconds30))
            .expect("open spool");
        for second in 0..12_u64 {
            loop {
                match spool.try_submit(packet(second * NANOSECONDS_PER_SECOND, second % 2 == 0)) {
                    Ok(()) => {
                        thread::sleep(Duration::from_millis(1));
                        break;
                    }
                    Err(ReplaySpoolSubmitError::QueueFull(_)) => thread::yield_now(),
                    Err(error) => panic!("submit failed: {error}"),
                }
            }
        }
        wait_for_segments(&spool, 2);
        let plan = spool
            .snapshot_plan(ReplayDuration::Seconds15)
            .expect("snapshot plan");
        assert!(!plan.segments.is_empty());
        assert!(
            plan.segments
                .iter()
                .all(|segment| segment.start_timestamp_ns % 2_000_000_000 == 0)
        );
        let early_long_clip = spool
            .snapshot_plan(ReplayDuration::Seconds900)
            .expect("save available history before the full duration exists");
        assert!(
            early_long_clip.available_duration_ns
                < u64::from(ReplayDuration::Seconds900.seconds()) * NANOSECONDS_PER_SECOND
        );
        assert!(!early_long_clip.segments.is_empty());
        let before_repeat = spool.stats();
        let repeated = spool
            .snapshot_plan(ReplayDuration::Seconds15)
            .expect("repeated save must not consume history");
        assert!(!repeated.segments.is_empty());
        assert_eq!(spool.stats(), before_repeat);
        spool.shutdown();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn asynchronous_save_keeps_spooling_and_does_not_consume_history() {
        let root = fixture();
        let spool_root = root.join("spool");
        let clip_root = root.join("clips");
        let configured = settings(ReplayDuration::Seconds30);
        let mut spool = ReplaySegmentSpool::open(&spool_root, configured).expect("open spool");
        for second in 0..12_u64 {
            loop {
                match spool.try_submit(packet(second * NANOSECONDS_PER_SECOND, second % 2 == 0)) {
                    Ok(()) => {
                        thread::sleep(Duration::from_millis(1));
                        break;
                    }
                    Err(ReplaySpoolSubmitError::QueueFull(_)) => thread::yield_now(),
                    Err(error) => panic!("submit failed: {error}"),
                }
            }
        }
        wait_for_segments(&spool, 2);
        let store = ReplayClipStore::open(&clip_root, ReplayBudget::from_settings(configured))
            .expect("open store");
        let before = spool
            .snapshot_plan(ReplayDuration::Seconds15)
            .expect("history");
        let first = spool
            .save_async(
                ReplayDuration::Seconds15,
                configured,
                stream(),
                store.clone(),
            )
            .expect("start save");

        for second in 12..16_u64 {
            loop {
                match spool.try_submit(packet(second * NANOSECONDS_PER_SECOND, second % 2 == 0)) {
                    Ok(()) => break,
                    Err(ReplaySpoolSubmitError::QueueFull(_)) => thread::yield_now(),
                    Err(error) => panic!("submit during save failed: {error}"),
                }
            }
        }
        let first_clip = first.join().expect("assemble first clip");
        assert!(first_clip.bytes > 4);
        let second_clip = spool
            .save_async(
                ReplayDuration::Seconds15,
                configured,
                stream(),
                store.clone(),
            )
            .expect("start repeated save")
            .join()
            .expect("assemble repeated clip");
        assert_ne!(first_clip.path, second_clip.path);
        assert_eq!(store.inventory().expect("inventory").len(), 2);
        let after = spool
            .snapshot_plan(ReplayDuration::Seconds15)
            .expect("history remains");
        assert!(after.generation >= before.generation);
        assert!(!after.segments.is_empty());
        spool.shutdown();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn full_target_bitrate_clip_does_not_double_count_container_headroom() {
        let root = fixture();
        let spool_root = root.join("spool");
        let clip_root = root.join("clips");
        let configured = settings(ReplayDuration::Seconds15);
        let mut spool = ReplaySegmentSpool::open(&spool_root, configured).expect("open spool");
        for second in 0..15_u64 {
            let mut packet =
                target_bitrate_packet(second * NANOSECONDS_PER_SECOND, second % 2 == 0);
            loop {
                match spool.try_submit(packet) {
                    Ok(()) => break,
                    Err(ReplaySpoolSubmitError::QueueFull(returned)) => {
                        packet = returned;
                        thread::yield_now();
                    }
                    Err(error) => panic!("submit failed: {error}"),
                }
            }
        }
        let store = ReplayClipStore::open(&clip_root, ReplayBudget::from_settings(configured))
            .expect("open store");
        let clip = spool
            .save_async(ReplayDuration::Seconds15, configured, stream(), store)
            .expect("start full target bitrate save")
            .join()
            .expect("full target bitrate history fits its single configured headroom");
        assert!(clip.bytes > 5 * 1024 * 1024);
        spool.shutdown();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn requested_duration_sets_clip_budget_independently_of_legacy_profile_duration() {
        let root = fixture();
        let spool_root = root.join("spool");
        let clip_root = root.join("clips");
        let configured = settings(ReplayDuration::Seconds15);
        let mut spool = ReplaySegmentSpool::open(&spool_root, configured).expect("open spool");
        for second in 0..30_u64 {
            let mut packet = packet_with_payload_bytes(
                second * NANOSECONDS_PER_SECOND,
                second % 2 == 0,
                2_500_000,
            );
            loop {
                match spool.try_submit(packet) {
                    Ok(()) => break,
                    Err(ReplaySpoolSubmitError::QueueFull(returned)) => {
                        packet = returned;
                        thread::yield_now();
                    }
                    Err(error) => panic!("submit failed: {error}"),
                }
            }
        }
        let legacy_budget = ReplayBudget::from_settings(configured).maximum_ring_bytes;
        let store = ReplayClipStore::open(&clip_root, ReplayBudget::from_settings(configured))
            .expect("open store");
        let clip = spool
            .save_async(ReplayDuration::Seconds30, configured, stream(), store)
            .expect("start save longer than legacy profile duration")
            .join()
            .expect("requested duration supplies the clip byte budget");

        assert!(clip.bytes > legacy_budget);
        spool.shutdown();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn asynchronous_save_seals_and_includes_the_active_tail() {
        let root = fixture();
        let spool_root = root.join("spool");
        let clip_root = root.join("clips");
        let configured = settings(ReplayDuration::Seconds30);
        let mut spool = ReplaySegmentSpool::open(&spool_root, configured).expect("open spool");

        // This is shorter than the normal two-second rotation interval, so no
        // completed segment exists before the save request.
        spool.try_submit(packet(0, true)).expect("queue keyframe");
        spool
            .try_submit(packet(NANOSECONDS_PER_SECOND, false))
            .expect("queue tail frame");
        assert_eq!(spool.stats().active_segments, 0);

        let store = ReplayClipStore::open(&clip_root, ReplayBudget::from_settings(configured))
            .expect("open store");
        let clip = spool
            .save_async(
                ReplayDuration::Seconds15,
                configured,
                stream(),
                store.clone(),
            )
            .expect("active tail snapshot starts")
            .join()
            .expect("active tail assembles");

        assert!(clip.bytes > 4);
        assert_eq!(store.inventory().expect("inventory").len(), 1);
        let plan = spool
            .snapshot_plan(ReplayDuration::Seconds15)
            .expect("sealed tail is retained");
        assert_eq!(plan.segments.len(), 1);
        assert_eq!(plan.segments[0].start_timestamp_ns, 0);
        assert_eq!(
            plan.segments[0].end_timestamp_ns,
            2 * NANOSECONDS_PER_SECOND
        );

        // A new segment still observes the keyframe-safe start contract.
        spool
            .try_submit(packet(2 * NANOSECONDS_PER_SECOND, false))
            .expect("queue non-keyframe after snapshot");
        spool
            .try_submit(packet(3 * NANOSECONDS_PER_SECOND, true))
            .expect("queue keyframe after snapshot");
        spool
            .try_submit(packet(5 * NANOSECONDS_PER_SECOND, true))
            .expect("queue rotation keyframe");
        wait_for_segments(&spool, 2);

        spool.shutdown();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sub_second_history_is_not_committed_as_a_replay_clip() {
        let root = fixture();
        let configured = settings(ReplayDuration::Seconds30);
        let mut spool =
            ReplaySegmentSpool::open(root.join("spool"), configured).expect("open spool");
        spool
            .try_submit(short_packet(0, true))
            .expect("queue keyframe");
        spool
            .try_submit(short_packet(81_000_000, false))
            .expect("queue second frame");
        spool
            .try_submit(short_packet(147_000_000, false))
            .expect("queue third frame");

        let store =
            ReplayClipStore::open(root.join("clips"), ReplayBudget::from_settings(configured))
                .expect("open store");
        let Err(error) = spool.save_async(
            ReplayDuration::Seconds15,
            configured,
            stream(),
            store.clone(),
        ) else {
            panic!("sub-second history must not start a clip save");
        };

        assert!(error.is_insufficient_history());
        assert!(error.to_string().contains("just started"));
        assert!(store.inventory().expect("inventory").is_empty());
        assert_eq!(spool.phase(), ReplaySpoolPhase::Running);
        spool.shutdown();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reset_epoch_discards_completed_history_before_accepting_new_packets() {
        let root = fixture();
        let mut spool = ReplaySegmentSpool::open(&root, settings(ReplayDuration::Seconds30))
            .expect("open spool");
        for second in 0..5_u64 {
            loop {
                match spool.try_submit(packet(second * NANOSECONDS_PER_SECOND, second % 2 == 0)) {
                    Ok(()) => break,
                    Err(ReplaySpoolSubmitError::QueueFull(_)) => thread::yield_now(),
                    Err(error) => panic!("submit failed: {error}"),
                }
            }
        }
        wait_for_segments(&spool, 1);

        spool.reset_epoch().expect("queue epoch reset");
        for _ in 0..100 {
            if spool.stats().active_segments == 0 {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(spool.stats().active_segments, 0);
        assert!(spool.snapshot_plan(ReplayDuration::Seconds15).is_err());

        // Timestamps may restart after a codec epoch reset. A non-keyframe
        // remains ineligible until a new independent segment starts.
        spool
            .try_submit(packet(0, false))
            .expect("queue first new-epoch packet");
        spool
            .try_submit(packet(NANOSECONDS_PER_SECOND, true))
            .expect("queue first new-epoch keyframe");
        spool
            .try_submit(packet(3 * NANOSECONDS_PER_SECOND, true))
            .expect("queue rotation keyframe");
        wait_for_segments(&spool, 1);
        let plan = spool
            .snapshot_plan(ReplayDuration::Seconds15)
            .expect("new epoch history");
        assert_eq!(plan.segments[0].start_timestamp_ns, NANOSECONDS_PER_SECOND);
        spool.shutdown();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovery_removes_partial_files_and_retains_only_exact_owned_segments() {
        let root = fixture();
        {
            let mut spool = ReplaySegmentSpool::open(&root, settings(ReplayDuration::Seconds30))
                .expect("open spool");
            for second in 0..5_u64 {
                loop {
                    if spool
                        .try_submit(packet(second * NANOSECONDS_PER_SECOND, second % 2 == 0))
                        .is_ok()
                    {
                        break;
                    }
                    thread::yield_now();
                }
            }
            wait_for_segments(&spool, 1);
            spool.shutdown();
        }
        fs::write(root.join(".redunar-segment-dead.tmp"), b"partial")
            .expect("write abandoned segment");
        fs::write(root.join("user-file"), b"keep").expect("write unknown file");
        let mut recovered = ReplaySegmentSpool::open(&root, settings(ReplayDuration::Seconds30))
            .expect("recover spool");
        assert!(recovered.stats().recovered_segments >= 1);
        assert!(!root.join(".redunar-segment-dead.tmp").exists());
        assert_eq!(
            fs::read(root.join("user-file")).expect("unknown retained"),
            b"keep"
        );
        recovered.shutdown();
        let _ = fs::remove_dir_all(root);
    }
}
