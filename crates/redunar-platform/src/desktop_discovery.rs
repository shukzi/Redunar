use crate::{DiscoveredGame, GameDiscoveryError, GameDiscoverySource};
use redunar_core::{GameLaunchConfig, GameMatchRule};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

const MAX_DESKTOP_FILE_BYTES: u64 = 256 * 1024;
const MAX_DESKTOP_FILES: usize = 8 * 1024;
const MAX_DISCOVERED_GAMES: usize = 512;
const MAX_NAME_BYTES: usize = 256;
const MAX_EXEC_BYTES: usize = 32 * 1024;
const MAX_EXEC_ARGUMENTS: usize = 64;
const MAX_PATH_BYTES: usize = 4096;
const MAX_POSTER_BYTES: u64 = 20 * 1024 * 1024;

/// Bounded read-only discovery of direct native games advertised through XDG
/// desktop entries. Entries which launch a shared store/runtime are ignored:
/// those need a dedicated adapter with stronger game identity.
#[derive(Clone, Debug)]
pub struct DesktopGameDiscovery {
    application_directories: Vec<PathBuf>,
    executable_search_directories: Vec<PathBuf>,
}

impl DesktopGameDiscovery {
    /// Build discovery from the current user's XDG data and executable search
    /// paths. Empty or relative environment entries are ignored.
    #[must_use]
    pub fn from_environment(home_directory: &Path) -> Self {
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home_directory.join(".local/share"));
        let mut application_directories = vec![data_home.join("applications")];
        let data_directories = std::env::var_os("XDG_DATA_DIRS")
            .unwrap_or_else(|| OsString::from("/usr/local/share:/usr/share"));
        application_directories.extend(
            std::env::split_paths(&data_directories)
                .filter(|path| path.is_absolute())
                .map(|path| path.join("applications")),
        );
        let executable_search_directories = std::env::var_os("PATH")
            .map(|value| {
                std::env::split_paths(&value)
                    .filter(|path| path.is_absolute())
                    .collect()
            })
            .unwrap_or_default();
        Self::new(application_directories, executable_search_directories)
    }

    #[must_use]
    pub fn new(
        application_directories: Vec<PathBuf>,
        executable_search_directories: Vec<PathBuf>,
    ) -> Self {
        Self {
            application_directories,
            executable_search_directories,
        }
    }

    /// Discover direct native games without executing an entry or invoking a
    /// shell. Malformed individual entries are skipped; an application root
    /// that exists but cannot be enumerated is reported.
    ///
    /// # Errors
    ///
    /// Returns [`GameDiscoveryError`] when configured roots are relative or an
    /// existing application directory cannot be read safely.
    pub fn discover(&self) -> Result<Vec<DiscoveredGame>, GameDiscoveryError> {
        if self
            .application_directories
            .iter()
            .chain(&self.executable_search_directories)
            .any(|path| !path.is_absolute())
        {
            return Err(GameDiscoveryError::new(
                "desktop game discovery paths must be absolute",
            ));
        }

        let mut files = Vec::new();
        let mut seen_directories = BTreeSet::new();
        for directory in &self.application_directories {
            collect_desktop_files(directory, &mut files, &mut seen_directories)?;
            if files.len() >= MAX_DESKTOP_FILES {
                break;
            }
        }

        // Earlier XDG roots have precedence. Deduplicating by executable also
        // avoids presenting several desktop actions for the same local game.
        let mut games = BTreeMap::<PathBuf, DiscoveredGame>::new();
        for path in files {
            if games.len() >= MAX_DISCOVERED_GAMES {
                break;
            }
            let Some(game) = self.read_candidate(&path) else {
                continue;
            };
            let Some(executable) = game.launch.as_ref().map(|launch| launch.executable.clone())
            else {
                continue;
            };
            games.entry(executable).or_insert(game);
        }
        Ok(games.into_values().collect())
    }

    fn read_candidate(&self, path: &Path) -> Option<DiscoveredGame> {
        let contents = read_bounded_regular_file(path)?;
        let entry = DesktopEntry::parse(&contents)?;
        if entry.entry_type.as_deref() != Some("Application")
            || entry.hidden
            || entry.no_display
            || entry.terminal
            || !entry.categories.iter().any(|category| category == "Game")
        {
            return None;
        }
        let name = entry.name?.trim().to_owned();
        if name.is_empty() || name.len() > MAX_NAME_BYTES || name.chars().any(char::is_control) {
            return None;
        }

        let tokens = tokenize_exec(entry.exec.as_deref()?)?;
        let (program, arguments) = normalize_exec_tokens(tokens)?;
        let executable = self.resolve_executable(Path::new(&program))?;
        // Check both names. Distribution launchers are commonly symlinks whose
        // canonical target has a different helper name (Fedora's `steam`, for
        // example, resolves to `bin_steam.sh`). Neither name is a game identity.
        if is_shared_launcher(Path::new(&program)) || is_shared_launcher(&executable) {
            return None;
        }
        if let Some(try_exec) = entry.try_exec
            && self.resolve_executable(Path::new(&try_exec)).is_none()
        {
            return None;
        }
        let working_directory = entry
            .working_directory
            .as_deref()
            .and_then(canonical_safe_directory);
        if entry.working_directory.is_some() && working_directory.is_none() {
            return None;
        }
        let install_directory = working_directory
            .clone()
            .or_else(|| executable.parent().map(Path::to_path_buf))?;
        let executable_name = executable.file_name()?.to_str()?.to_owned();
        let poster_path = entry.icon.as_deref().and_then(canonical_safe_poster);

        Some(DiscoveredGame {
            display_name: name,
            install_directory,
            poster_path,
            launch: Some(GameLaunchConfig {
                executable: executable.clone(),
                arguments,
                working_directory,
            }),
            match_rules: vec![
                GameMatchRule::Executable(executable),
                GameMatchRule::ExecutableName(executable_name),
            ],
            source: GameDiscoverySource::DesktopEntry {
                path: path.to_path_buf(),
            },
        })
    }

    fn resolve_executable(&self, program: &Path) -> Option<PathBuf> {
        if program.is_absolute() {
            return canonical_executable(program);
        }
        if program.components().count() != 1
            || !matches!(program.components().next(), Some(Component::Normal(_)))
        {
            return None;
        }
        self.executable_search_directories
            .iter()
            .find_map(|directory| canonical_executable(&directory.join(program)))
    }
}

fn collect_desktop_files(
    directory: &Path,
    files: &mut Vec<PathBuf>,
    seen_directories: &mut BTreeSet<PathBuf>,
) -> Result<(), GameDiscoveryError> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(GameDiscoveryError::new(format!(
                "could not inspect local applications at {}: {error}",
                directory.display()
            )));
        }
    };
    if !metadata.is_dir() {
        return Err(GameDiscoveryError::new(format!(
            "local application path is not a directory: {}",
            directory.display()
        )));
    }
    let canonical = fs::canonicalize(directory).map_err(|error| {
        GameDiscoveryError::new(format!(
            "could not resolve local applications at {}: {error}",
            directory.display()
        ))
    })?;
    if !seen_directories.insert(canonical.clone()) {
        return Ok(());
    }
    let entries = fs::read_dir(&canonical).map_err(|error| {
        GameDiscoveryError::new(format!(
            "could not read local applications at {}: {error}",
            canonical.display()
        ))
    })?;
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension() == Some(OsStr::new("desktop")))
        .collect::<Vec<_>>();
    paths.sort();
    let remaining = MAX_DESKTOP_FILES.saturating_sub(files.len());
    files.extend(paths.into_iter().take(remaining));
    Ok(())
}

fn read_bounded_regular_file(path: &Path) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_DESKTOP_FILE_BYTES {
        return None;
    }
    let file = File::open(path).ok()?;
    let opened = file.metadata().ok()?;
    if !opened.is_file()
        || opened.dev() != metadata.dev()
        || opened.ino() != metadata.ino()
        || opened.len() > MAX_DESKTOP_FILE_BYTES
    {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_DESKTOP_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if u64::try_from(bytes.len()).ok()? > MAX_DESKTOP_FILE_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

#[derive(Default)]
struct DesktopEntry {
    entry_type: Option<String>,
    name: Option<String>,
    exec: Option<String>,
    try_exec: Option<String>,
    working_directory: Option<String>,
    icon: Option<String>,
    categories: Vec<String>,
    hidden: bool,
    no_display: bool,
    terminal: bool,
}

impl DesktopEntry {
    fn parse(contents: &str) -> Option<Self> {
        let mut result = Self::default();
        let mut in_desktop_entry = false;
        let mut seen_desktop_entry = false;
        for raw_line in contents.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                in_desktop_entry = line == "[Desktop Entry]";
                if in_desktop_entry {
                    if seen_desktop_entry {
                        return None;
                    }
                    seen_desktop_entry = true;
                }
                continue;
            }
            if !in_desktop_entry {
                continue;
            }
            let (key, value) = line.split_once('=')?;
            match key {
                "Type" => set_once(&mut result.entry_type, value)?,
                "Name" => set_once(&mut result.name, value)?,
                "Exec" => set_once(&mut result.exec, value)?,
                "TryExec" => set_once(&mut result.try_exec, value)?,
                "Path" => set_once(&mut result.working_directory, value)?,
                "Icon" => set_once(&mut result.icon, value)?,
                "Categories" => {
                    if !result.categories.is_empty() {
                        return None;
                    }
                    result.categories = value
                        .split(';')
                        .filter(|category| !category.is_empty())
                        .map(str::to_owned)
                        .collect();
                }
                "Hidden" => result.hidden = parse_bool(value)?,
                "NoDisplay" => result.no_display = parse_bool(value)?,
                "Terminal" => result.terminal = parse_bool(value)?,
                _ => {}
            }
        }
        seen_desktop_entry.then_some(result)
    }
}

fn set_once(destination: &mut Option<String>, value: &str) -> Option<()> {
    if destination.is_some() || value.len() > MAX_EXEC_BYTES || value.contains('\0') {
        return None;
    }
    *destination = Some(value.to_owned());
    Some(())
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn tokenize_exec(exec: &str) -> Option<Vec<String>> {
    if exec.is_empty() || exec.len() > MAX_EXEC_BYTES {
        return None;
    }
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut token_started = false;
    for character in exec.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            token_started = true;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '"' => {
                quoted = !quoted;
                token_started = true;
            }
            character if character.is_whitespace() && !quoted => {
                if token_started {
                    tokens.push(std::mem::take(&mut current));
                    token_started = false;
                    if tokens.len() > MAX_EXEC_ARGUMENTS + 1 {
                        return None;
                    }
                }
            }
            character if character.is_control() => return None,
            character => {
                current.push(character);
                token_started = true;
            }
        }
    }
    if escaped || quoted {
        return None;
    }
    if token_started {
        tokens.push(current);
    }
    (!tokens.is_empty() && tokens.len() <= MAX_EXEC_ARGUMENTS + 1).then_some(tokens)
}

fn normalize_exec_tokens(tokens: Vec<String>) -> Option<(String, Vec<OsString>)> {
    let mut normalized = Vec::new();
    for token in tokens {
        if matches!(token.as_str(), "%f" | "%F" | "%u" | "%U") {
            continue;
        }
        let mut output = String::new();
        let mut characters = token.chars();
        while let Some(character) = characters.next() {
            if character != '%' {
                output.push(character);
                continue;
            }
            match characters.next()? {
                '%' => output.push('%'),
                // Substituting display text, icon arguments, or the desktop
                // file path would require additional trusted context. Reject
                // instead of silently changing launch semantics.
                _ => return None,
            }
        }
        normalized.push(output);
    }
    let program = normalized.first()?.clone();
    let arguments = normalized.into_iter().skip(1).map(OsString::from).collect();
    Some((program, arguments))
}

fn canonical_executable(path: &Path) -> Option<PathBuf> {
    if path.as_os_str().as_encoded_bytes().len() > MAX_PATH_BYTES {
        return None;
    }
    let canonical = fs::canonicalize(path).ok()?;
    let metadata = fs::metadata(&canonical).ok()?;
    (metadata.is_file() && metadata.permissions().mode() & 0o111 != 0).then_some(canonical)
}

fn canonical_safe_directory(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    if !path.is_absolute() || path.as_os_str().as_encoded_bytes().len() > MAX_PATH_BYTES {
        return None;
    }
    let canonical = fs::canonicalize(path).ok()?;
    canonical.is_dir().then_some(canonical)
}

fn canonical_safe_poster(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    if !path.is_absolute() || path.as_os_str().as_encoded_bytes().len() > MAX_PATH_BYTES {
        return None;
    }
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_POSTER_BYTES {
        return None;
    }
    fs::canonicalize(path).ok()
}

fn is_shared_launcher(executable: &Path) -> bool {
    const SHARED_LAUNCHERS: &[&str] = &[
        "bash",
        "bin_steam.sh",
        "dash",
        "env",
        "flatpak",
        "fish",
        "gamescope",
        "heroic",
        "lutris",
        "perl",
        "proton",
        "python",
        "python3",
        "redunar-daemon",
        "redunar-steam-launch",
        "redunar-tauri",
        "sh",
        "steam",
        "steam.sh",
        "steam-runtime-steam-remote",
        "wine",
        "wine64",
        "zsh",
    ];
    executable
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| {
            SHARED_LAUNCHERS
                .iter()
                .any(|launcher| name.eq_ignore_ascii_case(launcher))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        applications: PathBuf,
        binaries: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "redunar-desktop-discovery-{}-{id}",
                std::process::id()
            ));
            let applications = root.join("applications");
            let binaries = root.join("bin");
            fs::create_dir_all(&applications).expect("application fixture");
            fs::create_dir_all(&binaries).expect("binary fixture");
            Self {
                root,
                applications,
                binaries,
            }
        }

        fn executable(&self, name: &str) -> PathBuf {
            let path = self.binaries.join(name);
            fs::write(&path, b"fixture").expect("write executable");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("mode");
            path
        }

        fn desktop(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.applications.join(name);
            fs::write(&path, contents).expect("write desktop entry");
            path
        }

        fn discovery(&self) -> DesktopGameDiscovery {
            DesktopGameDiscovery::new(vec![self.applications.clone()], vec![self.binaries.clone()])
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).expect("remove fixture");
        }
    }

    #[test]
    fn discovers_direct_game_with_shell_free_arguments() {
        let fixture = Fixture::new();
        let executable = fixture.executable("native-game");
        let desktop = fixture.desktop(
            "native.desktop",
            "[Desktop Entry]\nType=Application\nName=Native Game\nCategories=Game;ActionGame;\nExec=native-game --mode \"fast run\" %F --literal=100%%\n",
        );

        let games = fixture.discovery().discover().expect("discover");
        assert_eq!(games.len(), 1);
        let game = &games[0];
        assert_eq!(game.display_name, "Native Game");
        assert_eq!(
            game.launch.as_ref().expect("launch").executable,
            fs::canonicalize(executable).expect("canonical executable")
        );
        assert_eq!(
            game.launch.as_ref().expect("launch").arguments,
            ["--mode", "fast run", "--literal=100%"].map(OsString::from)
        );
        assert_eq!(
            game.source,
            GameDiscoverySource::DesktopEntry { path: desktop }
        );
    }

    #[test]
    fn ignores_non_games_hidden_entries_and_shared_launchers() {
        let fixture = Fixture::new();
        fixture.executable("editor");
        fixture.executable("native-game");
        fixture.executable("sh");
        fixture.executable("steam");
        fixture.executable("bin_steam.sh");
        fixture.executable("redunar-tauri");
        fixture.desktop(
            "editor.desktop",
            "[Desktop Entry]\nType=Application\nName=Editor\nCategories=Utility;\nExec=editor\n",
        );
        fixture.desktop(
            "hidden.desktop",
            "[Desktop Entry]\nType=Application\nName=Hidden Game\nHidden=true\nCategories=Game;\nExec=native-game\n",
        );
        fixture.desktop(
            "store.desktop",
            "[Desktop Entry]\nType=Application\nName=Store Game\nCategories=Game;\nExec=steam -applaunch 42\n",
        );
        fixture.desktop(
            "canonical-store.desktop",
            "[Desktop Entry]\nType=Application\nName=Steam\nCategories=Network;Game;\nExec=bin_steam.sh %U\n",
        );
        fixture.desktop(
            "redunar.desktop",
            "[Desktop Entry]\nType=Application\nName=Redunar\nCategories=Game;\nExec=redunar-tauri\n",
        );
        fixture.desktop(
            "redunar-tauri.desktop",
            "[Desktop Entry]\nType=Application\nName=Redunar Tauri\nCategories=Game;\nExec=redunar-tauri\n",
        );

        assert!(fixture.discovery().discover().expect("discover").is_empty());
    }

    #[test]
    fn rejects_shell_syntax_fields_and_unusable_working_directories() {
        let fixture = Fixture::new();
        fixture.executable("native-game");
        fixture.desktop(
            "field.desktop",
            "[Desktop Entry]\nType=Application\nName=Field Game\nCategories=Game;\nExec=native-game %c\n",
        );
        fixture.desktop(
            "shell.desktop",
            "[Desktop Entry]\nType=Application\nName=Shell Game\nCategories=Game;\nExec=sh -c native-game\n",
        );
        fixture.desktop(
            "path.desktop",
            "[Desktop Entry]\nType=Application\nName=Path Game\nCategories=Game;\nPath=relative/path\nExec=native-game\n",
        );

        assert!(fixture.discovery().discover().expect("discover").is_empty());
    }

    #[test]
    fn earlier_xdg_root_wins_for_duplicate_executables() {
        let fixture = Fixture::new();
        fixture.executable("native-game");
        let second = fixture.root.join("system-applications");
        fs::create_dir_all(&second).expect("second root");
        fixture.desktop(
            "user.desktop",
            "[Desktop Entry]\nType=Application\nName=User Name\nCategories=Game;\nExec=native-game\n",
        );
        fs::write(
            second.join("system.desktop"),
            "[Desktop Entry]\nType=Application\nName=System Name\nCategories=Game;\nExec=native-game\n",
        )
        .expect("system entry");
        let discovery = DesktopGameDiscovery::new(
            vec![fixture.applications.clone(), second],
            vec![fixture.binaries.clone()],
        );

        let games = discovery.discover().expect("discover");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].display_name, "User Name");
    }
}
