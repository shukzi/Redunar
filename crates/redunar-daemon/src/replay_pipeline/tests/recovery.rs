use super::*;
use crate::replay_packet_source::ReplayPacketSource;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

fn empty_audio(_: u64, _: u64) -> ReplayAudioTracks {
    ReplayAudioTracks {
        system: ReplayAudioSnapshot {
            stream: redunar_capture_audio::OpusStreamDescription::default(),
            packets: Vec::new(),
        },
        microphone: None,
    }
}

#[test]
fn ram_save_preserves_reserve_and_recovers_without_stopping_capture() {
    for format in [ReplayOutputFormat::Matroska, ReplayOutputFormat::Mp4] {
        let fixture = Fixture::new();
        let settings = ReplaySettings::default();
        let save_duration = ReplayDuration::Seconds900;
        let save_settings = ReplaySettings {
            duration: save_duration,
            ..settings
        };
        let required =
            ReplayBudget::from_settings(save_settings).maximum_ring_bytes + 512 * 1024 * 1024;
        let store = fixture.store().with_available_bytes(required - 1);
        let mut pipeline = ReplayHardwarePipeline::new(
            settings,
            store,
            Box::new(FakeBackend::new(1920, 1080, Arc::new(AtomicUsize::new(0)))),
        )
        .unwrap();
        pipeline
            .submit_frame(1, frame_at(&fixture.root, 1, 1920, 1080, 0))
            .unwrap();
        pipeline
            .submit_frame(2, frame_at(&fixture.root, 2, 1920, 1080, 1_000_000_000))
            .unwrap();
        let retained = pipeline.ring_stats();
        let error = pipeline
            .save_recent_replay_as_with_audio_tracks(save_duration, format, empty_audio)
            .unwrap()
            .join()
            .unwrap_err();
        assert!(
            error.to_string().contains("512 MiB safety reserve"),
            "{error}"
        );
        assert!(pipeline.store.inventory().unwrap().is_empty());
        assert!(fs::read_dir(&fixture.root).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
        assert_eq!(pipeline.phase(), ReplayPipelinePhase::Buffering);
        assert_eq!(pipeline.ring_stats(), retained);
        pipeline.store = pipeline.store.clone().with_available_bytes(required);
        pipeline
            .submit_frame(3, frame_at(&fixture.root, 3, 1920, 1080, 2_000_000_000))
            .unwrap();
        let clip = pipeline
            .save_recent_replay_as_with_audio_tracks(save_duration, format, empty_audio)
            .unwrap()
            .join()
            .unwrap();
        assert!(clip.path.is_file());
        assert_eq!(pipeline.phase(), ReplayPipelinePhase::Buffering);
        pipeline.shutdown().unwrap();
    }
}

fn spooled_pipeline(fixture: &Fixture, payload_bytes: usize) -> ReplayHardwarePipeline {
    let settings = ReplaySettings::default();
    let store = fixture.store();
    let spool = ReplaySegmentSpool::open(fixture.root.join("spool"), settings).unwrap();
    let mut backend = FakeBackend::new(1920, 1080, Arc::new(AtomicUsize::new(0)));
    backend.payload_bytes = payload_bytes;
    ReplayHardwarePipeline::new(settings, store, Box::new(backend))
        .unwrap()
        .with_spool(spool)
}

fn submit(pipeline: &mut ReplayHardwarePipeline, _fixture: &Fixture, index: u64) {
    let frame = DmaBufReplayFrame::new(
        File::open("/dev/null").unwrap().into(),
        1920,
        1080,
        0x3432_4152,
        0,
        index * 16_666_667,
        16_666_667,
        vec![DmaBufPlane {
            offset: 0,
            stride: 1920 * 4,
        }],
    )
    .unwrap();
    pipeline.submit_frame(index + 1, frame).unwrap();
    pipeline.take_completed_inputs();
    assert_eq!(pipeline.phase(), ReplayPipelinePhase::Buffering);
    assert!(pipeline.ring_stats().stored_bytes <= crate::replay_ring::MAX_IN_MEMORY_REPLAY_BYTES);
}

#[test]
#[ignore = "paced fifteen-minute recording and storage-pressure endurance run"]
fn paced_fifteen_minute_pipeline_survives_repeated_storage_stalls() {
    let fixture = Fixture::new();
    let mut pipeline = spooled_pipeline(&fixture, 30 * 1024);
    let started = Instant::now();
    let mut pause = None;
    let mut peak_rss_kib = 0;
    for index in 0..54_000 {
        std::thread::sleep(
            (started + Duration::from_nanos(index * 16_666_667))
                .saturating_duration_since(Instant::now()),
        );
        if index % 9_000 == 180 {
            pause = Some(pipeline.spool.as_ref().unwrap().pause_worker());
        } else if index % 9_000 == 360 {
            drop(pause.take());
        }
        submit(&mut pipeline, &fixture, index);
        assert!(
            pipeline.disk_history_available(),
            "history degraded at frame {index}"
        );
        if index % 3_600 == 0 {
            let status = fs::read_to_string("/proc/self/status").unwrap();
            let rss = status
                .lines()
                .find_map(|line| line.strip_prefix("VmRSS:"))
                .and_then(|line| line.split_whitespace().next())
                .unwrap()
                .parse::<u64>()
                .unwrap();
            peak_rss_kib = peak_rss_kib.max(rss);
            println!(
                "elapsed_seconds={} accepted_packets={} rss_kib={rss}",
                started.elapsed().as_secs(),
                pipeline.spool_stats().unwrap().accepted_packets
            );
        }
    }
    drop(pause);
    std::thread::sleep(
        (started + Duration::from_mins(15)).saturating_duration_since(Instant::now()),
    );
    assert!(
        pipeline.spool_pending_from_ns.is_none(),
        "final backlog not drained"
    );
    pipeline
        .save_spooled_replay(ReplayDuration::Seconds900)
        .unwrap()
        .join()
        .unwrap();
    let spool = pipeline.spool.as_ref().unwrap();
    let stats = spool.stats();
    assert_eq!(stats.accepted_packets, 54_000);
    assert_eq!(stats.segment_resyncs, 0);
    assert!(stats.dropped_queue_full > 0);
    assert!(stats.buffered_duration_ns <= 900_000_000_000);
    let segments = spool
        .snapshot_plan(ReplayDuration::Seconds900)
        .unwrap()
        .segments
        .into_iter()
        .map(|segment| {
            let file = File::open(&segment.path).unwrap();
            (segment, file)
        })
        .collect();
    let mut packets = crate::replay_spool_reader::SpoolPackets::scan(
        segments,
        ReplaySettings {
            duration: ReplayDuration::Seconds900,
            ..ReplaySettings::default()
        },
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    let (first, end) = packets.window();
    assert!(end - first >= 897_000_000_000);
    let mut index = first / 16_666_667;
    while let Some(packet) = packets.next_packet().unwrap() {
        assert_eq!(packet.timestamp_ns(), index * 16_666_667);
        index += 1;
    }
    assert_eq!(index, 54_000);
    println!(
        "endurance_elapsed_seconds={} queue_pressure_events={} peak_sampled_rss_kib={peak_rss_kib}",
        started.elapsed().as_secs(),
        stats.dropped_queue_full
    );
    pipeline.shutdown().unwrap();
}

#[test]
fn production_packet_path_repairs_command_and_byte_pressure_without_gaps() {
    for (payload_bytes, initial_frames) in [(6, 180), (512 * 1024, 65)] {
        let fixture = Fixture::new();
        let mut pipeline = spooled_pipeline(&fixture, payload_bytes);
        let pause = pipeline.spool.as_ref().unwrap().pause_worker();
        for index in 0..initial_frames {
            submit(&mut pipeline, &fixture, index);
        }
        assert!(pipeline.spool_pending_from_ns.is_some());
        let stats = pipeline.spool_stats().unwrap();
        assert!(stats.dropped_queue_full > 0);
        assert!(stats.queued_packets <= 64);
        assert_eq!(stats.segment_resyncs, 0);
        drop(pause);
        let deadline = Instant::now() + Duration::from_secs(10);
        while pipeline.spool_pending_from_ns.is_some() {
            assert!(Instant::now() < deadline, "backlog did not recover");
            pipeline
                .flush_spool_backlog()
                .expect("ring backlog should remain recoverable");
            std::thread::sleep(Duration::from_millis(1));
        }
        pipeline
            .save_spooled_replay(ReplayDuration::Seconds15)
            .unwrap()
            .join()
            .unwrap();
        let spool = pipeline.spool.as_ref().unwrap();
        assert!(pipeline.disk_history_available());
        assert_eq!(spool.stats().accepted_packets, initial_frames);
        assert_eq!(spool.stats().segment_resyncs, 0);
        let plan = spool.snapshot_plan(ReplayDuration::Seconds15).unwrap();
        let segments = plan
            .segments
            .into_iter()
            .map(|segment| {
                let file = File::open(&segment.path).unwrap();
                (segment, file)
            })
            .collect();
        let mut packets = crate::replay_spool_reader::SpoolPackets::scan(
            segments,
            ReplaySettings::default(),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        for index in 0..initial_frames {
            let packet = packets.next_packet().unwrap().expect("missing packet");
            assert_eq!(packet.timestamp_ns(), index * 16_666_667);
            assert_eq!(packet.bytes().len(), payload_bytes);
        }
        assert!(packets.next_packet().unwrap().is_none());
        pipeline.shutdown().unwrap();
    }
}
