//! Bounded, on-demand container inspection for visible capture cards.
//! No frame decoding, network access or game-name inference is involved.
use serde::Serialize;
use std::{
    io::Read,
    process::Stdio,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

static READERS: AtomicUsize = AtomicUsize::new(0);
const OUTPUT_LIMIT: u64 = 16 * 1024;
struct ReaderPermit;
impl Drop for ReaderPermit {
    fn drop(&mut self) {
        READERS.fetch_sub(1, Ordering::Release);
    }
}

#[derive(Default, Debug, Serialize)]
pub struct ClipMetadata {
    duration_seconds: Option<f64>,
    width: Option<u64>,
    height: Option<u64>,
    fps: Option<f64>,
}

#[tauri::command]
pub async fn clip_metadata(file_name: String) -> Result<ClipMetadata, String> {
    READERS
        .fetch_update(Ordering::Acquire, Ordering::Relaxed, |count| {
            (count < 2).then_some(count + 1)
        })
        .map_err(|_| "Clip inspection is busy")?;
    let permit = ReaderPermit;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        inspect(&file_name)
    })
    .await
    .map_err(|_| "Clip inspection worker stopped")?
}

fn inspect(file_name: &str) -> Result<ClipMetadata, String> {
    let opened = crate::media::open_inventory_clip(file_name)?;
    // The descriptor remains pinned to the validated inode. The proc path
    // permits seeking for MP4 files whose metadata follows the media data.
    let mut child = crate::media_tools::ffprobe()?
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-probesize",
            "2097152",
            "-analyzeduration",
            "1000000",
            "-select_streams",
            "v:0",
            "-show_entries",
            "format=duration:stream=width,height,avg_frame_rate",
            "-of",
            "json",
            "/proc/self/fd/0",
        ])
        .stdin(Stdio::from(opened.file))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "FFprobe is unavailable for clip details")?;
    let stdout = child.stdout.take().ok_or("Clip details are unavailable")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(OUTPUT_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let started = Instant::now();
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < Duration::from_secs(4) => {
                std::thread::sleep(Duration::from_millis(25));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break Err("Clip inspection timed out or stopped");
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| "Clip details reader stopped")?
        .map_err(|_| "Clip details could not be read")?;
    if !outcome?.success() || bytes.len() as u64 > OUTPUT_LIMIT {
        return Err("Clip details are unavailable".into());
    }
    parse(&bytes)
}

fn parse(bytes: &[u8]) -> Result<ClipMetadata, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "Invalid clip details")?;
    let stream = &value["streams"][0];
    let positive = |number: Option<f64>, max| {
        number.filter(|number| number.is_finite() && *number > 0.0 && *number <= max)
    };
    let fps = stream["avg_frame_rate"].as_str().and_then(|rate| {
        let (numerator, denominator) = rate.split_once('/')?;
        Some(numerator.parse::<f64>().ok()? / denominator.parse::<f64>().ok()?)
    });
    Ok(ClipMetadata {
        duration_seconds: positive(
            value["format"]["duration"]
                .as_str()
                .and_then(|v| v.parse().ok()),
            86400.0,
        ),
        width: stream["width"].as_u64().filter(|v| (1..=16384).contains(v)),
        height: stream["height"]
            .as_u64()
            .filter(|v| (1..=16384).contains(v)),
        fps: positive(fps, 1000.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_real_container_values_and_fractional_frame_rates() {
        let result = parse(br#"{"format":{"duration":"29.5"},"streams":[{"width":2560,"height":1440,"avg_frame_rate":"60000/1001"}]}"#).unwrap();
        assert_eq!(result.duration_seconds, Some(29.5));
        assert_eq!(result.width, Some(2560));
        assert_eq!(result.height, Some(1440));
        assert!((result.fps.unwrap() - 59.94).abs() < 0.001);
    }
    #[test]
    fn absent_or_invalid_values_remain_unavailable() {
        for source in [br#"{}"#.as_slice(), br#"{"format":{"duration":"NaN"},"streams":[{"width":0,"height":999999,"avg_frame_rate":"1/0"}]}"#] {
            let result = parse(source).unwrap();
            assert!(result.duration_seconds.is_none());
            assert!(result.width.is_none());
            assert!(result.height.is_none());
            assert!(result.fps.is_none());
        }
        assert!(parse(b"bad json").is_err());
    }
}
