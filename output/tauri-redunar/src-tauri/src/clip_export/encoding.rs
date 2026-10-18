//! Bounded, cancellable software encoding of an already-open recording.
use super::{terminate, ExportJob, ExportPhase, EXPORT_POLL_INTERVAL, EXPORT_TIMEOUT};
use redunar_daemon::ReplayOutputFormat;
use serde::Deserialize;
use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Child, Stdio},
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub enum OutputFormat {
    #[default]
    #[serde(rename = "mp4")]
    Mp4,
    #[serde(rename = "mkv")]
    Mkv,
}
impl OutputFormat {
    pub fn store_format(self) -> ReplayOutputFormat {
        match self {
            Self::Mp4 => ReplayOutputFormat::Mp4,
            Self::Mkv => ReplayOutputFormat::Matroska,
        }
    }
    fn muxer(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "matroska",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportOptions {
    #[serde(default)]
    pub output_format: OutputFormat,
    pub max_size_mb: Option<f64>,
    #[serde(default)]
    pub compression: bool,
}
impl ExportOptions {
    pub fn size_limit(&self) -> Result<Option<u64>, String> {
        if self.compression && self.max_size_mb.is_none() {
            return Err("Choose a maximum file size".into());
        }
        self.max_size_mb
            .map(|mb| {
                if !mb.is_finite() || !(0.1..=500.0).contains(&mb) {
                    return Err("Choose a maximum size between 0.1 and 500 MB".into());
                }
                Ok((mb * 1_000_000.0).floor() as u64)
            })
            .transpose()
    }
}

pub(super) struct Encoding<'a> {
    pub source: &'a File,
    pub start: f64,
    pub duration: f64,
    pub audio_streams: usize,
    pub options: &'a ExportOptions,
    pub job: &'a ExportJob,
    pub started: Instant,
    pub temporary_limit: u64,
    pub quality_first: bool,
}

#[derive(Debug)]
pub(super) struct MediaInfo {
    pub duration: f64,
    pub audio_streams: usize,
}

fn check(job: &ExportJob, started: Instant) -> Result<(), String> {
    if job.cancelled.load(Ordering::Acquire) {
        return Err("Clip export was cancelled".into());
    }
    if started.elapsed() >= EXPORT_TIMEOUT {
        return Err("Clip export timed out".into());
    }
    Ok(())
}

// Seekable descriptor paths retain inode identity and let FFmpeg read MP4s whose
// index is at the end. No browser path is ever passed to a media tool.
pub(super) fn probe(source: &File, job: &ExportJob, started: Instant) -> Result<MediaInfo, String> {
    check(job, started)?;
    let mut child = crate::media_tools::ffprobe()?
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type",
            "-of",
            "json",
            "/proc/self/fd/0",
        ])
        .stdin(Stdio::from(
            source
                .try_clone()
                .map_err(|_| "The clip could not be read")?,
        ))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "The clip could not be inspected")?;
    let stdout = child.stdout.take().expect("piped media inspection");
    let reader = match std::thread::Builder::new()
        .name("redunar-export-probe".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .take(128 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        }) {
        Ok(reader) => reader,
        Err(_) => {
            terminate(&mut child);
            return Err("Clip inspection could not start".into());
        }
    };
    let probe_started = Instant::now();
    let result = loop {
        if let Err(error) = check(job, started) {
            terminate(&mut child);
            break Err(error);
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break Ok(()),
            Ok(Some(_)) => break Err("The clip could not be inspected".into()),
            Ok(None) if probe_started.elapsed() < Duration::from_secs(8) => {
                std::thread::sleep(EXPORT_POLL_INTERVAL)
            }
            _ => {
                terminate(&mut child);
                break Err("Clip inspection timed out or stopped".into());
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| "Clip inspection stopped")?
        .map_err(|_| "Clip inspection could not be read")?;
    result?;
    if bytes.len() > 128 * 1024 {
        return Err("Clip inspection exceeded its limit".into());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "The clip reported invalid metadata")?;
    let duration = value["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .ok_or("The clip has no usable duration")?;
    let streams = value["streams"]
        .as_array()
        .ok_or("The clip has no usable video")?;
    if !duration.is_finite()
        || duration <= 0.0
        || !streams.iter().any(|s| s["codec_type"] == "video")
    {
        return Err("The clip has no usable video duration".into());
    }
    let audio_streams = streams
        .iter()
        .filter(|s| s["codec_type"] == "audio")
        .count();
    if audio_streams > 16 {
        return Err("This clip has too many audio tracks to export".into());
    }
    Ok(MediaInfo {
        duration,
        audio_streams,
    })
}

fn video_bitrate(limit: u64, seconds: f64, audio_streams: usize) -> Result<u64, String> {
    // Budget every preserved audio stream and muxing overhead before video.
    // MB is decimal, matching upload limits; small/long requests fail honestly.
    let overhead = (limit / 25).max(16_384);
    let available = limit.saturating_sub(overhead) as f64 * 8.0 / seconds;
    let video = available.floor() as u64;
    video.checked_sub(audio_streams as u64 * 96_000).filter(|rate| *rate >= 16_000)
        .ok_or_else(|| "This size is too small for the clip duration and audio. Increase the size or export a shorter range.".into())
}

impl Encoding<'_> {
    pub fn render(&self, output: &Path) -> Result<(), String> {
        let Some(limit) = self.options.size_limit()? else {
            return self.pass(output, None, None, 0, 950);
        };
        // A maximum is a ceiling, not a target: avoid spending an oversized
        // upload allowance on a recording that already fits it.
        if self.quality_first {
            self.pass(output, None, None, 0, 200)?;
            if output
                .metadata()
                .map_err(|_| "Exported clip size is unavailable")?
                .len()
                <= limit
            {
                return Ok(());
            }
        }
        let mut bitrate = video_bitrate(limit, self.duration, self.audio_streams)?;
        self.pass(output, Some(bitrate), Some(1), 0, 350)?;
        // Do not use -fs: it silently truncates media. Verify the complete muxed
        // file instead, retrying a small overshoot with a lower video budget.
        for attempt in 0..3 {
            self.pass(output, Some(bitrate), Some(2), 350 + attempt * 180, 180)?;
            let bytes = output
                .metadata()
                .map_err(|_| "Exported clip size is unavailable")?
                .len();
            if bytes <= limit {
                return Ok(());
            }
            let next = (bitrate as f64 * limit as f64 / bytes as f64 * 0.92).floor() as u64;
            if next < 16_000 {
                break;
            }
            bitrate = next;
        }
        Err("The complete clip could not fit within this size. Increase the size or export a shorter range.".into())
    }

    fn pass(
        &self,
        output: &Path,
        bitrate: Option<u64>,
        pass: Option<u8>,
        base: u32,
        span: u32,
    ) -> Result<(), String> {
        check(self.job, self.started)?;
        if pass == Some(1) {
            // Analysis produces statistics, not an upload file. Hide any output
            // from the preceding quality attempt while it is being analysed.
            self.job.set_output(ExportPhase::Analysing, None);
        } else {
            // Reset before spawning so a retry never reports the previous
            // attempt's bytes while FFmpeg opens its new output.
            File::create(output).map_err(|_| "The temporary export could not be opened")?;
            self.job.set_output(ExportPhase::Encoding, Some(0));
        }
        let mut command = crate::media_tools::ffmpeg(Some("libx264"))?;
        command.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-ss",
            &format!("{:.6}", self.start),
            "-i",
            "/proc/self/fd/0",
            "-t",
            &format!("{:.6}", self.duration),
            "-map",
            "0:v:0",
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            "-pix_fmt",
            "yuv420p",
            "-fps_mode",
            "vfr",
        ]);
        if let Some(rate) = bitrate {
            command.args(["-b:v", &rate.to_string()]);
        } else {
            command.args(["-crf", "20"]);
        }
        if let Some(pass) = pass {
            command
                .args(["-pass", &pass.to_string(), "-passlogfile"])
                .arg(output.with_file_name("video-pass"));
        }
        if pass == Some(1) {
            command.args(["-an"]);
        } else {
            command.args([
                "-map",
                "0:a?",
                "-c:a",
                "aac",
                "-b:a",
                if bitrate.is_some() { "96k" } else { "192k" },
            ]);
        }
        command.args([
            "-map_metadata",
            "-1",
            "-avoid_negative_ts",
            "make_zero",
            "-progress",
            "pipe:2",
            "-nostats",
        ]);
        if pass == Some(1) {
            command.args(["-f", "null", "-"]);
        } else {
            if matches!(self.options.output_format, OutputFormat::Mp4) {
                command.args(["-movflags", "+faststart"]);
            }
            command
                .args(["-f", self.options.output_format.muxer(), "-y"])
                .arg(output);
        }
        let mut child = command
            .stdin(Stdio::from(
                self.source
                    .try_clone()
                    .map_err(|_| "The clip could not be read")?,
            ))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| "Clip export could not start")?;
        let stderr = child.stderr.take().expect("piped encoding diagnostics");
        let progress = Arc::new(AtomicU32::new(0));
        let read_progress = progress.clone();
        let duration = self.duration;
        let reader = match std::thread::Builder::new()
            .name("redunar-export-progress".into())
            .spawn(move || read_ffmpeg_progress(stderr, duration, read_progress))
        {
            Ok(reader) => reader,
            Err(_) => {
                terminate(&mut child);
                return Err("Clip export progress could not start".into());
            }
        };
        let result = self.monitor(&mut child, output, &progress, base, span);
        let diagnostics = reader
            .join()
            .unwrap_or_else(|_| "Clip export diagnostics stopped".into());
        let status = result?;
        if !status.success() {
            return Err(crate::media_tools::capability_error(&diagnostics).unwrap_or_else(|| "FFmpeg could not encode this clip. Check available codecs or try another format.".into()));
        }
        if pass != Some(1) {
            self.job.set_output(
                ExportPhase::Encoding,
                output.metadata().ok().map(|m| m.len()),
            );
        }
        self.job
            .progress_thousandths
            .fetch_max(base + span, Ordering::Release);
        Ok(())
    }

    fn monitor(
        &self,
        child: &mut Child,
        output: &Path,
        progress: &AtomicU32,
        base: u32,
        span: u32,
    ) -> Result<std::process::ExitStatus, String> {
        loop {
            if let Ok(mut current) = self.job.output.lock() {
                if matches!(current.phase, ExportPhase::Encoding) {
                    current.bytes = output.metadata().ok().map(|metadata| metadata.len());
                }
            }
            let guard = check(self.job, self.started).and_then(|()| {
                // High-resolution/high-cadence clips can need more than 64 MiB
                // of analysis even when their compressed output is small. Keep
                // both pass files together within the same checked source-based
                // allowance as temporary output, not the requested upload size.
                let stats = output
                    .with_file_name("video-pass-0.log")
                    .metadata()
                    .map_or(0, |m| m.len())
                    .saturating_add(
                        output
                            .with_file_name("video-pass-0.log.mbtree")
                            .metadata()
                            .map_or(0, |m| m.len()),
                    );
                if stats > self.temporary_limit {
                    Err("Clip export analysis exceeded its safe temporary size limit".into())
                } else if output
                    .metadata()
                    .is_ok_and(|m| m.len() > self.temporary_limit)
                {
                    Err("Clip export exceeded its safe temporary size limit".into())
                } else {
                    Ok(())
                }
            });
            if let Err(error) = guard {
                terminate(child);
                return Err(error);
            }
            match child.try_wait() {
                Ok(Some(status)) => return Ok(status),
                Ok(None) => {}
                Err(_) => {
                    terminate(child);
                    return Err("Clip export process stopped unexpectedly".into());
                }
            }
            self.job.progress_thousandths.fetch_max(
                base + progress.load(Ordering::Acquire) * span / 1000,
                Ordering::Release,
            );
            std::thread::sleep(EXPORT_POLL_INTERVAL);
        }
    }
}

pub(super) fn read_ffmpeg_progress(
    stderr: impl Read,
    duration: f64,
    progress: Arc<AtomicU32>,
) -> String {
    let mut diagnostics = String::new();
    // read_until with a byte cap prevents a corrupt tool from allocating an
    // unbounded line; drain excess data without retaining it.
    let mut reader = BufReader::new(stderr);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.by_ref().take(4096).read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line);
        if let Some(value) = text.trim().strip_prefix("out_time_us=") {
            if let Ok(us) = value.parse::<f64>() {
                progress.store(
                    ((us / 1_000_000.0 / duration).clamp(0.0, 0.95) * 1000.0) as u32,
                    Ordering::Release,
                );
            }
        } else if !text.contains('=') && diagnostics.len() < 1200 {
            diagnostics.extend(text.trim().chars().take(200));
            diagnostics.push(' ');
        }
    }
    diagnostics.trim().into()
}

#[cfg(test)]
#[path = "encoding_tests.rs"]
mod tests;
