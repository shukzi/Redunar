//! Optional local artwork belongs to a verified catalog identity. Never accept
//! arbitrary file paths from the webview or fetch third-party assets online.
use redunar_core::GameMatchRule;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

static ARTWORK_STORE: Mutex<()> = Mutex::new(());
static NEXT_WRITE: AtomicU64 = AtomicU64::new(0);

fn store_directory() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|p| p.join(".local/share"))
        })
        .map(|p| p.join("redunar/artwork-v1"))
}

fn saved_path(store: &Path, app_id: u32, banner: bool) -> PathBuf {
    store.join(format!(
        "steam-{app_id}-{}.image",
        if banner { "banner" } else { "poster" }
    ))
}

fn preserve_raster(destination: &Path, source: Option<&Path>) -> Option<Vec<u8>> {
    if let Some(bytes) = read_raster(destination) {
        return Some(bytes);
    }
    let bytes = read_raster(source?)?;
    // Cache persistence is best effort; a full/unwritable disk must not break
    // display of valid local artwork. No existing copy is deleted on failure.
    let _ = write_copy(destination, &bytes);
    Some(bytes)
}

fn write_copy(destination: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = destination.parent().expect("artwork path has a parent");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    if !parent.symlink_metadata()?.file_type().is_dir() {
        return Err(std::io::Error::other("artwork store is not a directory"));
    }
    let temporary = parent.join(format!(
        ".artwork-{}-{}.tmp",
        std::process::id(),
        NEXT_WRITE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Remove only this app's copies after the last matching library record is removed.
pub fn remove_unused(app_id: u32) {
    let _guard = ARTWORK_STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Ok(catalog) = crate::backend::service().load_game_catalog() else {
        return;
    };
    if catalog
        .games
        .iter()
        .any(|game| crate::installation::steam_id(game) == Some(app_id))
    {
        return;
    }
    if let Some(store) = store_directory() {
        for banner in [false, true] {
            let _ = fs::remove_file(saved_path(&store, app_id, banner));
        }
    }
}

const MAX_IMAGE_BYTES: u64 = 1024 * 1024;

#[tauri::command]
pub async fn game_poster(game_id: String) -> Result<Option<Vec<u8>>, String> {
    tauri::async_runtime::spawn_blocking(move || read_artwork(&game_id, false))
        .await
        .map_err(|_| "Artwork worker unavailable".to_string())?
}

#[tauri::command]
pub async fn game_banner(game_id: String) -> Result<Option<Vec<u8>>, String> {
    tauri::async_runtime::spawn_blocking(move || read_artwork(&game_id, true))
        .await
        .map_err(|_| "Artwork worker unavailable".to_string())?
}

fn read_artwork(game_id: &str, banner: bool) -> Result<Option<Vec<u8>>, String> {
    let _guard = ARTWORK_STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let catalog = crate::backend::service()
        .load_game_catalog()
        .map_err(|e| e.to_string())?;
    let game = catalog
        .games
        .iter()
        .find(|game| game.id.get().to_string() == game_id)
        .ok_or("Unknown game")?;
    let Some(app_id) = game.match_rules.iter().find_map(|rule| match rule {
        GameMatchRule::SteamAppId(id) => Some(*id),
        _ => None,
    }) else {
        return Ok(None);
    };
    let discovery = std::env::var_os("HOME").map(redunar_platform::SteamGameDiscovery::new);
    let store = store_directory();
    let mut requested = None;
    // Preserve both independently on the first artwork request. A game need
    // not be selected for its landscape banner to survive an uninstall.
    for kind in [false, true] {
        let source = discovery.as_ref().and_then(|discovery| {
            if kind {
                discovery.local_banner(app_id)
            } else {
                discovery.local_poster(app_id)
            }
        });
        let bytes = if let Some(store) = &store {
            preserve_raster(&saved_path(store, app_id, kind), source.as_deref())
        } else {
            source.as_deref().and_then(read_raster)
        };
        if kind == banner {
            requested = bytes;
        }
    }
    Ok(requested)
}

fn read_raster(path: &std::path::Path) -> Option<Vec<u8>> {
    let Ok(file) = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    else {
        return None;
    };
    let Ok(metadata) = file.metadata() else {
        return None;
    };
    if !metadata.is_file() || metadata.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    if file
        .take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() > MAX_IMAGE_BYTES as usize
    {
        return None;
    }
    if !supported_image(&bytes) {
        return None;
    }
    Some(bytes)
}

fn supported_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.starts_with(b"\xff\xd8\xff")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separate_local_copies_survive_source_removal_and_bad_cache_recovers() {
        let root =
            std::env::temp_dir().join(format!("redunar-artwork-copies-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("steam-poster.jpg");
        let hero = root.join("steam-hero.png");
        let poster_bytes = b"\xff\xd8\xffportrait";
        let hero_bytes = b"\x89PNG\r\n\x1a\nlandscape";
        fs::write(&source, poster_bytes).unwrap();
        fs::write(&hero, hero_bytes).unwrap();
        let store = root.join("redunar/artwork-v1");
        let poster = saved_path(&store, 42, false);
        let banner = saved_path(&store, 42, true);
        assert_eq!(
            preserve_raster(&poster, Some(&source)).unwrap(),
            poster_bytes
        );
        assert_eq!(preserve_raster(&banner, Some(&hero)).unwrap(), hero_bytes);
        fs::remove_file(&source).unwrap();
        fs::remove_file(&hero).unwrap();
        // Fresh reads use only persisted files, as after an app restart/cache clear.
        assert_eq!(preserve_raster(&poster, None).unwrap(), poster_bytes);
        assert_eq!(preserve_raster(&banner, None).unwrap(), hero_bytes);
        assert!(preserve_raster(&saved_path(&store, 43, true), None).is_none());
        fs::write(&banner, b"corrupt").unwrap();
        assert!(preserve_raster(&banner, None).is_none());
        fs::write(&hero, hero_bytes).unwrap();
        assert_eq!(preserve_raster(&banner, Some(&hero)).unwrap(), hero_bytes);
        assert_eq!(read_raster(&banner).unwrap(), hero_bytes);
        // An unwritable destination never prevents valid Steam artwork display.
        let blocked = root.join("file-not-directory");
        fs::write(&blocked, b"blocked").unwrap();
        assert_eq!(
            preserve_raster(&blocked.join("banner"), Some(&hero)).unwrap(),
            hero_bytes
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn artwork_reads_are_bounded_and_do_not_follow_symlinks() {
        use std::os::unix::fs::symlink;
        let directory = std::env::temp_dir().join(format!(
            "redunar-artwork-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("poster.jpg");
        std::fs::write(&path, b"\xff\xd8\xfftest").unwrap();
        assert_eq!(read_raster(&path), Some(b"\xff\xd8\xfftest".to_vec()));
        let link = directory.join("linked.jpg");
        symlink(&path, &link).unwrap();
        assert_eq!(read_raster(&link), None);
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_IMAGE_BYTES + 1)
            .unwrap();
        assert_eq!(read_raster(&path), None);
        std::fs::write(&path, b"<svg onload='script'>").unwrap();
        assert_eq!(read_raster(&path), None);
        assert_eq!(read_raster(&directory.join("missing.jpg")), None);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
