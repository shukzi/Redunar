use redunar_daemon::RedunarService;

fn main() {
    let requested_frames = std::env::args()
        .nth(1)
        .map(|value| value.parse::<u16>())
        .transpose()
        .unwrap_or_else(|error| {
            eprintln!("kms_replay_live_probe_invalid_frame_count={error}");
            std::process::exit(2);
        })
        .unwrap_or(8);

    match RedunarService::default().run_kms_replay_live_diagnostic(requested_frames) {
        Ok(report) => println!(
            "kms_replay_live_ready={}x{} requested={} completed={} packets={} bytes={} keyframes={} elapsed_ms={}",
            report.width,
            report.height,
            report.requested_frames,
            report.completed_frames,
            report.encoded_packets,
            report.encoded_bytes,
            report.keyframes,
            report.elapsed_milliseconds
        ),
        Err(error) => {
            eprintln!("kms_replay_live_unavailable={error}");
            std::process::exit(1);
        }
    }
}
