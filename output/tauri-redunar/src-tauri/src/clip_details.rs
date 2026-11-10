//! Native mutations of exact owned recording identifiers.
use serde::Serialize;

#[derive(Serialize)]
pub struct ClipMutation {
    file_name: String,
    favorite: bool,
}
#[tauri::command]
pub async fn rename_replay_clip(
    file_name: String,
    new_file_name: String,
    exports: tauri::State<'_, crate::clip_export::ClipExports>,
) -> Result<ClipMutation, String> {
    crate::backend::ensure_write_access()?;
    let exports = exports.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        exports.with_idle(|| {
            let service = crate::backend::service();
            service
                .rename_replay_clip(&file_name, &new_file_name)
                .map_err(|e| e.to_string())?;
            let favorite = service
                .replay_clip_inventory()
                .map_err(|e| e.to_string())?
                .iter()
                .find(|c| c.file_name == new_file_name)
                .is_some_and(|c| c.favorite);
            Ok(ClipMutation {
                file_name: new_file_name,
                favorite,
            })
        })
    })
    .await
    .map_err(|_| "Filename editing stopped unexpectedly".to_owned())?
}
#[tauri::command]
pub async fn set_clip_details(file_name: String, favorite: bool) -> Result<ClipMutation, String> {
    crate::backend::ensure_write_access()?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::backend::service()
            .set_replay_clip_favorite(&file_name, favorite)
            .map_err(|e| e.to_string())?;
        Ok(ClipMutation {
            file_name,
            favorite,
        })
    })
    .await
    .map_err(|_| "Saving the favorite stopped unexpectedly".to_owned())?
}

#[tauri::command]
pub async fn trashed_replay_clips() -> Result<Vec<crate::media::Clip>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        crate::backend::service()
            .replay_trash_inventory()
            .map(|clips| clips.into_iter().map(Into::into).collect())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "Reading the trash bin stopped unexpectedly".to_owned())?
}

// No batch selection exists. Enforce one exact identifier at the native boundary
// so malformed requests cannot turn a single-clip action into an unbounded write.
fn single_clip(file_names: Vec<String>) -> Result<String, String> {
    if file_names.len() != 1 {
        return Err("Choose one clip".into());
    }
    Ok(file_names.into_iter().next().expect("one clip"))
}

async fn change_trash(
    file_names: Vec<String>,
    trashed: bool,
    exports: crate::clip_export::ClipExports,
) -> Result<(), String> {
    crate::backend::ensure_write_access()?;
    let name = single_clip(file_names)?;
    tauri::async_runtime::spawn_blocking(move || {
        exports.with_idle(|| {
            crate::backend::service()
                .set_replay_clip_trashed(&name, trashed)
                .map_err(|e| e.to_string())
        })
    })
    .await
    .map_err(|_| "Changing the trash bin stopped unexpectedly".to_owned())?
}

#[tauri::command]
pub async fn move_replay_clips_to_trash(
    file_names: Vec<String>,
    exports: tauri::State<'_, crate::clip_export::ClipExports>,
) -> Result<(), String> {
    change_trash(file_names, true, exports.inner().clone()).await
}

#[tauri::command]
pub async fn restore_replay_clips(
    file_names: Vec<String>,
    exports: tauri::State<'_, crate::clip_export::ClipExports>,
) -> Result<(), String> {
    change_trash(file_names, false, exports.inner().clone()).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn confirmation_rejects_stale_expired_and_repeated_requests() {
        use redunar_daemon::ReplayClipStore;
        use std::time::{Duration, Instant};
        let root =
            std::env::temp_dir().join(format!("redunar-confirmation-{}", std::process::id()));
        let store = ReplayClipStore::open(
            root.clone(),
            redunar_daemon::ReplayBudget::from_settings(redunar_core::ReplaySettings::default()),
        )
        .unwrap();
        let clip = store
            .save_matroska(&mut [0x1a, 0x45, 0xdf, 0xa3].as_slice())
            .unwrap();
        store
            .set_trashed(clip.path.file_name().unwrap().to_str().unwrap(), true)
            .unwrap();
        let mut pending = super::PendingTrashDeletion {
            revision: 2,
            plan: Some((Instant::now(), store.prepare_trash_deletion(None).unwrap())),
        };
        assert!(pending.take_confirmed("1").is_err());
        assert!(pending.plan.is_some());
        assert!(pending.take_confirmed("2").is_ok());
        assert!(pending.take_confirmed("2").is_err());
        pending.plan = Some((
            Instant::now() - Duration::from_secs(301),
            store.prepare_trash_deletion(None).unwrap(),
        ));
        assert!(pending
            .take_confirmed("2")
            .err()
            .unwrap()
            .contains("expired"));
        assert!(pending.plan.is_none());
        assert!(clip.path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn trash_commands_bound_the_number_of_targets() {
        assert!(super::single_clip(vec![]).is_err());
        assert!(super::single_clip(vec!["one.mp4".into(), "two.mp4".into()]).is_err());
        assert_eq!(
            super::single_clip(vec!["one.mp4".into()]).unwrap(),
            "one.mp4"
        );
    }
}

#[derive(Clone, Default)]
pub struct TrashDeletion(std::sync::Arc<std::sync::Mutex<PendingTrashDeletion>>);
#[derive(Default)]
struct PendingTrashDeletion {
    revision: u64,
    plan: Option<(std::time::Instant, redunar_daemon::TrashDeletionPlan)>,
}
impl PendingTrashDeletion {
    fn take_confirmed(&mut self, token: &str) -> Result<redunar_daemon::TrashDeletionPlan, String> {
        if token != self.revision.to_string() {
            return Err("Trash confirmation changed. Review the bin again.".into());
        }
        let (created, plan) = self
            .plan
            .take()
            .ok_or("Review the trash bin before deleting.")?;
        if created.elapsed() > std::time::Duration::from_secs(300) {
            return Err("Trash confirmation expired. Review the bin again.".into());
        }
        Ok(plan)
    }
}
#[derive(Serialize)]
pub struct TrashDeletionPreview {
    token: String,
    entries: Vec<TrashDeletionEntry>,
}
#[derive(Serialize)]
pub struct TrashDeletionEntry {
    file_name: String,
    bytes: u64,
}
#[derive(Serialize)]
pub struct TrashDeletionReport {
    deleted: Vec<String>,
    failures: Vec<(String, String)>,
}

#[tauri::command]
pub async fn prepare_trash_deletion(
    file_name: Option<String>,
    pending: tauri::State<'_, TrashDeletion>,
    exports: tauri::State<'_, crate::clip_export::ClipExports>,
) -> Result<TrashDeletionPreview, String> {
    crate::backend::ensure_write_access()?;
    let pending = pending.inner().clone();
    let exports = exports.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        exports.with_idle(|| {
            let mut pending = pending
                .0
                .lock()
                .map_err(|_| "Trash confirmation unavailable")?;
            pending.plan = None;
            let plan = crate::backend::service()
                .prepare_trash_deletion(file_name.as_deref())
                .map_err(|e| e.to_string())?;
            pending.revision = pending
                .revision
                .checked_add(1)
                .ok_or("Trash confirmation limit reached")?;
            let preview = TrashDeletionPreview {
                token: pending.revision.to_string(),
                entries: plan
                    .entries()
                    .into_iter()
                    .map(|(file_name, bytes)| TrashDeletionEntry { file_name, bytes })
                    .collect(),
            };
            pending.plan = Some((std::time::Instant::now(), plan));
            Ok(preview)
        })
    })
    .await
    .map_err(|_| "Preparing trash confirmation stopped unexpectedly".to_owned())?
}

#[tauri::command]
pub async fn delete_trashed_replay_clips(
    token: String,
    pending: tauri::State<'_, TrashDeletion>,
    exports: tauri::State<'_, crate::clip_export::ClipExports>,
) -> Result<TrashDeletionReport, String> {
    crate::backend::ensure_write_access()?;
    let pending = pending.inner().clone();
    let exports = exports.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        exports.with_idle(|| {
            let plan = {
                let mut pending = pending
                    .0
                    .lock()
                    .map_err(|_| "Trash confirmation unavailable")?;
                pending.take_confirmed(&token)?
            };
            let result = crate::backend::service()
                .delete_trashed_replay_clips(plan)
                .map_err(|e| e.to_string())?;
            Ok(TrashDeletionReport {
                deleted: result.deleted,
                failures: result.failures,
            })
        })
    })
    .await
    .map_err(|_| {
        "Trash deletion stopped unexpectedly. Refresh the bin to check its contents.".to_owned()
    })?
}
