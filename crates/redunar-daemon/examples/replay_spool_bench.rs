use redunar_core::{ReplayFrameRate, ReplayQuality, ReplaySettings};
use redunar_daemon::{EncodedReplayPacket, ReplaySegmentSpool, ReplaySpoolSubmitError};
use std::collections::VecDeque;
use std::env;
use std::thread;
use std::time::{Duration, Instant};

const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;
const NORMAL_PACKETS: u64 = 6_000;
const NORMAL_PAYLOAD_BYTES: usize = 30 * 1024;
const PRESSURE_PACKETS: u64 = 120;
const PRESSURE_PAYLOAD_BYTES: usize = 2 * 1024 * 1024;

fn main() {
    let arguments = env::args().collect::<Vec<_>>();
    let pressure = arguments.iter().any(|argument| argument == "--pressure");
    let profile = if arguments.iter().any(|argument| argument == "--4k-variable") {
        Profile::FourKVariable
    } else if arguments.iter().any(|argument| argument == "--4k-high") {
        Profile::FourKHigh60
    } else if pressure {
        Profile::Pressure
    } else {
        Profile::Normal
    };
    let (
        profile_name,
        settings,
        packet_count,
        payload_bytes,
        duration_ns,
        keyframe_interval,
        paced,
    ) = profile.parameters();
    let root = env::temp_dir().join(format!("redunar-replay-spool-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut spool = ReplaySegmentSpool::open(&root, settings).expect("open spool");
    let mut pending = VecDeque::new();
    let mut pressure_events = 0_u64;
    let started = Instant::now();

    for index in 0..packet_count {
        pending.push_back(packet(index, payload_bytes, duration_ns, keyframe_interval));
        drain_pending(&spool, &mut pending, &mut pressure_events);
        if paced {
            thread::sleep(Duration::from_nanos(duration_ns));
        }
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while !pending.is_empty() {
        drain_pending(&spool, &mut pending, &mut pressure_events);
        assert!(
            Instant::now() < deadline,
            "spool worker did not drain benchmark packets"
        );
        thread::sleep(Duration::from_millis(1));
    }
    while spool.stats().accepted_packets < packet_count {
        assert!(
            Instant::now() < deadline,
            "spool worker did not accept benchmark packets"
        );
        thread::sleep(Duration::from_millis(1));
    }

    let stats = spool.stats();
    println!("profile={profile_name}");
    println!("pressure={pressure}");
    println!("packets={packet_count}");
    println!("payload_bytes={payload_bytes}");
    println!("pressure_events={pressure_events}");
    println!("accepted_packets={}", stats.accepted_packets);
    println!("queue_pressure_events={}", stats.dropped_queue_full);
    println!("segment_resyncs={}", stats.segment_resyncs);
    println!("elapsed_ms={}", started.elapsed().as_millis());
    spool.shutdown();
    let _ = std::fs::remove_dir_all(root);
}

#[derive(Clone, Copy)]
enum Profile {
    Normal,
    Pressure,
    FourKHigh60,
    FourKVariable,
}

impl Profile {
    fn parameters(self) -> (&'static str, ReplaySettings, u64, usize, u64, u64, bool) {
        let mut settings = ReplaySettings::default();
        match self {
            Self::Normal => (
                "normal",
                settings,
                NORMAL_PACKETS,
                NORMAL_PAYLOAD_BYTES,
                NANOSECONDS_PER_SECOND,
                2,
                false,
            ),
            Self::Pressure => (
                "pressure",
                settings,
                PRESSURE_PACKETS,
                PRESSURE_PAYLOAD_BYTES,
                NANOSECONDS_PER_SECOND,
                2,
                false,
            ),
            Self::FourKHigh60 => {
                settings.quality = ReplayQuality::High;
                settings.frame_rate = ReplayFrameRate::Fps60;
                (
                    "4k-high-60",
                    settings,
                    600,
                    5_000_000 / 60,
                    NANOSECONDS_PER_SECOND / 60,
                    120,
                    true,
                )
            }
            Self::FourKVariable => {
                settings.quality = ReplayQuality::High;
                settings.frame_rate = ReplayFrameRate::Variable;
                (
                    "4k-high-variable-64",
                    settings,
                    640,
                    5_000_000 / 64,
                    NANOSECONDS_PER_SECOND / 64,
                    128,
                    true,
                )
            }
        }
    }
}

fn packet(
    index: u64,
    payload_bytes: usize,
    duration_ns: u64,
    keyframe_interval: u64,
) -> EncodedReplayPacket {
    let keyframe = index.is_multiple_of(keyframe_interval);
    let mut payload = vec![0_u8; payload_bytes];
    payload[..4].copy_from_slice(
        &u32::try_from(payload_bytes - 4)
            .expect("benchmark payload fits framing")
            .to_be_bytes(),
    );
    payload[4] = if keyframe { 0x65 } else { 0x41 };
    EncodedReplayPacket::new(
        index.saturating_mul(duration_ns),
        duration_ns,
        keyframe,
        payload,
    )
    .expect("benchmark packet")
}

fn drain_pending(
    spool: &ReplaySegmentSpool,
    pending: &mut VecDeque<EncodedReplayPacket>,
    pressure_events: &mut u64,
) {
    let attempts = pending.len();
    for _ in 0..attempts {
        let Some(packet) = pending.pop_front() else {
            return;
        };
        match spool.try_submit(packet) {
            Ok(()) => {}
            Err(ReplaySpoolSubmitError::QueueFull(packet)) => {
                *pressure_events = pressure_events.saturating_add(1);
                pending.push_front(packet);
                return;
            }
            Err(ReplaySpoolSubmitError::NotRunning(packet)) => {
                panic!(
                    "spool stopped while benchmarking packet {}",
                    packet.timestamp_ns()
                );
            }
        }
    }
}
