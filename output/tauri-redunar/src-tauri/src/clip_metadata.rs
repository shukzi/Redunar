//! Bounded, on-demand container inspection for visible capture cards.
//! No frame decoding, network access or game-name inference is involved.
use serde::Serialize;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
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
    let count_file = opened
        .file
        .try_clone()
        .map_err(|_| "Clip details are unavailable")?;
    let bytes = probe(opened.file, false)?;
    let mut metadata = parse(&bytes)?;
    // Packet counting scans the clip. Keep the UI responsive for long saves;
    // absent is more honest than FFprobe's nominal H.264 frame rate for VFR.
    if metadata
        .duration_seconds
        .is_some_and(|seconds| seconds <= 120.0)
    {
        if let Ok(bytes) = probe(count_file, true) {
            metadata.fps = recorded_fps(&bytes, metadata.duration_seconds);
        }
    }
    Ok(metadata)
}

fn probe(mut file: File, count_packets: bool) -> Result<Vec<u8>, String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "Clip details are unavailable")?;
    // The descriptor remains pinned to the validated inode. The proc path
    // permits seeking for MP4 files whose metadata follows the media data.
    let mut command = crate::media_tools::ffprobe()?;
    if count_packets {
        command.arg("-count_packets");
    }
    command.args([
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
        "format=duration:stream=width,height,nb_read_packets",
        "-of",
        "json",
        "/proc/self/fd/0",
    ]);
    let mut child = command
        .stdin(Stdio::from(file))
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
    Ok(bytes)
}

fn parse(bytes: &[u8]) -> Result<ClipMetadata, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "Invalid clip details")?;
    let stream = &value["streams"][0];
    let positive = |number: Option<f64>, max| {
        number.filter(|number| number.is_finite() && *number > 0.0 && *number <= max)
    };
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
        fps: None,
    })
}

fn recorded_fps(bytes: &[u8], duration: Option<f64>) -> Option<f64> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let packets = value["streams"][0]["nb_read_packets"]
        .as_str()?
        .parse::<u64>()
        .ok()?;
    let rate = packets as f64 / duration?;
    (rate.is_finite() && rate > 0.0 && rate <= 1000.0).then_some(rate)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_real_container_values_without_trusting_nominal_frame_rate() {
        let result = parse(br#"{"format":{"duration":"29.5"},"streams":[{"width":2560,"height":1440,"avg_frame_rate":"60000/1001"}]}"#).unwrap();
        assert_eq!(result.duration_seconds, Some(29.5));
        assert_eq!(result.width, Some(2560));
        assert_eq!(result.height, Some(1440));
        assert_eq!(result.fps, None);
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
