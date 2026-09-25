use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

const PREFERENCES_FILE: &str = "app-preferences-v1.txt";
const PREFERENCES_HEADER_V1: &str = "redunar-app-preferences-v1";
const PREFERENCES_HEADER_V2: &str = "redunar-app-preferences-v2";
const PREFERENCES_HEADER_V3: &str = "redunar-app-preferences-v3";
const PREFERENCES_HEADER_V4: &str = "redunar-app-preferences-v4";
const PREFERENCES_HEADER_V5: &str = "redunar-app-preferences-v5";
const PREFERENCES_HEADER_V6: &str = "redunar-app-preferences-v6";
const PREFERENCES_HEADER_V7: &str = "redunar-app-preferences-v7";
const PREFERENCES_HEADER_V8: &str = "redunar-app-preferences-v8";
const MAX_PREFERENCES_BYTES: u64 = 1_024;
const MIN_WINDOW_WIDTH: i32 = 800;
const MAX_WINDOW_WIDTH: i32 = 7_680;
const MIN_WINDOW_HEIGHT: i32 = 600;
const MAX_WINDOW_HEIGHT: i32 = 4_320;
static NEXT_TEMPORARY_FILE: AtomicU64 = AtomicU64::new(0);
static PREFERENCES_OPERATIONS: Mutex<()> = Mutex::new(());

/// Daemon-owned application behavior that is independent of game profiles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each persisted preference is an independent user-facing toggle; grouping them would obscure the saved format"
)]
pub struct AppPreferences {
    /// Hide a closed window only while a desktop tray item is confirmed live.
    pub close_to_tray: bool,
    /// Check signed release metadata automatically. Installation always
    /// remains an explicit user action owned by the native updater.
    pub automatic_updates: bool,
    pub window_width: i32,
    pub window_height: i32,
    pub window_maximized: bool,
    /// Older files saved physical pixels; Tauri converts them once on restore.
    /// Keep this bit across unrelated preference writes until geometry is saved.
    pub window_size_is_physical: bool,
    /// Opt-in diagnostic logging of Redunar's operational messages to a
    /// bounded private file. Off by default; the file may contain game names,
    /// session identifiers, and failure detail useful for debugging.
    pub diagnostic_log: bool,
    /// Voluntary access to experimental features present in this installed
    /// build. This is a preference, never proof of hardware capability.
    pub beta_access: bool,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            close_to_tray: false,
            automatic_updates: true,
            // The approved interface is composed for a 1280x720 logical
            // workspace. HiDPI desktops still render this at their native
            // scale (for example 2560x1440 at 2x) without making the controls
            // themselves twice as spacious.
            window_width: 1_280,
            window_height: 720,
            window_maximized: false,
            window_size_is_physical: false,
            diagnostic_log: false,
            beta_access: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppPreferencesError {
    message: String,
}

impl AppPreferencesError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for AppPreferencesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for AppPreferencesError {}

pub(crate) fn load(state_directory: &Path) -> Result<AppPreferences, AppPreferencesError> {
    let _operation = lock_operations();
    load_unlocked(state_directory)
}

pub(crate) fn set_close_to_tray(
    state_directory: &Path,
    enabled: bool,
) -> Result<AppPreferences, AppPreferencesError> {
    let _operation = lock_operations();
    let mut preferences = load_unlocked(state_directory)?;
    preferences.close_to_tray = enabled;
    save_unlocked(state_directory, preferences)?;
    Ok(preferences)
}

pub(crate) fn set_automatic_updates(
    state_directory: &Path,
    enabled: bool,
) -> Result<AppPreferences, AppPreferencesError> {
    let _operation = lock_operations();
    let mut preferences = load_unlocked(state_directory)?;
    preferences.automatic_updates = enabled;
    save_unlocked(state_directory, preferences)?;
    Ok(preferences)
}

pub(crate) fn set_diagnostic_log(
    state_directory: &Path,
    enabled: bool,
) -> Result<AppPreferences, AppPreferencesError> {
    let _operation = lock_operations();
    let mut preferences = load_unlocked(state_directory)?;
    preferences.diagnostic_log = enabled;
    save_unlocked(state_directory, preferences)?;
    Ok(preferences)
}

pub(crate) fn set_beta_access(
    state_directory: &Path,
    enabled: bool,
) -> Result<AppPreferences, AppPreferencesError> {
    let _operation = lock_operations();
    let mut preferences = load_unlocked(state_directory)?;
    preferences.beta_access = enabled;
    save_unlocked(state_directory, preferences)?;
    Ok(preferences)
}

pub(crate) fn set_window_state(
    state_directory: &Path,
    width: i32,
    height: i32,
    maximized: bool,
) -> Result<AppPreferences, AppPreferencesError> {
    if !(MIN_WINDOW_WIDTH..=MAX_WINDOW_WIDTH).contains(&width)
        || !(MIN_WINDOW_HEIGHT..=MAX_WINDOW_HEIGHT).contains(&height)
    {
        return Err(AppPreferencesError::new(
            "window size is outside Redunar's supported bounds",
        ));
    }
    let _operation = lock_operations();
    let mut preferences = load_unlocked(state_directory)?;
    preferences.window_width = width;
    preferences.window_height = height;
    preferences.window_maximized = maximized;
    preferences.window_size_is_physical = false;
    save_unlocked(state_directory, preferences)?;
    Ok(preferences)
}

fn load_unlocked(state_directory: &Path) -> Result<AppPreferences, AppPreferencesError> {
    let path = state_directory.join(PREFERENCES_FILE);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(AppPreferences::default());
        }
        Err(error) => return Err(load_error(&path, &error)),
    };
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(AppPreferencesError::new(format!(
            "could not load app preferences from {}: state is not a regular file",
            path.display()
        )));
    }
    if metadata.len() > MAX_PREFERENCES_BYTES {
        return Err(AppPreferencesError::new(format!(
            "could not load app preferences from {}: state exceeds {MAX_PREFERENCES_BYTES} bytes",
            path.display()
        )));
    }
    let mut contents = String::new();
    File::open(&path)
        .and_then(|file| {
            file.take(MAX_PREFERENCES_BYTES + 1)
                .read_to_string(&mut contents)
        })
        .map_err(|error| load_error(&path, &error))?;
    if contents.len() as u64 > MAX_PREFERENCES_BYTES {
        return Err(AppPreferencesError::new(format!(
            "could not load app preferences from {}: state exceeds {MAX_PREFERENCES_BYTES} bytes",
            path.display()
        )));
    }
    parse(&contents).map_err(|message| {
        AppPreferencesError::new(format!(
            "could not load app preferences from {}: {message}",
            path.display()
        ))
    })
}

fn save_unlocked(
    state_directory: &Path,
    preferences: AppPreferences,
) -> Result<(), AppPreferencesError> {
    fs::create_dir_all(state_directory).map_err(|error| {
        AppPreferencesError::new(format!(
            "could not create Redunar state directory {}: {error}",
            state_directory.display()
        ))
    })?;
    fs::set_permissions(state_directory, fs::Permissions::from_mode(0o700)).map_err(|error| {
        AppPreferencesError::new(format!(
            "could not make Redunar state directory private at {}: {error}",
            state_directory.display()
        ))
    })?;
    let path = state_directory.join(PREFERENCES_FILE);
    atomic_replace(&path, serialize(preferences).as_bytes()).map_err(|error| {
        AppPreferencesError::new(format!(
            "could not save app preferences to {}: {error}",
            path.display()
        ))
    })
}

fn serialize(preferences: AppPreferences) -> String {
    format!(
        "{PREFERENCES_HEADER_V8}\nclose-to-tray={}\nwindow-width={}\nwindow-height={}\nwindow-maximized={}\nautomatic-updates={}\nwindow-size-units={}\ndiagnostic-log={}\nbeta-access={}\n",
        if preferences.close_to_tray {
            "on"
        } else {
            "off"
        },
        preferences.window_width,
        preferences.window_height,
        if preferences.window_maximized {
            "on"
        } else {
            "off"
        },
        if preferences.automatic_updates {
            "on"
        } else {
            "off"
        },
        if preferences.window_size_is_physical {
            "physical"
        } else {
            "logical"
        },
        if preferences.diagnostic_log {
            "on"
        } else {
            "off"
        },
        if preferences.beta_access { "on" } else { "off" },
    )
}

fn parse(contents: &str) -> Result<AppPreferences, &'static str> {
    let mut lines = contents.lines();
    let version = lines.next().ok_or("preferences are empty")?;
    let (name, value) = lines
        .next()
        .and_then(|line| line.split_once('='))
        .ok_or("preferences are missing the close-to-tray field")?;
    if name != "close-to-tray" {
        return Err("preferences contain missing or reordered fields");
    }
    let close_to_tray = match value {
        "on" => true,
        "off" => false,
        _ => return Err("preferences contain an invalid switch value"),
    };
    if version == PREFERENCES_HEADER_V1 {
        if lines.next().is_some() {
            return Err("preferences contain unknown fields");
        }
        return Ok(AppPreferences {
            close_to_tray,
            ..AppPreferences::default()
        });
    }
    if version != PREFERENCES_HEADER_V2
        && version != PREFERENCES_HEADER_V3
        && version != PREFERENCES_HEADER_V4
        && version != PREFERENCES_HEADER_V5
        && version != PREFERENCES_HEADER_V6
        && version != PREFERENCES_HEADER_V7
        && version != PREFERENCES_HEADER_V8
    {
        return Err("unsupported preferences version");
    }
    let width = parse_i32(lines.next(), "window-width")?;
    let height = parse_i32(lines.next(), "window-height")?;
    let maximized = parse_bool(lines.next(), "window-maximized")?;
    // Version 3 carried one compatibility boolean after the window fields.
    // Accept and discard it without carrying that setting into current state.
    if version == PREFERENCES_HEADER_V3 {
        parse_compatibility_bool(lines.next())?;
    }
    let automatic_updates = if version == PREFERENCES_HEADER_V5
        || version == PREFERENCES_HEADER_V6
        || version == PREFERENCES_HEADER_V7
        || version == PREFERENCES_HEADER_V8
    {
        parse_bool(lines.next(), "automatic-updates")?
    } else {
        true
    };
    let window_size_is_physical = if version == PREFERENCES_HEADER_V6
        || version == PREFERENCES_HEADER_V7
        || version == PREFERENCES_HEADER_V8
    {
        match lines.next() {
            Some("window-size-units=physical") => true,
            Some("window-size-units=logical") => false,
            _ => return Err("preferences contain invalid window size units"),
        }
    } else {
        true
    };
    let diagnostic_log = if version == PREFERENCES_HEADER_V7 || version == PREFERENCES_HEADER_V8 {
        parse_bool(lines.next(), "diagnostic-log")?
    } else {
        false
    };
    let beta_access = if version == PREFERENCES_HEADER_V8 {
        parse_bool(lines.next(), "beta-access")?
    } else {
        false
    };
    if lines.next().is_some()
        || !(MIN_WINDOW_WIDTH..=MAX_WINDOW_WIDTH).contains(&width)
        || !(MIN_WINDOW_HEIGHT..=MAX_WINDOW_HEIGHT).contains(&height)
    {
        return Err("preferences contain unknown fields or an invalid window size");
    }
    Ok(AppPreferences {
        close_to_tray,
        automatic_updates,
        window_width: width,
        window_height: height,
        window_maximized: maximized,
        window_size_is_physical,
        diagnostic_log,
        beta_access,
    })
}

fn parse_compatibility_bool(line: Option<&str>) -> Result<(), &'static str> {
    let (_, value) = line
        .and_then(|line| line.split_once('='))
        .ok_or("preferences are missing a version 3 compatibility field")?;
    match value {
        "on" | "off" => Ok(()),
        _ => Err("preferences contain an invalid switch value"),
    }
}

fn parse_i32(line: Option<&str>, expected: &str) -> Result<i32, &'static str> {
    let (name, value) = line
        .and_then(|line| line.split_once('='))
        .ok_or("preferences are missing a window field")?;
    if name != expected {
        return Err("preferences contain missing or reordered fields");
    }
    value
        .parse()
        .map_err(|_| "preferences contain an invalid window size")
}

fn parse_bool(line: Option<&str>, expected: &str) -> Result<bool, &'static str> {
    let (name, value) = line
        .and_then(|line| line.split_once('='))
        .ok_or("preferences are missing a boolean field")?;
    if name != expected {
        return Err("preferences contain missing or reordered fields");
    }
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err("preferences contain an invalid switch value"),
    }
}

fn atomic_replace(destination: &Path, contents: &[u8]) -> io::Result<()> {
    let directory = destination
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "state path has no parent"))?;
    let mut last_collision = None;
    for _ in 0..32 {
        let id = NEXT_TEMPORARY_FILE.fetch_add(1, Ordering::Relaxed);
        let temporary = directory.join(format!(
            ".{PREFERENCES_FILE}.{}.{id}.tmp",
            std::process::id()
        ));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                last_collision = Some(error);
                continue;
            }
            Err(error) => return Err(error),
        };
        let result = (|| {
            file.write_all(contents)?;
            file.sync_all()?;
            fs::rename(&temporary, destination)?;
            File::open(directory)?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        return result;
    }
    Err(last_collision.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a temporary app preferences file",
        )
    }))
}

fn load_error(path: &Path, error: &io::Error) -> AppPreferencesError {
    AppPreferencesError::new(format!(
        "could not load app preferences from {}: {error}",
        path.display()
    ))
}

fn lock_operations() -> MutexGuard<'static, ()> {
    PREFERENCES_OPERATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> std::path::PathBuf {
        let id = NEXT_TEMPORARY_FILE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "redunar-app-preferences-{}-{id}",
            std::process::id()
        ))
    }

    #[test]
    fn application_preferences_use_safe_defaults_and_round_trip_privately() {
        let root = fixture();
        assert_eq!(
            load(&root).expect("default preferences"),
            AppPreferences::default()
        );
        let updated = set_close_to_tray(&root, true).expect("enable close to tray");
        assert!(updated.close_to_tray);
        assert!(updated.automatic_updates);
        let updated = set_automatic_updates(&root, false).expect("disable automatic updates");
        assert!(!updated.automatic_updates);
        let updated = set_window_state(&root, 1_280, 800, false).expect("save window state");
        assert_eq!(updated.window_width, 1_280);
        assert_eq!(updated.window_height, 800);
        assert!(!updated.window_maximized);
        assert_eq!(load(&root).expect("reload preferences"), updated);
        assert_eq!(
            fs::metadata(root.join(PREFERENCES_FILE))
                .expect("file metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&root)
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn malformed_and_unknown_preferences_fail_closed() {
        for contents in [
            "redunar-app-preferences-v3\nclose-to-tray=on\n",
            "redunar-app-preferences-v1\nclose-to-tray=yes\n",
            "redunar-app-preferences-v1\nclose-to-tray=on\nextra=on\n",
            "redunar-app-preferences-v2\nclose-to-tray=on\nwindow-width=200\nwindow-height=960\nwindow-maximized=on\n",
        ] {
            assert!(parse(contents).is_err());
        }
    }

    #[test]
    fn version_one_migrates_to_default_window_state() {
        assert_eq!(
            parse("redunar-app-preferences-v1\nclose-to-tray=on\n").expect("v1"),
            AppPreferences {
                close_to_tray: true,
                ..AppPreferences::default()
            }
        );
    }

    #[test]
    fn version_three_discards_its_removed_switch_and_writes_current_format() {
        let parsed = parse(concat!(
            "redunar-app-preferences-v3\n",
            "close-to-tray=on\n",
            "window-width=1320\n",
            "window-height=840\n",
            "window-maximized=off\n",
            "compatibility-switch=on\n",
        ))
        .expect("v3 preferences");
        assert_eq!(
            parsed,
            AppPreferences {
                close_to_tray: true,
                automatic_updates: true,
                window_width: 1_320,
                window_height: 840,
                window_maximized: false,
                window_size_is_physical: true,
                diagnostic_log: false,
                beta_access: false,
            }
        );
        let serialized = serialize(parsed);
        assert_eq!(serialized.lines().next(), Some(PREFERENCES_HEADER_V8));
        assert!(!serialized.contains("compatibility-switch"));
        assert!(serialized.contains("automatic-updates=on"));
        assert!(serialized.contains("window-size-units=physical"));
    }

    #[test]
    fn legacy_physical_size_survives_unrelated_write_until_geometry_is_saved() {
        let root = fixture();
        fs::create_dir_all(&root).expect("fixture directory");
        fs::write(
            root.join(PREFERENCES_FILE),
            concat!(
                "redunar-app-preferences-v5\n",
                "close-to-tray=off\n",
                "window-width=2560\n",
                "window-height=1440\n",
                "window-maximized=off\n",
                "automatic-updates=on\n",
            ),
        )
        .expect("legacy preferences");
        let migrated = set_automatic_updates(&root, false).expect("unrelated preference write");
        assert!(migrated.window_size_is_physical);
        assert!(
            fs::read_to_string(root.join(PREFERENCES_FILE))
                .expect("migrated preferences")
                .contains("window-size-units=physical")
        );
        let saved = set_window_state(&root, 1_280, 720, false).expect("logical geometry write");
        assert!(!saved.window_size_is_physical);
        assert!(
            !load(&root)
                .expect("reloaded geometry")
                .window_size_is_physical
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn preference_updates_preserve_unrelated_fields() {
        let root = fixture();
        let window = set_window_state(&root, 1_320, 840, false).expect("save window state");
        assert!(!window.close_to_tray);

        let tray = set_close_to_tray(&root, true).expect("enable close to tray");
        assert_eq!(
            tray,
            AppPreferences {
                close_to_tray: true,
                automatic_updates: true,
                window_width: 1_320,
                window_height: 840,
                window_maximized: false,
                window_size_is_physical: false,
                diagnostic_log: false,
                beta_access: false,
            }
        );

        let maximized = set_window_state(&root, 1_320, 840, true).expect("save maximized state");
        assert!(maximized.close_to_tray);
        assert!(maximized.automatic_updates);
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn automatic_updates_default_on_and_preserve_window_and_tray_preferences() {
        let root = fixture();
        let window = set_window_state(&root, 1_440, 900, true).expect("save window state");
        assert!(window.automatic_updates);
        let tray = set_close_to_tray(&root, true).expect("enable tray");
        let updates = set_automatic_updates(&root, false).expect("disable automatic update checks");
        assert_eq!(
            updates,
            AppPreferences {
                close_to_tray: true,
                automatic_updates: false,
                window_width: 1_440,
                window_height: 900,
                window_maximized: true,
                window_size_is_physical: false,
                diagnostic_log: false,
                beta_access: false,
            }
        );
        assert!(tray.automatic_updates);
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn invalid_window_sizes_do_not_replace_saved_preferences() {
        let root = fixture();
        let saved = set_window_state(&root, 1_440, 900, false).expect("save valid state");
        for (width, height) in [
            (MIN_WINDOW_WIDTH - 1, 900),
            (MAX_WINDOW_WIDTH + 1, 900),
            (1_440, MIN_WINDOW_HEIGHT - 1),
            (1_440, MAX_WINDOW_HEIGHT + 1),
        ] {
            assert!(set_window_state(&root, width, height, true).is_err());
            assert_eq!(load(&root).expect("reload saved preferences"), saved);
        }
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn window_size_bounds_are_inclusive() {
        for (width, height) in [
            (MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT),
            (MAX_WINDOW_WIDTH, MAX_WINDOW_HEIGHT),
        ] {
            let parsed = parse(&format!(
                "{PREFERENCES_HEADER_V2}\nclose-to-tray=off\nwindow-width={width}\nwindow-height={height}\nwindow-maximized=off\n"
            ))
            .expect("boundary window size");
            assert_eq!((parsed.window_width, parsed.window_height), (width, height));
        }
    }
}
