use super::ClipMetadata;
use std::{collections::VecDeque, fs::Metadata, os::unix::fs::MetadataExt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Identity {
    device: u64,
    inode: u64,
    bytes: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl Identity {
    pub(super) fn new(meta: &Metadata) -> Self {
        Self {
            device: meta.dev(),
            inode: meta.ino(),
            bytes: meta.len(),
            modified: (meta.mtime(), meta.mtime_nsec()),
            changed: (meta.ctime(), meta.ctime_nsec()),
        }
    }
}
#[derive(Default)]
pub(super) struct Cache {
    entries: VecDeque<(Identity, ClipMetadata, bool)>,
}
impl Cache {
    pub(super) fn get(&self, identity: &Identity, detailed: bool) -> Option<ClipMetadata> {
        self.entries
            .iter()
            .find(|(key, _, complete)| key == identity && (!detailed || *complete))
            .map(|(_, value, _)| value.clone())
    }
    pub(super) fn put(&mut self, identity: Identity, value: ClipMetadata, detailed: bool) {
        if self.get(&identity, true).is_some() && !detailed {
            return;
        }
        self.entries.retain(|(key, _, _)| key != &identity);
        self.entries.push_back((identity, value, detailed));
        while self.entries.len() > 64 {
            self.entries.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn details_upgrade_without_downgrade_and_changed_file_misses() {
        let path =
            std::env::temp_dir().join(format!("redunar-metadata-cache-{}", std::process::id()));
        std::fs::write(&path, b"original").unwrap();
        let key = Identity::new(&std::fs::metadata(&path).unwrap());
        let mut cache = Cache::default();
        cache.put(key.clone(), ClipMetadata::default(), false);
        assert!(cache.get(&key, false).is_some());
        assert!(cache.get(&key, true).is_none());
        cache.put(
            key.clone(),
            ClipMetadata {
                fps: Some(60.0),
                ..ClipMetadata::default()
            },
            true,
        );
        cache.put(key.clone(), ClipMetadata::default(), false);
        assert_eq!(cache.get(&key, true).unwrap().fps, Some(60.0));
        std::fs::write(&path, b"changed").unwrap();
        assert!(cache
            .get(&Identity::new(&std::fs::metadata(&path).unwrap()), false)
            .is_none());
        for inode in 0..100 {
            cache.put(
                Identity {
                    inode,
                    ..key.clone()
                },
                ClipMetadata::default(),
                false,
            );
        }
        assert_eq!(cache.entries.len(), 64);
        std::fs::remove_file(path).unwrap();
    }
}
