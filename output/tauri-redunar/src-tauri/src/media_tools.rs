//! Resolve media tools by executable/capability, never by distribution package
//! name. Package installation belongs to the installer, not a running app.
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    io::Read,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const PROBE_LIMIT: usize = 128 * 1024;
const CACHE_LIFETIME: Duration = Duration::from_secs(60);
const PROVIDER_HELP: &str = "Use compatible FFmpeg/codecs from your distribution or RPM Fusion. Redunar does not install or replace packages.";
static ENCODERS: OnceLock<Mutex<Option<EncoderCache>>> = OnceLock::new();

struct EncoderCache {
    path: PathBuf,
    modified: Option<SystemTime>,
    bytes: u64,
    checked: Instant,
    names: HashSet<String>,
}

pub fn ffmpeg(required_encoder: Option<&str>) -> Result<Command, String> {
    let path = resolve("ffmpeg", std::env::var_os("PATH").as_deref())?;
    if let Some(encoder) = required_encoder {
        require_encoder(
            &path,
            encoder,
            ENCODERS.get_or_init(|| Mutex::new(None)),
            || probe_encoders(&path, PROBE_TIMEOUT, PROBE_LIMIT),
        )?;
    }
    Ok(kill_when_parent_dies(Command::new(path)))
}

pub fn ffprobe() -> Result<Command, String> {
    Ok(kill_when_parent_dies(Command::new(resolve(
        "ffprobe",
        std::env::var_os("PATH").as_deref(),
    )?)))
}

/// Ask the kernel to send SIGKILL to a spawned media tool when the thread
/// that spawned it exits. The UI always reaps its children cooperatively;
/// this is a backstop so a killed or crashed Redunar cannot leave FFmpeg or
/// FFprobe blocked forever on a closed pipe. The shortcut helper is
/// deliberately excluded: it exits on stdin end-of-file, which keeps
/// shortcuts alive across unrelated worker-thread turnover.
#[must_use]
pub fn kill_when_parent_dies(command: Command) -> Command {
    let mut command = command;
    // SAFETY: the pre-exec handler runs in the child before exec and calls
    // only the async-signal-safe prctl with a fixed request and signal.
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
}

fn resolve(tool: &str, search: Option<&OsStr>) -> Result<PathBuf, String> {
    search
        .into_iter()
        .flat_map(std::env::split_paths)
        .map(|directory| directory.join(tool))
        .find(|path| {
            fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
        .ok_or_else(|| format!("The {tool} executable is unavailable. {PROVIDER_HELP}"))
}

fn require_encoder(
    path: &Path,
    required: &str,
    cache: &Mutex<Option<EncoderCache>>,
    query: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    let metadata = fs::metadata(path).map_err(|_| "FFmpeg is no longer available")?;
    let mut cache = cache
        .lock()
        .map_err(|_| "FFmpeg capability check is busy")?;
    if cache.as_ref().is_some_and(|entry| {
        entry.path == path
            && entry.modified == metadata.modified().ok()
            && entry.bytes == metadata.len()
            && entry.checked.elapsed() < CACHE_LIFETIME
            && entry.names.contains(required)
    }) {
        return Ok(());
    }
    // Retry absent capabilities immediately: codec packages can be added to
    // shared libraries without replacing the ffmpeg executable or restarting us.
    let output = query()?;
    let names = encoder_names(&output);
    let supported = names.contains(required);
    *cache = Some(EncoderCache {
        path: path.to_owned(),
        modified: metadata.modified().ok(),
        bytes: metadata.len(),
        checked: Instant::now(),
        names,
    });
    if supported {
        Ok(())
    } else {
        Err(format!(
            "The installed FFmpeg does not provide the {required} encoder needed for this operation. {PROVIDER_HELP}"
        ))
    }
}

fn encoder_names(output: &[u8]) -> HashSet<String> {
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let flags = fields.next()?;
            let name = fields.next()?;
            (flags.len() == 6 && matches!(flags.as_bytes()[0], b'V' | b'A' | b'S') && name != "=")
                .then(|| name.to_owned())
        })
        .collect()
}

fn probe_encoders(path: &Path, timeout: Duration, limit: usize) -> Result<Vec<u8>, String> {
    run_probe(
        Command::new(path).args(["-hide_banner", "-encoders"]),
        timeout,
        limit,
    )
}

fn run_probe(command: &mut Command, timeout: Duration, limit: usize) -> Result<Vec<u8>, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| format!("FFmpeg could not start. {PROVIDER_HELP}"))?;
    let stdout = child.stdout.take().expect("piped encoder list");
    let reader = match std::thread::Builder::new()
        .name("redunar-media-capabilities".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            stdout.take((limit + 1) as u64).read_to_end(&mut bytes)?;
            Ok::<_, std::io::Error>(bytes)
        }) {
        Ok(reader) => reader,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("FFmpeg capability reader could not start".into());
        }
    };
    let started = Instant::now();
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break Err("FFmpeg capability check timed out or stopped");
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| "FFmpeg capability reader stopped")?
        .map_err(|_| "FFmpeg capabilities could not be read")?;
    if !result?.success() || bytes.len() > limit || encoder_names(&bytes).is_empty() {
        return Err(format!(
            "FFmpeg returned no usable encoder information. {PROVIDER_HELP}"
        ));
    }
    Ok(bytes)
}

/// Some requirements depend on the actual clip (e.g. optional audio). Diagnose
/// those at conversion time instead of rejecting a silent clip unnecessarily.
pub fn capability_error(details: &str) -> Option<String> {
    let lower = details.to_ascii_lowercase();
    [
        "unknown encoder",
        "unknown decoder",
        "encoder not found",
        "decoder not found",
        "no decoder found",
        "no such filter",
        "requested output format",
        "error while loading shared libraries",
    ]
    .iter()
    .any(|message| lower.contains(message))
    .then(|| format!("The installed FFmpeg lacks codec or format support needed for this clip. {PROVIDER_HELP}"))
}

#[cfg(test)]
#[path = "media_tools_tests.rs"]
mod tests;
