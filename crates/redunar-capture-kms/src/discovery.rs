use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const MAX_DRM_ENTRIES: usize = 128;
const MAX_CARD_INDEX: u8 = 15;
const MAX_CONNECTOR_NAME_BYTES: usize = 64;
const MAX_STATUS_BYTES: u64 = 64;
const MAX_ENABLED_BYTES: u64 = 64;
const MAX_MODES_BYTES: u64 = 4 * 1024;
const MAX_MODE_LINES: usize = 128;
const MAX_UNIQUE_MODES: usize = 64;
const MAX_MODE_DIMENSION: u32 = 16_384;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KmsMode {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KmsOutput {
    pub card_index: u8,
    pub connector: String,
    pub card_path: PathBuf,
    pub sysfs_path: PathBuf,
    pub modes: Vec<KmsMode>,
}

#[derive(Debug)]
pub struct KmsDiscoveryError {
    context: &'static str,
    source: Option<io::Error>,
}

impl KmsDiscoveryError {
    fn new(context: &'static str) -> Self {
        Self {
            context,
            source: None,
        }
    }

    fn io(context: &'static str, source: io::Error) -> Self {
        Self {
            context,
            source: Some(source),
        }
    }
}

impl fmt::Display for KmsDiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.context)
    }
}

impl Error for KmsDiscoveryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

/// Discover connected DRM connector records below a selectable filesystem
/// root. Tests use fixtures; production diagnostics pass `/`.
///
/// # Errors
///
/// Returns an error for unreadable or unbounded DRM metadata. A disconnected
/// connector is ignored rather than treated as a failure.
pub fn discover_outputs_at(root: &Path) -> Result<Vec<KmsOutput>, KmsDiscoveryError> {
    let drm_root = root.join("sys/class/drm");
    let entries = fs::read_dir(&drm_root)
        .map_err(|error| KmsDiscoveryError::io("DRM connector metadata is unavailable", error))?;
    let mut outputs = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index >= MAX_DRM_ENTRIES {
            return Err(KmsDiscoveryError::new(
                "DRM connector metadata exceeds the bounded entry limit",
            ));
        }
        let entry = entry
            .map_err(|error| KmsDiscoveryError::io("DRM connector entry is unreadable", error))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some((card_index, connector)) = parse_connector_name(&name) else {
            continue;
        };
        let path = entry.path();
        if read_bounded(&path.join("status"), MAX_STATUS_BYTES)?.trim() != "connected" {
            continue;
        }
        match read_optional_bounded(&path.join("enabled"), MAX_ENABLED_BYTES)? {
            Some(value) if value.trim() != "enabled" => continue,
            Some(_) | None => {}
        }
        let modes = parse_modes(&read_bounded(&path.join("modes"), MAX_MODES_BYTES)?)?;
        outputs.push(KmsOutput {
            card_index,
            connector,
            card_path: PathBuf::from(format!("/dev/dri/card{card_index}")),
            sysfs_path: path,
            modes,
        });
    }
    outputs.sort_by(|left, right| {
        left.card_index
            .cmp(&right.card_index)
            .then_with(|| left.connector.cmp(&right.connector))
    });
    Ok(outputs)
}

fn parse_connector_name(name: &str) -> Option<(u8, String)> {
    let (card, connector) = name.split_once('-')?;
    let card_index = card.strip_prefix("card")?.parse::<u8>().ok()?;
    if card_index > MAX_CARD_INDEX
        || connector.is_empty()
        || connector.len() > MAX_CONNECTOR_NAME_BYTES
        || !connector
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return None;
    }
    Some((card_index, connector.to_owned()))
}

fn read_bounded(path: &Path, limit: u64) -> Result<String, KmsDiscoveryError> {
    let file = File::open(path)
        .map_err(|error| KmsDiscoveryError::io("DRM connector property is unreadable", error))?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| KmsDiscoveryError::io("DRM connector property is unreadable", error))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(KmsDiscoveryError::new(
            "DRM connector property exceeds its bounded size",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| KmsDiscoveryError::new("DRM connector property is not UTF-8"))
}

fn read_optional_bounded(path: &Path, limit: u64) -> Result<Option<String>, KmsDiscoveryError> {
    match File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(limit.saturating_add(1))
                .read_to_end(&mut bytes)
                .map_err(|error| {
                    KmsDiscoveryError::io("DRM connector property is unreadable", error)
                })?;
            if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
                return Err(KmsDiscoveryError::new(
                    "DRM connector property exceeds its bounded size",
                ));
            }
            String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| KmsDiscoveryError::new("DRM connector property is not UTF-8"))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(KmsDiscoveryError::io(
            "DRM connector property is unreadable",
            error,
        )),
    }
}

fn parse_modes(value: &str) -> Result<Vec<KmsMode>, KmsDiscoveryError> {
    let mut modes = Vec::new();
    for (line_index, line) in value.lines().filter(|line| !line.is_empty()).enumerate() {
        if line_index >= MAX_MODE_LINES {
            return Err(KmsDiscoveryError::new(
                "DRM connector mode list exceeds the bounded mode limit",
            ));
        }
        let (width, height) = line
            .split_once('x')
            .ok_or_else(|| KmsDiscoveryError::new("DRM connector mode is malformed"))?;
        let width = width
            .parse::<u32>()
            .map_err(|_| KmsDiscoveryError::new("DRM connector mode is malformed"))?;
        let height = height
            .parse::<u32>()
            .map_err(|_| KmsDiscoveryError::new("DRM connector mode is malformed"))?;
        if width == 0 || height == 0 || width > MAX_MODE_DIMENSION || height > MAX_MODE_DIMENSION {
            return Err(KmsDiscoveryError::new(
                "DRM connector mode is outside the supported bounds",
            ));
        }
        let mode = KmsMode { width, height };
        // Sysfs may repeat dimensions for multiple refresh-rate variants. The
        // diagnostic plan needs bounded dimensions, not duplicate timing rows.
        if !modes.contains(&mode) {
            if modes.len() >= MAX_UNIQUE_MODES {
                return Err(KmsDiscoveryError::new(
                    "DRM connector mode list exceeds the bounded unique-mode limit",
                ));
            }
            modes.push(mode);
        }
    }
    Ok(modes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "redunar-kms-discovery-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(root.join("sys/class/drm")).expect("fixture root");
            Self { root }
        }

        fn connector(&self, name: &str, status: &str, modes: &str) {
            let path = self.root.join("sys/class/drm").join(name);
            fs::create_dir_all(&path).expect("connector");
            fs::write(path.join("status"), status).expect("status");
            fs::write(path.join("modes"), modes).expect("modes");
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn discovery_returns_only_connected_bounded_connectors() {
        let fixture = Fixture::new();
        fixture.connector("card1-DP-2", "connected\n", "2560x1440\n1920x1080\n");
        fixture.connector("card0-HDMI-A-1", "disconnected\n", "1920x1080\n");
        let disabled = fixture.root.join("sys/class/drm/card2-DP-3");
        fs::create_dir_all(&disabled).expect("disabled connector");
        fs::write(disabled.join("status"), "connected\n").expect("disabled status");
        fs::write(disabled.join("enabled"), "disabled\n").expect("disabled state");
        fs::write(disabled.join("modes"), "1920x1080\n").expect("disabled modes");
        fs::create_dir_all(fixture.root.join("sys/class/drm/renderD128")).expect("render node");

        let outputs = discover_outputs_at(&fixture.root).expect("outputs");
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].card_index, 1);
        assert_eq!(outputs[0].connector, "DP-2");
        assert_eq!(outputs[0].card_path, Path::new("/dev/dri/card1"));
        assert_eq!(
            outputs[0].modes,
            vec![
                KmsMode {
                    width: 2_560,
                    height: 1_440,
                },
                KmsMode {
                    width: 1_920,
                    height: 1_080,
                },
            ]
        );
    }

    #[test]
    fn malformed_or_unbounded_connector_metadata_fails_closed() {
        let fixture = Fixture::new();
        fixture.connector("card1-DP-1", "connected\n", "not-a-mode\n");
        assert!(discover_outputs_at(&fixture.root).is_err());

        let oversized_modes = (0..=MAX_MODE_LINES).fold(String::new(), |mut modes, index| {
            writeln!(modes, "{}x720", 640 + index).expect("string write");
            modes
        });
        fs::write(
            fixture.root.join("sys/class/drm/card1-DP-1/modes"),
            oversized_modes,
        )
        .expect("oversized modes");
        assert!(discover_outputs_at(&fixture.root).is_err());
    }
}
