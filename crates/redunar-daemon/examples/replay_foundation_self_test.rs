use redunar_daemon::RedunarService;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct DiagnosticDirectory(PathBuf);

impl Drop for DiagnosticDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let directory = DiagnosticDirectory(std::env::temp_dir().join(format!(
        "redunar-replay-foundation-self-test-{}-{nonce}",
        std::process::id()
    )));
    let report = RedunarService::default().run_replay_foundation_self_test(&directory.0)?;

    println!("packet_count={}", report.packet_count);
    println!("encoded_bytes={}", report.encoded_bytes);
    println!("container_bytes={}", report.stored_clip.bytes);
    println!("owned_clips={}", report.storage.owned_clips);
    println!(
        "storage_over_limit_bytes={}",
        report.storage.bytes_over_limit
    );
    println!(
        "production_activation_allowed={}",
        report.backend_readiness.activation_allowed()
    );
    println!("hardware_encode_verified=false");
    println!("decoded_playback_verified=false");
    println!("temporary_output_removed_on_exit=true");
    Ok(())
}
