use redunar_core::{GameLaunchConfig, GameMatchRule, GameProcess};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

pub(super) const MAX_VDF_BYTES: u64 = 1024 * 1024;
const MAX_VDF_DEPTH: usize = 16;
const MAX_VDF_ENTRIES: usize = 16 * 1024;
const MAX_VDF_PATH_BYTES: usize = 4096;
const MAX_VDF_TOKEN_BYTES: usize = 64 * 1024;
const MAX_LIBRARY_FOLDERS: usize = 64;
const MAX_MANIFESTS_PER_LIBRARY: usize = 4096;
const MAX_DISCOVERED_GAMES: usize = 512;
const MAX_NAME_BYTES: usize = 256;
const MAX_POSTER_BYTES: u64 = 20 * 1024 * 1024;
const MAX_POSTER_VARIANTS: usize = 64;
const POSTER_NAMES: &[&str] = &["library_600x900.jpg", "library_600x900.png"];
const FLATPAK_STEAM_APP: &str = "com.valvesoftware.Steam";
const STEAM_STATE_FULLY_INSTALLED: u64 = 1 << 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GameDiscoverySource {
    Steam { app_id: u32 },
    DesktopEntry { path: PathBuf },
    RunningProcess { pid: u32 },
}

/// A local game candidate backed by bounded, read-only identity evidence.
/// `launch` is absent when Redunar cannot construct a verified shell-free
/// launch. The install directory remains useful for display and later runtime
/// matching; callers must not guess an executable in that case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredGame {
    pub display_name: String,
    pub install_directory: PathBuf,
    /// Optional portrait artwork already cached by the local Steam client.
    /// Redunar never downloads artwork during discovery.
    pub poster_path: Option<PathBuf>,
    pub launch: Option<GameLaunchConfig>,
    pub match_rules: Vec<GameMatchRule>,
    pub source: GameDiscoverySource,
}

#[derive(Clone, Debug)]
pub struct SteamGameDiscovery {
    home_directory: PathBuf,
    native_client: PathBuf,
    flatpak_client: PathBuf,
}

impl SteamGameDiscovery {
    #[must_use]
    pub fn new(home_directory: impl Into<PathBuf>) -> Self {
        Self {
            home_directory: home_directory.into(),
            native_client: PathBuf::from("/usr/bin/steam"),
            flatpak_client: PathBuf::from("/usr/bin/flatpak"),
        }
    }

    #[cfg(test)]
    fn with_clients(
        home_directory: impl Into<PathBuf>,
        native_client: impl Into<PathBuf>,
        flatpak_client: impl Into<PathBuf>,
    ) -> Self {
        Self {
            home_directory: home_directory.into(),
            native_client: native_client.into(),
            flatpak_client: flatpak_client.into(),
        }
    }

    /// Discover installed Steam games from local manifests only.
    ///
    /// Missing Steam roots are normal. An existing root that cannot be read is
    /// reported, while malformed or racing individual manifests are skipped.
    /// No command is launched and no network or Steam database is consulted.
    ///
    /// # Errors
    ///
    /// Returns [`GameDiscoveryError`] when the home path is not absolute or an
    /// existing Steam library cannot be enumerated safely.
    pub fn discover(&self) -> Result<Vec<DiscoveredGame>, GameDiscoveryError> {
        if !self.home_directory.is_absolute() {
            return Err(GameDiscoveryError::new(
                "the game discovery home directory must be absolute",
            ));
        }

        let roots = [
            SteamRoot::new(self.home_directory.join(".local/share/Steam"), false),
            SteamRoot::new(self.home_directory.join(".steam/steam"), false),
            SteamRoot::new(
                self.home_directory
                    .join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
                true,
            ),
        ];
        let mut libraries = BTreeMap::<PathBuf, bool>::new();
        for root in roots {
            let Some(canonical_root) = canonical_directory_if_present(&root.path)? else {
                continue;
            };
            libraries
                .entry(canonical_root.clone())
                .or_insert(root.is_flatpak);
            for path in read_extra_libraries(&canonical_root)? {
                if libraries.len() == MAX_LIBRARY_FOLDERS {
                    break;
                }
                libraries.entry(path).or_insert(root.is_flatpak);
            }
        }

        let mut games = BTreeMap::<u32, DiscoveredGame>::new();
        for (library, is_flatpak) in libraries {
            for manifest in manifest_paths(&library)? {
                if games.len() == MAX_DISCOVERED_GAMES {
                    break;
                }
                let Some((app_id, name, install_directory)) = read_manifest(&library, &manifest)
                else {
                    continue;
                };
                let launch = self.launch_config(app_id, is_flatpak);
                let poster_path = self.cached_poster(app_id, is_flatpak);
                games.entry(app_id).or_insert_with(|| DiscoveredGame {
                    display_name: name,
                    install_directory,
                    poster_path,
                    launch,
                    match_rules: vec![GameMatchRule::SteamAppId(app_id)],
                    source: GameDiscoverySource::Steam { app_id },
                });
            }
        }
        Ok(games.into_values().collect())
    }

    fn launch_config(&self, app_id: u32, is_flatpak: bool) -> Option<GameLaunchConfig> {
        let executable = if is_flatpak {
            &self.flatpak_client
        } else {
            &self.native_client
        };
        if !is_executable_file(executable) {
            return None;
        }
        let arguments = if is_flatpak {
            vec![
                OsString::from("run"),
                OsString::from(FLATPAK_STEAM_APP),
                OsString::from("-applaunch"),
                OsString::from(app_id.to_string()),
            ]
        } else {
            vec![
                OsString::from("-applaunch"),
                OsString::from(app_id.to_string()),
            ]
        };
        Some(GameLaunchConfig {
            executable: executable.clone(),
            arguments,
            working_directory: None,
        })
    }

    /// Locate optional artwork in Steam's local cache, without discovery or network access.
    #[must_use]
    pub fn local_poster(&self, app_id: u32) -> Option<PathBuf> {
        self.cached_poster(app_id, false)
            .or_else(|| self.cached_poster(app_id, true))
    }

    /// Prefer wide hero art, then wide library/store headers. Portrait art is
    /// deliberately excluded when no banner exists in the local cache.
    #[must_use]
    pub fn local_banner(&self, app_id: u32) -> Option<PathBuf> {
        [
            "library_hero.jpg",
            "library_hero.png",
            "library_header.jpg",
            "library_header.png",
            "header.jpg",
            "header.png",
        ]
        .into_iter()
        .find_map(|name| {
            self.cached_artwork(app_id, false, &[name])
                .or_else(|| self.cached_artwork(app_id, true, &[name]))
        })
    }

    fn cached_poster(&self, app_id: u32, is_flatpak: bool) -> Option<PathBuf> {
        self.cached_artwork(app_id, is_flatpak, POSTER_NAMES)
    }

    fn cached_artwork(&self, app_id: u32, is_flatpak: bool, names: &[&str]) -> Option<PathBuf> {
        let roots = if is_flatpak {
            vec![
                self.home_directory
                    .join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
            ]
        } else {
            vec![
                self.home_directory.join(".local/share/Steam"),
                self.home_directory.join(".steam/steam"),
            ]
        };
        roots
            .into_iter()
            .find_map(|root| cached_artwork_in_root(&root, app_id, names))
    }
}

#[derive(Clone, Debug)]
struct SteamRoot {
    path: PathBuf,
    is_flatpak: bool,
}

impl SteamRoot {
    fn new(path: PathBuf, is_flatpak: bool) -> Self {
        Self { path, is_flatpak }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameDiscoveryError {
    message: String,
}

impl GameDiscoveryError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for GameDiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for GameDiscoveryError {}

/// Convert a conservatively detected process only when its executable is a
/// concrete host executable. Compatibility runtimes and shared launchers are
/// rejected because importing them would assign many games the same identity.
///
/// # Errors
///
/// Returns [`GameDiscoveryError`] when the executable is stale, not a regular
/// executable, belongs to a shared runtime, or carries an invalid display name.
pub fn candidate_from_process(process: &GameProcess) -> Result<DiscoveredGame, GameDiscoveryError> {
    candidate_from_process_at(Path::new("/proc"), process)
}

fn candidate_from_process_at(
    proc_root: &Path,
    process: &GameProcess,
) -> Result<DiscoveredGame, GameDiscoveryError> {
    let executable = fs::canonicalize(&process.executable).map_err(|error| {
        GameDiscoveryError::new(format!(
            "could not resolve the detected game executable: {error}"
        ))
    })?;
    let live_executable = fs::read_link(proc_root.join(process.pid.to_string()).join("exe"))
        .and_then(fs::canonicalize)
        .map_err(|error| {
            GameDiscoveryError::new(format!(
                "could not confirm the detected game process identity: {error}"
            ))
        })?;
    if live_executable != executable {
        return Err(GameDiscoveryError::new(
            "the detected game process executable changed before it could be verified",
        ));
    }
    if !is_executable_file(&executable) || is_shared_runtime(&executable) {
        return Err(GameDiscoveryError::new(
            "the detected process does not expose a unique directly launchable executable",
        ));
    }
    let display_name = process.comm.trim();
    if display_name.is_empty()
        || display_name.len() > MAX_NAME_BYTES
        || display_name.chars().any(char::is_control)
    {
        return Err(GameDiscoveryError::new(
            "the detected game name is invalid or too large",
        ));
    }
    let install_directory = executable
        .parent()
        .ok_or_else(|| GameDiscoveryError::new("the detected executable has no parent directory"))?
        .to_path_buf();
    let mut match_rules = vec![GameMatchRule::Executable(executable.clone())];
    if let Some(app_id) = process.steam_app_id {
        match_rules.push(GameMatchRule::SteamAppId(app_id));
    }
    Ok(DiscoveredGame {
        display_name: display_name.to_owned(),
        install_directory: install_directory.clone(),
        poster_path: None,
        launch: Some(GameLaunchConfig {
            executable,
            arguments: Vec::new(),
            working_directory: Some(install_directory),
        }),
        match_rules,
        source: GameDiscoverySource::RunningProcess { pid: process.pid },
    })
}

#[cfg(test)]
fn cached_poster_in_root(root: &Path, app_id: u32) -> Option<PathBuf> {
    cached_artwork_in_root(root, app_id, POSTER_NAMES)
}

fn cached_artwork_in_root(root: &Path, app_id: u32, names: &[&str]) -> Option<PathBuf> {
    let cache_path = root.join("appcache/librarycache");
    if !fs::symlink_metadata(&cache_path).ok()?.file_type().is_dir() {
        return None;
    }
    let cache = fs::canonicalize(cache_path).ok()?;

    // Older Steam clients stored grid art directly in librarycache, while
    // current clients normally place it in an app-id directory.
    for name in names {
        let flat_name = format!("{app_id}_{name}");
        if let Some(path) = safe_cached_image(&cache, &cache.join(flat_name)) {
            return Some(path);
        }
    }

    let app_directory = cache.join(app_id.to_string());
    if !fs::symlink_metadata(&app_directory)
        .ok()?
        .file_type()
        .is_dir()
    {
        return None;
    }
    for name in names {
        if let Some(path) = safe_cached_image(&cache, &app_directory.join(name)) {
            return Some(path);
        }
    }

    let entries = fs::read_dir(&app_directory).ok()?;
    for entry in entries.flatten().take(MAX_POSTER_VARIANTS) {
        if !entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            continue;
        }
        for name in names {
            if let Some(path) = safe_cached_image(&cache, &entry.path().join(name)) {
                return Some(path);
            }
        }
    }
    None
}

fn safe_cached_image(cache: &Path, path: &Path) -> Option<PathBuf> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > MAX_POSTER_BYTES {
        return None;
    }
    let canonical = fs::canonicalize(path).ok()?;
    canonical.starts_with(cache).then_some(canonical)
}

fn canonical_directory_if_present(path: &Path) -> Result<Option<PathBuf>, GameDiscoveryError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(GameDiscoveryError::new(format!(
                "could not inspect Steam path {}: {error}",
                path.display()
            )));
        }
    };
    if !metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
        return Ok(None);
    }
    let canonical = fs::canonicalize(path).map_err(|error| {
        GameDiscoveryError::new(format!(
            "could not resolve Steam path {}: {error}",
            path.display()
        ))
    })?;
    Ok(fs::metadata(&canonical)
        .ok()
        .filter(fs::Metadata::is_dir)
        .map(|_| canonical))
}

fn read_extra_libraries(root: &Path) -> Result<Vec<PathBuf>, GameDiscoveryError> {
    let path = root.join("steamapps/libraryfolders.vdf");
    let Some(document) = read_vdf_if_present(&path) else {
        return Ok(Vec::new());
    };
    let Some(library_folders) = document.object("libraryfolders") else {
        return Ok(Vec::new());
    };
    let mut paths = BTreeSet::new();
    for (_, value) in library_folders.entries.iter().take(MAX_LIBRARY_FOLDERS) {
        let Some(path) = value.as_object().and_then(|object| object.text("path")) else {
            continue;
        };
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            continue;
        }
        let Some(canonical) = canonical_directory_if_present(&path)? else {
            continue;
        };
        if canonical.join("steamapps").is_dir() {
            paths.insert(canonical);
        }
    }
    Ok(paths.into_iter().collect())
}

fn manifest_paths(library: &Path) -> Result<Vec<PathBuf>, GameDiscoveryError> {
    let directory = library.join("steamapps");
    let entries = fs::read_dir(&directory).map_err(|error| {
        GameDiscoveryError::new(format!(
            "could not scan Steam library {}: {error}",
            directory.display()
        ))
    })?;
    let mut paths = entries
        .flatten()
        .take(MAX_MANIFESTS_PER_LIBRARY)
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            manifest_app_id(name)?;
            entry.file_type().ok()?.is_file().then_some(entry.path())
        })
        .collect::<Vec<_>>();
    paths.sort_unstable();
    Ok(paths)
}

fn manifest_app_id(name: &str) -> Option<u32> {
    let digits = name.strip_prefix("appmanifest_")?.strip_suffix(".acf")?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let app_id = digits.parse().ok()?;
    (app_id != 0).then_some(app_id)
}

fn read_manifest(library: &Path, manifest: &Path) -> Option<(u32, String, PathBuf)> {
    let file_id = manifest_app_id(manifest.file_name()?.to_str()?)?;
    let document = read_vdf_if_present(manifest)?;
    let app_state = document.object("AppState")?;
    let app_id = app_state.text("appid")?.parse::<u32>().ok()?;
    if app_id == 0 || app_id != file_id {
        return None;
    }
    let state_flags = app_state.text("StateFlags")?.parse::<u64>().ok()?;
    if state_flags & STEAM_STATE_FULLY_INSTALLED == 0 {
        return None;
    }
    let name = app_state.text("name")?.trim();
    if name.is_empty() || name.len() > MAX_NAME_BYTES || name.chars().any(char::is_control) {
        return None;
    }
    if is_steam_support_tool(name) {
        return None;
    }
    let install_dir = app_state.text("installdir")?;
    if install_dir.len() > MAX_VDF_PATH_BYTES || !is_single_path_component(install_dir) {
        return None;
    }
    let common = fs::canonicalize(library.join("steamapps/common")).ok()?;
    let installed = fs::canonicalize(common.join(install_dir)).ok()?;
    if !installed.starts_with(&common) || !installed.is_dir() {
        return None;
    }
    Some((app_id, name.to_owned(), installed))
}

fn is_steam_support_tool(name: &str) -> bool {
    const TOOL_NAMES: &[&str] = &[
        "Proton",
        "Steam Linux Runtime",
        "Steamworks Common Redistributables",
        "Steamworks SDK Redist",
    ];

    TOOL_NAMES.iter().any(|tool| {
        name.eq_ignore_ascii_case(tool)
            || name
                .get(..tool.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(tool))
                && name.as_bytes().get(tool.len()) == Some(&b' ')
    })
}

fn is_single_path_component(value: &str) -> bool {
    let mut components = Path::new(value).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn is_shared_runtime(executable: &Path) -> bool {
    const RUNTIMES: &[&str] = &[
        "flatpak",
        "pressure-vessel",
        "proton",
        "steam",
        "wine",
        "wine-preloader",
        "wine64",
        "wine64-preloader",
    ];
    executable
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| {
            RUNTIMES
                .iter()
                .any(|runtime| name.eq_ignore_ascii_case(runtime))
        })
}

fn read_vdf_if_present(path: &Path) -> Option<VdfObject> {
    read_vdf(path).ok().flatten()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum VdfReadError {
    Inspect,
    UnsafeFile,
    Oversized,
    Open,
    ChangedDuringOpen,
    Read,
    InvalidUtf8,
    Malformed,
}

pub(super) fn read_vdf(path: &Path) -> Result<Option<VdfObject>, VdfReadError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(VdfReadError::Inspect),
    };
    if !metadata.file_type().is_file() {
        return Err(VdfReadError::UnsafeFile);
    }
    if metadata.len() > MAX_VDF_BYTES {
        return Err(VdfReadError::Oversized);
    }
    let file = File::open(path).map_err(|_| VdfReadError::Open)?;
    let opened = file.metadata().map_err(|_| VdfReadError::Open)?;
    if !opened.is_file() || opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
        return Err(VdfReadError::ChangedDuringOpen);
    }
    if opened.len() > MAX_VDF_BYTES {
        return Err(VdfReadError::Oversized);
    }
    let mut bytes = Vec::new();
    file.take(MAX_VDF_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| VdfReadError::Read)?;
    if u64::try_from(bytes.len()).map_err(|_| VdfReadError::Oversized)? > MAX_VDF_BYTES {
        return Err(VdfReadError::Oversized);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| VdfReadError::InvalidUtf8)?;
    VdfParser::new(text)
        .parse()
        .map(Some)
        .map_err(|()| VdfReadError::Malformed)
}

#[derive(Clone, Debug)]
pub(super) struct VdfObject {
    entries: Vec<(String, VdfValue)>,
}

impl VdfObject {
    fn object(&self, key: &str) -> Option<&Self> {
        self.entries.iter().find_map(|(candidate, value)| {
            (candidate.eq_ignore_ascii_case(key))
                .then(|| value.as_object())
                .flatten()
        })
    }

    fn text(&self, key: &str) -> Option<&str> {
        self.entries.iter().find_map(|(candidate, value)| {
            (candidate.eq_ignore_ascii_case(key))
                .then(|| value.as_text())
                .flatten()
        })
    }

    pub(super) fn unique_object(&self, key: &str) -> Result<Option<&Self>, ()> {
        let mut matches = self
            .entries
            .iter()
            .filter(|(candidate, _)| candidate.eq_ignore_ascii_case(key));
        let Some((_, value)) = matches.next() else {
            return Ok(None);
        };
        if matches.next().is_some() {
            return Err(());
        }
        value.as_object().map(Some).ok_or(())
    }

    pub(super) fn unique_exact_object(&self, key: &str) -> Result<Option<&Self>, ()> {
        let mut matches = self
            .entries
            .iter()
            .filter(|(candidate, _)| candidate == key);
        let Some((_, value)) = matches.next() else {
            return Ok(None);
        };
        if matches.next().is_some() {
            return Err(());
        }
        value.as_object().map(Some).ok_or(())
    }

    pub(super) fn unique_text(&self, key: &str) -> Result<Option<&str>, ()> {
        let mut matches = self
            .entries
            .iter()
            .filter(|(candidate, _)| candidate.eq_ignore_ascii_case(key));
        let Some((_, value)) = matches.next() else {
            return Ok(None);
        };
        if matches.next().is_some() {
            return Err(());
        }
        value.as_text().map(Some).ok_or(())
    }
}

#[derive(Clone, Debug)]
enum VdfValue {
    Text(String),
    Object(VdfObject),
}

impl VdfValue {
    fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            Self::Object(_) => None,
        }
    }

    fn as_object(&self) -> Option<&VdfObject> {
        match self {
            Self::Object(value) => Some(value),
            Self::Text(_) => None,
        }
    }
}

struct VdfParser<'a> {
    input: &'a [u8],
    position: usize,
    entries: usize,
}

impl<'a> VdfParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            position: 0,
            entries: 0,
        }
    }

    fn parse(mut self) -> Result<VdfObject, ()> {
        let object = self.parse_object(0, false)?;
        self.skip_space_and_comments();
        (self.position == self.input.len())
            .then_some(object)
            .ok_or(())
    }

    fn parse_object(&mut self, depth: usize, closed: bool) -> Result<VdfObject, ()> {
        if depth > MAX_VDF_DEPTH {
            return Err(());
        }
        let mut entries = Vec::new();
        loop {
            self.skip_space_and_comments();
            if self.peek() == Some(b'}') {
                if !closed {
                    return Err(());
                }
                self.position += 1;
                return Ok(VdfObject { entries });
            }
            if self.peek().is_none() {
                return (!closed).then_some(VdfObject { entries }).ok_or(());
            }
            let key = self.parse_token()?;
            self.skip_space_and_comments();
            let value = if self.peek() == Some(b'{') {
                self.position += 1;
                VdfValue::Object(self.parse_object(depth + 1, true)?)
            } else {
                VdfValue::Text(self.parse_token()?)
            };
            self.entries += 1;
            if self.entries > MAX_VDF_ENTRIES {
                return Err(());
            }
            entries.push((key, value));
        }
    }

    fn parse_token(&mut self) -> Result<String, ()> {
        self.skip_space_and_comments();
        let bytes = if self.peek() == Some(b'"') {
            self.position += 1;
            let mut output = Vec::new();
            loop {
                let byte = self.take().ok_or(())?;
                match byte {
                    b'"' => break,
                    b'\\' => {
                        let escaped = self.take().ok_or(())?;
                        match escaped {
                            b'"' | b'\\' => output.push(escaped),
                            _ => {
                                output.push(b'\\');
                                output.push(escaped);
                            }
                        }
                    }
                    _ => output.push(byte),
                }
                if output.len() > MAX_VDF_TOKEN_BYTES {
                    return Err(());
                }
            }
            output
        } else {
            let start = self.position;
            while self
                .peek()
                .is_some_and(|byte| !byte.is_ascii_whitespace() && byte != b'{' && byte != b'}')
            {
                self.position += 1;
                if self.position - start > MAX_VDF_TOKEN_BYTES {
                    return Err(());
                }
            }
            if self.position == start {
                return Err(());
            }
            self.input[start..self.position].to_vec()
        };
        String::from_utf8(bytes).map_err(|_| ())
    }

    fn skip_space_and_comments(&mut self) {
        loop {
            while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
                self.position += 1;
            }
            if self.input.get(self.position..self.position + 2) == Some(b"//") {
                self.position += 2;
                while self.peek().is_some_and(|byte| byte != b'\n') {
                    self.position += 1;
                }
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.position).copied()
    }

    fn take(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.position += 1;
        Some(byte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        home: PathBuf,
        native_client: PathBuf,
        flatpak_client: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "redunar-steam-discovery-{}-{id}",
                std::process::id()
            ));
            let home = root.join("home");
            let native_client = root.join("bin/steam");
            let flatpak_client = root.join("bin/flatpak");
            fs::create_dir_all(native_client.parent().expect("client parent")).expect("bin");
            for client in [&native_client, &flatpak_client] {
                fs::write(client, b"fixture").expect("client");
                fs::set_permissions(client, fs::Permissions::from_mode(0o700)).expect("mode");
            }
            fs::create_dir_all(&home).expect("home");
            Self {
                root,
                home,
                native_client,
                flatpak_client,
            }
        }

        fn discovery(&self) -> SteamGameDiscovery {
            SteamGameDiscovery::with_clients(&self.home, &self.native_client, &self.flatpak_client)
        }

        fn add_manifest(steam_root: &Path, app_id: u32, name: &str, install: &str) {
            fs::create_dir_all(steam_root.join("steamapps/common").join(install))
                .expect("install dir");
            fs::write(
                steam_root
                    .join("steamapps")
                    .join(format!("appmanifest_{app_id}.acf")),
                format!(
                    "\"AppState\"\n{{\n\"appid\" \"{app_id}\"\n\"name\" \"{name}\"\n\"StateFlags\" \"4\"\n\"installdir\" \"{install}\"\n}}\n"
                ),
            )
            .expect("manifest");
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).expect("fixture cleanup");
        }
    }

    #[test]
    fn discovers_arc_raiders_from_flatpak_root_with_typed_launch() {
        let fixture = Fixture::new();
        let steam = fixture
            .home
            .join(".var/app/com.valvesoftware.Steam/.local/share/Steam");
        Fixture::add_manifest(&steam, 1_808_500, "ARC Raiders", "Arc Raiders");

        let games = fixture.discovery().discover().expect("discover");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].display_name, "ARC Raiders");
        assert_eq!(
            games[0].match_rules,
            vec![GameMatchRule::SteamAppId(1_808_500)]
        );
        assert_eq!(
            games[0].launch.as_ref().expect("launch").arguments,
            ["run", FLATPAK_STEAM_APP, "-applaunch", "1808500"]
                .into_iter()
                .map(OsString::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn follows_bounded_extra_library_folder_and_deduplicates_app_ids() {
        let fixture = Fixture::new();
        let steam = fixture.home.join(".local/share/Steam");
        let extra = fixture.root.join("extra-library");
        fs::create_dir_all(steam.join("steamapps")).expect("primary");
        fs::create_dir_all(extra.join("steamapps")).expect("extra");
        fs::write(
            steam.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} }}",
                extra.display()
            ),
        )
        .expect("libraries");
        Fixture::add_manifest(&extra, 42, "Extra Game", "Extra Game");
        Fixture::add_manifest(&steam, 42, "Duplicate", "Duplicate");

        let games = fixture.discovery().discover().expect("discover");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].source, GameDiscoverySource::Steam { app_id: 42 });
    }

    #[test]
    fn rejects_manifest_path_escape_and_mismatched_identity() {
        let fixture = Fixture::new();
        let steam = fixture.home.join(".local/share/Steam");
        fs::create_dir_all(steam.join("steamapps/common")).expect("common");
        fs::write(
            steam.join("steamapps/appmanifest_7.acf"),
            "\"AppState\" { \"appid\" \"8\" \"name\" \"Escape\" \"StateFlags\" \"4\" \"installdir\" \"../outside\" }",
        )
        .expect("manifest");
        assert!(fixture.discovery().discover().expect("discover").is_empty());
    }

    #[test]
    fn ignores_incomplete_or_uninstalled_manifests() {
        let fixture = Fixture::new();
        let steam = fixture.home.join(".local/share/Steam");
        fs::create_dir_all(steam.join("steamapps/common/Partial")).expect("partial dir");
        fs::write(
            steam.join("steamapps/appmanifest_99.acf"),
            "\"AppState\" { \"appid\" \"99\" \"name\" \"Partial\" \"StateFlags\" \"2\" \"installdir\" \"Partial\" }",
        )
        .expect("partial manifest");
        fs::write(
            steam.join("steamapps/appmanifest_100.acf"),
            "\"AppState\" { \"appid\" \"100\" \"name\" \"Uninstalled\" \"StateFlags\" \"1\" \"installdir\" \"Partial\" }",
        )
        .expect("uninstalled manifest");

        assert!(fixture.discovery().discover().expect("discover").is_empty());
    }

    #[test]
    fn excludes_steam_support_tools_from_the_game_catalog() {
        let fixture = Fixture::new();
        let steam = fixture.home.join(".local/share/Steam");
        Fixture::add_manifest(&steam, 1, "Proton Experimental", "Proton - Experimental");
        Fixture::add_manifest(&steam, 2, "Proton 11.0", "Proton 11.0");
        Fixture::add_manifest(
            &steam,
            3,
            "Steam Linux Runtime 4.0",
            "SteamLinuxRuntime_soldier",
        );
        Fixture::add_manifest(
            &steam,
            4,
            "Steamworks Common Redistributables",
            "Steamworks Shared",
        );
        Fixture::add_manifest(&steam, 5, "ARC Raiders", "Arc Raiders");

        let games = fixture.discovery().discover().expect("discover");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].display_name, "ARC Raiders");
    }

    #[test]
    fn banner_prefers_nested_hero_and_never_uses_a_portrait() {
        let fixture = Fixture::new();
        let cache = fixture
            .home
            .join(".local/share/Steam/appcache/librarycache");
        fs::create_dir_all(cache.join("42/hash")).unwrap();
        let poster = cache.join("42/library_600x900.jpg");
        fs::write(&poster, b"poster").unwrap();
        assert_eq!(fixture.discovery().local_banner(42), None);
        let header = cache.join("42_header.jpg");
        fs::write(&header, b"header").unwrap();
        assert_eq!(
            fixture.discovery().local_banner(42),
            Some(fs::canonicalize(&header).unwrap())
        );
        let hero = cache.join("42/hash/library_hero.jpg");
        fs::write(&hero, b"hero").unwrap();
        assert_eq!(
            fixture.discovery().local_banner(42),
            Some(fs::canonicalize(&hero).unwrap())
        );
        assert_eq!(
            fixture.discovery().local_poster(42),
            Some(fs::canonicalize(poster).unwrap())
        );
        fs::remove_file(&hero).unwrap();
        std::os::unix::fs::symlink(&header, &hero).unwrap();
        assert_eq!(
            fixture.discovery().local_banner(42),
            Some(fs::canonicalize(&header).unwrap())
        );
    }

    #[test]
    fn discovers_local_portrait_art_in_flat_and_app_directories() {
        let fixture = Fixture::new();
        let steam = fixture.home.join(".local/share/Steam");
        Fixture::add_manifest(&steam, 41, "Flat art", "Flat art");
        Fixture::add_manifest(&steam, 42, "Nested art", "Nested art");
        let cache = steam.join("appcache/librarycache");
        fs::create_dir_all(cache.join("42")).expect("poster directories");
        let flat_poster = cache.join("41_library_600x900.jpg");
        let nested_poster = cache.join("42/library_600x900.png");
        fs::write(&flat_poster, b"local poster").expect("flat poster");
        fs::write(&nested_poster, b"local poster").expect("nested poster");

        let games = fixture.discovery().discover().expect("discover");
        assert_eq!(games.len(), 2);
        assert_eq!(
            games[0].poster_path,
            Some(fs::canonicalize(flat_poster).expect("flat poster path"))
        );
        assert_eq!(
            games[1].poster_path,
            Some(fs::canonicalize(nested_poster).expect("nested poster path"))
        );
    }

    #[test]
    fn discovers_one_level_nested_local_portrait_art() {
        let fixture = Fixture::new();
        let steam = fixture.home.join(".local/share/Steam");
        Fixture::add_manifest(&steam, 43, "Variant art", "Variant art");
        let poster = steam
            .join("appcache/librarycache/43/custom")
            .join("library_600x900.jpg");
        fs::create_dir_all(poster.parent().expect("poster parent")).expect("poster directories");
        fs::write(&poster, b"local poster").expect("poster");

        let games = fixture.discovery().discover().expect("discover");
        assert_eq!(games.len(), 1);
        assert_eq!(
            games[0].poster_path,
            Some(fs::canonicalize(poster).expect("poster path"))
        );
    }

    #[test]
    fn rejects_symlinked_and_oversized_local_portraits() {
        let fixture = Fixture::new();
        let steam = fixture.home.join(".local/share/Steam");
        let cache = steam.join("appcache/librarycache");
        fs::create_dir_all(cache.join("44")).expect("poster directories");

        let outside = fixture.root.join("outside-poster.jpg");
        fs::write(&outside, b"outside").expect("outside poster");
        symlink(&outside, cache.join("44/library_600x900.jpg")).expect("poster symlink");
        assert_eq!(cached_poster_in_root(&steam, 44), None);

        let oversized = cache.join("45_library_600x900.jpg");
        let file = File::create(&oversized).expect("oversized poster");
        file.set_len(MAX_POSTER_BYTES + 1)
            .expect("oversized poster length");
        assert_eq!(cached_poster_in_root(&steam, 45), None);
    }

    #[test]
    fn exact_running_process_candidate_rejects_shared_runtime() {
        let fixture = Fixture::new();
        let game = fixture.root.join("bin/native-game");
        fs::write(&game, b"game").expect("game");
        fs::set_permissions(&game, fs::Permissions::from_mode(0o700)).expect("mode");
        let proc_root = fixture.root.join("proc");
        fs::create_dir_all(proc_root.join("91")).expect("proc game");
        symlink(&game, proc_root.join("91/exe")).expect("game exe link");
        let candidate = candidate_from_process_at(
            &proc_root,
            &GameProcess {
                pid: 91,
                comm: "Native Game".into(),
                executable: game.clone(),
                steam_app_id: Some(9),
                game_mode_active: false,
            },
        )
        .expect("candidate");
        assert_eq!(candidate.launch.expect("launch").executable, game);

        fs::create_dir_all(proc_root.join("92")).expect("proc runtime");
        symlink(&fixture.flatpak_client, proc_root.join("92/exe")).expect("runtime exe link");
        assert!(
            candidate_from_process_at(
                &proc_root,
                &GameProcess {
                    pid: 92,
                    comm: "WindowsGame.exe".into(),
                    executable: fixture.flatpak_client.clone(),
                    steam_app_id: Some(10),
                    game_mode_active: false,
                }
            )
            .is_err()
        );
    }

    #[test]
    #[ignore = "requires a local Flatpak Steam ARC Raiders installation"]
    fn host_flatpak_manifest_discovers_arc_raiders_without_network() {
        let home = std::env::var_os("HOME").map(PathBuf::from).expect("HOME");
        let manifest = home.join(
            ".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/appmanifest_1808500.acf",
        );
        assert!(manifest.is_file(), "ARC Raiders manifest is not installed");
        let games = SteamGameDiscovery::new(home).discover().expect("discover");
        let arc = games
            .iter()
            .find(|game| game.source == GameDiscoverySource::Steam { app_id: 1_808_500 })
            .expect("ARC Raiders candidate");
        assert_eq!(arc.display_name, "ARC Raiders");
        assert!(arc.install_directory.is_dir());
        assert!(arc.launch.is_some(), "Flatpak client should be available");
    }
}
