use redunar_capture_audio::{PipeWireGameAudioCapture, discover_pipewire_game_node};
use std::collections::BTreeSet;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn main() {
    // No real process can own this PID, so production discovery must exercise
    // the default-output fallback rather than the game-owned path.
    let node = discover_pipewire_game_node(&BTreeSet::from([u32::MAX]))
        .unwrap_or_else(|error| {
            eprintln!("output_audio_live_unavailable=discovery {error}");
            std::process::exit(1);
        })
        .unwrap_or_else(|| {
            eprintln!("output_audio_live_unavailable=default_output_not_found");
            std::process::exit(1);
        });
    if node.process_id != 0 {
        eprintln!("output_audio_live_unavailable=unexpected_owned_node");
        std::process::exit(1);
    }

    let started = Instant::now();
    let mut capture = PipeWireGameAudioCapture::start(&node, 1, started).unwrap_or_else(|error| {
        eprintln!("output_audio_live_unavailable=capture_start {error}");
        std::process::exit(1);
    });
    let mut playback = Command::new("pw-cat")
        .args([
            "--playback",
            "--raw",
            "--rate",
            "48000",
            "--channels",
            "2",
            "--format",
            "s16",
            "--latency",
            "20ms",
            "--target",
            &node.serial.to_string(),
            "--properties",
            "application.name=Redunar Output Audio Probe",
            "/dev/zero",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|error| {
            eprintln!("output_audio_live_unavailable=playback_start {error}");
            std::process::exit(1);
        });

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut packets = 0_u8;
    let mut bytes = 0_usize;
    while packets < 10 && Instant::now() < deadline {
        match capture.next_packet(Duration::from_millis(250)) {
            Ok(Some(packet)) => {
                packets += 1;
                bytes = bytes.saturating_add(packet.bytes.len());
            }
            Ok(None) => {}
            Err(error) => {
                let _ = playback.kill();
                let _ = playback.wait();
                eprintln!("output_audio_live_unavailable=capture {error}");
                std::process::exit(1);
            }
        }
    }
    drop(capture);
    let _ = playback.kill();
    let _ = playback.wait();
    if packets != 10 || bytes == 0 {
        eprintln!(
            "output_audio_live_unavailable=insufficient_packets packets={packets} bytes={bytes}"
        );
        std::process::exit(1);
    }
    println!(
        "output_audio_live_ready=node_serial={} node_name={} packets={} opus_bytes={}",
        node.serial, node.node_name, packets, bytes
    );
}
