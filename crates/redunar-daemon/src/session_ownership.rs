//! Process-lifetime ownership, acquired before startup cleanup or any writes.
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

#[derive(Debug)]
pub(crate) struct SessionLease {
    _file: File,
}

impl SessionLease {
    pub(crate) fn acquire(state: &Path) -> io::Result<Option<Self>> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(state)?;
        let directory = fs::symlink_metadata(state)?;
        if !directory.is_dir() {
            return Err(io::Error::other("backend state is not a directory"));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(state.join("backend-owner-v1.lock"))?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.uid() != directory.uid() || metadata.nlink() != 1 {
            return Err(io::Error::other("backend ownership file is unsafe"));
        }
        match file.try_lock() {
            Ok(()) => {
                file.set_permissions(fs::Permissions::from_mode(0o600))?;
                // Never unlink a lock file: another process may already have
                // opened its inode. The kernel releases ownership on exit.
                Ok(Some(Self { _file: file }))
            }
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn concurrent_start_has_one_owner_and_releases_without_unlinking() {
        let root = std::env::temp_dir().join(format!("redunar-owner-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let barrier = Arc::new(Barrier::new(8));
        let threads = (0..8)
            .map(|_| {
                let root = root.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let lease = SessionLease::acquire(&root).unwrap();
                    barrier.wait();
                    lease
                })
            })
            .collect::<Vec<_>>();
        let leases = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(leases.iter().filter(|lease| lease.is_some()).count(), 1);
        assert!(SessionLease::acquire(&root).unwrap().is_none());
        let inode = fs::metadata(root.join("backend-owner-v1.lock"))
            .unwrap()
            .ino();
        drop(leases);
        let lease = SessionLease::acquire(&root).unwrap().unwrap();
        assert_eq!(
            fs::metadata(root.join("backend-owner-v1.lock"))
                .unwrap()
                .ino(),
            inode
        );
        drop(lease);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_symlink_without_touching_target() {
        let root = std::env::temp_dir().join(format!("redunar-owner-link-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let target = root.join("unrelated");
        fs::write(&target, "preserve").unwrap();
        std::os::unix::fs::symlink(&target, root.join("backend-owner-v1.lock")).unwrap();
        assert!(SessionLease::acquire(&root).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "preserve");
        fs::remove_dir_all(root).unwrap();
    }
}
