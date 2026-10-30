use redunar_capture_kms::{KmsCapturePlan, KmsProbe};
use redunar_core::ReplayFrameRate;
use std::time::Instant;

fn main() {
    let started = Instant::now();
    let probe = KmsProbe::local();
    println!("Redunar KMS Replay diagnostic");
    for output in &probe.outputs {
        let mode = output.modes.first().map_or_else(
            || "unknown".to_owned(),
            |mode| format!("{}x{}", mode.width, mode.height),
        );
        println!(
            "output=card{}-{} mode={} device={}",
            output.card_index,
            output.connector,
            mode,
            output.card_path.display()
        );
        match KmsCapturePlan::new(output, ReplayFrameRate::Fps60) {
            Ok(plan) => println!(
                "plan={}x{}@{}fps",
                plan.source_mode.width,
                plan.source_mode.height,
                plan.frame_rate.frames_per_second()
            ),
            Err(error) => println!("plan-blocked={error}"),
        }
    }
    for blocker in &probe.blockers {
        println!("blocker={blocker:?}");
    }
    println!("probe_elapsed_us={}", started.elapsed().as_micros());
}
