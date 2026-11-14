use super::*;
use crate::capture_session::CaptureModuleGates;
use crate::{
    CaptureSessionConfig, EncodedPacketBatch, HardwareEncodeOutput, HardwareEncoderApi,
    ReplayBudget, ReplayClipStore, ReplayEncoderError, ReplayPacketFormat, ReplaySegmentSpool,
    ReplayVideoCodec, ReplayVideoStream,
};
use std::fs;
use std::sync::atomic::AtomicU64;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rd-recovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn session(&self) -> ProductionGameSessionCoordinator {
        let library = self.0.join("layer.so");
        fs::write(&library, b"fixture").unwrap();
        let capture = CaptureSessionHandle::start(
            &CaptureSessionConfig::new(self.0.join("runtime"), library),
            CaptureModuleGates::default(),
        )
        .unwrap();
        let coordinator = ProductionGameSessionCoordinator::new(self.0.join("state"));
        lock_unpoisoned(&coordinator.inner.replay_preferences).microphone = MicrophoneMode::Off;
        coordinator
            .begin(
                request(),
                Some(capture),
                ReplayRuntimeStatus::validation_candidate(ReplaySettings::default()),
            )
            .unwrap();
        coordinator
    }
    fn pipeline(&self, fail_shutdown: bool) -> ReplayHardwarePipeline {
        let settings = ReplaySettings::default();
        ReplayHardwarePipeline::new(
            settings,
            ReplayClipStore::open(self.0.join("clips"), ReplayBudget::from_settings(settings))
                .unwrap(),
            Box::new(Backend::new(fail_shutdown)),
        )
        .unwrap()
        .with_spool(ReplaySegmentSpool::open(self.0.join("spool"), settings).unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn request() -> GameSessionRequest {
    GameSessionRequest {
        game_id: None,
        game_name: "Fixture".into(),
        metrics: true.into(),
        overlay: true.into(),
        replay: true.into(),
    }
}

struct Backend {
    stream: ReplayVideoStream,
    fail_shutdown: bool,
}
impl Backend {
    fn new(fail_shutdown: bool) -> Self {
        Self {
            stream: ReplayVideoStream::new(
                ReplayVideoCodec::H264,
                ReplayPacketFormat::H264LengthPrefixed4,
                320,
                240,
                60,
                vec![
                    1, 66, 0, 30, 0xff, 0xe1, 0, 2, 0x67, 0x42, 1, 0, 2, 0x68, 0xce,
                ],
            )
            .unwrap(),
            fail_shutdown,
        }
    }
}
impl HardwareEncoderBackend for Backend {
    fn api(&self) -> HardwareEncoderApi {
        HardwareEncoderApi::VulkanVideo
    }
    fn stream(&self) -> &ReplayVideoStream {
        &self.stream
    }
    fn encode(
        &mut self,
        sequence: u64,
        frame: DmaBufReplayFrame,
        force: bool,
    ) -> Result<HardwareEncodeOutput, ReplayEncoderError> {
        HardwareEncodeOutput::new(
            EncodedPacketBatch::new(vec![
                crate::EncodedReplayPacket::new(
                    frame.timestamp_ns,
                    frame.duration_ns,
                    force,
                    vec![0, 0, 0, 2, 0x65, 0x88],
                )
                .unwrap(),
            ])?,
            vec![sequence],
        )
    }
    fn drain(&mut self) -> Result<HardwareEncodeOutput, ReplayEncoderError> {
        HardwareEncodeOutput::new(EncodedPacketBatch::default(), vec![])
    }
    fn shutdown(&mut self) -> Result<(), ReplayEncoderError> {
        if self.fail_shutdown {
            Err(ReplayEncoderError::BackendFailed(
                "fixture unsafe shutdown".into(),
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Default)]
struct Source {
    fail: AtomicBool,
    released: Mutex<Vec<u64>>,
    frames: Mutex<VecDeque<(u64, DmaBufReplayFrame)>>,
}
impl ReplayFrameSource for Source {
    fn wait_next(
        &self,
        timeout: Duration,
    ) -> Result<Option<(u64, DmaBufReplayFrame)>, ReplayRuntimeError> {
        if let Some(frame) = lock_unpoisoned(&self.frames).pop_front() {
            return Ok(Some(frame));
        }
        thread::sleep(timeout.min(Duration::from_millis(5)));
        Ok(None)
    }
    fn release(&self, sequence: u64) -> Result<(), ReplayRuntimeError> {
        if self.fail.load(Ordering::Acquire) {
            return Err(ReplayRuntimeError::new("fixture ACK failed"));
        }
        lock_unpoisoned(&self.released).push(sequence);
        Ok(())
    }
    fn drain_and_release(&self) -> Result<(), ReplayRuntimeError> {
        Ok(())
    }
    fn wake(&self) {}
}

#[test]
fn coordinator_retains_failed_gpu_cleanup_and_refuses_new_sessions() {
    let fixture = Fixture::new();
    let session = fixture.session();
    session
        .inner
        .replay
        .start_validated_pipeline(
            ReplaySettings::default(),
            fixture.pipeline(true),
            ReplayBackendReadiness::fully_verified(),
        )
        .unwrap();
    for _ in 0..2 {
        assert!(session.end().is_err());
        assert!(session.capture_snapshot().is_some());
        assert_eq!(session.status().phase, GameSessionPhase::Ending);
        assert!(session.status().failure.is_some());
        assert!(lock_unpoisoned(&session.inner.state).capture.is_some());
        assert!(
            session
                .begin(
                    request(),
                    None,
                    ReplayRuntimeStatus::unavailable(ReplaySettings::default())
                )
                .is_err()
        );
        assert!(session.claim_replay_start(Instant::now()).is_none());
    }
    drop(session);
}

#[test]
fn failed_acknowledgements_retain_transport_and_retry_without_duplicate_success() {
    let fixture = Fixture::new();
    let session = fixture.session();
    let source = Arc::new(Source::default());
    source.fail.store(true, Ordering::Release);
    lock_unpoisoned(&session.inner.state).replay_pump = Some(ReplayExportPump {
        stop: Arc::new(AtomicBool::new(false)),
        source: source.clone(),
        worker: Some(thread::spawn(|| ReplayPumpExit {
            shutdown: Ok(()),
            pending_releases: VecDeque::from([7]),
        })),
        shutdown_result: None,
        pending_releases: VecDeque::new(),
    });
    assert!(session.stop_capture().is_err());
    assert!(session.end().is_err());
    assert!(lock_unpoisoned(&session.inner.state).capture.is_some());
    assert!(lock_unpoisoned(&source.released).is_empty());
    source.fail.store(false, Ordering::Release);
    session.end().unwrap();
    session.end().unwrap();
    assert_eq!(*lock_unpoisoned(&source.released), vec![7]);
    assert_eq!(session.status().phase, GameSessionPhase::Ended);
    assert!(lock_unpoisoned(&session.inner.state).capture.is_none());
    drop(session);
}

#[test]
fn a_panicked_worker_never_becomes_successful_on_retry() {
    let fixture = Fixture::new();
    let session = fixture.session();
    lock_unpoisoned(&session.inner.state).replay_pump = Some(ReplayExportPump {
        stop: Arc::new(AtomicBool::new(false)),
        source: Arc::new(Source::default()),
        worker: Some(thread::spawn(|| panic!("fixture worker panic"))),
        shutdown_result: None,
        pending_releases: VecDeque::new(),
    });
    assert!(session.end().unwrap_err().to_string().contains("panicked"));
    assert!(session.end().unwrap_err().to_string().contains("panicked"));
    assert_eq!(session.status().phase, GameSessionPhase::Ending);
    drop(session);
}

#[test]
fn failed_initial_start_can_attach_a_worker_on_a_later_supervisor_attempt() {
    let fixture = Fixture::new();
    let session = fixture.session();
    let now = Instant::now();
    let generation = session.claim_replay_start(now).unwrap();
    assert!(session.claim_replay_start(now).is_none());
    session.finish_replay_start(generation, now, false);
    assert_eq!(session.status().replay, GameSessionComponentState::Failed);
    assert!(session.claim_replay_start(now).is_none());
    let generation = session
        .claim_replay_start(now + Duration::from_secs(1))
        .unwrap();
    let source = Arc::new(Source::default());
    let pump_stop = Arc::new(AtomicBool::new(false));
    lock_unpoisoned(&session.inner.state).replay_pump = Some(ReplayExportPump {
        stop: pump_stop,
        source,
        worker: Some(thread::spawn(|| ReplayPumpExit {
            shutdown: Ok(()),
            pending_releases: VecDeque::new(),
        })),
        shutdown_result: None,
        pending_releases: VecDeque::new(),
    });
    session.finish_replay_start(generation, now, true);
    assert_eq!(session.status().replay, GameSessionComponentState::Pending);
    assert!(
        session
            .claim_replay_start(now + Duration::from_secs(100))
            .is_none()
    );
    session.end().unwrap();
    assert!(
        session
            .claim_replay_start(now + Duration::from_secs(100))
            .is_none()
    );
    drop(session);
}

fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < deadline, "fixture recovery timed out");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn transient_encoder_creation_failure_recovers_from_fresh_frames_in_the_same_game() {
    let fixture = Fixture::new();
    let session = fixture.session();
    let source = Arc::new(Source::default());
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let create_attempts = attempts.clone();
    let root = fixture.0.clone();
    let coordinator = Arc::downgrade(&session.inner);
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker_source = source.clone();
    let runtime = session.inner.replay.clone();
    let worker = thread::spawn(move || {
        run_replay_export_pump(
            worker_stop,
            worker_source,
            runtime,
            coordinator,
            ReplaySettings::default(),
            ReplayBackendReadiness::fully_verified(),
            Arc::new(move |_| {
                if create_attempts.fetch_add(1, Ordering::Relaxed) == 0 {
                    return Err("fixture transient initialization failure".into());
                }
                let settings = ReplaySettings::default();
                let store = ReplayClipStore::open(
                    root.join("recovered-clips"),
                    ReplayBudget::from_settings(settings),
                )
                .unwrap();
                let spool =
                    ReplaySegmentSpool::open(root.join("recovered-spool"), settings).unwrap();
                Ok(
                    ReplayHardwarePipeline::new(settings, store, Box::new(Backend::new(false)))
                        .unwrap()
                        .with_spool(spool),
                )
            }),
            Arc::new(|_| Ok(Box::new(Backend::new(false)))),
            |_, _, _| None,
        )
    });
    lock_unpoisoned(&session.inner.state).replay_pump = Some(ReplayExportPump {
        stop,
        source: source.clone(),
        worker: Some(worker),
        shutdown_result: None,
        pending_releases: VecDeque::new(),
    });
    let send = |sequence| {
        let frame = DmaBufReplayFrame::new(
            fs::File::open("/dev/null").unwrap().into(),
            320,
            240,
            u32::from_le_bytes(*b"RA24"),
            0,
            sequence * 16_666_667,
            16_666_667,
            vec![crate::DmaBufPlane {
                offset: 0,
                stride: 1280,
            }],
        )
        .unwrap();
        lock_unpoisoned(&source.frames).push_back((sequence, frame));
    };
    send(1);
    wait_until(|| session.inner.replay.status().phase == ReplayPhase::Failed);
    send(2);
    wait_until(|| lock_unpoisoned(&source.released).contains(&2));
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        1,
        "backoff does not construct another encoder"
    );
    thread::sleep(REPLAY_REARM_BACKOFF);
    send(3);
    wait_until(|| session.inner.replay.status().encoded_packet_count > 0);
    assert_eq!(attempts.load(Ordering::Relaxed), 2);
    assert_eq!(session.inner.replay.status().phase, ReplayPhase::Buffering);
    assert_eq!(session.status().replay, GameSessionComponentState::Active);
    session.end().unwrap();
    assert_eq!(*lock_unpoisoned(&source.released), vec![1, 2, 3]);
    drop(session);
}

#[test]
fn an_earlier_sessions_start_cannot_attach_to_or_fail_a_new_session() {
    let fixture = Fixture::new();
    let session = fixture.session();
    let generation = session.claim_replay_start(Instant::now()).unwrap();
    session.end().unwrap();
    let capture = CaptureSessionHandle::start(
        &CaptureSessionConfig::new(fixture.0.join("runtime"), fixture.0.join("layer.so")),
        CaptureModuleGates::default(),
    )
    .unwrap();
    session
        .begin(
            request(),
            Some(capture),
            ReplayRuntimeStatus::validation_candidate(ReplaySettings::default()),
        )
        .unwrap();
    assert!(
        session
            .start_replay_export_pump_for_generation(
                ReplaySettings::default(),
                ReplayBackendReadiness::fully_verified(),
                |_| panic!("stale pipeline"),
                |_| panic!("stale replacement"),
                Some(generation)
            )
            .is_err()
    );
    assert!(!session.replay_start_matches(Some(generation)));
    session.finish_replay_start(generation, Instant::now(), false);
    assert_eq!(session.status().replay, GameSessionComponentState::Pending);
    assert!(session.claim_replay_start(Instant::now()).is_some());
    session.end().unwrap();
    drop(session);
}
