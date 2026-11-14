//! File boundaries for the private updater cache and installer snapshot.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

pub(crate) fn open_owned(path: &Path, uid: u32, limit: u64) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != uid
        || metadata.nlink() != 1
        || metadata.len() > limit
    {
        return Err(io::Error::other(
            "unsafe updater file ownership, type or size",
        ));
    }
    Ok(file)
}

pub(crate) fn read_metadata(path: &Path, limit: u64) -> io::Result<String> {
    let uid = unsafe { libc::geteuid() };
    let file = open_owned(path, uid, limit)?;
    let mut text = String::new();
    file.take(limit + 1).read_to_string(&mut text)?;
    if text.len() as u64 > limit {
        return Err(io::Error::other("updater metadata grew beyond its limit"));
    }
    Ok(text)
}

// The caller holds UPDATE_OPERATION. This reserved slot also permits recovery
// after an interrupted write without accumulating per-attempt cache files.
pub(crate) fn write_metadata(path: &Path, bytes: &[u8], limit: u64) -> io::Result<()> {
    if bytes.len() as u64 > limit {
        return Err(io::Error::other("updater metadata exceeds its limit"));
    }
    let temporary = path.with_file_name(".pending-update.txt.tmp");
    let mut file = create_private(&temporary, limit)?;
    let identity = file.metadata()?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        let current = fs::symlink_metadata(&temporary)?;
        if (identity.dev(), identity.ino()) != (current.dev(), current.ino()) {
            return Err(io::Error::other("pending updater file changed"));
        }
        fs::rename(&temporary, path)?;
        File::open(
            path.parent()
                .ok_or_else(|| io::Error::other("missing updater directory"))?,
        )?
        .sync_all()
    })();
    if result.is_err()
        && fs::symlink_metadata(&temporary)
            .is_ok_and(|m| (m.dev(), m.ino()) == (identity.dev(), identity.ino()))
    {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn create_private(path: &Path, limit: u64) -> io::Result<File> {
    let uid = unsafe { libc::geteuid() };
    if path.try_exists()? {
        let pending = open_owned(path, uid, limit)?;
        let opened = pending.metadata()?;
        let current = fs::symlink_metadata(path)?;
        if (opened.dev(), opened.ino()) != (current.dev(), current.ino()) {
            return Err(io::Error::other("pending updater file changed"));
        }
        fs::remove_file(path)?;
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

pub(crate) fn copy_private(source: &Path, temporary: &Path, limit: u64) -> io::Result<()> {
    let input = open_owned(source, unsafe { libc::geteuid() }, limit)?;
    let mut output = create_private(temporary, limit)?;
    let identity = output.metadata()?;
    let result = (|| {
        let copied = io::copy(&mut input.take(limit + 1), &mut output)?;
        if copied > limit {
            return Err(io::Error::other("updater input grew beyond its limit"));
        }
        output.sync_all()?;
        let current = fs::symlink_metadata(temporary)?;
        if (current.dev(), current.ino()) != (identity.dev(), identity.ino()) {
            return Err(io::Error::other("updater temporary file changed"));
        }
        Ok(())
    })();
    if result.is_err()
        && fs::symlink_metadata(temporary)
            .is_ok_and(|m| (m.dev(), m.ino()) == (identity.dev(), identity.ino()))
    {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "redunar-update-files-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn interrupted_metadata_write_recovers_and_commits_private_file() {
        let root = Fixture::new();
        let path = root.0.join("pending-update.txt");
        fs::write(&path, "previous").unwrap();
        fs::write(root.0.join(".pending-update.txt.tmp"), "interrupted").unwrap();
        write_metadata(&path, b"next", 32).unwrap();
        assert_eq!(read_metadata(&path, 32).unwrap(), "next");
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn linked_temporary_files_never_truncate_their_target() {
        for hard in [false, true] {
            let root = Fixture::new();
            let sentinel = root.0.join("sentinel");
            fs::write(&sentinel, "retain").unwrap();
            let temporary = root.0.join(".pending-update.txt.tmp");
            if hard {
                fs::hard_link(&sentinel, &temporary).unwrap();
            } else {
                symlink(&sentinel, &temporary).unwrap();
            }
            assert!(write_metadata(&root.0.join("pending-update.txt"), b"next", 32).is_err());
            assert_eq!(fs::read_to_string(sentinel).unwrap(), "retain");
            assert!(fs::symlink_metadata(temporary).is_ok());
        }
    }

    #[test]
    fn cache_reads_refuse_links_oversize_and_special_files() {
        let root = Fixture::new();
        let path = root.0.join("metadata");
        fs::write(&path, [b'x'; 33]).unwrap();
        assert!(read_metadata(&path, 32).is_err());
        let linked = root.0.join("linked");
        symlink(&path, &linked).unwrap();
        assert!(read_metadata(&linked, 64).is_err());
        fs::hard_link(&path, root.0.join("hard")).unwrap();
        assert!(read_metadata(&path, 64).is_err());
        assert!(read_metadata(&root.0, 64).is_err());
        let fifo = root.0.join("fifo");
        let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(read_metadata(&fifo, 64).is_err());
        assert!(open_owned(&fifo, unsafe { libc::geteuid() }, 64).is_err());
    }

    #[test]
    fn download_slots_reject_links_and_recover_interrupted_owned_copies() {
        let root = Fixture::new();
        let source = root.0.join("source");
        let target = root.0.join(".package.download");
        let sentinel = root.0.join("sentinel");
        fs::write(&source, "verified fixture").unwrap();
        fs::write(&sentinel, "retain").unwrap();
        symlink(&sentinel, &target).unwrap();
        assert!(copy_private(&source, &target, 64).is_err());
        assert_eq!(fs::read_to_string(&sentinel).unwrap(), "retain");
        fs::remove_file(&target).unwrap();
        fs::write(&target, "interrupted").unwrap();
        copy_private(&source, &target, 64).unwrap();
        assert_eq!(read_metadata(&target, 64).unwrap(), "verified fixture");
        assert!(copy_private(&source, &target, 4).is_err());
        assert_eq!(read_metadata(&target, 64).unwrap(), "verified fixture");
    }
}
