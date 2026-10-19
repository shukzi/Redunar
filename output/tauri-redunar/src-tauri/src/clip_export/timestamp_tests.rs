use super::*;
use std::{fs, process::Command};

fn run(command: &mut Command) -> Vec<u8> {
    let result = command.output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    result.stdout
}

fn timing(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&run(Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_packets",
            "-show_packets",
            "-show_entries",
            "format=start_time,duration:stream=codec_type,start_time,nb_read_packets:packet=stream_index,pts_time",
            "-of",
            "json",
        ])
        .arg(path)))
    .unwrap()
}

#[test]
fn sparse_video_keeps_selected_duration_frame_holds_and_relative_audio_timing() {
    let temporary = super::super::TemporaryExport::new().unwrap();
    let mp4 = temporary.directory.join("sparse.mp4");
    // One initial frame, a two-second hold, then normal cadence. Capture can
    // produce these gaps. Two audio tracks include a deliberately delayed one.
    run(Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=320x180:r=30:d=6",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=6",
            "-itsoffset",
            "0.25",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:sample_rate=48000:duration=6",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-vf",
            "select=eq(n\\,0)+gte(n\\,60)",
            "-fps_mode",
            "vfr",
            "-c:v",
            "libx264",
            "-bf",
            "0",
            "-threads",
            "2",
            "-c:a",
            "aac",
            "-t",
            "6",
        ])
        .arg(&mp4));
    let mkv = temporary.directory.join("sparse.mkv");
    run(Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&mp4)
        .args(["-map", "0", "-c", "copy", "-avoid_negative_ts", "disabled"])
        .arg(&mkv));

    for input in [&mp4, &mkv] {
        let original = fs::read(input).unwrap();
        let source = File::open(input).unwrap();
        let job = ExportJob::default();
        let started = Instant::now();
        let info = probe(&source, &job, started).unwrap();
        assert_eq!(info.audio_streams, 2);
        for format in [OutputFormat::Mp4, OutputFormat::Mkv] {
            // Include normal trim, two-pass trim and whole-clip compression.
            for (whole, limited) in [(false, false), (false, true), (true, true)] {
                let duration = if whole { info.duration } else { 4.0 };
                let options = ExportOptions {
                    output_format: format,
                    max_size_mb: limited.then_some(0.35),
                    compression: whole,
                };
                Encoding {
                    source: &source,
                    start: 0.0,
                    duration,
                    audio_streams: 2,
                    options: &options,
                    job: &job,
                    started,
                    temporary_limit: 2_000_000,
                    quality_first: false,
                }
                .render(&temporary.path)
                .unwrap();
                let result = probe(&File::open(&temporary.path).unwrap(), &job, started).unwrap();
                assert!((result.duration - duration).abs() < 0.15,
                    "{input:?} -> {format:?}, limited={limited}, whole={whole}: {} instead of {duration}", result.duration);
                assert_eq!(result.audio_streams, 2);
                let output = timing(&temporary.path);
                let streams = output["streams"].as_array().unwrap();
                let video = streams.iter().find(|s| s["codec_type"] == "video").unwrap();
                let frames: u64 = video["nb_read_packets"].as_str().unwrap().parse().unwrap();
                assert!(
                    frames > 1 && frames <= if whole { 121 } else { 61 },
                    "a frame hold must not be filled with invented frames"
                );
                let mut times: Vec<f64> = output["packets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|packet| packet["stream_index"] == 0)
                    .map(|packet| packet["pts_time"].as_str().unwrap().parse().unwrap())
                    .collect();
                times.sort_by(f64::total_cmp);
                assert!(
                    (times[1] - times[0] - 2.0).abs() < 0.06,
                    "the two-second video hold must remain on the selected timeline"
                );
                let audio: Vec<f64> = streams
                    .iter()
                    .filter(|s| s["codec_type"] == "audio")
                    .map(|s| s["start_time"].as_str().unwrap().parse().unwrap())
                    .collect();
                assert!(
                    (audio[1] - audio[0] - 0.25).abs() < 0.06,
                    "relative audio offset changed: {audio:?}"
                );
                assert!(
                    output["format"]["start_time"]
                        .as_str()
                        .unwrap()
                        .parse::<f64>()
                        .unwrap()
                        .abs()
                        < 0.05,
                    "encoder delay must not become a leading multi-second gap"
                );
                if limited {
                    assert!(temporary.path.metadata().unwrap().len() <= 350_000);
                }
                run(Command::new("ffmpeg")
                    .args(["-v", "error", "-threads", "2", "-i"])
                    .arg(&temporary.path)
                    .args(["-map", "0", "-f", "null", "-"]));
                assert_eq!(fs::read(input).unwrap(), original);
            }
        }
    }
    let directory = temporary.directory.clone();
    drop(temporary);
    assert!(
        !directory.exists(),
        "private pass and output files must be removed"
    );
}
