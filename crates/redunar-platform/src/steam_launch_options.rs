//! Conservative, read-only inspection of native Steam Launch Options.

use crate::SteamAppId;
use crate::steam_discovery::{VdfObject, VdfReadError, read_vdf};
use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const MAX_USERDATA_ENTRIES: usize = 512;
const MAX_USER_ACCOUNTS: usize = 64;
const MAX_PROC_ENTRIES: usize = 32 * 1024;
const MAX_PROCESS_NAME_BYTES: u64 = 64;
const MAX_EXPECTED_LAUNCH_OPTIONS_BYTES: usize = 4096;

/// The result of comparing Steam's persisted Launch Options with Redunar's
/// exact app-specific wrapper value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SteamLaunchOptionsStatus {
    Configured,
    NotConfigured,
    Ambiguous(SteamLaunchOptionsReason),
    Unavailable(SteamLaunchOptionsReason),
}

impl SteamLaunchOptionsStatus {
    #[must_use]
    pub const fn is_configured(&self) -> bool {
        matches!(self, Self::Configured)
    }
}

impl fmt::Display for SteamLaunchOptionsStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configured => formatter.write_str("Configured"),
            Self::NotConfigured => formatter.write_str("Not configured"),
            Self::Ambiguous(reason) => write!(formatter, "Needs attention: {reason}"),
            Self::Unavailable(reason) => write!(formatter, "Unavailable: {reason}"),
        }
    }
}

/// A stable reason suitable for daemon and UI decisions. Display text avoids
/// exposing account identifiers or private filesystem paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SteamLaunchOptionsReason {
    HomeDirectoryUnavailable,
    NativeSteamNotFound,
    SteamDataUnsafe,
    SteamDataUnreadable,
    ConfigurationUnsafe,
    ConfigurationTooLarge,
    ConfigurationMalformed,
    ConfigurationUnreadable,
    DuplicateConfigurationEntries,
    MultipleAccountsDisagree,
    SteamRunning,
    ProcessStateUnavailable,
    ScanLimitExceeded,
    ExpectedValueInvalid,
}

impl fmt::Display for SteamLaunchOptionsReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::HomeDirectoryUnavailable => {
                "The local home directory is unavailable for Steam setup detection"
            }
            Self::NativeSteamNotFound => "A native Steam installation was not found",
            Self::SteamDataUnsafe => {
                "Steam's local user-data layout contains an unsafe link or file type"
            }
            Self::SteamDataUnreadable => "Steam's local user data could not be read safely",
            Self::ConfigurationUnsafe => {
                "A Steam configuration file is linked or changed while it is being inspected"
            }
            Self::ConfigurationTooLarge => {
                "A Steam configuration file exceeds Redunar's inspection limit"
            }
            Self::ConfigurationMalformed => {
                "A Steam configuration file is malformed or is not valid UTF-8"
            }
            Self::ConfigurationUnreadable => "A Steam configuration file could not be read",
            Self::DuplicateConfigurationEntries => {
                "Steam contains duplicate app or Launch Options entries"
            }
            Self::MultipleAccountsDisagree => {
                "Steam user accounts do not agree on this game's Launch Options"
            }
            Self::SteamRunning => {
                "Steam is running, so its in-memory Launch Options may differ from the saved file"
            }
            Self::ProcessStateUnavailable => {
                "Redunar could not determine whether Steam is currently running"
            }
            Self::ScanLimitExceeded => "Steam's local data exceeds Redunar's inspection limits",
            Self::ExpectedValueInvalid => {
                "Redunar's expected Steam Launch Options value is invalid"
            }
        })
    }
}

/// Reads only native Steam's local per-user configuration and `/proc` state.
/// It never starts Steam, changes Launch Options, or contacts a remote service.
#[derive(Clone, Debug)]
pub struct SteamLaunchOptionsDetector {
    home_directory: PathBuf,
    proc_root: PathBuf,
}

impl SteamLaunchOptionsDetector {
    #[must_use]
    pub fn new(home_directory: impl Into<PathBuf>) -> Self {
        Self {
            home_directory: home_directory.into(),
            proc_root: PathBuf::from("/proc"),
        }
    }

    #[cfg(test)]
    fn with_proc_root(home_directory: impl Into<PathBuf>, proc_root: impl Into<PathBuf>) -> Self {
        Self {
            home_directory: home_directory.into(),
            proc_root: proc_root.into(),
        }
    }

    /// Compare the exact persisted value for `app_id` with `expected`.
    ///
    /// `Configured` requires every detected account to contain the same exact
    /// value and Steam to be confirmed stopped. Prefix, suffix, substring, and
    /// different-app matches are deliberately rejected.
    #[must_use]
    pub fn status(&self, app_id: SteamAppId, expected: &str) -> SteamLaunchOptionsStatus {
        if expected.is_empty()
            || expected.len() > MAX_EXPECTED_LAUNCH_OPTIONS_BYTES
            || expected.chars().any(char::is_control)
        {
            return SteamLaunchOptionsStatus::Unavailable(
                SteamLaunchOptionsReason::ExpectedValueInvalid,
            );
        }

        let disk_status = match self.persisted_status(app_id, expected) {
            Ok(status) => status,
            Err(reason) => return SteamLaunchOptionsStatus::Unavailable(reason),
        };
        self.combine(disk_status)
    }

    fn combine(&self, disk_status: SteamLaunchOptionsStatus) -> SteamLaunchOptionsStatus {
        match steam_process_state(&self.proc_root) {
            Ok(SteamProcessState::Stopped) => disk_status,
            Ok(SteamProcessState::Running) => match disk_status {
                // Steam must be running for a game launch. A persisted exact
                // match is sufficient to proceed; an unconfigured or mixed
                // disk state remains ambiguous until Steam is restarted.
                SteamLaunchOptionsStatus::Configured => SteamLaunchOptionsStatus::Configured,
                _ => SteamLaunchOptionsStatus::Ambiguous(SteamLaunchOptionsReason::SteamRunning),
            },
            Err(reason) => SteamLaunchOptionsStatus::Unavailable(reason),
        }
    }

    fn persisted_status(
        &self,
        app_id: SteamAppId,
        expected: &str,
    ) -> Result<SteamLaunchOptionsStatus, SteamLaunchOptionsReason> {
        let roots = native_steam_roots(&self.home_directory)?;
        if roots.is_empty() {
            return Err(SteamLaunchOptionsReason::NativeSteamNotFound);
        }

        let mut configured = 0_usize;
        let mut not_configured = 0_usize;
        for root in roots {
            for config in account_configurations(&root)? {
                match account_has_exact_launch_options(&config, app_id, expected)? {
                    AccountLaunchOptions::Configured => configured += 1,
                    AccountLaunchOptions::NotConfigured => not_configured += 1,
                    AccountLaunchOptions::Ambiguous => {
                        return Ok(SteamLaunchOptionsStatus::Ambiguous(
                            SteamLaunchOptionsReason::DuplicateConfigurationEntries,
                        ));
                    }
                }
            }
        }

        Ok(match (configured, not_configured) {
            (0, _) => SteamLaunchOptionsStatus::NotConfigured,
            (_, 0) => SteamLaunchOptionsStatus::Configured,
            _ => SteamLaunchOptionsStatus::Ambiguous(
                SteamLaunchOptionsReason::MultipleAccountsDisagree,
            ),
        })
    }
}

fn native_steam_roots(home_directory: &Path) -> Result<Vec<PathBuf>, SteamLaunchOptionsReason> {
    if !home_directory.is_absolute() {
        return Err(SteamLaunchOptionsReason::HomeDirectoryUnavailable);
    }
    let canonical_home = fs::canonicalize(home_directory)
        .map_err(|_| SteamLaunchOptionsReason::HomeDirectoryUnavailable)?;
    let mut roots = BTreeSet::new();
    for candidate in [
        home_directory.join(".local/share/Steam"),
        home_directory.join(".steam/steam"),
    ] {
        let metadata = match fs::symlink_metadata(&candidate) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(SteamLaunchOptionsReason::SteamDataUnreadable),
        };
        if !metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            return Err(SteamLaunchOptionsReason::SteamDataUnsafe);
        }
        let canonical = fs::canonicalize(&candidate)
            .map_err(|_| SteamLaunchOptionsReason::SteamDataUnreadable)?;
        if !canonical.starts_with(&canonical_home)
            || !fs::metadata(&canonical).is_ok_and(|metadata| metadata.is_dir())
        {
            return Err(SteamLaunchOptionsReason::SteamDataUnsafe);
        }
        roots.insert(canonical);
    }
    Ok(roots.into_iter().collect())
}

fn account_configurations(root: &Path) -> Result<Vec<PathBuf>, SteamLaunchOptionsReason> {
    let userdata = root.join("userdata");
    let metadata = match fs::symlink_metadata(&userdata) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(SteamLaunchOptionsReason::SteamDataUnreadable),
    };
    if !metadata.file_type().is_dir() {
        return Err(SteamLaunchOptionsReason::SteamDataUnsafe);
    }
    let entries =
        fs::read_dir(&userdata).map_err(|_| SteamLaunchOptionsReason::SteamDataUnreadable)?;
    let mut configurations = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index == MAX_USERDATA_ENTRIES {
            return Err(SteamLaunchOptionsReason::ScanLimitExceeded);
        }
        let entry = entry.map_err(|_| SteamLaunchOptionsReason::SteamDataUnreadable)?;
        let Some(account_id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if account_id.is_empty()
            || !account_id.bytes().all(|byte| byte.is_ascii_digit())
            || account_id.bytes().all(|byte| byte == b'0')
        {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|_| SteamLaunchOptionsReason::SteamDataUnreadable)?;
        if !file_type.is_dir() {
            return Err(SteamLaunchOptionsReason::SteamDataUnsafe);
        }
        if configurations.len() == MAX_USER_ACCOUNTS {
            return Err(SteamLaunchOptionsReason::ScanLimitExceeded);
        }
        let config_directory = entry.path().join("config");
        match fs::symlink_metadata(&config_directory) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => return Err(SteamLaunchOptionsReason::SteamDataUnsafe),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                configurations.push(config_directory.join("localconfig.vdf"));
                continue;
            }
            Err(_) => return Err(SteamLaunchOptionsReason::SteamDataUnreadable),
        }
        configurations.push(config_directory.join("localconfig.vdf"));
    }
    configurations.sort_unstable();
    Ok(configurations)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AccountLaunchOptions {
    Configured,
    NotConfigured,
    Ambiguous,
}

fn account_has_exact_launch_options(
    path: &Path,
    app_id: SteamAppId,
    expected: &str,
) -> Result<AccountLaunchOptions, SteamLaunchOptionsReason> {
    let Some(document) = read_vdf(path).map_err(vdf_reason)? else {
        return Ok(AccountLaunchOptions::NotConfigured);
    };
    launch_options_value(&document, app_id).map_or(Ok(AccountLaunchOptions::Ambiguous), |value| {
        Ok(if value == Some(expected) {
            AccountLaunchOptions::Configured
        } else {
            AccountLaunchOptions::NotConfigured
        })
    })
}

fn launch_options_value(document: &VdfObject, app_id: SteamAppId) -> Result<Option<&str>, ()> {
    let Some(store) = document.unique_object("UserLocalConfigStore")? else {
        return Ok(None);
    };
    let Some(software) = store.unique_object("Software")? else {
        return Ok(None);
    };
    let Some(valve) = software.unique_object("Valve")? else {
        return Ok(None);
    };
    let Some(steam) = valve.unique_object("Steam")? else {
        return Ok(None);
    };
    let Some(apps) = steam.unique_object("apps")? else {
        return Ok(None);
    };
    let key = app_id.get().to_string();
    let Some(app) = apps.unique_exact_object(&key)? else {
        return Ok(None);
    };
    app.unique_text("LaunchOptions")
}

fn vdf_reason(error: VdfReadError) -> SteamLaunchOptionsReason {
    match error {
        VdfReadError::UnsafeFile | VdfReadError::ChangedDuringOpen => {
            SteamLaunchOptionsReason::ConfigurationUnsafe
        }
        VdfReadError::Oversized => SteamLaunchOptionsReason::ConfigurationTooLarge,
        VdfReadError::InvalidUtf8 | VdfReadError::Malformed => {
            SteamLaunchOptionsReason::ConfigurationMalformed
        }
        VdfReadError::Inspect | VdfReadError::Open | VdfReadError::Read => {
            SteamLaunchOptionsReason::ConfigurationUnreadable
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SteamProcessState {
    Running,
    Stopped,
}

fn steam_process_state(proc_root: &Path) -> Result<SteamProcessState, SteamLaunchOptionsReason> {
    let current_uid = fs::metadata(proc_root.join("self"))
        .map_err(|_| SteamLaunchOptionsReason::ProcessStateUnavailable)?
        .uid();
    let entries =
        fs::read_dir(proc_root).map_err(|_| SteamLaunchOptionsReason::ProcessStateUnavailable)?;
    for (index, entry) in entries.enumerate() {
        if index == MAX_PROC_ENTRIES {
            return Err(SteamLaunchOptionsReason::ScanLimitExceeded);
        }
        let entry = entry.map_err(|_| SteamLaunchOptionsReason::ProcessStateUnavailable)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let process = entry.path();
        let metadata = match fs::metadata(&process) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(SteamLaunchOptionsReason::ProcessStateUnavailable),
        };
        if metadata.uid() != current_uid {
            continue;
        }
        let process_name = read_process_name(&process.join("comm"));
        let executable_name = fs::read_link(process.join("exe"))
            .ok()
            .and_then(|path| path.file_name().map(std::ffi::OsStr::to_owned));
        if process_name.as_deref().is_some_and(is_steam_process_name)
            || executable_name
                .as_deref()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(is_steam_process_name)
        {
            return Ok(SteamProcessState::Running);
        }
        if process_name.is_none()
            && executable_name.is_none()
            && !matches!(
                fs::metadata(&process),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound
            )
        {
            return Err(SteamLaunchOptionsReason::ProcessStateUnavailable);
        }
    }
    Ok(SteamProcessState::Stopped)
}

fn read_process_name(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_PROCESS_NAME_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if u64::try_from(bytes.len()).ok()? > MAX_PROCESS_NAME_BYTES {
        return None;
    }
    let name = std::str::from_utf8(&bytes).ok()?.trim_end_matches('\n');
    Some(name.to_owned())
}

fn is_steam_process_name(name: &str) -> bool {
    ["steam", "steam.sh", "bin_steam.sh"]
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
    const APP_ID: u32 = 1_808_500;
    const EXPECTED: &str = "/usr/bin/redunar-steam-launch --app-id 1808500 -- %command%";

    struct Fixture {
        root: PathBuf,
        home: PathBuf,
        steam: PathBuf,
        proc_root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "redunar-steam-launch-options-{}-{id}",
                std::process::id()
            ));
            let home = root.join("home");
            let steam = home.join(".local/share/Steam");
            let proc_root = root.join("proc");
            fs::create_dir_all(&steam).expect("Steam root");
            fs::create_dir_all(proc_root.join("self")).expect("proc self");
            Self {
                root,
                home,
                steam,
                proc_root,
            }
        }

        fn detector(&self) -> SteamLaunchOptionsDetector {
            SteamLaunchOptionsDetector::with_proc_root(&self.home, &self.proc_root)
        }

        fn write_account(&self, account_id: &str, app_id: u32, launch_options: Option<&str>) {
            let directory = self.steam.join("userdata").join(account_id).join("config");
            fs::create_dir_all(&directory).expect("account config");
            let launch_options = launch_options.map_or_else(String::new, |value| {
                format!("\"LaunchOptions\" \"{value}\"")
            });
            fs::write(
                directory.join("localconfig.vdf"),
                format!(
                    "\"UserLocalConfigStore\" {{ \"Software\" {{ \"Valve\" {{ \"Steam\" {{ \"apps\" {{ \"{app_id}\" {{ {launch_options} }} }} }} }} }} }}"
                ),
            )
            .expect("local config");
        }

        fn app_id() -> SteamAppId {
            SteamAppId::new(APP_ID).expect("app ID")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).expect("fixture cleanup");
        }
    }

    #[test]
    fn requires_exact_app_and_complete_launch_options_value() {
        let fixture = Fixture::new();
        fixture.write_account("1001", APP_ID, Some(EXPECTED));
        assert_eq!(
            fixture.detector().status(Fixture::app_id(), EXPECTED),
            SteamLaunchOptionsStatus::Configured
        );

        fixture.write_account(
            "1001",
            APP_ID,
            Some("env REDUNAR_HINT=/usr/bin/redunar-steam-launch --app-id 1808500 -- %command%"),
        );
        assert_eq!(
            fixture.detector().status(Fixture::app_id(), EXPECTED),
            SteamLaunchOptionsStatus::NotConfigured
        );

        fixture.write_account("1001", APP_ID + 1, Some(EXPECTED));
        assert_eq!(
            fixture.detector().status(Fixture::app_id(), EXPECTED),
            SteamLaunchOptionsStatus::NotConfigured
        );
    }

    #[test]
    fn accepts_bounded_large_unrelated_steam_cache_values() {
        let fixture = Fixture::new();
        fixture.write_account("1001", APP_ID, Some(EXPECTED));
        let config = fixture.steam.join("userdata/1001/config/localconfig.vdf");
        let large_cache = "x".repeat(18_500);
        fs::write(
            &config,
            format!(
                "\"UserLocalConfigStore\" {{ \"LargeCache\" \"{large_cache}\" \"Software\" {{ \"Valve\" {{ \"Steam\" {{ \"apps\" {{ \"{APP_ID}\" {{ \"LaunchOptions\" \"{EXPECTED}\" }} }} }} }} }} }}"
            ),
        )
        .expect("large valid local config");

        assert_eq!(
            fixture.detector().status(Fixture::app_id(), EXPECTED),
            SteamLaunchOptionsStatus::Configured
        );
    }

    #[test]
    fn treats_disagreeing_accounts_and_duplicate_entries_as_ambiguous() {
        let fixture = Fixture::new();
        fixture.write_account("1001", APP_ID, Some(EXPECTED));
        fixture.write_account("1002", APP_ID, Some("MANGOHUD=1 %command%"));
        assert_eq!(
            fixture.detector().status(Fixture::app_id(), EXPECTED),
            SteamLaunchOptionsStatus::Ambiguous(SteamLaunchOptionsReason::MultipleAccountsDisagree)
        );

        fs::remove_dir_all(fixture.steam.join("userdata/1002")).expect("remove account");
        let config = fixture.steam.join("userdata/1001/config/localconfig.vdf");
        fs::write(
            config,
            format!(
                "\"UserLocalConfigStore\" {{ \"Software\" {{ \"Valve\" {{ \"Steam\" {{ \"apps\" {{ \"{APP_ID}\" {{ \"LaunchOptions\" \"{EXPECTED}\" \"LaunchOptions\" \"%command%\" }} }} }} }} }} }}"
            ),
        )
        .expect("duplicate config");
        assert_eq!(
            fixture.detector().status(Fixture::app_id(), EXPECTED),
            SteamLaunchOptionsStatus::Ambiguous(
                SteamLaunchOptionsReason::DuplicateConfigurationEntries
            )
        );
    }

    #[test]
    fn rejects_malformed_oversized_and_symlinked_configuration_files() {
        let fixture = Fixture::new();
        fixture.write_account("1001", APP_ID, Some(EXPECTED));
        let config = fixture.steam.join("userdata/1001/config/localconfig.vdf");

        fs::write(&config, b"\"unterminated").expect("malformed config");
        assert_eq!(
            fixture.detector().status(Fixture::app_id(), EXPECTED),
            SteamLaunchOptionsStatus::Unavailable(SteamLaunchOptionsReason::ConfigurationMalformed)
        );

        let oversized = File::create(&config).expect("oversized config");
        oversized
            .set_len(super::super::steam_discovery::MAX_VDF_BYTES + 1)
            .expect("oversized length");
        assert_eq!(
            fixture.detector().status(Fixture::app_id(), EXPECTED),
            SteamLaunchOptionsStatus::Unavailable(SteamLaunchOptionsReason::ConfigurationTooLarge)
        );

        fs::remove_file(&config).expect("remove config");
        let outside = fixture.root.join("outside.vdf");
        fs::write(&outside, b"fixture").expect("outside config");
        symlink(&outside, &config).expect("symlink config");
        assert_eq!(
            fixture.detector().status(Fixture::app_id(), EXPECTED),
            SteamLaunchOptionsStatus::Unavailable(SteamLaunchOptionsReason::ConfigurationUnsafe)
        );
    }

    #[test]
    fn exact_configuration_is_usable_while_steam_is_running() {
        let fixture = Fixture::new();
        fixture.write_account("1001", APP_ID, Some(EXPECTED));
        let process = fixture.proc_root.join("1234");
        fs::create_dir_all(&process).expect("Steam process");
        fs::write(process.join("comm"), b"steam\n").expect("Steam comm");
        assert_eq!(
            fixture.detector().status(Fixture::app_id(), EXPECTED),
            SteamLaunchOptionsStatus::Configured
        );
    }

    #[test]
    fn missing_app_entry_is_not_configured_but_missing_native_steam_is_unavailable() {
        let fixture = Fixture::new();
        fixture.write_account("1001", APP_ID + 1, Some(EXPECTED));
        assert_eq!(
            fixture.detector().status(Fixture::app_id(), EXPECTED),
            SteamLaunchOptionsStatus::NotConfigured
        );

        fs::remove_dir_all(&fixture.steam).expect("remove Steam root");
        assert_eq!(
            fixture.detector().status(Fixture::app_id(), EXPECTED),
            SteamLaunchOptionsStatus::Unavailable(SteamLaunchOptionsReason::NativeSteamNotFound)
        );
    }

    #[test]
    fn status_display_is_user_facing_and_keeps_reason_text() {
        assert_eq!(
            SteamLaunchOptionsStatus::Configured.to_string(),
            "Configured"
        );
        assert_eq!(
            SteamLaunchOptionsStatus::NotConfigured.to_string(),
            "Not configured"
        );
        assert_eq!(
            SteamLaunchOptionsStatus::Ambiguous(SteamLaunchOptionsReason::SteamRunning).to_string(),
            "Needs attention: Steam is running, so its in-memory Launch Options may differ from the saved file"
        );
        assert_eq!(
            SteamLaunchOptionsStatus::Unavailable(SteamLaunchOptionsReason::NativeSteamNotFound)
                .to_string(),
            "Unavailable: A native Steam installation was not found"
        );
    }

    #[test]
    #[ignore = "requires a local native Steam installation; read-only host probe"]
    fn host_native_steam_configuration_is_parseable() {
        let host_home = std::env::var_os("HOME").expect("host home directory");
        let status = SteamLaunchOptionsDetector::new(PathBuf::from(host_home))
            .status(Fixture::app_id(), EXPECTED);
        assert_ne!(
            status,
            SteamLaunchOptionsStatus::Unavailable(SteamLaunchOptionsReason::ConfigurationMalformed)
        );
    }
}
