//! Capability discovery fixtures never inspect the host GPU or launch a game.
use super::*;
use redunar_core::{ReplayDuration, ReplaySettings};
use std::sync::{Barrier, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(5);

pub(super) fn snapshot(revision: u64) -> CapabilitySnapshot {
    let reason = format!("fixture capability {revision}");
    CapabilitySnapshot {
        capture_runtime: CaptureRuntimeStatus::Unavailable(reason.clone()),
        encoder_probe: None,
        module_statuses: ModuleStatuses {
            frame_metrics: ModuleStatus::UnavailableOnSystem {
                reason: reason.clone(),
            },
            in_game_overlay: ModuleStatus::UnavailableOnSystem {
                reason: reason.clone(),
            },
            instant_replay: ModuleStatus::PlannedUnavailable { reason },
        },
    }
}

pub(super) fn fixture_service(
    state: PathBuf,
    discover: impl Fn() -> CapabilitySnapshot + Send + Sync + 'static,
) -> RedunarService {
    RedunarService {
        runtime: Arc::new(ServiceRuntime {
            capability_discovery: Some(Arc::new(discover)),
            ..ServiceRuntime::default()
        }),
        ..RedunarService::with_state_directory(state)
    }
}

pub(super) fn counting_service(state: PathBuf) -> RedunarService {
    let discoveries = AtomicU64::new(0);
    fixture_service(state, move || {
        snapshot(discoveries.fetch_add(1, Ordering::Relaxed) + 1)
    })
}

#[test]
fn capability_discovery_is_cached_and_explicitly_refreshable() {
    let service = counting_service(PathBuf::new());
    assert_eq!(service.capability_discovery_revision(), 0);
    let first = service.module_statuses();
    for _ in 0..10 {
        assert_eq!(service.module_statuses(), first);
        assert_eq!(
            service.capture_runtime_status(),
            snapshot(1).capture_runtime
        );
        assert!(!service.replay_backend_readiness().validation_allowed());
    }
    assert_eq!(service.capability_discovery_revision(), 1);

    service.invalidate_capability_snapshot();
    assert_eq!(service.capability_discovery_revision(), 1);
    assert_eq!(service.module_statuses(), snapshot(2).module_statuses);
    service.refresh_capability_snapshot();
    assert_eq!(service.module_statuses(), snapshot(3).module_statuses);
    assert_eq!(service.capability_discovery_revision(), 3);
    // Cloned command handlers share the same discovery and invalidation owner.
    let clone = service.clone();
    clone.invalidate_capability_snapshot();
    assert_eq!(service.module_statuses(), snapshot(4).module_statuses);
    assert_eq!(clone.capability_discovery_revision(), 4);
}

#[test]
fn capability_refresh_and_invalidation_observe_loss_and_recovery() {
    let available = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let discovery_available = Arc::clone(&available);
    let service = fixture_service(PathBuf::new(), move || {
        let mut evidence = snapshot(1);
        if discovery_available.load(Ordering::Relaxed) {
            evidence.capture_runtime = CaptureRuntimeStatus::Available;
            evidence.module_statuses = ModuleStatuses {
                frame_metrics: ModuleStatus::Enabled,
                in_game_overlay: ModuleStatus::Enabled,
                instant_replay: ModuleStatus::Enabled,
            };
        }
        evidence
    });
    assert!(!service.replay_backend_readiness().validation_allowed());
    available.store(true, Ordering::Relaxed);
    assert!(!service.replay_backend_readiness().validation_allowed());
    service.refresh_capability_snapshot();
    assert_eq!(
        service.capture_runtime_status(),
        CaptureRuntimeStatus::Available
    );
    assert!(service.replay_backend_readiness().validation_allowed());

    available.store(false, Ordering::Relaxed);
    service.invalidate_capability_snapshot();
    assert!(
        !service
            .module_status(AppModule::InstantReplay)
            .allows_runtime()
    );
    assert!(!service.replay_backend_readiness().validation_allowed());
    assert_eq!(service.capability_discovery_revision(), 3);
}

#[test]
fn concurrent_cold_readers_share_one_discovery() {
    let service = counting_service(PathBuf::new());
    let start = Barrier::new(8);
    thread::scope(|scope| {
        let readers: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    start.wait();
                    service.module_statuses()
                })
            })
            .collect();
        for reader in readers {
            assert_eq!(reader.join().unwrap(), snapshot(1).module_statuses);
        }
    });
    assert_eq!(service.capability_discovery_revision(), 1);
}

#[test]
fn invalidation_waits_for_in_flight_discovery_and_discards_its_result() {
    for refresh in [false, true] {
        let (entered, started) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let resume = Mutex::new(resume);
        let discoveries = AtomicU64::new(0);
        let service = fixture_service(PathBuf::new(), move || {
            let revision = discoveries.fetch_add(1, Ordering::Relaxed) + 1;
            if revision == 1 {
                entered.send(()).unwrap();
                resume.lock().unwrap().recv_timeout(WAIT).unwrap();
            }
            snapshot(revision)
        });
        thread::scope(|scope| {
            let reader = scope.spawn(|| {
                if refresh {
                    service.refresh_capability_snapshot();
                } else {
                    assert_eq!(service.module_statuses(), snapshot(1).module_statuses);
                }
            });
            started.recv_timeout(WAIT).unwrap();
            // Both lazy and explicit discovery hold the cache lock until
            // publication; an invalidation cannot be overtaken by old work.
            assert!(service.runtime.capabilities.try_lock().is_err());
            let invalidator = scope.spawn(|| service.invalidate_capability_snapshot());
            release.send(()).unwrap();
            reader.join().unwrap();
            invalidator.join().unwrap();
        });
        assert!(service.runtime.capabilities.lock().unwrap().is_none());
        assert_eq!(service.module_statuses(), snapshot(2).module_statuses);
        assert_eq!(service.capability_discovery_revision(), 2);
    }
}

#[test]
fn concurrent_reads_refreshes_and_invalidations_remain_usable() {
    let service = counting_service(PathBuf::new());
    let start = Barrier::new(4);
    thread::scope(|scope| {
        for worker in 0..4 {
            let service = &service;
            let start = &start;
            scope.spawn(move || {
                start.wait();
                for _ in 0..64 {
                    match worker {
                        0 => service.invalidate_capability_snapshot(),
                        1 => service.refresh_capability_snapshot(),
                        _ => {
                            let _ = service.module_statuses();
                            let _ = service.capture_runtime_status();
                        }
                    }
                }
            });
        }
    });
    service.invalidate_capability_snapshot();
    let revision = service.capability_discovery_revision();
    assert_eq!(
        service.module_statuses(),
        snapshot(revision + 1).module_statuses
    );
    assert_eq!(service.capability_discovery_revision(), revision + 1);
}

struct FakeEncoder(ReplayVideoStream);

impl HardwareEncoderBackend for FakeEncoder {
    fn api(&self) -> HardwareEncoderApi {
        HardwareEncoderApi::VaApi
    }

    fn stream(&self) -> &ReplayVideoStream {
        &self.0
    }

    fn encode(
        &mut self,
        sequence: u64,
        frame: DmaBufReplayFrame,
        keyframe: bool,
    ) -> Result<HardwareEncodeOutput, ReplayEncoderError> {
        let packet = EncodedReplayPacket::new(
            frame.timestamp_ns,
            frame.duration_ns,
            keyframe,
            vec![0, 0, 0, 2, if keyframe { 0x65 } else { 0x41 }, 0x88],
        )
        .unwrap();
        HardwareEncodeOutput::new(EncodedPacketBatch::new(vec![packet])?, vec![sequence])
    }

    fn drain(&mut self) -> Result<HardwareEncodeOutput, ReplayEncoderError> {
        HardwareEncodeOutput::new(EncodedPacketBatch::default(), Vec::new())
    }

    fn shutdown(&mut self) -> Result<(), ReplayEncoderError> {
        Ok(())
    }
}

#[test]
fn live_session_and_replay_readings_advance_through_capability_changes() {
    // The existing fixture owns only a unique private temporary directory.
    let fixture = crate::tests::Fixture::new();
    let service = counting_service(fixture.root.join("state"));
    let coordinator = service.game_session_coordinator();
    let settings = ReplaySettings::default();
    coordinator
        .begin(
            GameSessionRequest {
                game_id: None,
                game_name: "Fixture game".into(),
                metrics: GameSessionFeatureRequest::Disabled,
                overlay: GameSessionFeatureRequest::Disabled,
                replay: GameSessionFeatureRequest::Enabled,
            },
            None,
            ReplayRuntimeStatus::unavailable(settings),
        )
        .unwrap();
    let launching = coordinator.status();
    service.refresh_capability_snapshot();
    assert_eq!(service.game_session_coordinator().status(), launching);
    coordinator.mark_running();
    let running = service.game_session_coordinator().status();
    assert_eq!(running.phase, GameSessionPhase::Running);
    assert!(running.revision > launching.revision);

    let runtime = coordinator.replay_runtime();
    start_fake_replay(&runtime, &fixture.root, settings);
    for sequence in 1..=4 {
        let before = service.replay_runtime_status().unwrap();
        match sequence {
            1 | 4 => {
                let _ = service.module_statuses();
            }
            2 => service.refresh_capability_snapshot(),
            _ => service.invalidate_capability_snapshot(),
        }
        assert_eq!(service.replay_runtime_status().unwrap(), before);
        let frame = DmaBufReplayFrame::new_multi_object(
            vec![std::fs::File::open("/dev/null").unwrap().into()],
            320,
            240,
            u32::from_le_bytes(*b"XR24"),
            0,
            (sequence - 1) * 20_000_000,
            20_000_000,
            vec![DmaBufImagePlane {
                object_index: 0,
                offset: 0,
                stride: 1_280,
            }],
        )
        .unwrap();
        runtime.submit_frame(sequence, frame).unwrap();
        runtime.take_completed_exports();
        runtime
            .submit_audio(redunar_capture_audio::EncodedOpusPacket {
                timestamp_ns: (sequence - 1) * 20_000_000,
                duration_ns: 20_000_000,
                bytes: vec![1, 2, 3],
            })
            .unwrap();
        let deadline = Instant::now() + WAIT;
        let after = loop {
            let status = service.replay_runtime_status().unwrap();
            if status.encoded_packet_count == sequence {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "fake packets did not reach the spool"
            );
            thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(after.phase, ReplayPhase::Buffering);
        assert_eq!(after.received_frame_count, sequence);
        assert_eq!(after.audio_packet_count, sequence);
        assert_eq!(after.audio_byte_count, sequence * 3);
        assert!(after.buffered_duration_ns > before.buffered_duration_ns);
        assert_eq!(service.game_session_coordinator().status(), running);
    }
    coordinator.end().unwrap();
    service.invalidate_capability_snapshot();
    let _ = service.module_statuses();
    let ended = service.game_session_coordinator().status();
    assert_eq!(ended.phase, GameSessionPhase::Ended);
    assert!(ended.revision > running.revision);
}

#[test]
fn runtime_save_uses_ram_history_when_disk_spool_fails() {
    let fixture = crate::tests::Fixture::new();
    let settings = ReplaySettings::default();
    let runtime = ProductionReplayRuntime::unavailable(settings);
    start_fake_replay(&runtime, &fixture.root, settings);

    for sequence in 1..=60_u64 {
        let frame = DmaBufReplayFrame::new_multi_object(
            vec![std::fs::File::open("/dev/null").unwrap().into()],
            320,
            240,
            u32::from_le_bytes(*b"XR24"),
            0,
            (sequence - 1) * 20_000_000,
            20_000_000,
            vec![DmaBufImagePlane {
                object_index: 0,
                offset: 0,
                stride: 1_280,
            }],
        )
        .unwrap();
        runtime.submit_frame(sequence, frame).unwrap();
        runtime.take_completed_exports();
    }
    let deadline = Instant::now() + WAIT;
    while runtime.status().encoded_packet_count < 60 {
        assert!(
            Instant::now() < deadline,
            "fake packets did not reach the spool"
        );
        thread::sleep(Duration::from_millis(1));
    }

    let spool_directory = fixture.root.join("spool");
    std::fs::remove_dir_all(&spool_directory).unwrap();
    for (sequence, timestamp_ns) in [(101, 2_000_000_000), (102, 2_020_000_000)] {
        let frame = DmaBufReplayFrame::new_multi_object(
            vec![std::fs::File::open("/dev/null").unwrap().into()],
            320,
            240,
            u32::from_le_bytes(*b"XR24"),
            0,
            timestamp_ns,
            20_000_000,
            vec![DmaBufImagePlane {
                object_index: 0,
                offset: 0,
                stride: 1_280,
            }],
        )
        .unwrap();
        runtime.submit_frame(sequence, frame).unwrap();
    }
    let deadline = Instant::now() + WAIT;
    loop {
        let stats = runtime
            .diagnostic_counters()
            .1
            .expect("spool stats after failure");
        if stats.segment_resyncs > 0 {
            break;
        }
        assert!(Instant::now() < deadline, "spool failure was not observed");
        thread::sleep(Duration::from_millis(1));
    }

    runtime
        .save(ReplayDuration::Seconds15)
        .expect("production save should use recent RAM history");
    let deadline = Instant::now() + WAIT;
    loop {
        let status = runtime.status();
        if status.phase == ReplayPhase::Buffering && status.completed_save_revision == 1 {
            break;
        }
        assert!(Instant::now() < deadline, "RAM-backed save did not finish");
        thread::sleep(Duration::from_millis(1));
    }
    runtime.shutdown().unwrap();
}

fn start_fake_replay(runtime: &ProductionReplayRuntime, root: &Path, settings: ReplaySettings) {
    let stream = ReplayVideoStream::new(
        ReplayVideoCodec::H264,
        ReplayPacketFormat::H264LengthPrefixed4,
        320,
        240,
        u8::try_from(settings.frame_rate.frames_per_second()).unwrap(),
        vec![
            1, 66, 0, 30, 0xff, 0xe1, 0, 2, 0x67, 0x42, 1, 0, 2, 0x68, 0xce,
        ],
    )
    .unwrap();
    let store =
        ReplayClipStore::open(root.join("clips"), ReplayBudget::from_settings(settings)).unwrap();
    let pipeline = ReplayHardwarePipeline::new(settings, store, Box::new(FakeEncoder(stream)))
        .unwrap()
        .with_spool(ReplaySegmentSpool::open(root.join("spool"), settings).unwrap());
    // Synthetic attestation is confined to this fake backend; no device opens.
    runtime
        .start_validated_pipeline(settings, pipeline, ReplayBackendReadiness::fully_verified())
        .unwrap();
}
