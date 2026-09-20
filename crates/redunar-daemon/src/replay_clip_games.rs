//! Bounded local ledger mapping saved Replay clip names to the game that
//! recorded each clip.
//!
//! The ledger is labeling data for the Recent captures view only. A clip is
//! fully usable without it, so every read degrades to empty and every write
//! failure leaves the clip and the save itself untouched. The inventory falls
//! back to matching a clip's commit time against recorded session windows.
use crate::SessionRecord;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::Mutex;

const FILE: &str = "replay-clip-games-v1.tsv";
const HEADER: &str = "redunar-replay-clip-games-v1";
const MAX_ENTRIES: usize = 512;
const MAX_FILE_NAME_BYTES: usize = 128;
static OPERATIONS: Mutex<()> = Mutex::new(());

/// Clip names arrive from the store, but validation here keeps a future
/// caller from writing a line the reader could never parse back.
fn is_storable_file_name(file_name: &str) -> bool {
    !file_name.is_empty()
        && file_name.len() <= MAX_FILE_NAME_BYTES
        && file_name.bytes().all(|byte| byte.is_ascii_graphic())
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

/// Single-pass decoder so sequences like a backslash before t are
/// not double-interpreted by chained replacements.
fn unescape(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        match characters.next() {
            Some('\\') | None => decoded.push('\\'),
            Some('t') => decoded.push('\t'),
            Some('n') => decoded.push('\n'),
            Some(other) => {
                decoded.push('\\');
                decoded.push(other);
            }
        }
    }
    decoded
}

/// Append (or replace) the attribution for one clip and rewrite the bounded
/// ledger atomically.
///
/// # Errors
///
/// Returns an error when the name is not storable or the private state
/// directory cannot hold the rewritten ledger.
pub(crate) fn record(state: &Path, file_name: &str, game_name: &str) -> io::Result<()> {
    if !is_storable_file_name(file_name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "replay clip name cannot be stored",
        ));
    }
    let _guard = OPERATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut entries = match load_unlocked(state) {
        Ok(entries) => entries,
        // A ledger this version cannot read is recoverable labeling data, so
        // start a fresh file instead of failing every future attribution.
        Err(error) if error.kind() == io::ErrorKind::InvalidData => Vec::new(),
        Err(error) => return Err(error),
    };
    entries.retain(|(name, _)| name != file_name);
    entries.push((file_name.to_owned(), game_name.to_owned()));
    if entries.len() > MAX_ENTRIES {
        let excess = entries.len() - MAX_ENTRIES;
        entries.drain(..excess);
    }
    save_unlocked(state, &entries)
}

/// Read the ledger for inventory joins. Any failure is a normal empty ledger.
pub(crate) fn games(state: &Path) -> Vec<(String, String)> {
    let _guard = OPERATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    load_unlocked(state).unwrap_or_default()
}

/// Resolve one clip's game: an exact ledger line wins, otherwise the clip's
/// commit time must fall inside exactly one distinct recorded session game.
/// Ambiguity resolves to none rather than guessing.
#[must_use]
pub(crate) fn resolve(
    file_name: &str,
    ledger: &[(String, String)],
    sessions: &[SessionRecord],
) -> Option<String> {
    if let Some((_, game)) = ledger.iter().rev().find(|(name, _)| name == file_name) {
        return Some(game.clone());
    }
    let committed = commit_unix_seconds(file_name)?;
    let mut resolved: Option<String> = None;
    for session in sessions {
        // Two seconds of padding on each side absorbs duration truncation
        // and clock rounding at the edges of a recorded session window.
        let started = session.started_unix.saturating_sub(2);
        let ended = session
            .started_unix
            .saturating_add(u64::from(session.duration_seconds) + 2);
        if committed < started || committed > ended {
            continue;
        }
        match &resolved {
            Some(game) if *game != session.game => return None,
            Some(_) => {}
            None => resolved = Some(session.game.clone()),
        }
    }
    resolved
}

/// Parse the commit wall-clock seconds encoded in a
/// name pattern: `redunar-replay-<secs>-<nanos>-<id>.<ext>`.
fn commit_unix_seconds(file_name: &str) -> Option<u64> {
    let prefixed = file_name.strip_prefix("redunar-replay-")?;
    let middle = prefixed
        .strip_suffix(".mkv")
        .or_else(|| prefixed.strip_suffix(".mp4"))?;
    let seconds = middle.split('-').next()?;
    if seconds.is_empty() || !seconds.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    seconds.parse().ok()
}

fn load_unlocked(state: &Path) -> io::Result<Vec<(String, String)>> {
    let path = state.join(FILE);
    let mut text = String::new();
    match fs::File::open(&path) {
        Ok(mut file) => {
            file.read_to_string(&mut text)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    }
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "replay clip game ledger header is invalid",
        ));
    }
    Ok(lines.filter_map(parse).collect())
}

fn parse(line: &str) -> Option<(String, String)> {
    let mut fields = line.split('\t');
    let file_name = fields.next()?;
    if !is_storable_file_name(file_name) {
        return None;
    }
    let game = unescape(fields.next()?);
    if fields.next().is_some() {
        return None;
    }
    Some((file_name.to_owned(), game))
}

fn save_unlocked(state: &Path, entries: &[(String, String)]) -> io::Result<()> {
    fs::create_dir_all(state)?;
    let path = state.join(FILE);
    let temporary = state.join(format!(".{FILE}.tmp"));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)?;
    writeln!(file, "{HEADER}")?;
    for (name, game) in entries {
        writeln!(file, "{name}\t{}", escape(game))?;
    }
    file.sync_all()?;
    fs::rename(&temporary, &path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
}

#[cfg(test)]
mod tests {
    use super::{games, record, resolve};
    use crate::SessionRecord;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_state(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "redunar-clip-games-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn session(game: &str, started_unix: u64, duration_seconds: u32) -> SessionRecord {
        SessionRecord {
            id: 1,
            game: game.to_owned(),
            started_unix,
            duration_seconds,
            average_fps: None,
            one_percent_low_fps: None,
            point_one_percent_low_fps: None,
            frame_intervals_ns: Vec::new(),
            timeline: Vec::new(),
        }
    }

    #[test]
    fn round_trips_sanitized_game_names() {
        let root = temp_state("round-trip");
        let clip = "redunar-replay-1000-000000123-0.mkv";
        record(&root, clip, "A game\twith\nnewlines \\ done").unwrap();
        let loaded = games(&root);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].0, clip);
        assert_eq!(loaded[0].1, "A game\twith\nnewlines \\ done");
        // Re-recording the same clip replaces rather than duplicates.
        record(&root, clip, "Second Name").unwrap();
        let loaded = games(&root);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].1, "Second Name");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn skips_malformed_lines_and_bad_names() {
        let root = temp_state("malformed");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("replay-clip-games-v1.tsv"),
            "redunar-replay-clip-games-v1\n\
good.mkv\tGame One\n\
extra\tcolumns\there\n\
\ttab-leading name\n",
        )
        .unwrap();
        let loaded = games(&root);
        assert_eq!(loaded, vec![("good.mkv".to_owned(), "Game One".to_owned())]);
        assert!(record(&root, "not a clip name.txt", "X").is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ledger_stays_bounded_keeping_newest_entries() {
        let root = temp_state("bounded");
        for index in 0..520u32 {
            record(&root, &format!("clip-{index}.mkv"), "Game").unwrap();
        }
        let loaded = games(&root);
        assert_eq!(loaded.len(), 512);
        assert_eq!(loaded.first().unwrap().0, "clip-8.mkv");
        assert_eq!(loaded.last().unwrap().0, "clip-519.mkv");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn exact_ledger_entry_wins_over_window_fallback() {
        let ledger = vec![(
            "redunar-replay-5000-000000001-0.mkv".to_owned(),
            "Ledger".to_owned(),
        )];
        let sessions = vec![session("Window Game", 4000, 5000)];
        assert_eq!(
            resolve("redunar-replay-5000-000000001-0.mkv", &ledger, &sessions).as_deref(),
            Some("Ledger")
        );
    }

    #[test]
    fn fallback_requires_one_unambiguous_session_window() {
        let inside = "redunar-replay-2500-000000001-0.mp4";
        let between = "redunar-replay-4000-000000001-0.mkv";
        let sessions = vec![session("First", 2000, 1000), session("Second", 5000, 1000)];
        assert_eq!(resolve(inside, &[], &sessions).as_deref(), Some("First"));
        assert_eq!(resolve(between, &[], &sessions), None);
        // Overlapping sessions of the same game remain one attribution.
        let overlapping = "redunar-replay-2150-000000001-0.mkv";
        let same = vec![session("First", 2000, 1000), session("First", 2100, 100)];
        assert_eq!(resolve(overlapping, &[], &same).as_deref(), Some("First"));
        // Sessions of different names covering the same clip stay ambiguous.
        let clash = vec![session("First", 2000, 1000), session("Other", 2100, 100)];
        assert_eq!(resolve(overlapping, &[], &clash), None);
        // A malformed name with no parseable commit time gets nothing.
        assert_eq!(resolve("clip.mkv", &[], &sessions), None);
    }
}
