use redunar_capture_audio::{SystemAudioCapture, discover_system_audio_source};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn main() {
    let mut playback = Command::new("/usr/bin/pw-cat")
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
            "--properties",
            "application.name=Redunar Audio Probe",
            "/dev/zero",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|error| {
            eprintln!("game_audio_live_unavailable=playback_start {error}");
            std::process::exit(1);
        });

    let source = discover_system_audio_source().unwrap_or_else(|error| {
        let _ = playback.kill();
        let _ = playback.wait();
        eprintln!("game_audio_live_unavailable=discovery {error}");
        std::process::exit(1);
    });

    let mut capture =
        SystemAudioCapture::start(&source, 1, Instant::now()).unwrap_or_else(|error| {
            let _ = playback.kill();
            let _ = playback.wait();
            eprintln!("game_audio_live_unavailable=capture_start {error}");
            std::process::exit(1);
        });
    let mut packets = 0_u8;
    let mut bytes = 0_usize;
    let capture_deadline = Instant::now() + Duration::from_secs(5);
    while packets < 10 && Instant::now() < capture_deadline {
        match capture.next_packet(Duration::from_millis(250)) {
            Ok(Some(packet)) => {
                packets += 1;
                bytes = bytes.saturating_add(packet.bytes.len());
            }
            Ok(None) => {}
            Err(error) => {
                drop(capture);
                let _ = playback.kill();
                let _ = playback.wait();
                eprintln!("game_audio_live_unavailable=capture {error}");
                std::process::exit(1);
            }
        }
    }
    drop(capture);
    let _ = playback.kill();
    let _ = playback.wait();
    if packets != 10 || bytes == 0 {
        eprintln!(
            "game_audio_live_unavailable=insufficient_packets packets={packets} bytes={bytes}"
        );
        std::process::exit(1);
    }
    println!(
        "game_audio_live_ready=backend={} source={} packets={} opus_bytes={}",
        source.backend_name(),
        source.name(),
        packets,
        bytes
    );
}
