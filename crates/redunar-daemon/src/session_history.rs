//! Bounded local journal for completed Redunar sessions.
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const FILE: &str = "session-history-v1.tsv";
const HEADER: &str = "redunar-session-history-v1";
const MAX_RECORDS: usize = 64;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
static NEXT_WRITE: AtomicU64 = AtomicU64::new(0);
const MAX_TIMELINE_SAMPLES: usize = 7_200;
static OPERATIONS: Mutex<()> = Mutex::new(());

/// One bounded observation in a completed game session. Values remain
/// optional because frame capture and hardware sensors can become available
/// at different times during launch and shutdown.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionTelemetrySample {
    pub elapsed_seconds: u32,
    pub fps: Option<f64>,
    pub frame_time_ms: Option<f64>,
    pub cpu_temperature_celsius: Option<f64>,
    pub gpu_temperature_celsius: Option<f64>,
    pub cpu_utilization_percent: Option<f64>,
    pub gpu_utilization_percent: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionRecord {
    pub id: u64,
    pub game: String,
    pub started_unix: u64,
    pub duration_seconds: u32,
    pub average_fps: Option<f64>,
    pub one_percent_low_fps: Option<f64>,
    pub point_one_percent_low_fps: Option<f64>,
    pub frame_intervals_ns: Vec<u64>,
    pub timeline: Vec<SessionTelemetrySample>,
}

pub(crate) fn load(state: &Path) -> io::Result<Vec<SessionRecord>> {
    let _guard = OPERATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    load_unlocked(state)
}

pub(crate) fn append(state: &Path, mut record: SessionRecord) -> io::Result<()> {
    let _guard = OPERATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut records = load_unlocked(state)?;
    record.id = records.last().map_or(1, |item| item.id.saturating_add(1));
    records.push(record);
    if records.len() > MAX_RECORDS {
        records.drain(..records.len() - MAX_RECORDS);
    }
    save_unlocked(state, &records)
}

pub(crate) fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn load_unlocked(state: &Path) -> io::Result<Vec<SessionRecord>> {
    let path = state.join(FILE);
    let mut text = String::new();
    match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)
    {
        Ok(file) => {
            let metadata = file.metadata()?;
            if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "session history exceeds its file bounds",
                ));
            }
            file.take(MAX_FILE_BYTES + 1).read_to_string(&mut text)?;
            if text.len() as u64 > MAX_FILE_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "session history grew beyond its file bound",
                ));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    }
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session history header is invalid",
        ));
    }
    let mut records = lines
        .rev()
        .filter_map(parse)
        .take(MAX_RECORDS)
        .collect::<Vec<_>>();
    records.reverse();
    Ok(records)
}

fn parse(line: &str) -> Option<SessionRecord> {
    let mut fields = line.split('\t');
    let id = fields.next()?.parse().ok()?;
    let escaped_game = fields.next()?;
    if escaped_game.len() > 4096 {
        return None;
    }
    let game = unescape(escaped_game);
    let started_unix = fields.next()?.parse().ok()?;
    let duration_seconds = fields.next()?.parse().ok()?;
    let average_fps = parse_opt(fields.next()?);
    let one_percent_low_fps = parse_opt(fields.next()?);
    let point_one_percent_low_fps = parse_opt(fields.next()?);
    let frame_intervals_ns = fields
        .next()
        .unwrap_or_default()
        .split(',')
        .filter_map(|value| value.parse().ok())
        .take(240)
        .collect();
    let timeline = parse_timeline(fields.next().unwrap_or_default());
    Some(SessionRecord {
        id,
        game,
        started_unix,
        duration_seconds,
        average_fps,
        one_percent_low_fps,
        point_one_percent_low_fps,
        frame_intervals_ns,
        timeline,
    })
}
fn parse_opt(value: &str) -> Option<f64> {
    (value != "-").then(|| value.parse().ok()).flatten()
}
fn save_unlocked(state: &Path, records: &[SessionRecord]) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(state)?;
    if !fs::symlink_metadata(state)?.is_dir() {
        return Err(io::Error::other("session state is not a directory"));
    }
    fs::set_permissions(state, fs::Permissions::from_mode(0o700))?;
    let path = state.join(FILE);
    let temporary = state.join(format!(
        ".{FILE}.{}-{}.tmp",
        std::process::id(),
        NEXT_WRITE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .mode(0o600)
        .write(true)
        .open(&temporary)?;
    let result = (|| {
        writeln!(file, "{HEADER}")?;
        for record in records {
            let game = record
                .game
                .replace('\\', "\\\\")
                .replace('\t', "\\t")
                .replace('\n', "\\n");
            let intervals = record
                .frame_intervals_ns
                .iter()
                .take(240)
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            writeln!(
                file,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                record.id,
                game,
                record.started_unix,
                record.duration_seconds,
                format_opt(record.average_fps),
                format_opt(record.one_percent_low_fps),
                format_opt(record.point_one_percent_low_fps),
                intervals,
                format_timeline(&record.timeline)
            )?;
        }
        if file.metadata()?.len() > MAX_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session history exceeds its file bound",
            ));
        }
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::File::open(state)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

// Decode the existing v1 writer once, left to right. Chained replacements
// mistake a literal backslash followed by n/t for an escaped control character.
fn unescape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        match chars.next() {
            Some('n') => output.push('\n'),
            Some('t') => output.push('\t'),
            Some('\\') | None => output.push('\\'),
            Some(other) => {
                output.push('\\');
                output.push(other);
            }
        }
    }
    output
}
fn format_opt(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| format!("{value:.4}"))
}

fn parse_timeline(value: &str) -> Vec<SessionTelemetrySample> {
    value
        .split(';')
        .filter(|sample| !sample.is_empty())
        .filter_map(|sample| {
            let fields = sample.splitn(8, ',').collect::<Vec<_>>();
            if fields.len() != 7 {
                return None;
            }
            Some(SessionTelemetrySample {
                elapsed_seconds: fields[0].parse().ok()?,
                fps: parse_opt(fields[1]),
                frame_time_ms: parse_opt(fields[2]),
                cpu_temperature_celsius: parse_opt(fields[3]),
                gpu_temperature_celsius: parse_opt(fields[4]),
                cpu_utilization_percent: parse_opt(fields[5]),
                gpu_utilization_percent: parse_opt(fields[6]),
            })
        })
        .take(MAX_TIMELINE_SAMPLES)
        .collect()
}

fn format_timeline(samples: &[SessionTelemetrySample]) -> String {
    samples
        .iter()
        .take(MAX_TIMELINE_SAMPLES)
        .map(|sample| {
            format!(
                "{},{},{},{},{},{},{}",
                sample.elapsed_seconds,
                format_opt(sample.fps),
                format_opt(sample.frame_time_ms),
                format_opt(sample.cpu_temperature_celsius),
                format_opt(sample.gpu_temperature_celsius),
                format_opt(sample.cpu_utilization_percent),
                format_opt(sample.gpu_utilization_percent),
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

#[cfg(test)]
mod tests {
    use super::{SessionRecord, SessionTelemetrySample, append, load};
    use std::fmt::Write as _;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};
    #[test]
    fn round_trips_bounded_session_records() {
        let root = std::env::temp_dir().join(format!(
            "redunar-history-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let record = SessionRecord {
            id: 0,
            game: "A game\twith\nnewlines and \\new \\tabs \\unknown".to_owned(),
            started_unix: 42,
            duration_seconds: 63,
            average_fps: Some(144.0),
            one_percent_low_fps: Some(90.0),
            point_one_percent_low_fps: None,
            frame_intervals_ns: vec![16_000_000, 17_000_000],
            timeline: vec![SessionTelemetrySample {
                elapsed_seconds: 12,
                fps: Some(59.8),
                frame_time_ms: Some(16.7),
                cpu_temperature_celsius: Some(62.0),
                gpu_temperature_celsius: Some(68.5),
                cpu_utilization_percent: Some(31.0),
                gpu_utilization_percent: Some(94.0),
            }],
        };
        append(&root, record.clone()).unwrap();
        let loaded = load(&root).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].game, record.game);
        assert_eq!(loaded[0].frame_intervals_ns, record.frame_intervals_ns);
        assert_eq!(loaded[0].timeline, record.timeline);
        assert_eq!(
            std::fs::metadata(root.join(super::FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn loads_legacy_records_without_timeline_samples() {
        let root = std::env::temp_dir().join(format!(
            "redunar-legacy-history-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("session-history-v1.tsv"),
            "redunar-session-history-v1\n1\tLegacy\t42\t60\t60.0000\t50.0000\t40.0000\t16666667\n",
        )
        .unwrap();
        let loaded = load(&root).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].timeline.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn oversized_history_and_symlinks_fail_without_touching_targets() {
        let root =
            std::env::temp_dir().join(format!("redunar-history-bounds-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(super::FILE);
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(super::MAX_FILE_BYTES + 1).unwrap();
        assert!(load(&root).is_err());
        drop(file);
        std::fs::remove_file(&path).unwrap();
        let target = root.join("unrelated");
        std::fs::write(&target, "preserve").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(load(&root).is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "preserve");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loading_keeps_only_the_newest_bounded_records() {
        let root =
            std::env::temp_dir().join(format!("redunar-history-records-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut contents = format!("{}\n", super::HEADER);
        for id in 1..=70 {
            writeln!(contents, "{id}\tFixture\t42\t60\t60\t50\t40\t16666667").unwrap();
        }
        std::fs::write(root.join(super::FILE), contents).unwrap();
        let records = load(&root).unwrap();
        assert_eq!(records.len(), 64);
        assert_eq!(records[0].id, 7);
        assert_eq!(records[63].id, 70);
        std::fs::remove_dir_all(root).unwrap();
    }
}
