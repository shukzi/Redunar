//! Typed requests for recording-only controls and bounded local test audio.
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static WINDOW_FOCUSED: AtomicBool = AtomicBool::new(true);
static TEST_EPOCH: AtomicU64 = AtomicU64::new(0);

pub(crate) fn window_focus_changed(focused: bool) {
    WINDOW_FOCUSED.store(focused, Ordering::Release);
    if !focused {
        invalidate_pending_tests();
    }
}

pub(crate) fn invalidate_pending_tests() {
    TEST_EPOCH.fetch_add(1, Ordering::AcqRel);
}

fn test_is_current(epoch: u64) -> bool {
    WINDOW_FOCUSED.load(Ordering::Acquire) && TEST_EPOCH.load(Ordering::Acquire) == epoch
}

#[derive(Serialize)]
pub struct LevelsDto {
    peak: f32,
    rms: f32,
    limiting: bool,
}

impl From<redunar_capture_audio::MicrophoneLevels> for LevelsDto {
    fn from(levels: redunar_capture_audio::MicrophoneLevels) -> Self {
        Self {
            peak: levels.peak,
            rms: levels.rms,
            limiting: levels.limiting,
        }
    }
}

#[derive(Serialize)]
pub struct TestDto {
    id: u64,
    phase: &'static str,
    seconds_recorded: u16,
    levels: Option<LevelsDto>,
    message: Option<&'static str>,
}

impl From<redunar_daemon::MicrophoneTestStatus> for TestDto {
    fn from(status: redunar_daemon::MicrophoneTestStatus) -> Self {
        Self {
            id: status.id,
            phase: status.phase,
            seconds_recorded: status.seconds_recorded,
            levels: status.levels.map(Into::into),
            message: status.message,
        }
    }
}

#[tauri::command]
pub async fn set_microphone_gain(
    gain_db: i16,
) -> Result<crate::runtime::ReplayPreferencesDto, String> {
    crate::backend::ensure_write_access()?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::backend::service()
            .set_microphone_gain(gain_db)
            .map_err(|error| error.to_string())?;
        crate::runtime::replay_preferences_dto()
    })
    .await
    .map_err(|_| "Microphone setting task failed".to_owned())?
}

#[tauri::command]
pub async fn start_microphone_test() -> Result<TestDto, String> {
    crate::backend::ensure_write_access()?;
    let epoch = TEST_EPOCH.load(Ordering::Acquire);
    tauri::async_runtime::spawn_blocking(move || {
        if !test_is_current(epoch) {
            return Err("Keep Redunar focused while testing the microphone".to_owned());
        }
        let service = crate::backend::service();
        let status = service.start_microphone_test()?;
        // A queued start can race focus/close before the test identity exists.
        // Reject that old request even if the user has since focused us again.
        if !test_is_current(epoch) {
            service.stop_microphone_test(status.id);
            return Err("Microphone test cancelled because the window lost focus".to_owned());
        }
        Ok(status.into())
    })
    .await
    .map_err(|_| "Microphone test task failed".to_owned())?
}

#[tauri::command]
pub async fn microphone_test_status(id: u64) -> Result<TestDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::backend::service()
            .microphone_test_status(id)
            .map(Into::into)
    })
    .await
    .map_err(|_| "Microphone status task failed".to_owned())?
}

#[tauri::command]
pub async fn microphone_test_audio(id: u64) -> Result<Vec<u8>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::backend::service().microphone_test_audio(id)
    })
    .await
    .map_err(|_| "Microphone audio task failed".to_owned())?
}

#[tauri::command]
pub async fn stop_microphone_test(id: u64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::backend::service().stop_microphone_test(id))
        .await
        .map_err(|_| "Microphone stop task failed".to_owned())
}

#[tauri::command]
pub fn microphone_levels() -> Option<LevelsDto> {
    crate::backend::service()
        .game_session_coordinator()
        .replay_runtime()
        .microphone_levels()
        .map(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn focus_loss_invalidates_queued_start_even_after_focus_returns() {
        let before = TEST_EPOCH.load(Ordering::Acquire);
        window_focus_changed(false);
        assert!(!test_is_current(before));
        window_focus_changed(true);
        assert!(!test_is_current(before));
        let current = TEST_EPOCH.load(Ordering::Acquire);
        assert!(test_is_current(current));
        invalidate_pending_tests();
        assert!(!test_is_current(current));
    }
}
