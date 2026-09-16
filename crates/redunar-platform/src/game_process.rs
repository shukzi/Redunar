use redunar_core::GameProcess;
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const MAX_COMM_BYTES: u64 = 256;
const MAX_ENVIRON_BYTES: u64 = 64 * 1024;
const MAX_PROC_ENTRIES_PER_SCAN: usize = 32 * 1024;
const MAX_DETECTED_GAMES: usize = 256;

/// Finds likely game processes without launching commands or retaining file
/// handles into `/proc`. Callers own the polling interval; each call performs
/// exactly one pass over the configured proc directory.
#[derive(Clone, Debug)]
pub struct LinuxGameProcessDetector {
    proc_root: PathBuf,
    user_id: u32,
}

impl Default for LinuxGameProcessDetector {
    fn default() -> Self {
        let proc_root = PathBuf::from("/proc");
        let user_id =
            fs::metadata(proc_root.join("self")).map_or(u32::MAX, |metadata| metadata.uid());
        Self { proc_root, user_id }
    }
}

impl LinuxGameProcessDetector {
    #[must_use]
    pub fn new(proc_root: impl Into<PathBuf>, user_id: u32) -> Self {
        Self {
            proc_root: proc_root.into(),
            user_id,
        }
    }

    /// Returns conservatively identified game processes in PID order.
    ///
    /// Entries that disappear, become unreadable, or exceed the input bounds
    /// during the scan are ignored because process churn is normal.
    ///
    /// # Errors
    ///
    /// Returns an error only when the proc root itself cannot be enumerated.
    pub fn detect(&self) -> Result<Vec<GameProcess>, GameDetectionError> {
        let entries = fs::read_dir(&self.proc_root).map_err(|error| {
            GameDetectionError::new(format!(
                "could not scan {}: {error}",
                self.proc_root.display()
            ))
        })?;

        let mut games = Vec::new();
        for entry in entries.flatten().take(MAX_PROC_ENTRIES_PER_SCAN) {
            let Some(pid) = numeric_pid(&entry.file_name()) else {
                continue;
            };
            let process_root = entry.path();
            let Ok(metadata) = fs::metadata(&process_root) else {
                continue;
            };
            if metadata.uid() != self.user_id {
                continue;
            }

            if let Some(game) = inspect_process(&process_root, pid) {
                games.push(game);
                if games.len() == MAX_DETECTED_GAMES {
                    break;
                }
            }
        }
        games.sort_unstable_by_key(|game| game.pid);
        Ok(games)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameDetectionError {
    message: String,
}

impl GameDetectionError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for GameDetectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for GameDetectionError {}

fn numeric_pid(name: &std::ffi::OsStr) -> Option<u32> {
    name.to_str()?.parse().ok()
}

fn inspect_process(process_root: &Path, pid: u32) -> Option<GameProcess> {
    // Kernel threads have no executable link. Resolving it first also avoids
    // reading environment data for entries Redunar cannot report usefully.
    let executable = fs::read_link(process_root.join("exe")).ok()?;
    let comm = read_bounded(process_root.join("comm"), MAX_COMM_BYTES)?;
    let comm = String::from_utf8(comm).ok()?.trim().to_owned();
    if comm.is_empty()
        || is_redunar_process(&comm, &executable)
        || is_game_runtime_helper(&comm, &executable)
    {
        return None;
    }

    let environ = read_bounded(process_root.join("environ"), MAX_ENVIRON_BYTES)?;
    let steam_app_id = environment_value(&environ, b"SteamAppId")
        .or_else(|| environment_value(&environ, b"SteamGameId"))
        .or_else(|| environment_value(&environ, b"STEAM_COMPAT_APP_ID"))
        .and_then(parse_app_id);
    let game_mode_active = environment_value(&environ, b"GAMEMODERUNEXEC").is_some();
    let has_game_runtime_marker = steam_app_id.is_some()
        || environment_value(&environ, b"STEAM_COMPAT_DATA_PATH").is_some()
        || game_mode_active;

    // Explicit Steam/Proton/GameMode state is deliberately required. This
    // avoids classifying browsers, launchers, and desktop applications from
    // executable-name heuristics. Other launchers can add explicit adapters
    // later without weakening this boundary.
    if !has_game_runtime_marker {
        return None;
    }

    Some(GameProcess {
        pid,
        comm,
        executable,
        steam_app_id,
        game_mode_active,
    })
}

fn is_redunar_process(comm: &str, executable: &Path) -> bool {
    let comm = comm.to_ascii_lowercase();
    let executable_name = executable
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    comm.starts_with("redunar") || executable_name.starts_with("redunar")
}

fn is_game_runtime_helper(comm: &str, executable: &Path) -> bool {
    const HELPERS: &[&str] = &[
        "bin_steam.sh",
        "gamemoderun",
        "mangohud",
        "pressure-vessel",
        "pv-bwrap",
        "reaper",
        "steam-runtime",
        "steam-runtime-steam-remote",
        "steam.sh",
        "steam",
        "steamwebhelper",
        "wineserver",
    ];
    let comm = comm.to_ascii_lowercase();
    let executable_name = executable
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    HELPERS
        .iter()
        .any(|helper| comm == *helper || executable_name == *helper)
}

fn read_bounded(path: impl AsRef<Path>, max_bytes: u64) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(max_bytes + 1).read_to_end(&mut bytes).ok()?;
    (u64::try_from(bytes.len()).ok()? <= max_bytes).then_some(bytes)
}

fn environment_value<'a>(environment: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    environment.split(|byte| *byte == 0).find_map(|entry| {
        let separator = entry.iter().position(|byte| *byte == b'=')?;
        let (key, value) = entry.split_at(separator);
        (key == name).then_some(&value[1..])
    })
}

fn parse_app_id(value: &[u8]) -> Option<u32> {
    let value = std::str::from_utf8(value).ok()?;
    let app_id = value.parse().ok()?;
    (app_id != 0).then_some(app_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct ProcFixture {
        root: PathBuf,
        user_id: u32,
    }

    impl ProcFixture {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir()
                .join(format!("redunar-game-process-{}-{id}", std::process::id()));
            fs::create_dir_all(&root).expect("create proc fixture");
            let user_id = fs::metadata(&root).expect("fixture metadata").uid();
            Self { root, user_id }
        }

        fn process(&self, pid: u32, comm: &str, executable: &str, environ: &[u8]) {
            let root = self.root.join(pid.to_string());
            fs::create_dir_all(&root).expect("create process fixture");
            fs::write(root.join("comm"), comm).expect("write comm fixture");
            fs::write(root.join("environ"), environ).expect("write environ fixture");
            symlink(executable, root.join("exe")).expect("link executable fixture");
        }

        fn detector(&self) -> LinuxGameProcessDetector {
            LinuxGameProcessDetector::new(&self.root, self.user_id)
        }
    }

    impl Drop for ProcFixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).expect("remove proc fixture");
        }
    }

    #[test]
    fn detects_steam_game_and_reads_fallback_identifier() {
        let fixture = ProcFixture::new();
        fixture.process(
            812,
            "spacegame\n",
            "/games/spacegame",
            b"PATH=/usr/bin\0SteamGameId=123456\0",
        );

        assert_eq!(
            fixture.detector().detect().expect("detect games"),
            vec![GameProcess {
                pid: 812,
                comm: "spacegame".into(),
                executable: PathBuf::from("/games/spacegame"),
                steam_app_id: Some(123_456),
                game_mode_active: false,
            }]
        );
    }

    #[test]
    fn detects_explicit_non_steam_game_runtime_without_an_app_id() {
        let fixture = ProcFixture::new();
        fixture.process(
            900,
            "indie-game\n",
            "/opt/games/indie-game",
            b"GAMEMODERUNEXEC=indie-game\0",
        );

        let games = fixture.detector().detect().expect("detect games");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].steam_app_id, None);
        assert!(games[0].game_mode_active);
    }

    #[test]
    fn ignores_ordinary_processes_kernel_threads_and_redunar() {
        let fixture = ProcFixture::new();
        fixture.process(10, "browser\n", "/usr/bin/browser", b"PATH=/usr/bin\0");
        fixture.process(
            20,
            "redunar-tauri\n",
            "/usr/bin/redunar-tauri",
            b"SteamAppId=42\0",
        );
        fixture.process(
            21,
            "steamwebhelper\n",
            "/usr/lib/steam/steamwebhelper",
            b"SteamAppId=42\0",
        );
        fixture.process(
            22,
            "steam\n",
            "/usr/lib/steam/bin_steam.sh",
            b"SteamAppId=42\0",
        );
        let kernel = fixture.root.join("30");
        fs::create_dir_all(&kernel).expect("create kernel thread fixture");
        fs::write(kernel.join("comm"), "kworker/0:1\n").expect("write kernel comm");
        fs::write(kernel.join("environ"), []).expect("write kernel environment");

        assert!(
            fixture
                .detector()
                .detect()
                .expect("detect games")
                .is_empty()
        );
    }

    #[test]
    fn ignores_invalid_and_oversized_environment_data() {
        let fixture = ProcFixture::new();
        fixture.process(40, "zero\n", "/games/zero", b"SteamAppId=0\0");
        fixture.process(41, "text\n", "/games/text", b"SteamAppId=abc\0");
        let oversized = vec![
            b'x';
            usize::try_from(MAX_ENVIRON_BYTES + 1)
                .expect("environment bound fits usize")
        ];
        fixture.process(42, "large\n", "/games/large", &oversized);

        assert!(
            fixture
                .detector()
                .detect()
                .expect("detect games")
                .is_empty()
        );
    }

    #[test]
    fn detection_result_is_bounded_under_process_flood() {
        let fixture = ProcFixture::new();
        for pid in 1..=u32::try_from(MAX_DETECTED_GAMES + 8).expect("fixture count") {
            fixture.process(
                pid,
                "fixture-game\n",
                "/games/fixture-game",
                b"GAMEMODERUNEXEC=fixture-game\0",
            );
        }

        assert_eq!(
            fixture.detector().detect().expect("detect games").len(),
            MAX_DETECTED_GAMES
        );
    }

    #[test]
    fn missing_proc_root_is_reported() {
        let fixture = ProcFixture::new();
        let detector = LinuxGameProcessDetector::new(fixture.root.join("missing"), fixture.user_id);
        assert!(detector.detect().is_err());
    }
}
