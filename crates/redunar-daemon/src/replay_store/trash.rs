//! Recoverable logical bin. Media stays in its owned store, never OS Trash.
//! A separate inode-bound ledger preserves the existing filename-label format.
use super::{
    Path, ReplayClipEntry, ReplayClipStore, ReplayStoreError, fs, io_error, labels, owned_metadata,
    store_operation, validate_store,
};
use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
pub(super) const FILE: &str = ".redunar-clip-trash-v1";

/// Native-only confirmation snapshot. Callers cannot construct deletion targets.
pub struct TrashDeletionPlan {
    directory: std::path::PathBuf,
    directory_identity: (u64, u64),
    directory_handle: std::fs::File,
    targets: Vec<(String, fs::Metadata)>,
}
impl TrashDeletionPlan {
    #[must_use]
    pub fn entries(&self) -> Vec<(String, u64)> {
        self.targets
            .iter()
            .map(|(name, metadata)| (name.clone(), metadata.len()))
            .collect()
    }
}

#[derive(Default)]
pub struct TrashDeletionResult {
    pub deleted: Vec<String>,
    pub failures: Vec<(String, String)>,
}

fn identity(metadata: &fs::Metadata) -> (u64, u64, u64, i64, i64) {
    use std::os::unix::fs::MetadataExt;
    (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
    )
}

impl ReplayClipStore {
    /// Snapshot either one trashed clip or the current bin for confirmation.
    ///
    /// # Errors
    /// Rejects unsafe stores, missing targets and malformed bin metadata.
    pub fn prepare_trash_deletion(
        &self,
        file_name: Option<&str>,
    ) -> Result<TrashDeletionPlan, ReplayStoreError> {
        use std::os::unix::fs::MetadataExt;
        let _operation = store_operation();
        validate_store(&self.directory)?;
        let directory = fs::symlink_metadata(&self.directory)
            .map_err(|e| io_error("could not inspect Replay store", &e))?;
        let bin = labels::load_from(&self.directory, FILE)
            .map_err(|e| io_error("could not read Replay trash bin", &e))?;
        let directory_handle = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&self.directory)
            .map_err(|e| io_error("could not pin Replay store", &e))?;
        let pinned = directory_handle
            .metadata()
            .map_err(|e| io_error("could not inspect pinned Replay store", &e))?;
        if (pinned.dev(), pinned.ino()) != (directory.dev(), directory.ino()) {
            return Err(ReplayStoreError::new(
                "The Replay folder changed. Review the bin again.",
            ));
        }
        let mut targets = Vec::new();
        for (name, label) in &bin {
            if file_name.is_some_and(|selected| selected != name) {
                continue;
            }
            let metadata = match fs::symlink_metadata(self.directory.join(name)) {
                Ok(metadata) => metadata,
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound && file_name.is_none() =>
                {
                    continue;
                }
                Err(error) => return Err(io_error("could not inspect trashed clip", &error)),
            };
            if !label.matches(&metadata) {
                if file_name.is_none() {
                    continue;
                }
                return Err(ReplayStoreError::new(
                    "A trashed clip changed. Refresh the bin before deleting.",
                ));
            }
            targets.push((name.clone(), metadata));
        }
        if targets.is_empty() {
            return Err(ReplayStoreError::new(
                "No matching clips remain in the trash bin.",
            ));
        }
        Ok(TrashDeletionPlan {
            directory: self.directory.clone(),
            directory_identity: (directory.dev(), directory.ino()),
            targets,
            directory_handle,
        })
    }

    /// Delete only the unchanged, still-trashed files in a confirmation snapshot.
    /// Partial failures retain the remaining files and identify each result.
    ///
    /// # Errors
    /// A changed store or unreadable bin rejects the complete operation.
    pub fn delete_trashed(
        &self,
        plan: TrashDeletionPlan,
    ) -> Result<TrashDeletionResult, ReplayStoreError> {
        use std::os::unix::fs::MetadataExt;
        let _operation = store_operation();
        validate_store(&self.directory)?;
        let directory = fs::symlink_metadata(&self.directory)
            .map_err(|e| io_error("could not inspect Replay store", &e))?;
        if self.directory != plan.directory
            || (directory.dev(), directory.ino()) != plan.directory_identity
        {
            return Err(ReplayStoreError::new(
                "The Replay folder changed. Review the trash bin again.",
            ));
        }
        // Resolve every destructive operation through the held directory. A
        // renamed/replaced parent path cannot redirect deletion elsewhere.
        let pinned_directory = std::path::PathBuf::from(format!(
            "/proc/self/fd/{}",
            plan.directory_handle.as_raw_fd()
        ));
        let mut bin = labels::load_from(&pinned_directory, FILE)
            .map_err(|e| io_error("could not read Replay trash bin", &e))?;
        let mut result = TrashDeletionResult::default();
        for (name, expected) in plan.targets {
            let remove = || -> Result<(), ReplayStoreError> {
                let path = pinned_directory.join(&name);
                let current =
                    fs::symlink_metadata(&path).map_err(|e| io_error("clip is unavailable", &e))?;
                if !bin.get(&name).is_some_and(|label| label.matches(&current))
                    || identity(&current) != identity(&expected)
                {
                    return Err(ReplayStoreError::new(
                        "The clip changed or was restored. It was not deleted.",
                    ));
                }
                super::thumbnails::remove(&pinned_directory, &current)?;
                let current = fs::symlink_metadata(&path)
                    .map_err(|e| io_error("clip changed before deletion", &e))?;
                if !current.is_file()
                    || current.nlink() != 1
                    || identity(&current) != identity(&expected)
                {
                    return Err(ReplayStoreError::new(
                        "The clip changed before deletion. It was not deleted.",
                    ));
                }
                fs::remove_file(path).map_err(|e| io_error("could not permanently delete clip", &e))
            };
            match remove() {
                Ok(()) => {
                    bin.remove(&name);
                    result.deleted.push(name);
                }
                Err(error) => result.failures.push((name, error.to_string())),
            }
        }
        if !result.deleted.is_empty() {
            // Persist once per batch. Stale ledger entries never authorize a
            // replacement inode if a metadata write or directory sync fails.
            if let Err(error) = labels::save_to(&pinned_directory, FILE, &bin)
                .and_then(|()| super::sync_directory(&pinned_directory))
            {
                result.failures.push((
                    "Trash bin metadata".into(),
                    format!("Files were deleted, but the bin could not be synchronized: {error}"),
                ));
            }
        }
        Ok(result)
    }
}

impl ReplayClipStore {
    /// List recoverable recordings, with the same bounded ownership checks.
    ///
    /// # Errors
    /// Invalid storage, metadata or directory scans fail closed.
    pub fn trash_inventory(&self) -> Result<Vec<ReplayClipEntry>, ReplayStoreError> {
        self.inventory_state(true)
    }

    /// Persist one clip's bin state atomically without deleting or moving media.
    /// Repeated requests are idempotent. Only matching file inodes can be restored.
    ///
    /// # Errors
    /// Unsafe/replaced files and failed durable metadata writes are rejected.
    pub fn set_trashed(&self, file_name: &str, trashed: bool) -> Result<(), ReplayStoreError> {
        let _operation = store_operation();
        validate_store(&self.directory)?;
        let (metadata, label) = owned_metadata(&self.directory, file_name)?;
        let mut bin = labels::load_from(&self.directory, FILE)
            .map_err(|e| io_error("could not read Replay trash bin", &e))?;
        if bin.get(file_name).is_some_and(|l| l.matches(&metadata)) == trashed {
            return Ok(());
        }
        labels::prune(&self.directory, &mut bin);
        if trashed {
            bin.insert(file_name.to_owned(), label);
        } else {
            bin.remove(file_name);
        }
        labels::save_to(&self.directory, FILE, &bin)
            .map_err(|e| io_error("could not save Replay trash bin", &e))
    }
}

pub(super) fn require_live(
    directory: &Path,
    name: &str,
    metadata: &fs::Metadata,
) -> Result<(), ReplayStoreError> {
    let bin = labels::load_from(directory, FILE)
        .map_err(|e| io_error("could not read Replay trash bin", &e))?;
    if bin.get(name).is_some_and(|l| l.matches(metadata)) {
        return Err(ReplayStoreError::new(
            "Restore the clip from the trash bin first",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{MATROSKA_MAGIC, NEXT_FILE, Ordering};
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;
    fn store() -> (PathBuf, ReplayClipStore, String) {
        let root = std::env::temp_dir().join(format!(
            "redunar-trash-{}-{}",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let store = ReplayClipStore::open_with_limits(root.clone(), 1024, 8192).unwrap();
        let clip = store.save_matroska(&mut MATROSKA_MAGIC.as_slice()).unwrap();
        let name = clip.path.file_name().unwrap().to_str().unwrap().to_owned();
        (root, store, name)
    }
    #[test]
    fn deletion_snapshot_retains_restored_new_and_replaced_files() {
        let (root, store, first) = store();
        store.set_trashed(&first, true).unwrap();
        let restored = store
            .save_matroska(&mut MATROSKA_MAGIC.as_slice())
            .unwrap()
            .path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        store.set_trashed(&restored, true).unwrap();
        let replaced = store
            .save_matroska(&mut MATROSKA_MAGIC.as_slice())
            .unwrap()
            .path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        store.set_trashed(&replaced, true).unwrap();
        let plan = store.prepare_trash_deletion(None).unwrap();
        store.set_trashed(&restored, false).unwrap();
        fs::rename(root.join(&replaced), root.join("retained-original")).unwrap();
        fs::write(root.join(&replaced), MATROSKA_MAGIC).unwrap();
        let later = store
            .save_matroska(&mut MATROSKA_MAGIC.as_slice())
            .unwrap()
            .path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        store.set_trashed(&later, true).unwrap();
        let result = store.delete_trashed(plan).unwrap();
        assert_eq!(result.deleted, vec![first.clone()]);
        assert_eq!(result.failures.len(), 2);
        assert!(!root.join(first).exists());
        for name in [&restored, &replaced, &later] {
            assert_eq!(fs::read(root.join(name)).unwrap(), MATROSKA_MAGIC);
        }
        let remaining = store.prepare_trash_deletion(None).unwrap();
        assert_eq!(remaining.entries(), vec![(later, 4)]);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn single_delete_preserves_other_clips_and_reclaims_bytes() {
        let (root, store, name) = store();
        store.rename_clip(&name, "Favorite.mkv").unwrap();
        store.set_favorite("Favorite.mkv", true).unwrap();
        store.set_trashed("Favorite.mkv", true).unwrap();
        let live = store.save_matroska(&mut MATROSKA_MAGIC.as_slice()).unwrap();
        let plan = store.prepare_trash_deletion(Some("Favorite.mkv")).unwrap();
        let result = store.delete_trashed(plan).unwrap();
        assert_eq!(result.deleted, vec!["Favorite.mkv"]);
        assert_eq!(result.failures, [] as [(String, String); 0]);
        assert_eq!(store.trash_inventory().unwrap(), [] as [ReplayClipEntry; 0]);
        assert!(live.path.exists());
        assert_eq!(
            ReplayClipStore::inspect_with_limits(root.clone(), 1024, 8192)
                .unwrap()
                .used_bytes,
            4
        );
        assert!(
            store
                .prepare_trash_deletion(Some("../foreign.mkv"))
                .is_err()
        );
        assert!(
            store
                .prepare_trash_deletion(Some(live.path.file_name().unwrap().to_str().unwrap()))
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn deletion_rejects_changed_store_links_and_modified_contents() {
        let (root, store, name) = store();
        store.set_trashed(&name, true).unwrap();
        let plan = store.prepare_trash_deletion(None).unwrap();
        let saved = root.with_extension("saved");
        fs::rename(&root, &saved).unwrap();
        fs::create_dir(&root).unwrap();
        assert!(store.delete_trashed(plan).is_err());
        fs::remove_dir(&root).unwrap();
        fs::rename(&saved, &root).unwrap();
        for kind in ["hardlink", "symlink", "modified"] {
            let plan = store.prepare_trash_deletion(None).unwrap();
            match kind {
                "hardlink" => fs::hard_link(root.join(&name), root.join("other-link")).unwrap(),
                "symlink" => {
                    fs::rename(root.join(&name), root.join("original")).unwrap();
                    symlink("original", root.join(&name)).unwrap();
                }
                _ => fs::write(root.join(&name), [0; 32]).unwrap(),
            }
            let result = store.delete_trashed(plan).unwrap();
            assert_eq!(result.deleted, [] as [String; 0]);
            assert_eq!(result.failures.len(), 1);
            match kind {
                "hardlink" => fs::remove_file(root.join("other-link")).unwrap(),
                "symlink" => {
                    fs::remove_file(root.join(&name)).unwrap();
                    fs::rename(root.join("original"), root.join(&name)).unwrap();
                }
                _ => {}
            }
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn failed_thumbnail_cleanup_retains_recording_and_allows_fresh_retry() {
        let (root, store, name) = store();
        store.set_trashed(&name, true).unwrap();
        let plan = store.prepare_trash_deletion(None).unwrap();
        let cache = root.join(".redunar-thumbnails-v1");
        fs::write(&cache, b"invalid cache directory").unwrap();
        let result = store.delete_trashed(plan).unwrap();
        assert_eq!(result.deleted, [] as [String; 0]);
        assert_eq!(result.failures.len(), 1);
        assert!(result.failures[0].1.contains("recording retained"));
        assert_eq!(fs::read(root.join(&name)).unwrap(), MATROSKA_MAGIC);
        assert_eq!(store.trash_inventory().unwrap().len(), 1);
        fs::remove_file(cache).unwrap();
        let result = store
            .delete_trashed(store.prepare_trash_deletion(None).unwrap())
            .unwrap();
        assert_eq!(result.deleted, vec![name]);
        assert_eq!(result.failures, [] as [(String, String); 0]);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn malformed_bin_does_not_delete_recording() {
        let (root, store, name) = store();
        store.set_trashed(&name, true).unwrap();
        let plan = store.prepare_trash_deletion(None).unwrap();
        fs::write(root.join(FILE), "invalid").unwrap();
        assert!(store.delete_trashed(plan).is_err());
        assert!(root.join(name).exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn trash_restore_survives_reopen_with_filename_favorite_and_bytes() {
        let (root, store, name) = store();
        store.rename_clip(&name, "My clip.mkv").unwrap();
        store.set_favorite("My clip.mkv", true).unwrap();
        store.set_trashed("My clip.mkv", true).unwrap();
        store.set_trashed("My clip.mkv", true).unwrap();
        assert_eq!(
            store.inventory().unwrap(),
            [] as [crate::replay_store::ReplayClipEntry; 0]
        );
        assert!(store.rename_clip("My clip.mkv", "Other.mkv").is_err());
        let reopened = ReplayClipStore::open_with_limits(root.clone(), 1024, 8192).unwrap();
        let bin = reopened.trash_inventory().unwrap();
        assert_eq!(bin.len(), 1);
        assert!(bin[0].favorite);
        assert_eq!(bin[0].original_file_name, name);
        assert_eq!(fs::read(&bin[0].path).unwrap(), MATROSKA_MAGIC);
        assert_eq!(
            ReplayClipStore::inspect_with_limits(root.clone(), 1024, 8192)
                .unwrap()
                .used_bytes,
            4
        );
        reopened.set_trashed("My clip.mkv", false).unwrap();
        reopened.set_trashed("My clip.mkv", false).unwrap();
        assert_eq!(
            reopened.trash_inventory().unwrap(),
            [] as [crate::replay_store::ReplayClipEntry; 0]
        );
        assert!(reopened.inventory().unwrap()[0].favorite);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unsafe_targets_failed_writes_and_replaced_inodes_never_restore_foreign_media() {
        let (root, store, name) = store();
        for invalid in ["../foreign.mkv", "arbitrary.mkv", ""] {
            assert!(store.set_trashed(invalid, true).is_err());
        }
        fs::create_dir(root.join(FILE)).unwrap();
        assert!(store.set_trashed(&name, true).is_err());
        assert_eq!(fs::read(root.join(&name)).unwrap(), MATROSKA_MAGIC);
        fs::remove_dir(root.join(FILE)).unwrap();
        store.set_trashed(&name, true).unwrap();
        // Keep the old inode alive: replacing a name must not inherit its bin identity.
        fs::rename(root.join(&name), root.join("previous")).unwrap();
        symlink("previous", root.join(&name)).unwrap();
        assert!(store.set_trashed(&name, false).is_err());
        assert_eq!(
            store.trash_inventory().unwrap(),
            [] as [crate::replay_store::ReplayClipEntry; 0]
        );
        fs::remove_file(root.join(&name)).unwrap();
        fs::write(root.join(&name), MATROSKA_MAGIC).unwrap();
        assert_eq!(
            store.trash_inventory().unwrap(),
            [] as [crate::replay_store::ReplayClipEntry; 0]
        );
        assert_eq!(store.inventory().unwrap().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
