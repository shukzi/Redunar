use redunar_core::ReplaySettings;
use redunar_daemon::{EncodedReplayPacket, ReplayBudget, ReplayRing};
use std::error::Error;
use std::time::Instant;

const SIMULATED_MINUTES: u64 = 10;
const FRAMES_PER_SECOND: u64 = 60;
const KEYFRAME_INTERVAL: u64 = 120;
const PACKET_BYTES: usize = 30 * 1024;
const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;

fn main() -> Result<(), Box<dyn Error>> {
    let settings = ReplaySettings::default();
    let budget = ReplayBudget::from_settings(settings);
    let mut ring = ReplayRing::new(settings);
    let frame_count = SIMULATED_MINUTES * 60 * FRAMES_PER_SECOND;
    let frame_duration_ns = NANOSECONDS_PER_SECOND / FRAMES_PER_SECOND;
    let started = Instant::now();

    for frame in 0..frame_count {
        let packet = EncodedReplayPacket::new(
            frame.saturating_mul(frame_duration_ns),
            frame_duration_ns,
            frame.is_multiple_of(KEYFRAME_INTERVAL),
            vec![0x5a; PACKET_BYTES],
        )?;
        ring.push(packet)?;
    }

    let elapsed = started.elapsed();
    let stats = ring.stats();
    assert!(stats.stored_packets <= budget.frame_capacity);
    assert!(stats.stored_bytes <= budget.maximum_ring_bytes);
    assert!(
        ring.packets()
            .next()
            .is_some_and(EncodedReplayPacket::is_keyframe)
    );

    println!("simulated_minutes={SIMULATED_MINUTES}");
    println!("packets_inserted={frame_count}");
    println!("stored_packets={}", stats.stored_packets);
    println!("stored_bytes={}", stats.stored_bytes);
    println!("evicted_packets={}", stats.evicted_packets);
    println!("elapsed_ms={}", elapsed.as_millis());
    println!(
        "average_push_ns={}",
        elapsed.as_nanos() / u128::from(frame_count)
    );
    Ok(())
}
