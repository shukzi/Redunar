mod encoding;
pub use encoding::ExportOptions;

use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const MINIMUM_TRIM_SECONDS: f64 = 0.1;
const MAXIMUM_TRIM_SECONDS: f64 = 24.0 * 60.0 * 60.0;
const EXPORT_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const EXPORT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const OUTPUT_SIZE_ALLOWANCE: u64 = 64 * 1024 * 1024;
const OUTPUT_SIZE_MULTIPLIER: u64 = 2;
static NEXT_EXPORT: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Default)]
pub struct ClipExports(Arc<Mutex<Option<ActiveExport>>>, Arc<AtomicBool>);

struct ActiveExport {
    id: u64,
    job: Arc<ExportJob>,
}

#[derive(Default)]
struct ExportJob {
    cancelled: AtomicBool,
    progress_thousandths: AtomicU32,
    output: Mutex<ExportOutput>,
}

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
enum ExportPhase {
    #[default]
    Preparing,
    Analysing,
    Encoding,
    Checking,
    Saving,
}

#[derive(Clone, Copy, Default)]
struct ExportOutput {
    phase: ExportPhase,
    bytes: Option<u64>,
}

impl ExportJob {
    fn set_output(&self, phase: ExportPhase, bytes: Option<u64>) {
        if let Ok(mut output) = self.output.lock() {
            *output = ExportOutput { phase, bytes };
        }
    }

    fn status(&self) -> ClipExportStatus {
        let output = self.output.lock().map(|output| *output).unwrap_or_default();
        ClipExportStatus {
            active: true,
            progress: f64::from(self.progress_thousandths.load(Ordering::Acquire)) / 10.0,
            phase: output.phase,
            output_bytes: output.bytes,
        }
    }
}

impl ClipExports {
    fn begin(&self) -> Result<(u64, Arc<ExportJob>), String> {
        let mut active = self.0.lock().map_err(|_| "Clip export is unavailable")?;
        if self.1.load(Ordering::Acquire) {
            return Err("Redunar is restarting".into());
        }
        if active.is_some() {
            return Err("Another clip export is already running".into());
        }
        let id = NEXT_EXPORT.fetch_add(1, Ordering::Relaxed);
        let job = Arc::new(ExportJob::default());
        *active = Some(ActiveExport {
            id,
            job: job.clone(),
        });
        Ok((id, job))
    }

    fn finish(&self, id: u64) {
        if let Ok(mut active) = self.0.lock() {
            if active.as_ref().is_some_and(|export| export.id == id) {
                active.take();
            }
        }
    }

    // Serialize mutations with export admission. An export owns its source
    // filename until archive/commit finishes; playback owns an open descriptor.
    pub fn with_idle<T>(&self, action: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let active = self.0.lock().map_err(|_| "Clip export is unavailable")?;
        if self.1.load(Ordering::Acquire) {
            return Err("Redunar is restarting".into());
        }
        if active.is_some() {
            return Err(
                "Finish or cancel the export before changing the clip or replay folder".into(),
            );
        }
        action()
    }

    pub(crate) fn claim_restart(
        &self,
        claim: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        self.with_idle(|| {
            claim()?;
            self.1.store(true, Ordering::Release);
            Ok(())
        })
    }

    pub(crate) fn cancel_restart(&self) {
        self.1.store(false, Ordering::Release);
    }

    pub fn shutdown(&self) {
        if let Ok(active) = self.0.lock() {
            if let Some(export) = active.as_ref() {
                export.job.cancelled.store(true, Ordering::Release);
            }
        }
    }
}

#[derive(Default, Serialize)]
pub struct ClipExportStatus {
    active: bool,
    progress: f64,
    phase: ExportPhase,
    output_bytes: Option<u64>,
}

#[derive(Serialize)]
pub struct ExportedClip {
    file_name: String,
    bytes: u64,
    duration_seconds: f64,
    destination: String,
}

#[tauri::command]
pub fn clip_export_status(exports: tauri::State<'_, ClipExports>) -> ClipExportStatus {
    let Ok(active) = exports.0.lock() else {
        return ClipExportStatus::default();
    };
    active
        .as_ref()
        .map_or(ClipExportStatus::default(), |export| export.job.status())
}

#[tauri::command]
pub fn cancel_clip_export(exports: tauri::State<'_, ClipExports>) -> Result<(), String> {
    let active = exports.0.lock().map_err(|_| "Clip export is unavailable")?;
    let export = active.as_ref().ok_or("No clip export is running")?;
    export.job.cancelled.store(true, Ordering::Release);
    Ok(())
}

#[tauri::command]
pub async fn export_replay_clip(
    file_name: String,
    start_seconds: f64,
    end_seconds: f64,
    output_name: Option<String>,
    options: Option<ExportOptions>,
    exports: tauri::State<'_, ClipExports>,
) -> Result<ExportedClip, String> {
    crate::backend::ensure_write_access()?;
    let duration = validate_trim_range(start_seconds, end_seconds)?;
    let options = options.unwrap_or_default();
    options.size_limit()?;
    let exports = exports.inner().clone();
    let (id, job) = exports.begin()?;
    let opened = match crate::media::open_inventory_clip(&file_name) {
        Ok(opened) => opened,
        Err(error) => {
            exports.finish(id);
            return Err(error);
        }
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        export_clip(
            opened.file,
            opened.bytes,
            opened.game_name,
            ExportRequest {
                start_seconds,
                duration_seconds: duration,
                output_name,
                options,
            },
            &job,
        )
    })
    .await
    .map_err(|error| format!("Clip export worker failed: {error}"));
    exports.finish(id);
    result?
}

fn validate_trim_range(start_seconds: f64, end_seconds: f64) -> Result<f64, String> {
    if !start_seconds.is_finite() || !end_seconds.is_finite() {
        return Err("The selected trim range is invalid".into());
    }
    if start_seconds < 0.0 || end_seconds > MAXIMUM_TRIM_SECONDS {
        return Err("The selected trim range is outside the supported clip duration".into());
    }
    let duration = end_seconds - start_seconds;
    if duration < MINIMUM_TRIM_SECONDS {
        return Err("Select at least 0.1 seconds to export".into());
    }
    Ok(duration)
}

struct ExportRequest {
    start_seconds: f64,
    duration_seconds: f64,
    output_name: Option<String>,
    options: ExportOptions,
}

fn export_clip(
    source: File,
    source_bytes: u64,
    source_game_name: Option<String>,
    request: ExportRequest,
    job: &ExportJob,
) -> Result<ExportedClip, String> {
    let started = Instant::now();
    let info = encoding::probe(&source, job, started)?;
    let (start_seconds, duration_seconds) = if request.options.compression {
        // Whole-clip requests use inspected native duration, never a UI trim.
        (0.0, validate_trim_range(0.0, info.duration)?)
    } else {
        if request.start_seconds + request.duration_seconds > info.duration + 0.05 {
            return Err("The selected range is outside this recording".into());
        }
        (request.start_seconds, request.duration_seconds)
    };
    let temporary = TemporaryExport::new().map_err(|error| {
        format!("A private temporary export file could not be created: {error}")
    })?;
    let temporary_limit = temporary_output_limit(source_bytes)?;
    encoding::Encoding {
        source: &source,
        start: start_seconds,
        duration: duration_seconds,
        audio_streams: info.audio_streams,
        options: &request.options,
        job,
        started,
        temporary_limit,
        quality_first: request.options.size_limit()?.is_some_and(|limit| {
            source_bytes as f64 * duration_seconds / info.duration <= limit as f64
        }),
    }
    .render(&temporary.path)?;
    job.set_output(
        ExportPhase::Checking,
        temporary.path.metadata().ok().map(|m| m.len()),
    );
    if job.cancelled.load(Ordering::Acquire) {
        return Err("Clip export was cancelled".into());
    }
    let rendered_file =
        File::open(&temporary.path).map_err(|_| "The exported clip could not be read")?;
    let rendered_info = encoding::probe(&rendered_file, job, started)?;
    let actual_duration = rendered_info.duration;
    if rendered_info.audio_streams != info.audio_streams {
        return Err("The exported clip did not preserve its audio tracks".into());
    }
    if job.cancelled.load(Ordering::Acquire) {
        return Err("Clip export was cancelled".into());
    }
    let minimum_expected = (duration_seconds - 0.35).max(MINIMUM_TRIM_SECONDS * 0.5);
    if actual_duration < minimum_expected || actual_duration > duration_seconds + 0.75 {
        return Err(format!(
            "The exported clip duration was unexpected ({actual_duration:.2}s instead of {duration_seconds:.2}s)"
        ));
    }
    let bytes = temporary
        .path
        .metadata()
        .map_err(|error| format!("Exported clip metadata is unavailable: {error}"))?
        .len();
    let output_limit = request
        .options
        .size_limit()?
        .unwrap_or(temporary_limit)
        .min(temporary_limit);
    if bytes > output_limit {
        return Err("The exported clip exceeded its maximum file size".into());
    }
    let store = crate::backend::service()
        .prepare_replay_store()
        .map_err(|error| error.to_string())?;
    job.set_output(ExportPhase::Saving, Some(bytes));
    store
        .ensure_free_space_for_export(bytes)
        .map_err(|error| error.to_string())?;
    let mut rendered = File::open(&temporary.path)
        .map_err(|error| format!("The trimmed clip could not be reopened: {error}"))?;
    let saved = store
        .save_export_preserving_existing_with_limit(
            &mut rendered,
            request.output_name.as_deref(),
            request.options.output_format.store_format(),
            output_limit,
        )
        .map_err(|error| error.to_string())?;
    let file_name = saved
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("The exported clip name is invalid")?
        .to_owned();
    // Carry the source clip's game attribution onto the trimmed export.
    // Labeling only: a ledger failure never fails a completed export.
    if let Some(game_name) = source_game_name {
        let identity = store.inventory().ok().and_then(|clips| {
            clips
                .into_iter()
                .find(|clip| clip.file_name == file_name)
                .map(|clip| clip.original_file_name)
        });
        if let Some(identity) = identity {
            let _ = crate::backend::service().record_clip_game(&identity, &game_name);
        }
    }
    job.progress_thousandths.store(1000, Ordering::Release);
    Ok(ExportedClip {
        file_name,
        bytes: saved.bytes,
        duration_seconds: actual_duration,
        destination: store.directory().to_string_lossy().into_owned(),
    })
}

fn temporary_output_limit(source_bytes: u64) -> Result<u64, String> {
    source_bytes
        .checked_mul(OUTPUT_SIZE_MULTIPLIER)
        .and_then(|bytes| bytes.checked_add(OUTPUT_SIZE_ALLOWANCE))
        .ok_or_else(|| "The source clip is too large to export safely".into())
}

fn terminate(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

struct TemporaryExport {
    directory: PathBuf,
    path: PathBuf,
}

impl TemporaryExport {
    fn new() -> io::Result<Self> {
        for _ in 0..8 {
            let mut random = [0_u8; 16];
            File::open("/dev/urandom")?.read_exact(&mut random)?;
            let token = random
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let directory = std::env::temp_dir().join(format!("redunar-export-{token}"));
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&directory) {
                Ok(()) => return Self::initialize(directory),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a private clip export directory",
        ))
    }

    // Only called after creating this job's private directory. Establish its
    // cleanup owner before the fallible file creation, including disk errors.
    fn initialize(directory: PathBuf) -> io::Result<Self> {
        let temporary = Self {
            path: directory.join("output.media"),
            directory,
        };
        let _ = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary.path)?;
        Ok(temporary)
    }
}

impl Drop for TemporaryExport {
    fn drop(&mut self) {
        // This private 0700 directory contains only this job's output and pass
        // statistics. Failure/cancellation must release every temporary file.
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding::read_ffmpeg_progress;

    #[test]
    fn failed_temporary_output_creation_cleans_its_owned_directory() {
        let directory = std::env::temp_dir().join(format!(
            "redunar-export-init-test-{}-{}",
            std::process::id(),
            NEXT_EXPORT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        fs::create_dir(directory.join("output.media")).unwrap();
        let result = TemporaryExport::initialize(directory.clone());
        let leaked = directory.exists();
        if leaked {
            fs::remove_dir_all(&directory).unwrap();
        }
        assert!(result.is_err());
        assert!(
            !leaked,
            "failed initialization retained its temporary directory"
        );
    }

    #[test]
    fn trim_range_rejects_invalid_or_empty_intervals() {
        for (start, end) in [
            (f64::NAN, 1.0),
            (0.0, f64::INFINITY),
            (-1.0, 1.0),
            (2.0, 1.0),
            (1.0, 1.05),
            (0.0, MAXIMUM_TRIM_SECONDS + 1.0),
        ] {
            assert!(validate_trim_range(start, end).is_err());
        }
        assert_eq!(validate_trim_range(1.0, 2.5).unwrap(), 1.5);
    }

    #[test]
    fn export_state_rejects_overlap_and_shutdown_requests_cancellation() {
        let exports = ClipExports::default();
        let (id, job) = exports.begin().expect("begin export");
        assert!(exports.begin().is_err());
        exports.shutdown();
        assert!(job.cancelled.load(Ordering::Acquire));
        exports.finish(id);
        assert!(exports.begin().is_ok());
    }

    #[test]
    fn mutations_wait_for_export_completion_and_failures_release_admission() {
        let exports = ClipExports::default();
        let (id, _) = exports.begin().expect("begin export");
        let called = AtomicBool::new(false);
        assert!(exports
            .with_idle(|| {
                called.store(true, Ordering::Release);
                Ok(())
            })
            .is_err());
        assert!(!called.load(Ordering::Acquire));
        exports.finish(id);
        assert!(exports
            .with_idle(|| Err::<(), _>("Fixture mutation failed".into()))
            .is_err());
        let (next, _) = exports.begin().expect("failed mutation released admission");
        exports.finish(id);
        assert!(
            exports.with_idle(|| Ok(())).is_err(),
            "stale completion must not release a new export"
        );
        exports.finish(next);
        assert!(exports.with_idle(|| Ok(())).is_ok());
    }

    #[test]
    fn restart_claim_rejects_exports_and_failed_claims_keep_admission() {
        let exports = ClipExports::default();
        let (id, _) = exports.begin().unwrap();
        assert!(exports.claim_restart(|| Ok(())).is_err());
        exports.finish(id);
        assert!(exports
            .claim_restart(|| Err("Session started".into()))
            .is_err());
        let (id, _) = exports.begin().unwrap();
        exports.finish(id);
        exports.claim_restart(|| Ok(())).unwrap();
        assert!(exports.begin().is_err());
        assert!(exports.claim_restart(|| Ok(())).is_err());
        assert!(exports.with_idle(|| Ok(())).is_err());
    }

    #[test]
    fn ffmpeg_progress_is_bounded_to_the_encoding_stage() {
        let progress = Arc::new(AtomicU32::new(0));
        let diagnostics = read_ffmpeg_progress(
            b"out_time_us=2500000\nprogress=continue\nout_time_us=12000000\nencoder failed\n"
                .as_slice(),
            10.0,
            progress.clone(),
        );
        assert_eq!(progress.load(Ordering::Acquire), 950);
        assert_eq!(diagnostics, "encoder failed");
    }

    #[test]
    fn temporary_export_allows_reencoding_overhead_without_becoming_unbounded() {
        assert_eq!(
            temporary_output_limit(8 * 1024 * 1024).unwrap(),
            80 * 1024 * 1024
        );
        assert!(temporary_output_limit(u64::MAX).is_err());
    }
}
