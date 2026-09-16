//! Native release metadata verification for the in-app updater.
//!
//! Development builds stay source-unconfigured unless a test endpoint is
//! supplied. Release builds embed the public GitHub release channel, while the
//! native process owns downloads and signature verification in every case.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const SOURCE_ENV: &str = "REDUNAR_UPDATE_SOURCE_URL";
const ALLOW_INSECURE_ENV: &str = "REDUNAR_ALLOW_INSECURE_URL";
const RELEASE_SOURCE: Option<&str> = option_env!("REDUNAR_UPDATE_SOURCE_URL");
const PUBLIC_KEY_PEM: &str = include_str!("../../../../packaging/release-signing-public.pem");
const MAX_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const UPDATE_CACHE_FOLDER: &str = "redunar/updates";
const PENDING_UPDATE_FILE: &str = "pending-update.txt";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheckStatus {
    pub current_version: String,
    pub state: String,
    pub message: String,
    pub latest_version: Option<String>,
    pub package_kind: Option<String>,
    pub asset_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInstallStatus {
    pub state: String,
    pub message: String,
}

pub fn check_for_updates() -> UpdateCheckStatus {
    let current_version = env!("CARGO_PKG_VERSION").to_owned();
    if let Some(status) = reconcile_pending_update(&current_version) {
        return status;
    }
    let Some(source) = env::var(SOURCE_ENV)
        .ok()
        .or_else(|| RELEASE_SOURCE.map(str::to_owned))
    else {
        return unavailable(
            current_version,
            "Signed update checking is not configured in this build yet.",
        );
    };
    let source = source.trim_end_matches('/').to_owned();
    if source.is_empty() {
        return unavailable(
            current_version,
            "Signed update checking is not configured in this build yet.",
        );
    }
    if !is_allowed_source(&source) {
        return error_status(current_version, "Update sources must use HTTPS.");
    }

    let temporary_directory = match temporary_directory() {
        Ok(path) => path,
        Err(error) => {
            return error_status(
                current_version,
                &format!("Could not prepare update verification: {error}"),
            )
        }
    };
    let result = check_source(&source, &current_version, &temporary_directory);
    let _ = fs::remove_dir_all(&temporary_directory);
    result
}

fn check_source(source: &str, current_version: &str, directory: &Path) -> UpdateCheckStatus {
    let version_path = directory.join("VERSION");
    let manifest_path = directory.join("SHA256SUMS");
    let signature_path = directory.join("SHA256SUMS.sig");
    for (name, path) in [
        ("VERSION", &version_path),
        ("SHA256SUMS", &manifest_path),
        ("SHA256SUMS.sig", &signature_path),
    ] {
        if let Err(error) = download(&format!("{source}/{name}"), path) {
            return error_status(
                current_version.to_owned(),
                &format!("Could not download signed update metadata: {error}"),
            );
        }
    }
    let manifest = match fs::read_to_string(&manifest_path) {
        Ok(value) => value,
        Err(error) => {
            return error_status(
                current_version.to_owned(),
                &format!("Could not read update manifest: {error}"),
            )
        }
    };
    if let Err(error) = verify_signature(&manifest_path, &signature_path, directory) {
        return error_status(
            current_version.to_owned(),
            &format!("The update manifest signature is invalid: {error}"),
        );
    }
    let latest_version = match fs::read_to_string(&version_path) {
        Ok(value) => value.trim().to_owned(),
        Err(error) => {
            return error_status(
                current_version.to_owned(),
                &format!("Could not read release version: {error}"),
            )
        }
    };
    if !valid_version(&latest_version) {
        return error_status(
            current_version.to_owned(),
            "The signed release version is invalid.",
        );
    }
    if !is_newer_version(&latest_version, current_version) {
        return UpdateCheckStatus {
            current_version: current_version.to_owned(),
            state: "up-to-date".into(),
            message: format!("Redunar {current_version} is up to date."),
            latest_version: Some(latest_version),
            package_kind: None,
            asset_name: None,
        };
    }
    let package = match package_target() {
        Ok(package) => package,
        Err(error) => {
            return error_status(
                current_version.to_owned(),
                &format!("This system cannot select a Redunar update package: {error}"),
            )
        }
    };
    let manifest_entries = match parse_manifest(&manifest) {
        Ok(entries) => entries,
        Err(error) => {
            return error_status(
                current_version.to_owned(),
                &format!("The update manifest is invalid: {error}"),
            )
        }
    };
    let Some(expected_digest) = manifest_entries.get(package.asset_name) else {
        return error_status(
            current_version.to_owned(),
            &format!(
                "The signed release does not include {}.",
                package.asset_name
            ),
        );
    };
    let package_path = directory.join(package.asset_name);
    if let Err(error) = download_with_limit(
        &format!("{source}/{}", package.asset_name),
        &package_path,
        MAX_PACKAGE_BYTES,
    ) {
        return error_status(
            current_version.to_owned(),
            &format!("Could not download the Redunar update package: {error}"),
        );
    }
    if let Err(error) = verify_checksum(&package_path, expected_digest) {
        return error_status(
            current_version.to_owned(),
            &format!("The downloaded update package failed checksum verification: {error}"),
        );
    }
    if let Err(error) =
        persist_verified_package(&package_path, package, &latest_version, expected_digest)
    {
        return error_status(
            current_version.to_owned(),
            &format!("The verified update package could not be retained: {error}"),
        );
    }
    UpdateCheckStatus {
        current_version: current_version.to_owned(),
        state: "available".into(),
        message: format!("Redunar {latest_version} has a verified update package."),
        latest_version: Some(latest_version),
        package_kind: Some(package.kind.into()),
        asset_name: Some(package.asset_name.into()),
    }
}

pub fn install_update() -> UpdateInstallStatus {
    let pending = match read_pending_update() {
        Ok(update) => update,
        Err(error) => return install_error(error),
    };
    if !is_newer_version(&pending.version, env!("CARGO_PKG_VERSION")) {
        return install_error("The retained update is not newer than this build.".into());
    }
    let target = match package_target() {
        Ok(target) => target,
        Err(error) => {
            return install_error(format!("This system cannot install the update: {error}"))
        }
    };
    if pending.asset_name != target.asset_name || pending.kind != target.kind {
        return install_error("The downloaded package does not match this system.".into());
    }
    let path = match update_cache_directory() {
        Ok(directory) => directory.join(&pending.asset_name),
        Err(error) => return install_error(error),
    };
    if let Err(error) = verify_checksum(&path, &pending.digest) {
        return install_error(format!(
            "The retained update package failed verification: {error}"
        ));
    }
    let opener = ["/usr/bin/xdg-open", "/bin/xdg-open"]
        .iter()
        .map(Path::new)
        .find(|path| path.is_file());
    let Some(opener) = opener else {
        return install_error("The desktop package installer (xdg-open) is not installed.".into());
    };
    if let Err(error) = Command::new(opener)
        .arg(&path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        return install_error(format!("Could not open the package installer: {error}"));
    }
    if let Err(error) = write_pending_update(&PendingUpdate {
        state: "handoff".into(),
        ..pending.clone()
    }) {
        return install_error(format!(
            "Package installer opened, but update state could not be saved: {error}"
        ));
    }
    UpdateInstallStatus {
        state: "handoff".into(),
        message: format!(
            "Redunar {} was verified and handed to the desktop package installer.",
            pending.version
        ),
    }
}

fn install_error(message: String) -> UpdateInstallStatus {
    UpdateInstallStatus {
        state: "error".into(),
        message,
    }
}

fn unavailable(current_version: String, message: &str) -> UpdateCheckStatus {
    UpdateCheckStatus {
        current_version,
        state: "not-configured".into(),
        message: message.into(),
        latest_version: None,
        package_kind: None,
        asset_name: None,
    }
}

fn error_status(current_version: impl Into<String>, message: &str) -> UpdateCheckStatus {
    UpdateCheckStatus {
        current_version: current_version.into(),
        state: "error".into(),
        message: message.into(),
        latest_version: None,
        package_kind: None,
        asset_name: None,
    }
}

fn is_allowed_source(source: &str) -> bool {
    source.starts_with("https://") || env::var(ALLOW_INSECURE_ENV).ok().as_deref() == Some("1")
}

fn temporary_directory() -> Result<PathBuf, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let path = env::temp_dir().join(format!("redunar-update-{}-{nanos}", std::process::id()));
    fs::create_dir(&path).map_err(|error| error.to_string())?;
    Ok(path)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingUpdate {
    state: String,
    version: String,
    kind: String,
    asset_name: String,
    digest: String,
}

fn update_cache_directory() -> Result<PathBuf, String> {
    let base = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or("could not determine a user cache directory")?;
    let directory = base.join(UPDATE_CACHE_FOLDER);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    fs::set_permissions(
        &directory,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .map_err(|error| error.to_string())?;
    Ok(directory)
}

fn persist_verified_package(
    source: &Path,
    package: PackageTarget,
    version: &str,
    digest: &str,
) -> Result<(), String> {
    let directory = update_cache_directory()?;
    let temporary = directory.join(format!(".{}.download", package.asset_name));
    let destination = directory.join(package.asset_name);
    fs::copy(source, &temporary).map_err(|error| error.to_string())?;
    if let Err(error) = verify_checksum(&temporary, digest) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    fs::rename(&temporary, &destination).map_err(|error| error.to_string())?;
    write_pending_update(&PendingUpdate {
        state: "downloaded".into(),
        version: version.to_owned(),
        kind: package.kind.to_owned(),
        asset_name: package.asset_name.to_owned(),
        digest: digest.to_owned(),
    })
}

fn write_pending_update(update: &PendingUpdate) -> Result<(), String> {
    let directory = update_cache_directory()?;
    let metadata = format!(
        "state={}\nversion={}\nkind={}\nasset_name={}\ndigest={}\n",
        update.state, update.version, update.kind, update.asset_name, update.digest
    );
    let metadata_temporary = directory.join(".pending-update.txt.tmp");
    fs::write(&metadata_temporary, metadata).map_err(|error| error.to_string())?;
    fs::rename(metadata_temporary, directory.join(PENDING_UPDATE_FILE))
        .map_err(|error| error.to_string())
}

fn read_pending_update() -> Result<PendingUpdate, String> {
    let directory = update_cache_directory()?;
    let metadata = fs::read_to_string(directory.join(PENDING_UPDATE_FILE))
        .map_err(|error| format!("no verified update is ready: {error}"))?;
    parse_pending_update(&metadata)
}

fn parse_pending_update(metadata: &str) -> Result<PendingUpdate, String> {
    let mut values = BTreeMap::new();
    for line in metadata.lines() {
        let (key, value) = line
            .split_once('=')
            .ok_or("retained update metadata is malformed")?;
        if values.insert(key, value).is_some() {
            return Err("retained update metadata contains a duplicate field".into());
        }
    }
    let state = values
        .get("state")
        .copied()
        .ok_or("retained update metadata has no state")?;
    let version = values
        .get("version")
        .copied()
        .ok_or("retained update metadata has no version")?;
    let kind = values
        .get("kind")
        .copied()
        .ok_or("retained update metadata has no package kind")?;
    let asset_name = values
        .get("asset_name")
        .copied()
        .ok_or("retained update metadata has no package name")?;
    let digest = values
        .get("digest")
        .copied()
        .ok_or("retained update metadata has no checksum")?;
    if !matches!(state, "downloaded" | "handoff")
        || !valid_version(version)
        || !matches!(kind, "deb" | "rpm" | "arch")
        || asset_name.contains('/')
        || asset_name.contains('\\')
        || digest.len() != 64
        || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("retained update metadata is invalid".into());
    }
    Ok(PendingUpdate {
        state: state.to_owned(),
        version: version.to_owned(),
        kind: kind.to_owned(),
        asset_name: asset_name.to_owned(),
        digest: digest.to_ascii_lowercase(),
    })
}

fn reconcile_pending_update(current_version: &str) -> Option<UpdateCheckStatus> {
    let pending = read_pending_update().ok()?;
    let directory = update_cache_directory().ok()?;
    let package_path = directory.join(&pending.asset_name);
    if !is_newer_version(&pending.version, current_version) {
        let _ = fs::remove_file(package_path);
        let _ = fs::remove_file(directory.join(PENDING_UPDATE_FILE));
        return Some(UpdateCheckStatus {
            current_version: current_version.to_owned(),
            state: "up-to-date".into(),
            message: format!("Redunar {current_version} is up to date."),
            latest_version: Some(current_version.to_owned()),
            package_kind: None,
            asset_name: None,
        });
    }
    if verify_checksum(&package_path, &pending.digest).is_err() {
        let _ = fs::remove_file(package_path);
        let _ = fs::remove_file(directory.join(PENDING_UPDATE_FILE));
        return None;
    }
    let message = if pending.state == "handoff" {
        format!(
            "Redunar {} is waiting for the desktop package installer to finish.",
            pending.version
        )
    } else {
        format!("Redunar {} has a verified update package.", pending.version)
    };
    Some(UpdateCheckStatus {
        current_version: current_version.to_owned(),
        state: "available".into(),
        message,
        latest_version: Some(pending.version),
        package_kind: Some(pending.kind),
        asset_name: Some(pending.asset_name),
    })
}

fn download(url: &str, destination: &Path) -> Result<(), String> {
    download_with_limit(url, destination, MAX_METADATA_BYTES)
}

fn download_with_limit(url: &str, destination: &Path, maximum_bytes: u64) -> Result<(), String> {
    let curl = ["/usr/bin/curl", "/bin/curl"]
        .iter()
        .map(Path::new)
        .find(|path| path.is_file())
        .ok_or("curl is not installed")?;
    let output = destination
        .to_str()
        .ok_or("temporary update path is not valid UTF-8")?;
    let mut command = Command::new(curl);
    command.args([
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--max-time",
        "20",
        "--max-filesize",
        &maximum_bytes.to_string(),
        "--output",
        output,
    ]);
    if env::var(ALLOW_INSECURE_ENV).ok().as_deref() != Some("1") {
        command.args(["--proto", "=https", "--tlsv1.2"]);
    }
    let status = command
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("curl exited with {status}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PackageTarget {
    kind: &'static str,
    asset_name: &'static str,
}

fn package_target() -> Result<PackageTarget, String> {
    let architecture =
        env::var("REDUNAR_ARCHITECTURE").unwrap_or_else(|_| std::env::consts::ARCH.into());
    if !matches!(architecture.as_str(), "x86_64" | "amd64") {
        return Err(format!("unsupported architecture: {architecture}"));
    }
    let path = env::var_os("REDUNAR_OS_RELEASE_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/os-release"));
    let content = fs::read_to_string(path)
        .map_err(|error| format!("could not read distribution information: {error}"))?;
    let values = content
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key, value.trim_matches('"').trim_matches('\'').to_owned()))
        .collect::<BTreeMap<_, _>>();
    let id = values.get("ID").map(String::as_str).unwrap_or_default();
    let like = values
        .get("ID_LIKE")
        .map(String::as_str)
        .unwrap_or_default();
    select_package(
        &architecture,
        id,
        like,
        Path::new("/run/ostree-booted").exists(),
    )
}

fn select_package(
    architecture: &str,
    id: &str,
    like: &str,
    immutable_or_musl: bool,
) -> Result<PackageTarget, String> {
    if !matches!(architecture, "x86_64" | "amd64") {
        return Err(format!("unsupported architecture: {architecture}"));
    }
    if matches!(id, "alpine" | "nixos" | "steamos") || immutable_or_musl {
        return Err(format!(
            "immutable or musl distribution is not supported ({id})"
        ));
    }
    let words = format!(" {id} {like} ");
    if words
        .split_whitespace()
        .any(|word| matches!(word, "ubuntu" | "debian" | "linuxmint" | "pop"))
    {
        return Ok(PackageTarget {
            kind: "deb",
            asset_name: "redunar-app-linux-amd64.deb",
        });
    }
    if words.split_whitespace().any(|word| {
        matches!(
            word,
            "fedora" | "rhel" | "centos" | "rocky" | "almalinux" | "nobara"
        )
    }) {
        return Ok(PackageTarget {
            kind: "rpm",
            asset_name: "redunar-app-linux-x86_64.rpm",
        });
    }
    if words
        .split_whitespace()
        .any(|word| matches!(word, "arch" | "manjaro" | "endeavouros" | "cachyos"))
    {
        return Ok(PackageTarget {
            kind: "arch",
            asset_name: "redunar-app-linux-x86_64.pkg.tar.zst",
        });
    }
    if words.split_whitespace().any(|word| {
        matches!(
            word,
            "opensuse" | "opensuse-tumbleweed" | "opensuse-leap" | "sles" | "suse"
        )
    }) {
        return Ok(PackageTarget {
            kind: "rpm",
            asset_name: "redunar-app-linux-opensuse-x86_64.rpm",
        });
    }
    Err(format!("unsupported distribution: {id}"))
}

fn verify_checksum(path: &Path, expected: &str) -> Result<(), String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_PACKAGE_BYTES {
        return Err("package exceeds the size limit".into());
    }
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read =
            std::io::Read::read(&mut file, &mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let actual = format!("{:x}", digest.finalize());
    if actual == expected {
        Ok(())
    } else {
        Err(format!("expected {expected}, got {actual}"))
    }
}

fn verify_signature(manifest: &Path, signature: &Path, directory: &Path) -> Result<(), String> {
    let public_key = directory.join("release-public-key.pem");
    let mut file = File::create(&public_key).map_err(|error| error.to_string())?;
    file.write_all(PUBLIC_KEY_PEM.as_bytes())
        .map_err(|error| error.to_string())?;
    let openssl = ["/usr/bin/openssl", "/bin/openssl"]
        .iter()
        .map(Path::new)
        .find(|path| path.is_file())
        .ok_or("OpenSSL is not installed")?;
    let status = Command::new(openssl)
        .args(["dgst", "-sha256", "-verify"])
        .arg(&public_key)
        .args(["-signature"])
        .arg(signature)
        .arg(manifest)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("openssl exited with {status}"))
    }
}

fn parse_manifest(input: &str) -> Result<BTreeMap<String, String>, String> {
    if input.len() as u64 > MAX_METADATA_BYTES {
        return Err("manifest exceeds the size limit".into());
    }
    let mut entries = BTreeMap::new();
    for line in input.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split_whitespace();
        let digest = fields.next().ok_or("manifest entry has no digest")?;
        let name = fields.next().ok_or("manifest entry has no filename")?;
        if fields.next().is_some()
            || digest.len() != 64
            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || name.is_empty()
            || name.contains('/')
            || name.contains('\\')
            || name == "."
            || name == ".."
        {
            return Err(format!("invalid manifest entry: {line}"));
        }
        if entries
            .insert(name.to_owned(), digest.to_ascii_lowercase())
            .is_some()
        {
            return Err(format!("duplicate manifest entry: {name}"));
        }
    }
    if entries.is_empty() {
        return Err("manifest is empty".into());
    }
    Ok(entries)
}

fn valid_version(version: &str) -> bool {
    let trimmed = version.strip_prefix('v').unwrap_or(version);
    let parts: Vec<_> = trimmed.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.chars().all(|character| character.is_ascii_digit())
        })
}

fn is_newer_version(candidate: &str, current: &str) -> bool {
    let parse = |value: &str| {
        value
            .trim_start_matches('v')
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let candidate = parse(candidate);
    let current = parse(current);
    candidate > current
}

#[cfg(test)]
mod tests {
    use super::{
        is_newer_version, parse_manifest, parse_pending_update, select_package, valid_version,
        verify_checksum, PackageTarget,
    };
    use sha2::{Digest, Sha256};

    #[test]
    fn manifest_parser_accepts_sha256sum_output_and_rejects_paths() {
        let manifest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  redunar-app-linux-x86_64.rpm\n";
        let parsed = parse_manifest(manifest).expect("valid manifest");
        assert_eq!(parsed.len(), 1);
        assert!(parse_manifest(&format!("{}  ../escape", "0".repeat(64))).is_err());
    }

    #[test]
    fn manifest_parser_rejects_duplicates_and_bad_digests() {
        let digest = "a".repeat(64);
        let manifest = format!("{digest}  one\n{digest}  one\n");
        assert!(parse_manifest(&manifest).is_err());
        assert!(parse_manifest("not-a-digest  one").is_err());
    }

    #[test]
    fn versions_are_strict_and_numeric() {
        assert!(valid_version("0.1.0"));
        assert!(valid_version("v1.2.3"));
        assert!(!valid_version("1.2"));
        assert!(is_newer_version("0.2.0", "0.1.9"));
        assert!(!is_newer_version("0.1.0", "0.1.0"));
    }

    #[test]
    fn package_targets_match_supported_distribution_families() {
        assert_eq!(
            select_package("x86_64", "ubuntu", "debian", false),
            Ok(PackageTarget {
                kind: "deb",
                asset_name: "redunar-app-linux-amd64.deb"
            })
        );
        assert_eq!(
            select_package("x86_64", "fedora", "", false),
            Ok(PackageTarget {
                kind: "rpm",
                asset_name: "redunar-app-linux-x86_64.rpm"
            })
        );
        assert_eq!(
            select_package("amd64", "arch", "", false),
            Ok(PackageTarget {
                kind: "arch",
                asset_name: "redunar-app-linux-x86_64.pkg.tar.zst"
            })
        );
        assert_eq!(
            select_package("x86_64", "opensuse-tumbleweed", "", false),
            Ok(PackageTarget {
                kind: "rpm",
                asset_name: "redunar-app-linux-opensuse-x86_64.rpm"
            })
        );
        assert!(select_package("aarch64", "ubuntu", "", false).is_err());
        assert!(select_package("x86_64", "fedora", "", true).is_err());
    }

    #[test]
    fn checksum_verification_accepts_matching_bytes_and_rejects_changes() {
        let path = std::env::temp_dir().join(format!(
            "redunar-update-checksum-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let bytes = b"verified package fixture";
        std::fs::write(&path, bytes).expect("fixture write");
        let mut digest = Sha256::new();
        digest.update(bytes);
        let expected = format!("{:x}", digest.finalize());
        assert!(verify_checksum(&path, &expected).is_ok());
        assert!(verify_checksum(&path, &"0".repeat(64)).is_err());
        std::fs::remove_file(path).expect("fixture cleanup");
    }

    #[test]
    fn retained_update_metadata_is_strict_and_safe() {
        let metadata = format!(
            "state=downloaded\nversion=0.2.0\nkind=deb\nasset_name=redunar-app-linux-amd64.deb\ndigest={}\n",
            "a".repeat(64)
        );
        let parsed = parse_pending_update(&metadata).expect("valid retained metadata");
        assert_eq!(parsed.version, "0.2.0");
        assert!(parse_pending_update(
            "state=downloaded\nversion=0.2.0\nkind=deb\nasset_name=../escape\ndigest=bad\n"
        )
        .is_err());
        assert!(parse_pending_update(&metadata.replace("kind=deb", "kind=portable")).is_err());
    }
}
