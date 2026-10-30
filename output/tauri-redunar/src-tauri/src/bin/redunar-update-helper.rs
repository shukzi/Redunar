//! Fixed polkit entry point for installing one signed Redunar package.
//! Never accept a package-manager command or a package path from the webview.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const PUBLIC_KEY: &str = include_str!("../../../../../packaging/release-signing-public.pem");
const MAX_METADATA: u64 = 1024 * 1024;
const MAX_PACKAGE: u64 = 2 * 1024 * 1024 * 1024;

struct PrivateDirectory(PathBuf);

impl PrivateDirectory {
    fn new() -> Result<Self, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?;
        let path = PathBuf::from(format!(
            "/var/tmp/redunar-update-{}-{}",
            std::process::id(),
            now.as_nanos()
        ));
        DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|e| e.to_string())?;
        Ok(Self(path))
    }
}

impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Pending {
    version: String,
    kind: String,
    asset: String,
    digest: String,
}

fn main() {
    if let Err(error) = install() {
        eprintln!("Redunar update failed: {error}");
        std::process::exit(1);
    }
}

fn install() -> Result<(), String> {
    if unsafe { libc::geteuid() } != 0 {
        return Err("root authorization is required".into());
    }
    let uid = env::var("PKEXEC_UID")
        .map_err(|_| "pkexec did not identify the requesting user")?
        .parse::<u32>()
        .map_err(|_| "invalid requesting user")?;
    if uid == 0 {
        return Err("run Redunar as an ordinary user".into());
    }
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let cache = PathBuf::from(arguments.next().ok_or("missing update cache directory")?);
    if arguments.next().is_some() || !cache.is_absolute() {
        return Err("invalid update helper arguments".into());
    }
    let cache_metadata = fs::symlink_metadata(&cache).map_err(|e| e.to_string())?;
    if !cache_metadata.is_dir()
        || cache_metadata.uid() != uid
        || cache_metadata.permissions().mode() & 0o077 != 0
    {
        return Err("the private update cache is not owned by the requesting user".into());
    }

    // Snapshot every user-owned input before verification. The package manager
    // receives only the root-owned copy, closing the authorization-time swap.
    let private = PrivateDirectory::new()?;
    copy_bounded(
        &cache.join("pending-update.txt"),
        &private.0.join("pending-update.txt"),
        uid,
        MAX_METADATA,
    )?;
    let pending = parse_pending(
        &fs::read_to_string(private.0.join("pending-update.txt")).map_err(|e| e.to_string())?,
    )?;
    let (kind, asset, manager) = package_for_host()?;
    if pending.kind != kind || pending.asset != asset {
        return Err("the signed package does not match this system".into());
    }
    for name in ["VERSION", "SHA256SUMS", "SHA256SUMS.sig"] {
        copy_bounded(
            &cache.join(format!("{name}.{}", pending.digest)),
            &private.0.join(name),
            uid,
            MAX_METADATA,
        )?;
    }
    copy_bounded(
        &cache.join(format!("{}.{}", asset, pending.digest)),
        &private.0.join(asset),
        uid,
        MAX_PACKAGE,
    )?;
    fs::write(private.0.join("release-public-key.pem"), PUBLIC_KEY).map_err(|e| e.to_string())?;
    let verified = Command::new("/usr/bin/openssl")
        .args(["dgst", "-sha256", "-verify"])
        .arg(private.0.join("release-public-key.pem"))
        .arg("-signature")
        .arg(private.0.join("SHA256SUMS.sig"))
        .arg(private.0.join("SHA256SUMS"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("signature verification could not start: {e}"))?;
    if !verified.success() {
        return Err("the release signature is invalid".into());
    }
    let manifest = fs::read_to_string(private.0.join("SHA256SUMS")).map_err(|e| e.to_string())?;
    let entries = parse_manifest(&manifest)?;
    if entries.get("VERSION").ok_or("signed VERSION is missing")?
        != &digest(&private.0.join("VERSION"))?
    {
        return Err("the signed version checksum is invalid".into());
    }
    let version = fs::read_to_string(private.0.join("VERSION")).map_err(|e| e.to_string())?;
    if version.trim() != pending.version {
        return Err("the signed version changed".into());
    }
    if entries.get(asset) != Some(&pending.digest)
        || digest(&private.0.join(asset))? != pending.digest
    {
        return Err("the signed package checksum is invalid".into());
    }
    let installed =
        installed_version(kind)?.ok_or("Redunar is not installed as a system package")?;
    if !newer(&pending.version, &installed) {
        return Err("this update is not newer than the installed package".into());
    }

    let mut command = Command::new(manager);
    command
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin");
    match kind {
        "deb" => {
            command
                .args(["install", "-y"])
                .arg(private.0.join(asset))
                .env("DEBIAN_FRONTEND", "noninteractive");
        }
        "arch" => {
            command
                .args(["-U", "--needed", "--noconfirm"])
                .arg(private.0.join(asset));
        }
        _ if manager.ends_with("zypper") => {
            command
                .args(["--non-interactive", "install", "--no-recommends"])
                .arg(private.0.join(asset));
        }
        _ if manager.ends_with("yum") => {
            command
                .args(["localinstall", "-y"])
                .arg(private.0.join(asset));
        }
        "rpm" => {
            command.args(["install", "-y"]).arg(private.0.join(asset));
        }
        _ => return Err("unsupported package manager".into()),
    };
    let status = command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("package installation could not start: {e}"))?;
    if !status.success() {
        return Err(format!("package manager exited with {status}"));
    }
    if installed_version(kind)?.is_some_and(|value| !newer(&pending.version, &value)) {
        Ok(())
    } else {
        Err("the installed package version could not be confirmed".into())
    }
}

fn copy_bounded(source: &Path, target: &Path, uid: u32, limit: u64) -> Result<(), String> {
    let input = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(source)
        .map_err(|e| format!("could not read cached update: {e}"))?;
    let metadata = input.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.uid() != uid || metadata.len() > limit {
        return Err("cached update has invalid ownership or size".into());
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(target)
        .map_err(|e| e.to_string())?;
    let copied =
        std::io::copy(&mut input.take(limit + 1), &mut output).map_err(|e| e.to_string())?;
    if copied > limit {
        return Err("cached update exceeds its size limit".into());
    }
    output.flush().map_err(|e| e.to_string())
}

fn parse_pending(input: &str) -> Result<Pending, String> {
    let mut values = BTreeMap::new();
    for line in input.lines() {
        let (key, value) = line.split_once('=').ok_or("invalid update metadata")?;
        if values.insert(key, value).is_some() {
            return Err("duplicate update metadata".into());
        }
    }
    let version = *values.get("version").ok_or("missing update version")?;
    let kind = *values.get("kind").ok_or("missing package kind")?;
    let asset = *values.get("asset_name").ok_or("missing asset name")?;
    let digest = *values.get("digest").ok_or("missing package digest")?;
    if !version_valid(version)
        || !matches!(kind, "deb" | "rpm" | "arch")
        || !matches!(values.get("state"), Some(&"downloaded" | &"handoff"))
        || digest.len() != 64
        || !digest.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("invalid update metadata".into());
    }
    Ok(Pending {
        version: version.into(),
        kind: kind.into(),
        asset: asset.into(),
        digest: digest.to_ascii_lowercase(),
    })
}

fn parse_manifest(input: &str) -> Result<BTreeMap<String, String>, String> {
    let mut entries = BTreeMap::new();
    if input.len() as u64 > MAX_METADATA {
        return Err("manifest is too large".into());
    }
    for line in input.lines().filter(|line| !line.is_empty()) {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 2
            || fields[0].len() != 64
            || !fields[0].bytes().all(|b| b.is_ascii_hexdigit())
            || fields[1].contains('/')
            || fields[1].contains('\\')
            || entries
                .insert(fields[1].to_owned(), fields[0].to_ascii_lowercase())
                .is_some()
        {
            return Err("invalid signed manifest".into());
        }
    }
    Ok(entries)
}

fn digest(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut sha = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        sha.update(&buffer[..count]);
    }
    Ok(format!("{:x}", sha.finalize()))
}

fn version_valid(value: &str) -> bool {
    let parts: Vec<_> = value.trim_start_matches('v').split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

fn newer(candidate: &str, installed: &str) -> bool {
    let parse = |value: &str| {
        value
            .trim_start_matches('v')
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    parse(candidate) > parse(installed)
}

fn package_for_host() -> Result<(&'static str, &'static str, &'static str), String> {
    if std::env::consts::ARCH != "x86_64" || Path::new("/run/ostree-booted").exists() {
        return Err("unsupported system".into());
    }
    let os = fs::read_to_string("/etc/os-release").map_err(|e| e.to_string())?;
    let values: BTreeMap<_, _> = os
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key, value.trim_matches('"').trim_matches('\'')))
        .collect();
    let id = values.get("ID").copied().unwrap_or("");
    let like = values.get("ID_LIKE").copied().unwrap_or("");
    if matches!(id, "alpine" | "nixos" | "steamos") {
        return Err("unsupported distribution".into());
    }
    let words = format!(" {id} {like} ");
    let has = |options: &[&str]| words.split_whitespace().any(|word| options.contains(&word));
    if has(&["ubuntu", "debian", "linuxmint", "pop"]) {
        Ok(("deb", "redunar-app-linux-amd64.deb", "/usr/bin/apt-get"))
    } else if has(&["fedora", "rhel", "centos", "rocky", "almalinux", "nobara"]) {
        let manager = if Path::new("/usr/bin/dnf").is_file() {
            "/usr/bin/dnf"
        } else {
            "/usr/bin/yum"
        };
        Ok(("rpm", "redunar-app-linux-x86_64.rpm", manager))
    } else if has(&["arch", "manjaro", "endeavouros", "cachyos"]) {
        Ok((
            "arch",
            "redunar-app-linux-x86_64.pkg.tar.zst",
            "/usr/bin/pacman",
        ))
    } else if has(&[
        "opensuse",
        "opensuse-tumbleweed",
        "opensuse-leap",
        "sles",
        "suse",
    ]) {
        Ok((
            "rpm",
            "redunar-app-linux-opensuse-x86_64.rpm",
            "/usr/bin/zypper",
        ))
    } else {
        Err("unsupported distribution".into())
    }
}

fn installed_version(kind: &str) -> Result<Option<String>, String> {
    let (program, arguments): (&str, &[&str]) = match kind {
        "rpm" => ("/usr/bin/rpm", &["-q", "--qf", "%{VERSION}", "redunar-app"]),
        "deb" => (
            "/usr/bin/dpkg-query",
            &["-W", "-f=${Status}\t${Version}", "redunar-app"],
        ),
        "arch" => ("/usr/bin/pacman", &["-Q", "redunar-app"]),
        _ => return Err("unsupported package kind".into()),
    };
    let result = Command::new(program)
        .args(arguments)
        .output()
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Ok(None);
    }
    if result.stdout.len() > 128 {
        return Err("invalid installed version".into());
    }
    let text = std::str::from_utf8(&result.stdout)
        .map_err(|e| e.to_string())?
        .trim();
    let text = if kind == "deb" {
        let (status, version) = text
            .split_once('\t')
            .ok_or("invalid installed package status")?;
        if status != "install ok installed" {
            return Ok(None);
        }
        version
    } else {
        text
    };
    let value = match kind {
        "rpm" => text,
        "deb" => text
            .rsplit(':')
            .next()
            .unwrap_or("")
            .split('-')
            .next()
            .unwrap_or(""),
        "arch" => text
            .strip_prefix("redunar-app ")
            .unwrap_or("")
            .split('-')
            .next()
            .unwrap_or(""),
        _ => unreachable!(),
    };
    if !version_valid(value) {
        return Err("invalid installed version".into());
    }
    Ok(Some(value.to_owned()))
}
