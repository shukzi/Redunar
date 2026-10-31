use redunar_daemon::RedunarService;
use std::env;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

const DEFAULT_ITERATIONS: u64 = 2_000;

fn main() {
    let refresh = env::args().any(|argument| argument == "--refresh");
    let iterations = env::args()
        .find_map(|argument| {
            argument
                .strip_prefix("--iterations=")
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or(DEFAULT_ITERATIONS);
    assert!(iterations > 0, "--iterations must be greater than zero");

    let state = PathBuf::from(format!(
        "/tmp/redunar-capability-refresh-bench-{}",
        std::process::id()
    ));
    let service = RedunarService::for_isolated_replay_probe(state);

    // Populate the cache before measuring the routine diagnostics path. The
    // refresh mode deliberately performs a fresh discovery per iteration.
    service.refresh_capability_snapshot();
    let started = Instant::now();
    for _ in 0..iterations {
        if refresh {
            service.refresh_capability_snapshot();
        }
        black_box((
            service.module_statuses(),
            service.capture_runtime_status(),
            service.replay_backend_readiness(),
            service.replay_hardware_encoder_probe(),
        ));
    }

    println!("refresh={refresh}");
    println!("iterations={iterations}");
    println!("elapsed_ms={}", started.elapsed().as_millis());
}
