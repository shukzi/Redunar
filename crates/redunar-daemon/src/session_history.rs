//! Bounded local journal for completed Redunar sessions.
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const FILE: &str = "session-history-v1.tsv";
const HEADER: &str = "redunar-session-history-v1";
const MAX_RECORDS: usize = 64;
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
            "session history header is invalid",
        ));
    }
    Ok(lines.filter_map(parse).collect())
}

fn parse(line: &str) -> Option<SessionRecord> {
    let mut fields = line.split('\t');
    let id = fields.next()?.parse().ok()?;
    let game = fields.next()?.replace("\\t", "\t").replace("\\n", "\n");
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
    fs::create_dir_all(state)?;
    let path = state.join(FILE);
    let temporary = state.join(format!(".{FILE}.tmp"));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)?;
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
    file.sync_all()?;
    fs::rename(temporary, path)
}
fn format_opt(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| format!("{value:.4}"))
}

fn parse_timeline(value: &str) -> Vec<SessionTelemetrySample> {
    value
        .split(';')
        .filter(|sample| !sample.is_empty())
        .filter_map(|sample| {
            let fields = sample.split(',').collect::<Vec<_>>();
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
            game: "A game\twith\nnewlines".to_owned(),
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
}
