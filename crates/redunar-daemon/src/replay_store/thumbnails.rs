//! Optional store-local JPEGs. No media decoding or arbitrary paths live here.
use super::{
    MAX_DIRECTORY_ENTRIES, ReplayClipStore, ReplayStoreError, io_error, owned_metadata,
    store_operation, trash, validate_store,
};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const DIRECTORY: &str = ".redunar-thumbnails-v1";
const MAX_BYTES: usize = 256 * 1024;
const MAX_ENTRIES: usize = 256;

fn prefix(meta: &fs::Metadata) -> String {
    format!("{:x}-{:x}-", meta.dev(), meta.ino())
}
fn key(meta: &fs::Metadata) -> String {
    format!(
        "{}{:x}-{:x}-{:x}.jpg",
        prefix(meta),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec()
    )
}
fn valid_name(name: &str) -> bool {
    name.strip_suffix(".jpg").is_some_and(|stem| {
        let parts: Vec<_> = stem.split('-').collect();
        parts.len() == 5
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_hexdigit()))
    })
}
fn jpeg(bytes: &[u8]) -> bool {
    (4..=MAX_BYTES).contains(&bytes.len())
        && bytes.starts_with(&[0xff, 0xd8])
        && bytes.ends_with(&[0xff, 0xd9])
}
// Pin the private directory with a descriptor: subsequent operations cannot
// follow a substituted symlink. The application is Linux-only.
struct Directory(File);
impl Directory {
    fn path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.0.as_raw_fd()))
    }
    fn open(store: &Path, create: bool) -> io::Result<Option<Self>> {
        let path = store.join(DIRECTORY);
        if create {
            use std::os::unix::fs::DirBuilderExt;
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => (),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(e),
            }
        }
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let meta = file.metadata()?;
        if meta.uid() != fs::metadata(store)?.uid() || meta.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::other("Thumbnail cache is not private"));
        }
        Ok(Some(Self(file)))
    }
    fn entries(&self) -> io::Result<Vec<(String, fs::Metadata)>> {
        let mut entries = Vec::new();
        for (i, entry) in fs::read_dir(self.path())?.enumerate() {
            if i >= MAX_DIRECTORY_ENTRIES {
                return Err(io::Error::other("Thumbnail cache has too many entries"));
            }
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if valid_name(&name) || name.strip_suffix(".tmp").is_some_and(valid_name) {
                entries.push((name, fs::symlink_metadata(entry.path())?));
            }
        }
        Ok(entries)
    }
}

impl ReplayClipStore {
    /// Read an optional thumbnail only for the exact currently owned live clip.
    /// # Errors
    /// Rejects changed clips, unsafe storage and inaccessible cache files.
    pub fn cached_thumbnail(
        &self,
        name: &str,
        expected: &fs::Metadata,
    ) -> Result<Option<Vec<u8>>, ReplayStoreError> {
        let _operation = store_operation();
        validate_store(&self.directory)?;
        let (meta, _) = owned_metadata(&self.directory, name)?;
        trash::require_live(&self.directory, name, &meta)?;
        if key(&meta) != key(expected) {
            return Ok(None);
        }
        let read = || -> io::Result<Option<Vec<u8>>> {
            let Some(directory) = Directory::open(&self.directory, false)? else {
                return Ok(None);
            };
            let file = match OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(directory.path().join(key(&meta)))
            {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e),
            };
            let cached = file.metadata()?;
            if !cached.is_file()
                || cached.nlink() != 1
                || cached.uid() != meta.uid()
                || cached.len() > MAX_BYTES as u64
            {
                return Ok(None);
            }
            let mut bytes = Vec::new();
            file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes)?;
            Ok(jpeg(&bytes).then_some(bytes))
        };
        read().map_err(|e| io_error("could not read clip thumbnail cache", &e))
    }

    /// Commit regenerable JPEG bytes after decoding, rechecking clip identity.
    /// # Errors
    /// Rejects late results for changed/deleted/trashed clips and unsafe caches.
    pub fn cache_thumbnail(
        &self,
        name: &str,
        expected: &fs::Metadata,
        bytes: &[u8],
    ) -> Result<(), ReplayStoreError> {
        let _operation = store_operation();
        validate_store(&self.directory)?;
        let (meta, _) = owned_metadata(&self.directory, name)?;
        trash::require_live(&self.directory, name, &meta)?;
        if key(&meta) != key(expected) || !jpeg(bytes) {
            return Err(ReplayStoreError::new(
                "Clip changed or thumbnail is invalid",
            ));
        }
        let write = || -> io::Result<()> {
            let directory = Directory::open(&self.directory, true)?
                .ok_or_else(|| io::Error::other("Thumbnail cache unavailable"))?;
            let path = directory.path();
            let mut entries = Vec::new();
            // Drop superseded versions of this inode; never touch media files.
            for (name, metadata) in directory.entries()? {
                if name.starts_with(&prefix(&meta)) || name.strip_suffix(".tmp").is_some() {
                    fs::remove_file(path.join(name))?;
                } else {
                    entries.push((name, metadata));
                }
            }
            entries.sort_by_key(|(_, metadata)| metadata.modified().unwrap_or(UNIX_EPOCH));
            while entries.len() >= MAX_ENTRIES {
                let (name, _) = entries.remove(0);
                fs::remove_file(path.join(name))?;
            }
            let destination = path.join(key(&meta));
            let temporary_path = path.join(format!("{}.tmp", key(&meta)));
            let mut temporary = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary_path)?;
            let result = temporary
                .write_all(bytes)
                .and_then(|()| fs::rename(&temporary_path, destination));
            let _ = fs::remove_file(temporary_path);
            result
        };
        write().map_err(|e| io_error("could not cache clip thumbnail", &e))
    }
}

pub(super) fn remove(store: &Path, meta: &fs::Metadata) -> Result<(), ReplayStoreError> {
    let cleanup = || -> io::Result<()> {
        let Some(directory) = Directory::open(store, false)? else {
            return Ok(());
        };
        for (name, _) in directory.entries()? {
            if name.starts_with(&prefix(meta)) {
                fs::remove_file(directory.path().join(name))?;
            }
        }
        directory.0.sync_all()?;
        Ok(())
    };
    cleanup().map_err(|e| {
        io_error(
            "could not clear clip thumbnail cache; recording retained",
            &e,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::super::{MATROSKA_MAGIC, NEXT_FILE, Ordering};
    use super::*;
    const JPEG: &[u8] = &[0xff, 0xd8, 1, 2, 0xff, 0xd9];
    struct Fixture {
        store: ReplayClipStore,
        root: PathBuf,
        name: String,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "redunar-thumbnails-{}-{}",
                std::process::id(),
                NEXT_FILE.fetch_add(1, Ordering::Relaxed)
            ));
            let store = ReplayClipStore::open_with_limits(root.clone(), 1024, 8192).unwrap();
            let clip = store.save_matroska(&mut MATROSKA_MAGIC.as_slice()).unwrap();
            Self {
                name: clip.path.file_name().unwrap().to_str().unwrap().to_owned(),
                root,
                store,
            }
        }
        fn meta(&self) -> fs::Metadata {
            fs::metadata(self.root.join(&self.name)).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn reuse_survives_reopen_rename_trash_restore_and_permanent_delete() {
        let mut f = Fixture::new();
        let meta = f.meta();
        assert!(f.store.cached_thumbnail(&f.name, &meta).unwrap().is_none());
        f.store.cache_thumbnail(&f.name, &meta, JPEG).unwrap();
        f.store = ReplayClipStore::open_with_limits(f.root.clone(), 1024, 8192).unwrap();
        assert_eq!(
            f.store.cached_thumbnail(&f.name, &meta).unwrap().unwrap(),
            JPEG
        );
        f.store.rename_clip(&f.name, "Renamed.mkv").unwrap();
        f.name = "Renamed.mkv".into();
        assert_eq!(
            f.store
                .cached_thumbnail(&f.name, &f.meta())
                .unwrap()
                .unwrap(),
            JPEG
        );
        f.store.set_trashed(&f.name, true).unwrap();
        assert!(f.store.cache_thumbnail(&f.name, &meta, JPEG).is_err());
        assert!(f.root.join(DIRECTORY).join(key(&meta)).exists());
        f.store.set_trashed(&f.name, false).unwrap();
        assert_eq!(
            f.store
                .cached_thumbnail(&f.name, &f.meta())
                .unwrap()
                .unwrap(),
            JPEG
        );
        f.store.set_trashed(&f.name, true).unwrap();
        f.store.delete_clip(&f.name).unwrap();
        assert!(
            fs::read_dir(f.root.join(DIRECTORY))
                .unwrap()
                .next()
                .is_none()
        );
        assert!(f.store.cache_thumbnail(&f.name, &meta, JPEG).is_err());
    }
    #[test]
    fn changed_clip_corrupt_cache_and_failed_cache_writes_leave_media_usable() {
        let f = Fixture::new();
        let before = f.meta();
        f.store.cache_thumbnail(&f.name, &before, JPEG).unwrap();
        fs::write(f.root.join(DIRECTORY).join(key(&before)), b"corrupt").unwrap();
        assert!(
            f.store
                .cached_thumbnail(&f.name, &before)
                .unwrap()
                .is_none()
        );
        fs::write(
            f.root.join(&f.name),
            [MATROSKA_MAGIC.as_slice(), b"changed"].concat(),
        )
        .unwrap();
        assert!(
            f.store
                .cached_thumbnail(&f.name, &f.meta())
                .unwrap()
                .is_none()
        );
        assert!(f.store.cache_thumbnail(&f.name, &before, JPEG).is_err());
        assert!(
            f.store
                .cache_thumbnail(&f.name, &f.meta(), &vec![0; MAX_BYTES + 1])
                .is_err()
        );
        assert_eq!(f.store.inventory().unwrap().len(), 1);
        f.store.delete_clip(&f.name).unwrap();
        assert!(
            fs::read_dir(f.root.join(DIRECTORY))
                .unwrap()
                .next()
                .is_none()
        );
    }
    #[test]
    fn unsafe_cache_never_follows_symlinks_and_cleanup_failure_retains_recording() {
        let f = Fixture::new();
        let meta = f.meta();
        std::os::unix::fs::symlink(&f.root, f.root.join(DIRECTORY)).unwrap();
        assert!(f.store.cache_thumbnail(&f.name, &meta, JPEG).is_err());
        assert!(f.store.delete_clip(&f.name).is_err());
        assert!(f.root.join(&f.name).exists());
        fs::remove_file(f.root.join(DIRECTORY)).unwrap();
        f.store.cache_thumbnail(&f.name, &meta, JPEG).unwrap();
        let cache = f.root.join(DIRECTORY).join(key(&meta));
        fs::remove_file(&cache).unwrap();
        std::os::unix::fs::symlink(f.root.join(&f.name), &cache).unwrap();
        assert!(f.store.cached_thumbnail(&f.name, &meta).is_err());
        f.store.delete_clip(&f.name).unwrap();
        assert!(!cache.exists());
    }
    #[test]
    fn bounded_private_cache_evicts_only_cached_images() {
        let f = Fixture::new();
        let meta = f.meta();
        f.store.cache_thumbnail(&f.name, &meta, JPEG).unwrap();
        let directory = f.root.join(DIRECTORY);
        for i in 0..MAX_ENTRIES {
            fs::write(directory.join(format!("0-{i:x}-4-0-0.jpg")), JPEG).unwrap();
        }
        f.store.cache_thumbnail(&f.name, &meta, JPEG).unwrap();
        assert_eq!(fs::read_dir(&directory).unwrap().count(), MAX_ENTRIES);
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(directory.join(key(&meta)))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(f.store.inventory().unwrap().len(), 1);
    }
}
