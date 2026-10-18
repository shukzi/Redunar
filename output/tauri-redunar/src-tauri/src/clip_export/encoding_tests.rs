use super::*;
use std::{fs, process::Command};

fn job() -> ExportJob {
    ExportJob::default()
}

#[test]
fn upload_limits_are_decimal_and_validated_before_work() {
    let mut options = ExportOptions::default();
    assert_eq!(options.size_limit().unwrap(), None);
    for invalid in [f64::NAN, f64::INFINITY, -1.0, 0.0, 0.099, 500.1] {
        options.max_size_mb = Some(invalid);
        assert!(options.size_limit().is_err());
    }
    options.max_size_mb = Some(0.1);
    assert_eq!(options.size_limit().unwrap(), Some(100_000));
    options.max_size_mb = None;
    options.compression = true;
    assert!(options.size_limit().is_err());
    assert!(serde_json::from_str::<ExportOptions>(r#"{"outputFormat":"avi"}"#).is_err());
    assert!(
        serde_json::from_str::<ExportOptions>(r#"{"outputFormat":"mkv","path":"/outside"}"#)
            .is_err()
    );
    assert!(video_bitrate(100_000, 600.0, 2).is_err());
    assert!(
        video_bitrate(1_000_000, 10.0, 2).unwrap() < video_bitrate(1_000_000, 10.0, 0).unwrap()
    );
}

#[test]
fn generated_media_exports_both_containers_without_truncating_or_changing_source() {
    // Software-generated footage only. Exercise the actual encoder, all audio
    // tracks, descriptor reopening, final mux size, probe and cleanup paths.
    let temporary = super::super::TemporaryExport::new().unwrap();
    let input = temporary.directory.join("fixture.mp4");
    assert!(Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=320x180:r=20",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-t",
            "4",
            "-c:v",
            "libx264",
            "-c:a",
            "aac",
            "-pix_fmt",
            "yuv420p"
        ])
        .arg(&input)
        .status()
        .unwrap()
        .success());
    let original = fs::read(&input).unwrap();
    let source = File::open(&input).unwrap();
    let job = job();
    let started = Instant::now();
    let info = probe(&source, &job, started).unwrap();
    assert_eq!(info.audio_streams, 2);
    for format in [OutputFormat::Mp4, OutputFormat::Mkv] {
        for (limited, quality_first) in [(false, false), (true, false), (true, true)] {
            let options = ExportOptions {
                output_format: format,
                max_size_mb: limited.then_some(0.15),
                compression: false,
            };
            let encoder = Encoding {
                source: &source,
                start: 1.0,
                duration: 2.0,
                audio_streams: 2,
                options: &options,
                job: &job,
                started,
                temporary_limit: 2_000_000,
                quality_first,
            };
            encoder.render(&temporary.path).unwrap();
            let rendered = File::open(&temporary.path).unwrap();
            assert_eq!(
                job.status().output_bytes,
                Some(rendered.metadata().unwrap().len())
            );
            assert!(matches!(job.status().phase, ExportPhase::Encoding));
            let result = probe(&rendered, &job, started).unwrap();
            assert!(
                (result.duration - 2.0).abs() < 0.3,
                "duration: {}",
                result.duration
            );
            assert_eq!(result.audio_streams, 2);
            if limited {
                assert!(rendered.metadata().unwrap().len() <= 150_000);
            }
            let bytes = fs::read(&temporary.path).unwrap();
            match format {
                OutputFormat::Mp4 => assert_eq!(&bytes[4..8], b"ftyp"),
                OutputFormat::Mkv => assert_eq!(&bytes[..4], b"\x1a\x45\xdf\xa3"),
            }
            assert!(Command::new("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(&temporary.path)
                .args(["-f", "null", "-"])
                .status()
                .unwrap()
                .success());
            assert_eq!(fs::read(&input).unwrap(), original);
            if !limited && matches!(format, OutputFormat::Mp4) {
                let previous_bytes = rendered.metadata().unwrap().len();
                encoder
                    .pass(&temporary.path, Some(250_000), Some(1), 0, 350)
                    .unwrap();
                assert!(matches!(job.status().phase, ExportPhase::Analysing));
                assert_eq!(job.status().output_bytes, None);
                // Analysis keeps the prior attempt's file private and never
                // presents its stale bytes as a currently growing output.
                assert_eq!(temporary.path.metadata().unwrap().len(), previous_bytes);
            }
        }
    }
    let mkv = temporary.directory.join("fixture.mkv");
    assert!(Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&input)
        .args(["-map", "0", "-c", "copy"])
        .arg(&mkv)
        .status()
        .unwrap()
        .success());
    let mkv_source = File::open(&mkv).unwrap();
    let mkv_info = probe(&mkv_source, &job, started).unwrap();
    assert_eq!(mkv_info.audio_streams, 2);
    let options = ExportOptions {
        output_format: OutputFormat::Mp4,
        max_size_mb: Some(0.1),
        compression: false,
    };
    Encoding {
        source: &mkv_source,
        start: 1.0,
        duration: 2.0,
        audio_streams: 2,
        options: &options,
        job: &job,
        started,
        temporary_limit: 2_000_000,
        quality_first: false,
    }
    .render(&temporary.path)
    .unwrap();
    assert!(temporary.path.metadata().unwrap().len() <= 100_000);
    let path = temporary.directory.clone();
    drop(temporary);
    assert!(!path.exists());
}

#[test]
fn cancelled_and_expired_jobs_never_start_media_work() {
    let temporary = super::super::TemporaryExport::new().unwrap();
    let source = File::open(&temporary.path).unwrap();
    let job = job();
    job.cancelled.store(true, Ordering::Release);
    assert!(probe(&source, &job, Instant::now())
        .unwrap_err()
        .contains("cancelled"));
    job.cancelled.store(false, Ordering::Release);
    assert!(check(&job, Instant::now() - EXPORT_TIMEOUT)
        .unwrap_err()
        .contains("timed out"));
}

#[test]
fn analysis_statistics_use_the_source_budget_and_excess_work_is_reaped() {
    // Sparse private files model a normal 1440p/high-cadence analysis without
    // making the regression encode a long recording or consume physical disk.
    let temporary = super::super::TemporaryExport::new().unwrap();
    let source = File::open(&temporary.path).unwrap();
    let job = job();
    let options = ExportOptions {
        max_size_mb: Some(32.6),
        compression: true,
        ..ExportOptions::default()
    };
    let limit = super::super::temporary_output_limit(125_373_641).unwrap();
    let stats = File::create(temporary.directory.join("video-pass-0.log.mbtree")).unwrap();
    let log = File::create(temporary.directory.join("video-pass-0.log")).unwrap();
    let encoding = Encoding {
        source: &source,
        start: 0.0,
        duration: 46.65,
        audio_streams: 1,
        options: &options,
        job: &job,
        started: Instant::now(),
        temporary_limit: limit,
        quality_first: false,
    };
    stats.set_len(96 * 1024 * 1024).unwrap();
    let mut child = Command::new("sleep").arg("0.1").spawn().unwrap();
    assert!(encoding
        .monitor(&mut child, &temporary.path, &AtomicU32::new(0), 0, 350)
        .expect("normal analysis above 64 MiB should fit its source budget")
        .success());
    // The two statistics files share one cap, rather than each getting it.
    stats.set_len(limit).unwrap();
    log.set_len(1).unwrap();
    let mut child = Command::new("sleep").arg("10").spawn().unwrap();
    assert!(encoding
        .monitor(&mut child, &temporary.path, &AtomicU32::new(0), 0, 350)
        .unwrap_err()
        .contains("analysis"));
    assert!(child.try_wait().unwrap().is_some());
    stats.set_len(0).unwrap();
    log.set_len(0).unwrap();
    File::options()
        .write(true)
        .open(&temporary.path)
        .unwrap()
        .set_len(limit + 1)
        .unwrap();
    let mut child = Command::new("sleep").arg("10").spawn().unwrap();
    assert!(encoding
        .monitor(&mut child, &temporary.path, &AtomicU32::new(0), 0, 950)
        .unwrap_err()
        .contains("temporary size limit"));
    assert!(child.try_wait().unwrap().is_some());
    let directory = temporary.directory.clone();
    drop(stats);
    drop(log);
    drop(source);
    drop(temporary);
    assert!(
        !directory.exists(),
        "failure leaked private analysis/output files"
    );
}

#[test]
fn cancelling_a_running_encoder_reaps_it_and_releases_temporary_work() {
    let temporary = super::super::TemporaryExport::new().unwrap();
    let source = File::open(&temporary.path).unwrap();
    let job = Arc::new(job());
    let cancel = job.clone();
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        cancel.cancelled.store(true, Ordering::Release);
    });
    let mut child = Command::new("sleep").arg("10").spawn().unwrap();
    let options = ExportOptions::default();
    let encoding = Encoding {
        source: &source,
        start: 0.0,
        duration: 1.0,
        audio_streams: 0,
        options: &options,
        job: &job,
        started: Instant::now(),
        temporary_limit: 100_000,
        quality_first: false,
    };
    assert!(encoding
        .monitor(&mut child, &temporary.path, &AtomicU32::new(0), 0, 950)
        .unwrap_err()
        .contains("cancelled"));
    assert!(child.try_wait().unwrap().is_some());
    canceller.join().unwrap();
}
