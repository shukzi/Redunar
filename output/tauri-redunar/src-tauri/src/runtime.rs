use serde::Serialize;

fn module_status_dto(statuses: redunar_daemon::ModuleStatuses) -> ModuleStatusDto {
    ModuleStatusDto {
        frame_metrics: format!("{:?}", statuses.frame_metrics),
        in_game_overlay: format!("{:?}", statuses.in_game_overlay),
        instant_replay: format!("{:?}", statuses.instant_replay),
    }
}

#[derive(Debug, Serialize)]
pub struct ModuleStatusDto {
    frame_metrics: String,
    in_game_overlay: String,
    instant_replay: String,
}

#[tauri::command]
pub fn module_statuses() -> Result<ModuleStatusDto, String> {
    let statuses = crate::backend::service().module_statuses();
    Ok(module_status_dto(statuses))
}

#[derive(Debug, Serialize)]
pub struct SessionHistoryItem {
    id: u64,
    game: String,
    started_unix: u64,
    duration_seconds: u32,
    average_fps: Option<f64>,
    one_percent_low_fps: Option<f64>,
    point_one_percent_low_fps: Option<f64>,
    frame_intervals_ns: Vec<u64>,
    timeline: Vec<SessionTelemetrySampleDto>,
}

#[derive(Debug, Serialize)]
pub struct SessionTelemetrySampleDto {
    elapsed_seconds: u32,
    fps: Option<f64>,
    frame_time_ms: Option<f64>,
    cpu_temperature_celsius: Option<f64>,
    gpu_temperature_celsius: Option<f64>,
    cpu_utilization_percent: Option<f64>,
    gpu_utilization_percent: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct ReplayRuntimeDto {
    phase: String,
    capability: String,
    buffered_seconds: f64,
    completed_save_revision: u64,
    backend_ready: bool,
    recorder_health: String,
    can_save: bool,
    failure: Option<String>,
    received_frame_count: u64,
    encoded_packet_count: u64,
    audio_packet_count: u64,
    audio_byte_count: u64,
}

#[derive(Debug, Serialize)]
pub struct SessionStatusDto {
    elapsed_seconds: Option<u64>,
    captures_saved: Option<u64>,
    profile_label: Option<String>,
    feature_summary: Option<String>,
    phase: String,
    game: Option<String>,
    failure: Option<String>,
    message: Option<String>,
    launch_locked: bool,
    can_end: bool,
    history_revision: u64,
    measurements: Option<SessionMeasurements>,
}

#[derive(Debug, Serialize)]
pub struct SessionMeasurements {
    revision: u64,
    average_fps: Option<f64>,
    frame_time_ms: Option<f64>,
    one_percent_low_fps: Option<f64>,
    point_one_percent_low_fps: Option<f64>,
    frame_intervals_ns: Vec<u64>,
    phase: String,
}

#[tauri::command]
pub fn session_status(sessions: tauri::State<'_, crate::sessions::Sessions>) -> SessionStatusDto {
    sessions.read(|engine| {
        let status = crate::backend::service()
            .game_session_coordinator()
            .status();
        SessionStatusDto {
            elapsed_seconds: engine.elapsed_seconds(),
            captures_saved: engine.captures_saved(),
            profile_label: engine.elapsed_seconds().map(|_| "Session profile".into()),
            feature_summary: engine.feature_summary(),
            phase: format!("{:?}", status.phase),
            game: status.game_name,
            failure: status.failure,
            message: engine.message.clone(),
            launch_locked: engine.launch_locked(),
            can_end: engine.can_end(),
            history_revision: engine.history_revision,
            measurements: engine.capture.as_ref().map(|snapshot| SessionMeasurements {
                revision: snapshot.revision,
                average_fps: snapshot.metrics.as_ref().map(|m| m.average_fps),
                frame_time_ms: snapshot.metrics.as_ref().map(|m| m.newest_frame_time_ms),
                one_percent_low_fps: snapshot.metrics.as_ref().map(|m| m.one_percent_low_fps),
                point_one_percent_low_fps: snapshot
                    .metrics
                    .as_ref()
                    .map(|m| m.point_one_percent_low_fps),
                frame_intervals_ns: snapshot
                    .recent_frame_intervals_ns
                    .iter()
                    .copied()
                    .take(240)
                    .collect(),
                phase: format!("{:?}", snapshot.phase),
            }),
        }
    })
}

#[tauri::command]
pub fn end_session(sessions: tauri::State<'_, crate::sessions::Sessions>) -> Result<(), String> {
    crate::backend::ensure_write_access()?;
    sessions.end()
}

#[tauri::command]
pub fn replay_runtime_status() -> Result<ReplayRuntimeDto, String> {
    // Poll the cached runtime; this path must not reread the catalog or
    // reconfigure replay at the monitor cadence.
    let status = crate::backend::service()
        .game_session_coordinator()
        .replay_runtime()
        .status();
    Ok(ReplayRuntimeDto {
        phase: format!("{:?}", status.phase),
        capability: format!("{:?}", status.capability),
        buffered_seconds: status.buffered_duration_ns as f64 / 1_000_000_000.0,
        completed_save_revision: status.completed_save_revision,
        backend_ready: status.backend_readiness.validation_allowed(),
        recorder_health: format!("{:?}", status.recorder_health),
        can_save: status.phase == redunar_daemon::ReplayPhase::Buffering
            && status.recorder_health == redunar_daemon::ReplayRecorderHealth::Healthy
            && status.buffered_duration_ns >= 1_000_000_000,
        failure: status.last_failure.map(|failure| format!("{failure:?}")),
        received_frame_count: status.received_frame_count,
        encoded_packet_count: status.encoded_packet_count,
        audio_packet_count: status.audio_packet_count,
        audio_byte_count: status.audio_byte_count,
    })
}

#[tauri::command]
pub fn save_replay(duration_seconds: u16) -> Result<(), String> {
    crate::backend::ensure_write_access()?;
    use redunar_core::ReplayDuration;
    let duration = match duration_seconds {
        15 => ReplayDuration::Seconds15,
        30 => ReplayDuration::Seconds30,
        60 => ReplayDuration::Seconds60,
        120 => ReplayDuration::Seconds120,
        180 => ReplayDuration::Seconds180,
        300 => ReplayDuration::Seconds300,
        600 => ReplayDuration::Seconds600,
        900 => ReplayDuration::Seconds900,
        _ => return Err(format!("Unsupported replay duration: {duration_seconds}s")),
    };
    crate::backend::service()
        .save_replay(duration)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_replay_clip(file_name: String) -> Result<(), String> {
    crate::backend::ensure_write_access()?;
    crate::backend::service()
        .delete_replay_clip(&file_name)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn session_history() -> Result<Vec<SessionHistoryItem>, String> {
    let records = crate::backend::service()
        .session_history()
        .map_err(|error| error.to_string())?;
    Ok(records
        .into_iter()
        .map(|item| SessionHistoryItem {
            id: item.id,
            game: item.game,
            started_unix: item.started_unix,
            duration_seconds: item.duration_seconds,
            average_fps: item.average_fps,
            one_percent_low_fps: item.one_percent_low_fps,
            point_one_percent_low_fps: item.point_one_percent_low_fps,
            frame_intervals_ns: item.frame_intervals_ns,
            timeline: item
                .timeline
                .into_iter()
                .map(|sample| SessionTelemetrySampleDto {
                    elapsed_seconds: sample.elapsed_seconds,
                    fps: sample.fps,
                    frame_time_ms: sample.frame_time_ms,
                    cpu_temperature_celsius: sample.cpu_temperature_celsius,
                    gpu_temperature_celsius: sample.gpu_temperature_celsius,
                    cpu_utilization_percent: sample.cpu_utilization_percent,
                    gpu_utilization_percent: sample.gpu_utilization_percent,
                })
                .collect(),
        })
        .collect())
}

#[derive(Debug, Serialize)]
pub struct ReplayStorageStatusDto {
    initialized: bool,
    owned_clips: u32,
    used_bytes: u64,
    storage_limit_bytes: u64,
    bytes_over_limit: u64,
}

#[tauri::command]
pub fn replay_storage_status() -> Result<ReplayStorageStatusDto, String> {
    let status = crate::backend::service()
        .replay_storage_status()
        .map_err(|error| error.to_string())?;
    Ok(ReplayStorageStatusDto {
        initialized: status.initialized,
        owned_clips: status.owned_clips,
        used_bytes: status.used_bytes,
        storage_limit_bytes: status.storage_limit_bytes,
        bytes_over_limit: status.bytes_over_limit,
    })
}

#[derive(Debug, Serialize)]
pub struct AppPreferencesDto {
    close_to_tray: bool,
    automatic_updates: bool,
    window_width: i32,
    window_height: i32,
    window_maximized: bool,
}

impl From<redunar_daemon::AppPreferences> for AppPreferencesDto {
    fn from(value: redunar_daemon::AppPreferences) -> Self {
        Self {
            close_to_tray: value.close_to_tray,
            automatic_updates: value.automatic_updates,
            window_width: value.window_width,
            window_height: value.window_height,
            window_maximized: value.window_maximized,
        }
    }
}

#[tauri::command]
pub fn set_automatic_updates(enabled: bool) -> Result<AppPreferencesDto, String> {
    crate::backend::ensure_write_access()?;
    crate::backend::service()
        .set_automatic_updates_enabled(enabled)
        .map(Into::into)
        .map_err(|error| error.to_string())
}

#[derive(Debug, Serialize)]
pub struct UpdateCheckStatusDto {
    current_version: String,
    state: String,
    message: String,
    latest_version: Option<String>,
    package_kind: Option<String>,
    asset_name: Option<String>,
}

#[tauri::command]
pub async fn check_for_updates() -> UpdateCheckStatusDto {
    match tauri::async_runtime::spawn_blocking(crate::updates::check_for_updates).await {
        Ok(status) => UpdateCheckStatusDto {
            current_version: status.current_version,
            state: status.state,
            message: status.message,
            latest_version: status.latest_version,
            package_kind: status.package_kind,
            asset_name: status.asset_name,
        },
        Err(error) => UpdateCheckStatusDto {
            current_version: env!("CARGO_PKG_VERSION").into(),
            state: "error".into(),
            message: format!("Update check could not complete: {error}"),
            latest_version: None,
            package_kind: None,
            asset_name: None,
        },
    }
}

#[derive(Debug, Serialize)]
pub struct UpdateInstallStatusDto {
    state: String,
    message: String,
}

#[tauri::command]
pub async fn install_update() -> UpdateInstallStatusDto {
    match tauri::async_runtime::spawn_blocking(crate::updates::install_update).await {
        Ok(status) => UpdateInstallStatusDto {
            state: status.state,
            message: status.message,
        },
        Err(error) => UpdateInstallStatusDto {
            state: "error".into(),
            message: format!("Update installation could not start: {error}"),
        },
    }
}

#[tauri::command]
pub fn app_preferences() -> Result<AppPreferencesDto, String> {
    crate::backend::service()
        .app_preferences()
        .map(Into::into)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_close_to_tray(
    enabled: bool,
    app: tauri::AppHandle,
) -> Result<AppPreferencesDto, String> {
    crate::backend::ensure_write_access()?;
    let service = crate::backend::service();
    let previous = service
        .app_preferences()
        .map_err(|error| error.to_string())?
        .close_to_tray;
    crate::tray::set_enabled(&app, enabled)?;
    match service.set_close_to_tray_enabled(enabled) {
        Ok(preferences) => Ok(preferences.into()),
        Err(error) => {
            let _ = crate::tray::set_enabled(&app, previous);
            Err(error.to_string())
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ReplayPreferencesDto {
    custom_save_parent: Option<String>,
    resolved_directory: String,
    output_format: String,
    overlay_shortcut: String,
    close_overlay_on_outside_click: bool,
    initial_save_duration_seconds: u16,
}

#[derive(Debug, Serialize)]
pub struct ReplayDisplayCapabilityDto {
    connected_outputs: usize,
    compatible_120_modes: usize,
    maximum_width: Option<u32>,
    maximum_height: Option<u32>,
    status: String,
}

#[tauri::command]
pub fn replay_display_capability() -> ReplayDisplayCapabilityDto {
    let capability = crate::backend::service().replay_display_capability();
    ReplayDisplayCapabilityDto {
        connected_outputs: capability.connected_outputs,
        compatible_120_modes: capability.compatible_120_modes,
        maximum_width: capability.maximum_width,
        maximum_height: capability.maximum_height,
        status: capability.error.unwrap_or_else(|| {
            if capability.compatible_120_modes > 0 {
                "A compatible 120 FPS display mode was detected".into()
            } else {
                "No compatible 120 FPS display mode was detected".into()
            }
        }),
    }
}

fn replay_preferences_dto() -> Result<ReplayPreferencesDto, String> {
    let service = crate::backend::service();
    let preferences = service
        .replay_preferences()
        .map_err(|error| error.to_string())?;
    let resolved = service
        .replay_save_directory()
        .map_err(|error| error.to_string())?;
    Ok(ReplayPreferencesDto {
        custom_save_parent: preferences
            .custom_save_parent
            .map(|path| path.to_string_lossy().into_owned()),
        resolved_directory: resolved.to_string_lossy().into_owned(),
        output_format: preferences.output_format.extension().to_uppercase(),
        overlay_shortcut: preferences.overlay_shortcut,
        close_overlay_on_outside_click: preferences.close_overlay_on_outside_click,
        initial_save_duration_seconds: preferences.initial_save_duration.seconds(),
    })
}

#[tauri::command]
pub fn replay_preferences() -> Result<ReplayPreferencesDto, String> {
    replay_preferences_dto()
}

#[tauri::command]
pub fn set_replay_save_parent(parent: Option<String>) -> Result<ReplayPreferencesDto, String> {
    crate::backend::ensure_write_access()?;
    crate::backend::service()
        .set_replay_save_parent(parent.map(std::path::PathBuf::from))
        .map_err(|error| error.to_string())?;
    replay_preferences_dto()
}

#[tauri::command]
pub fn set_replay_overlay_behavior(
    close_on_outside_click: bool,
) -> Result<ReplayPreferencesDto, String> {
    crate::backend::ensure_write_access()?;
    crate::backend::service()
        .set_replay_outside_click(close_on_outside_click)
        .map_err(|error| error.to_string())?;
    replay_preferences_dto()
}

#[tauri::command]
pub fn set_replay_initial_save_duration(
    duration_seconds: u16,
) -> Result<ReplayPreferencesDto, String> {
    crate::backend::ensure_write_access()?;
    let duration = match duration_seconds {
        15 => redunar_core::ReplayDuration::Seconds15,
        30 => redunar_core::ReplayDuration::Seconds30,
        60 => redunar_core::ReplayDuration::Seconds60,
        120 => redunar_core::ReplayDuration::Seconds120,
        180 => redunar_core::ReplayDuration::Seconds180,
        300 => redunar_core::ReplayDuration::Seconds300,
        600 => redunar_core::ReplayDuration::Seconds600,
        900 => redunar_core::ReplayDuration::Seconds900,
        _ => return Err(format!("Unsupported replay duration: {duration_seconds}s")),
    };
    crate::backend::service()
        .set_replay_initial_save_duration(duration)
        .map_err(|error| error.to_string())?;
    replay_preferences_dto()
}

#[tauri::command]
pub fn hide_replay_menu(window: tauri::Window) -> Result<(), String> {
    // Keep the daemon's input state in lockstep with the Tauri surface. A
    // hidden window must not leave the Replay menu marked visible, otherwise
    // the next global shortcut can toggle the state in the wrong direction.
    crate::backend::service()
        .game_session_coordinator()
        .close_replay_menu();
    window.hide().map_err(|error| error.to_string())
}
