use redunar_core::ReplaySettings;
use redunar_daemon::{ReplayBudget, ReplayClipStore};
use std::error::Error;
use std::fs;
use std::io::Cursor;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const SYNTHETIC_CONTAINER_BYTES: usize = 16 * 1024 * 1024;
const MATROSKA_MAGIC: [u8; 4] = [0x1a, 0x45, 0xdf, 0xa3];

fn main() -> Result<(), Box<dyn Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::env::temp_dir().join(format!(
        "redunar-replay-store-profile-{}-{nonce}",
        std::process::id()
    ));
    let store = ReplayClipStore::open(
        &root,
        ReplayBudget::from_settings(ReplaySettings::default()),
    )?;
    let mut container = vec![0x3c; SYNTHETIC_CONTAINER_BYTES];
    container[..MATROSKA_MAGIC.len()].copy_from_slice(&MATROSKA_MAGIC);
    let started = Instant::now();
    let stored = store.save_matroska(&mut Cursor::new(container))?;
    let elapsed = started.elapsed();
    assert_eq!(stored.bytes, SYNTHETIC_CONTAINER_BYTES as u64);
    assert!(stored.path.is_file());

    println!("container_bytes={}", stored.bytes);
    println!("elapsed_ms={}", elapsed.as_millis());
    let stored_mib = u32::try_from(stored.bytes / (1024 * 1024))?;
    println!(
        "throughput_mib_per_second={:.1}",
        f64::from(stored_mib) / elapsed.as_secs_f64()
    );
    fs::remove_dir_all(root)?;
    Ok(())
}
