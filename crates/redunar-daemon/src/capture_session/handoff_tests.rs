use super::*;
use redunar_capture::{CaptureApi, CaptureMessage, ReplaySourceCandidate};
use std::os::fd::AsRawFd;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    socket: Arc<UnixDatagram>,
    helper: UnixDatagram,
    game: UnixDatagram,
    shared: Arc<CaptureShared>,
    model: CaptureSessionModel,
    selector: ReplayProducerSelector,
    endpoint: ReplayExportEndpoint,
    receiver: Option<JoinHandle<()>>,
}

impl Fixture {
    fn start_receiver(&mut self) {
        self.game.connect(self.root.join("daemon.sock")).unwrap();
        let shared = Arc::clone(&self.shared);
        let socket = Arc::clone(&self.socket);
        let session_id = shared.replay_release.session_id;
        self.receiver = Some(thread::spawn(move || {
            receiver_loop(&socket, &shared, session_id);
        }));
    }

    fn wire(&self, message: &CaptureMessage, export_fd: bool) {
        let mut bytes = [0; MAX_MESSAGE_BYTES];
        let length = redunar_capture::encode_message(message, &mut bytes).unwrap();
        if export_fd {
            let file = File::open("/dev/null").unwrap();
            redunar_capture_vulkan::fd_transport::send_datagram_fd(
                &self.game,
                &bytes[..length],
                file.as_raw_fd(),
            )
            .unwrap();
        } else {
            self.game.send(&bytes[..length]).unwrap();
        }
    }

    fn wait_exports(&self, count: u64) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while lock_unpoisoned(&self.shared.latest).replay_exported_frame_count < count {
            assert!(Instant::now() < deadline, "receiver did not resume exports");
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn new() -> Self {
        let root = env::temp_dir().join(format!(
            "rd-handoff-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let session_id = CaptureSessionId::new([12; 16]).unwrap();
        let socket = Arc::new(UnixDatagram::bind(root.join("daemon.sock")).unwrap());
        let helper = UnixDatagram::bind(root.join("helper.sock")).unwrap();
        let game = UnixDatagram::bind(root.join("game.sock")).unwrap();
        helper.set_nonblocking(true).unwrap();
        game.set_nonblocking(true).unwrap();
        let mut model = CaptureSessionModel::new(session_id);
        model
            .accept(CaptureMessage::Hello {
                session_id,
                process_id: 101,
                api: CaptureApi::Vulkan,
                producer_started_monotonic_ns: 1,
            })
            .unwrap();
        let shared = Arc::new(CaptureShared {
            latest: Mutex::new(Arc::new(model.snapshot())),
            stop: AtomicBool::new(false),
            replay_exports: Mutex::new(ReplayExportQueue::default()),
            replay_export_ready: Condvar::new(),
            replay_release: ReplayReleaseTransport::new(
                session_id,
                root.join("reply.sock"),
                Some(Arc::clone(&socket)),
            ),
        });
        let endpoint = ReplayExportEndpoint {
            shared: Arc::clone(&shared),
            last_imported_replay_export: Arc::new(AtomicU64::new(u64::MAX)),
        };
        Self {
            root,
            socket,
            helper,
            game,
            shared,
            model,
            selector: ReplayProducerSelector::default(),
            endpoint,
            receiver: None,
        }
    }

    fn source(width: u32) -> ReplaySourceCandidate {
        ReplaySourceCandidate {
            gpu_identity: None,
            width,
            height: 240,
            pixel_format: ReplayPixelFormat::Bgra8Unorm,
            target_frames_per_second: 60,
        }
    }

    fn announce(&mut self, producer: &str, source: ReplaySourceCandidate) {
        accept_capture_message(
            &mut self.model,
            &mut self.selector,
            &self.shared,
            CaptureMessage::ReplaySourceCandidate {
                session_id: self.shared.replay_release.session_id,
                candidate: source,
            },
            None,
            Some(&self.root.join(producer)),
        );
    }

    fn export(
        &mut self,
        producer: &str,
        sequence: u64,
        timestamp_ns: u64,
        source: ReplaySourceCandidate,
    ) {
        accept_capture_message(
            &mut self.model,
            &mut self.selector,
            &self.shared,
            CaptureMessage::ReplayFrameExported {
                session_id: self.shared.replay_release.session_id,
                sequence,
                fd_number: 10,
                source,
                offset: 0,
                stride: source.width * 4,
                modifier: 0,
                timestamp_ns,
                duration_ns: 16_666_667,
            },
            Some(File::open("/dev/null").unwrap().into()),
            Some(&self.root.join(producer)),
        );
        publish(&self.shared, self.model.snapshot());
    }

    fn ack(socket: &UnixDatagram) -> u64 {
        let mut bytes = [0; MAX_MESSAGE_BYTES];
        let length = socket.recv(&mut bytes).unwrap();
        let CaptureMessage::ReplayFrameReleased { sequence, .. } =
            decode_message(&bytes[..length]).unwrap()
        else {
            panic!("expected release")
        };
        sequence
    }

    fn no_ack(socket: &UnixDatagram) {
        assert_eq!(
            socket.recv(&mut [0; MAX_MESSAGE_BYTES]).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        let _ = redunar_capture_vulkan::fd_transport::send_datagram_to_nonblocking(
            &self.game,
            &[],
            &self.root.join("daemon.sock"),
        );
        if let Some(receiver) = self.receiver.take() {
            receiver.join().unwrap();
        }
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn overlapping_process_sequences_keep_delayed_gpu_completion_with_its_owner() {
    let mut f = Fixture::new();
    let helper_source = Fixture::source(640);
    let game_source = Fixture::source(320);
    f.announce("helper.sock", helper_source);
    for sequence in 498..=500 {
        f.export("helper.sock", sequence, sequence, helper_source);
    }
    assert_eq!(Fixture::ack(&f.helper), 498);
    assert_eq!(Fixture::ack(&f.helper), 499);
    let helper_export = f.endpoint.take_next().unwrap();
    let helper_token = helper_export.sequence;
    f.endpoint.import(helper_export).unwrap();
    // A duplicate cannot release an input which the encoder still owns.
    f.export("helper.sock", 500, 501, helper_source);
    Fixture::no_ack(&f.helper);
    assert_eq!(f.model.snapshot().rejected_message_count, 1);

    f.announce("game.sock", game_source);
    for sequence in 498..=500 {
        f.export(
            "game.sock",
            sequence,
            REPLAY_PRODUCER_STALE_NS + 1000 + sequence,
            game_source,
        );
    }
    assert_eq!(Fixture::ack(&f.game), 498);
    assert_eq!(Fixture::ack(&f.game), 499);
    let game_export = f.endpoint.take_next().unwrap();
    let game_token = game_export.sequence;
    assert!(game_token > helper_token);
    f.endpoint.import(game_export).unwrap();
    assert_eq!(
        f.model.snapshot().replay_source_candidate,
        Some(game_source)
    );
    assert_eq!(lock_unpoisoned(&f.shared.replay_release.targets).len(), 2);
    Fixture::no_ack(&f.helper);
    Fixture::no_ack(&f.game);
    // Completion order may differ from submission order.
    f.endpoint.release(game_token).unwrap();
    assert_eq!(Fixture::ack(&f.game), 500);
    Fixture::no_ack(&f.helper);
    f.endpoint.release(helper_token).unwrap();
    assert_eq!(Fixture::ack(&f.helper), 500);
    assert!(lock_unpoisoned(&f.shared.replay_release.targets).is_empty());
}

#[test]
fn lower_game_sequences_recover_and_duplicate_confirmations_do_not_select_a_process() {
    let mut f = Fixture::new();
    let source = Fixture::source(320);
    f.announce("helper.sock", source);
    for sequence in 998..=1000 {
        f.export("helper.sock", sequence, sequence, source);
    }
    let _ = Fixture::ack(&f.helper);
    let _ = Fixture::ack(&f.helper);
    let old = f.endpoint.take_next().unwrap().sequence;
    f.announce("game.sock", source);
    let time = REPLAY_PRODUCER_STALE_NS + 1001;
    for _ in 0..4 {
        f.export("game.sock", 1, time, source);
    }
    assert!(f.endpoint.take_next().is_none());
    for _ in 0..4 {
        assert_eq!(Fixture::ack(&f.game), 1);
    }
    f.export("game.sock", 2, time + 1, source);
    assert_eq!(Fixture::ack(&f.game), 2);
    f.export("game.sock", 3, time + 2, source);
    let token = f.endpoint.take_next().unwrap().sequence;
    assert!(token > old);
    f.export("game.sock", 2, time + 3, source);
    assert_eq!(Fixture::ack(&f.game), 2);
    assert!(f.endpoint.take_next().is_none());
    f.endpoint.release(token).unwrap();
    assert_eq!(Fixture::ack(&f.game), 3);
    f.endpoint.release(old).unwrap();
    assert_eq!(Fixture::ack(&f.helper), 1000);
}

#[test]
fn producer_exit_and_failed_release_preserve_other_process_ownership() {
    let f = Fixture::new();
    let transport = &f.shared.replay_release;
    let helper = transport
        .register(7, Some(&f.root.join("helper.sock")))
        .unwrap();
    let game = transport
        .register(7, Some(&f.root.join("game.sock")))
        .unwrap();
    fs::remove_file(f.root.join("helper.sock")).unwrap();
    transport.release(helper).unwrap();
    assert_eq!(lock_unpoisoned(&transport.targets).len(), 1);
    Fixture::no_ack(&f.game);
    transport.release(game).unwrap();
    assert_eq!(Fixture::ack(&f.game), 7);

    let mut failed =
        ReplayReleaseTransport::new(transport.session_id, f.root.join("reply.sock"), None);
    let token = failed.register(9, Some(&f.root.join("game.sock"))).unwrap();
    assert!(failed.release(token).is_err());
    assert_eq!(lock_unpoisoned(&failed.targets).len(), 1);
    failed.set_socket_for_test(Arc::new(UnixDatagram::unbound().unwrap()));
    failed.release(token).unwrap();
    assert_eq!(Fixture::ack(&f.game), 9);
    assert!(lock_unpoisoned(&failed.targets).is_empty());
}

#[test]
fn release_route_capacity_refuses_growth_without_replacing_pending_owners() {
    let f = Fixture::new();
    let transport = &f.shared.replay_release;
    for sequence in 1..=REPLAY_RELEASE_TARGET_CAPACITY as u64 {
        transport
            .register(sequence, Some(&f.root.join("game.sock")))
            .unwrap();
    }
    assert!(
        transport
            .register(999, Some(&f.root.join("helper.sock")))
            .is_err()
    );
    assert!(
        transport
            .register(1, Some(&f.root.join("game.sock")))
            .is_err()
    );
    assert!(
        transport
            .register(999, Some(Path::new("/outside.sock")))
            .is_err()
    );
    transport.release(1).unwrap();
    assert_eq!(Fixture::ack(&f.game), 1);
    let token = transport
        .register(999, Some(&f.root.join("helper.sock")))
        .unwrap();
    assert!(token > REPLAY_RELEASE_TARGET_CAPACITY as u64);
    transport.release(token).unwrap();
    assert_eq!(Fixture::ack(&f.helper), 999);
    assert_eq!(
        lock_unpoisoned(&transport.targets).len(),
        REPLAY_RELEASE_TARGET_CAPACITY - 1
    );
}

#[test]
fn rejected_import_ack_retry_does_not_release_an_equal_game_sequence() {
    let f = Fixture::new();
    let transport = &f.shared.replay_release;
    let helper_path = f.root.join("helper.sock");
    let helper = transport.register(9, Some(&helper_path)).unwrap();
    let game = transport
        .register(9, Some(&f.root.join("game.sock")))
        .unwrap();
    // Saturate the recipient's datagram queue without altering the shared
    // receiver socket's blocking mode. The ACK must return and retain ownership.
    let mut filled = 0;
    loop {
        match redunar_capture_vulkan::fd_transport::send_datagram_to_nonblocking(
            &f.socket,
            b"full",
            &helper_path,
        ) {
            Ok(_) => {
                filled += 1;
                assert!(filled < 1024);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("could not fill fixture queue: {error}"),
        }
    }
    let mut invalid = Fixture::source(320);
    invalid.width = 0;
    f.endpoint.enqueue_for_test(ReplayExportMetadata {
        sequence: helper,
        fd: File::open("/dev/null").unwrap().into(),
        source: invalid,
        offset: 0,
        stride: 1280,
        modifier: 0,
        timestamp_ns: 1,
        duration_ns: 16_666_667,
    });
    assert!(
        crate::game_session::ReplayFrameSource::wait_next(&f.endpoint, Duration::ZERO).is_err()
    );
    assert_eq!(lock_unpoisoned(&transport.targets).len(), 2);
    assert_eq!(f.endpoint.take_rejected_exports(), vec![helper]);
    Fixture::no_ack(&f.game);
    for _ in 0..filled {
        let mut bytes = [0; 4];
        assert_eq!(f.helper.recv(&mut bytes).unwrap(), 4);
        assert_eq!(&bytes, b"full");
    }
    f.endpoint.release(helper).unwrap();
    assert_eq!(Fixture::ack(&f.helper), 9);
    Fixture::no_ack(&f.game);
    f.endpoint.release(game).unwrap();
    assert_eq!(Fixture::ack(&f.game), 9);
}

#[test]
fn foreign_session_export_cannot_change_selection_or_release_routes() {
    let mut f = Fixture::new();
    accept_capture_message(
        &mut f.model,
        &mut f.selector,
        &f.shared,
        CaptureMessage::ReplayFrameExported {
            session_id: CaptureSessionId::new([13; 16]).unwrap(),
            sequence: 1,
            fd_number: 10,
            source: Fixture::source(320),
            offset: 0,
            stride: 1280,
            modifier: 0,
            timestamp_ns: 1,
            duration_ns: 16_666_667,
        },
        Some(File::open("/dev/null").unwrap().into()),
        Some(&f.root.join("game.sock")),
    );
    assert!(f.selector.selected.is_none());
    assert!(f.selector.candidates.is_empty());
    assert!(lock_unpoisoned(&f.shared.replay_release.targets).is_empty());
    Fixture::no_ack(&f.game);
}

#[test]
fn game_gpu_identity_survives_private_wire_and_dma_buf_handoff() {
    let mut f = Fixture::new();
    f.start_receiver();
    let session_id = f.shared.replay_release.session_id;
    let identity = redunar_capture::CaptureGpuIdentity::new(0x10de, [1; 16], [2; 16]).unwrap();
    let mut source = Fixture::source(320);
    source.gpu_identity = Some(identity);
    f.wire(
        &CaptureMessage::Hello {
            session_id,
            process_id: 101,
            api: CaptureApi::Vulkan,
            producer_started_monotonic_ns: 1,
        },
        false,
    );
    f.wire(
        &CaptureMessage::GpuIdentity {
            session_id,
            api: CaptureApi::Vulkan,
            identity: Some(identity),
        },
        false,
    );
    f.wire(
        &CaptureMessage::ReplaySourceCandidate {
            session_id,
            candidate: source,
        },
        false,
    );
    for sequence in 1..=3 {
        f.wire(
            &CaptureMessage::ReplayFrameExported {
                session_id,
                sequence,
                fd_number: 10,
                source,
                offset: 0,
                stride: 1_280,
                modifier: 0,
                timestamp_ns: sequence,
                duration_ns: 16_666_667,
            },
            true,
        );
    }
    f.wait_exports(1);
    assert_eq!(
        lock_unpoisoned(&f.shared.latest).gpu_identity,
        Some(identity)
    );
    assert_eq!(Fixture::ack(&f.game), 1);
    assert_eq!(Fixture::ack(&f.game), 2);
    let export = f.endpoint.take_next().unwrap();
    let (token, frame) = f.endpoint.import(export).unwrap();
    assert_eq!(frame.gpu_identity(), Some(identity));
    drop(frame);
    f.endpoint.release(token).unwrap();
    assert_eq!(Fixture::ack(&f.game), 3);
}

#[test]
fn device_recreation_resets_confirmation_but_preserves_old_gpu_routes() {
    let mut f = Fixture::new();
    let source = Fixture::source(320);
    let session_id = f.shared.replay_release.session_id;
    f.announce("helper.sock", source);
    for sequence in 498..=500 {
        f.export("helper.sock", sequence, sequence, source);
    }
    let _ = Fixture::ack(&f.helper);
    let _ = Fixture::ack(&f.helper);
    let old = f.endpoint.take_next().unwrap().sequence;
    f.model
        .accept(CaptureMessage::Goodbye {
            session_id,
            api: CaptureApi::Vulkan,
            last_sequence: 0,
            reason: redunar_capture::GoodbyeReason::Normal,
        })
        .unwrap();
    accept_capture_message(
        &mut f.model,
        &mut f.selector,
        &f.shared,
        CaptureMessage::Hello {
            session_id,
            process_id: 101,
            api: CaptureApi::Vulkan,
            producer_started_monotonic_ns: 501,
        },
        None,
        Some(&f.root.join("helper.sock")),
    );
    assert!(f.selector.selected.is_none());
    assert_eq!(lock_unpoisoned(&f.shared.replay_release.targets).len(), 1);
    f.announce("helper.sock", source);
    for sequence in 1..=3 {
        f.export("helper.sock", sequence, 501 + sequence, source);
    }
    assert_eq!(Fixture::ack(&f.helper), 1);
    assert_eq!(Fixture::ack(&f.helper), 2);
    let new = f.endpoint.take_next().unwrap().sequence;
    assert!(new > old);
    f.endpoint.release(new).unwrap();
    assert_eq!(Fixture::ack(&f.helper), 3);
    f.endpoint.release(old).unwrap();
    assert_eq!(Fixture::ack(&f.helper), 500);
}

#[test]
fn copied_telemetry_follows_confirmed_process_and_its_reset_sequence() {
    let mut f = Fixture::new();
    let source = Fixture::source(320);
    f.announce("helper.sock", source);
    for sequence in 498..=500 {
        f.export("helper.sock", sequence, sequence, source);
    }
    let copied = |sequence| CaptureMessage::ReplayFrameCopied {
        session_id: f.shared.replay_release.session_id,
        sequence,
        source,
        copied_bytes: source.width * source.height * 4,
        sample_checksum: 1,
    };
    let helper_copy = copied(500);
    let stale_copy = copied(501);
    let game_copy = copied(1);
    accept_capture_message(
        &mut f.model,
        &mut f.selector,
        &f.shared,
        helper_copy,
        None,
        Some(&f.root.join("helper.sock")),
    );
    assert_eq!(f.model.snapshot().replay_copied_frame_count, 1);
    f.announce("game.sock", source);
    for sequence in 1..=3 {
        f.export(
            "game.sock",
            sequence,
            REPLAY_PRODUCER_STALE_NS + 1000 + sequence,
            source,
        );
    }
    accept_capture_message(
        &mut f.model,
        &mut f.selector,
        &f.shared,
        stale_copy,
        None,
        Some(&f.root.join("helper.sock")),
    );
    assert!(f.model.snapshot().replay_latest_copy_source.is_none());
    accept_capture_message(
        &mut f.model,
        &mut f.selector,
        &f.shared,
        game_copy,
        None,
        Some(&f.root.join("game.sock")),
    );
    assert_eq!(f.model.snapshot().replay_copied_frame_count, 2);
    assert_eq!(f.model.snapshot().replay_latest_copy_source, Some(source));
}

#[test]
fn receiver_waits_for_route_capacity_before_taking_the_next_transferred_fd() {
    let mut f = Fixture::new();
    for sequence in 1..=REPLAY_RELEASE_TARGET_CAPACITY as u64 {
        f.shared
            .replay_release
            .register(sequence, Some(&f.root.join("helper.sock")))
            .unwrap();
    }
    f.start_receiver();
    let session_id = f.shared.replay_release.session_id;
    let source = Fixture::source(320);
    f.wire(
        &CaptureMessage::Hello {
            session_id,
            process_id: 101,
            api: CaptureApi::Vulkan,
            producer_started_monotonic_ns: 1,
        },
        false,
    );
    f.wire(
        &CaptureMessage::ReplaySourceCandidate {
            session_id,
            candidate: source,
        },
        false,
    );
    let exported = |sequence| CaptureMessage::ReplayFrameExported {
        session_id,
        sequence,
        fd_number: 10,
        source,
        offset: 0,
        stride: 1280,
        modifier: 0,
        timestamp_ns: sequence,
        duration_ns: 16_666_667,
    };
    for sequence in 1..=3 {
        f.wire(&exported(sequence), true);
    }
    f.shared.replay_release.release(1).unwrap();
    assert_eq!(Fixture::ack(&f.helper), 1);
    f.wait_exports(1);
    assert_eq!(Fixture::ack(&f.game), 1);
    assert_eq!(Fixture::ack(&f.game), 2);
    assert!(f.shared.replay_release.at_capacity());
    f.wire(&exported(4), true);
    // One old owner's completion frees admission. The queued new FD is then
    // received with its own token and remains leased until its own completion.
    f.shared.replay_release.release(2).unwrap();
    assert_eq!(Fixture::ack(&f.helper), 2);
    f.wait_exports(2);
    let first = f.endpoint.take_next().unwrap().sequence;
    let second = f.endpoint.take_next().unwrap().sequence;
    assert!(second > first);
    Fixture::no_ack(&f.game);
    f.endpoint.release(second).unwrap();
    assert_eq!(Fixture::ack(&f.game), 4);
    f.endpoint.release(first).unwrap();
    assert_eq!(Fixture::ack(&f.game), 3);
}

#[test]
fn rejected_confirmation_frame_still_resets_the_new_owners_copy_watermark() {
    let mut f = Fixture::new();
    let source = Fixture::source(320);
    let session_id = f.shared.replay_release.session_id;
    f.announce("helper.sock", source);
    for sequence in 498..=500 {
        f.export("helper.sock", sequence, sequence, source);
    }
    accept_capture_message(
        &mut f.model,
        &mut f.selector,
        &f.shared,
        CaptureMessage::ReplayFrameCopied {
            session_id,
            sequence: 500,
            source,
            copied_bytes: source.width * source.height * 4,
            sample_checksum: 1,
        },
        None,
        Some(&f.root.join("helper.sock")),
    );
    f.announce("game.sock", source);
    for sequence in 1..=2 {
        f.export(
            "game.sock",
            sequence,
            REPLAY_PRODUCER_STALE_NS + 1000 + sequence,
            source,
        );
    }
    accept_capture_message(
        &mut f.model,
        &mut f.selector,
        &f.shared,
        CaptureMessage::ReplayFrameExported {
            session_id,
            sequence: 3,
            fd_number: 10,
            source,
            offset: 0,
            stride: 1,
            modifier: 0,
            timestamp_ns: REPLAY_PRODUCER_STALE_NS + 1003,
            duration_ns: 16_666_667,
        },
        Some(File::open("/dev/null").unwrap().into()),
        Some(&f.root.join("game.sock")),
    );
    f.export("game.sock", 4, REPLAY_PRODUCER_STALE_NS + 1004, source);
    accept_capture_message(
        &mut f.model,
        &mut f.selector,
        &f.shared,
        CaptureMessage::ReplayFrameCopied {
            session_id,
            sequence: 1,
            source,
            copied_bytes: source.width * source.height * 4,
            sample_checksum: 1,
        },
        None,
        Some(&f.root.join("game.sock")),
    );
    assert_eq!(f.model.snapshot().replay_copied_frame_count, 2);
    assert_eq!(f.model.snapshot().replay_latest_copy_source, Some(source));
    assert_eq!(f.model.snapshot().rejected_message_count, 1);
}

#[test]
fn replacement_incarnation_cannot_receive_an_old_equal_sequence_ack() {
    let f = Fixture::new();
    let base = f.root.join("reply.sock");
    let old_path = redunar_capture::unique_replay_reply_path(&base, CaptureApi::Vulkan).unwrap();
    let new_path = redunar_capture::unique_replay_reply_path(&base, CaptureApi::Vulkan).unwrap();
    let old = UnixDatagram::bind(&old_path).unwrap();
    let new = UnixDatagram::bind(&new_path).unwrap();
    new.set_nonblocking(true).unwrap();
    let transport = &f.shared.replay_release;
    let old_token = transport.register(3, Some(&old_path)).unwrap();
    let new_token = transport.register(3, Some(&new_path)).unwrap();
    drop(old);
    fs::remove_file(old_path).unwrap();
    transport.release(old_token).unwrap();
    Fixture::no_ack(&new);
    assert_eq!(lock_unpoisoned(&transport.targets).len(), 1);
    transport.release(new_token).unwrap();
    assert_eq!(Fixture::ack(&new), 3);
}

#[test]
fn session_start_reserves_reply_path_before_creating_runtime_files() {
    let f = Fixture::new();
    let library = f.root.join(CAPTURE_LIBRARY_FILE);
    fs::write(&library, b"fixture").unwrap();
    // The generated /capture-<32 hex digits> adds 41 bytes to the root.
    let runtime_for = |length: usize| {
        f.root
            .join("r".repeat(length - f.root.as_os_str().len() - 1))
    };
    let runtime = runtime_for(42);
    fs::create_dir(&runtime).unwrap();
    let mut handle = CaptureSessionHandle::start(
        &CaptureSessionConfig::new(&runtime, &library),
        CaptureModuleGates::default(),
    )
    .expect("83-byte session directory fits every reply endpoint");
    for api in [CaptureApi::Vulkan, CaptureApi::OpenGl] {
        let path =
            redunar_capture::unique_replay_reply_path(&handle.reply_socket_path, api).unwrap();
        assert_eq!(path.as_os_str().len(), 107);
        // Verify the allowed boundary against an actual Linux socket bind.
        let socket = UnixDatagram::bind(&path).unwrap();
        drop(socket);
        fs::remove_file(path).unwrap();
    }
    handle.shutdown();
    assert_eq!(fs::read_dir(&runtime).unwrap().count(), 0);

    let runtime = runtime_for(43);
    fs::create_dir(&runtime).unwrap();
    let preserved = runtime.join("keep.mkv");
    fs::write(&preserved, b"private fixture recording").unwrap();
    let error = CaptureSessionHandle::start(
        &CaptureSessionConfig::new(&runtime, &library),
        CaptureModuleGates::default(),
    )
    .expect_err("84-byte session directory cannot hold producer reply endpoints");
    assert!(
        error
            .to_string()
            .contains("too long for producer reply endpoints")
    );
    assert_eq!(fs::read_dir(&runtime).unwrap().count(), 1);
    assert_eq!(fs::read(preserved).unwrap(), b"private fixture recording");
}
