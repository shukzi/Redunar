//! Short-lived inventory presentation cache; never authorizes an open/delete.
use crate::ReplayClipEntry;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    dev: u64,
    ino: u64,
    bytes: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl Stamp {
    fn read(path: &Path) -> Option<Self> {
        let meta = fs::symlink_metadata(path).ok()?;
        Some(Self {
            dev: meta.dev(),
            ino: meta.ino(),
            bytes: meta.len(),
            modified: (meta.mtime(), meta.mtime_nsec()),
            changed: (meta.ctime(), meta.ctime_nsec()),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InventoryStamp {
    directory: PathBuf,
    stamps: [Option<Stamp>; 3],
}
impl InventoryStamp {
    pub(crate) fn read(directory: &Path, state: &Path) -> Self {
        Self {
            directory: directory.to_owned(),
            stamps: [
                Stamp::read(directory),
                Stamp::read(&state.join("replay-clip-games-v1.tsv")),
                Stamp::read(&state.join("session-history-v1.tsv")),
            ],
        }
    }
}
#[derive(Default)]
pub(crate) struct InventoryCache {
    entry: Option<(InventoryStamp, Instant, Vec<ReplayClipEntry>)>,
}
impl InventoryCache {
    pub(crate) fn get(&self, stamp: &InventoryStamp) -> Option<Vec<ReplayClipEntry>> {
        self.entry
            .as_ref()
            .filter(|(key, time, _)| key == stamp && time.elapsed() < Duration::from_secs(1))
            .map(|(_, _, clips)| clips.clone())
    }
    pub(crate) fn put(&mut self, stamp: InventoryStamp, clips: Vec<ReplayClipEntry>) {
        self.entry = Some((stamp, Instant::now(), clips));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_directory_attribution_and_expiry_invalidate() {
        let root =
            std::env::temp_dir().join(format!("redunar-inventory-cache-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let clips = root.join("clips");
        fs::create_dir_all(&clips).unwrap();
        let mut cache = InventoryCache::default();
        let original = InventoryStamp::read(&clips, &root);
        cache.put(original.clone(), vec![]);
        assert!(cache.get(&original).is_some());
        fs::write(root.join("replay-clip-games-v1.tsv"), "new attribution").unwrap();
        assert!(cache.get(&InventoryStamp::read(&clips, &root)).is_none());
        fs::rename(&clips, root.join("old-clips")).unwrap();
        fs::create_dir(&clips).unwrap();
        assert!(cache.get(&InventoryStamp::read(&clips, &root)).is_none());
        cache.entry.as_mut().unwrap().1 =
            Instant::now().checked_sub(Duration::from_secs(2)).unwrap();
        assert!(cache.get(&original).is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
