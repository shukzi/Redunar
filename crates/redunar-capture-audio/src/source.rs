use crate::{
    AUDIO_CHANNELS, AUDIO_FRAME_DURATION_NS, AUDIO_FRAME_SAMPLES_PER_CHANNEL,
    AUDIO_PCM_FRAME_BYTES, AUDIO_SAMPLE_RATE, EncodedOpusPacket, GameAudioNode, OpusEncoder,
    discover_output_monitor_node,
};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use std::error::Error;
use std::fmt;
use std::io::{Read, Take};
use std::os::fd::AsFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

const PW_DUMP_PATH: &str = "/usr/bin/pw-dump";
const PW_CAT_PATH: &str = "/usr/bin/pw-cat";
const WPCTL_PATH: &str = "/usr/bin/wpctl";
const PACTL_PATH: &str = "/usr/bin/pactl";
const PAREC_PATH: &str = "/usr/bin/parec";
const MAX_REGISTRY_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug)]
pub struct AudioCaptureError(String);

impl AudioCaptureError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for AudioCaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for AudioCaptureError {}

/// One exact default-output monitor selected for the Replay worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AudioCaptureSource {
    PipeWire(GameAudioNode),
    PulseAudio { sink_name: String },
}

impl AudioCaptureSource {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::PipeWire(node) => &node.node_name,
            Self::PulseAudio { sink_name } => sink_name,
        }
    }

    #[must_use]
    pub const fn backend_name(&self) -> &'static str {
        match self {
            Self::PipeWire(_) => "PipeWire",
            Self::PulseAudio { .. } => "PulseAudio monitor",
        }
    }
}

/// Select the strongest available Linux output-capture route.
///
/// PulseAudio's default monitor is preferred because it works with both a real
/// PulseAudio server and PipeWire's Pulse server. A direct PipeWire default
/// output remains available when the Pulse compatibility service is absent.
///
/// # Errors
///
/// Returns [`AudioCaptureError`] when neither supported sound server exposes a
/// usable output route.
pub fn discover_system_audio_source() -> Result<AudioCaptureSource, AudioCaptureError> {
    discover_system_audio_sources()?
        .into_iter()
        .next()
        .ok_or_else(|| AudioCaptureError::new("no output-monitor capture route is available"))
}

/// Find every usable output-capture backend in preference order.
///
/// Keeping both routes lets the session worker fall back when a sound server
/// advertises a route but its recorder exits or stops delivering samples.
///
/// # Errors
///
/// Returns [`AudioCaptureError`] when neither supported sound server exposes a
/// usable output route.
pub fn discover_system_audio_sources() -> Result<Vec<AudioCaptureSource>, AudioCaptureError> {
    let mut sources = Vec::with_capacity(2);
    if let Some(source) = discover_pulse_default_monitor() {
        sources.push(source);
    }

    if system_utility(PW_CAT_PATH, "pw-cat").is_some() {
        if let Ok(registry) = inspect_pipewire_registry()
            && let Ok(Some(source)) = discover_default_output_monitor_node(&registry)
        {
            sources.push(AudioCaptureSource::PipeWire(source));
        }
    }

    if !sources.is_empty() {
        return Ok(sources);
    }

    Err(AudioCaptureError::new(
        "no output-monitor capture route is available; install PulseAudio utilities or PipeWire tools",
    ))
}

fn inspect_pipewire_registry() -> Result<Vec<u8>, AudioCaptureError> {
    let Some(pw_dump) = system_utility(PW_DUMP_PATH, "pw-dump") else {
        return Err(AudioCaptureError::new(
            "PipeWire registry utility is unavailable",
        ));
    };
    let mut child = Command::new(pw_dump)
        .args(["-N", "-i0"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| AudioCaptureError::new(format!("could not inspect PipeWire: {error}")))?;
    let mut bytes = Vec::new();
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(AudioCaptureError::new(
            "PipeWire registry pipe is unavailable",
        ));
    };
    let mut bounded: Take<ChildStdout> = stdout.take(MAX_REGISTRY_BYTES + 1);
    bounded.read_to_end(&mut bytes).map_err(|error| {
        AudioCaptureError::new(format!("could not read PipeWire registry: {error}"))
    })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_REGISTRY_BYTES {
        drop(bounded);
        let _ = child.kill();
        let _ = child.wait();
        return Err(AudioCaptureError::new(
            "PipeWire registry query exceeded its bound",
        ));
    }
    let status = child.wait().map_err(|error| {
        AudioCaptureError::new(format!("could not join PipeWire registry query: {error}"))
    })?;
    if !status.success() {
        return Err(AudioCaptureError::new(
            "PipeWire registry query failed or exceeded its bound",
        ));
    }
    Ok(bytes)
}

fn discover_pulse_default_monitor() -> Option<AudioCaptureSource> {
    system_utility(PAREC_PATH, "parec")?;
    let sink_name = inspect_pactl_default_sink_name()?;
    Some(AudioCaptureSource::PulseAudio { sink_name })
}

fn discover_default_output_monitor_node(
    registry: &[u8],
) -> Result<Option<GameAudioNode>, crate::GameAudioNodeError> {
    let preferred = inspect_default_sink_name();
    discover_output_monitor_node(registry, preferred.as_deref())
}

fn inspect_default_sink_name() -> Option<String> {
    if let Some(wpctl) = system_utility(WPCTL_PATH, "wpctl")
        && let Ok(output) = Command::new(wpctl)
            .args(["inspect", "@DEFAULT_AUDIO_SINK@"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
        && output.status.success()
        && output.stdout.len() <= 64 * 1024
        && let Some(name) = parse_default_sink_name(&output.stdout)
    {
        return Some(name);
    }
    inspect_pactl_default_sink_name()
}

fn inspect_pactl_default_sink_name() -> Option<String> {
    let pactl = system_utility(PACTL_PATH, "pactl")?;
    let output = Command::new(&pactl)
        .arg("get-default-sink")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if output.status.success()
        && output.stdout.len() <= 4 * 1024
        && let Some(name) = parse_pactl_default_sink_name(&output.stdout)
    {
        return Some(name);
    }

    // `get-default-sink` is newer than the long-standing `info` command.
    // Accept the latter so supported PulseAudio installations on older Linux
    // distributions do not lose replay audio solely due to client age.
    let output = Command::new(pactl)
        .arg("info")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.len() > 64 * 1024 {
        return None;
    }
    parse_pactl_info_default_sink_name(&output.stdout)
}

fn parse_default_sink_name(output: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(output).ok()?;
    let value = text.lines().find_map(|line| {
        let (_, value) = line.trim().split_once("node.name = ")?;
        value
            .trim()
            .strip_prefix('"')?
            .strip_suffix('"')
            .map(str::to_owned)
    })?;
    if value.is_empty()
        || value.len() > 256
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return None;
    }
    Some(value)
}

fn parse_pactl_default_sink_name(output: &[u8]) -> Option<String> {
    valid_sink_name(std::str::from_utf8(output).ok()?.trim())
}

fn parse_pactl_info_default_sink_name(output: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(output).ok()?;
    let value = text.lines().find_map(|line| {
        let (label, value) = line.split_once(':')?;
        label
            .trim()
            .eq_ignore_ascii_case("Default Sink")
            .then_some(value.trim())
    })?;
    valid_sink_name(value)
}

fn valid_sink_name(value: &str) -> Option<String> {
    if value.is_empty()
        || value.len() > 256
        || value.contains(char::is_whitespace)
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return None;
    }
    Some(value.to_owned())
}

fn system_utility(preferred: &str, name: &str) -> Option<PathBuf> {
    let preferred = Path::new(preferred);
    if preferred.is_file() {
        return Some(preferred.to_owned());
    }
    ["/bin", "/usr/local/bin"]
        .into_iter()
        .map(|directory| Path::new(directory).join(name))
        .find(|candidate| candidate.is_file())
}

pub struct SystemAudioCapture {
    child: Child,
    stdout: ChildStdout,
    encoder: OpusEncoder,
    pcm: [u8; AUDIO_PCM_FRAME_BYTES],
    filled: usize,
    timeline_timestamp_ns: u64,
    timeline_started: Instant,
    next_timestamp_ns: Option<u64>,
}

impl SystemAudioCapture {
    /// Start one long-lived raw capture stream for the selected system route.
    ///
    /// # Errors
    ///
    /// Returns [`AudioCaptureError`] when the selected capture utility, its
    /// pipe, or `libopus` cannot be initialized.
    pub fn start(
        source: &AudioCaptureSource,
        timeline_timestamp_ns: u64,
        timeline_started: Instant,
    ) -> Result<Self, AudioCaptureError> {
        let encoder =
            OpusEncoder::open().map_err(|error| AudioCaptureError::new(error.to_string()))?;
        let mut command = capture_command(source)?;
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        {
            // SAFETY: the closure runs in the child between fork and exec
            // and calls only the async-signal-safe prctl with fixed values.
            // Ask the kernel to kill this recording stream if Redunar dies
            // without reaping it. The cooperative paths (pause, epoch reset,
            // session end) already kill and wait; this is the backstop that
            // prevents an orphaned pw-cat from capturing audio forever after
            // a crash or SIGKILL. The worker that spawns it outlives every
            // child it owns, so the parent-death signal cannot fire early.
            unsafe {
                command.pre_exec(|| {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        let mut child = command.spawn().map_err(|error| {
            AudioCaptureError::new(format!("could not start game audio capture: {error}"))
        })?;
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AudioCaptureError::new(
                "game audio capture pipe is unavailable",
            ));
        };
        Ok(Self {
            child,
            stdout,
            encoder,
            pcm: [0; AUDIO_PCM_FRAME_BYTES],
            filled: 0,
            timeline_timestamp_ns,
            timeline_started,
            next_timestamp_ns: None,
        })
    }

    /// Read and encode at most one 20 ms packet within `timeout`.
    ///
    /// # Errors
    ///
    /// Returns [`AudioCaptureError`] for capture exit, pipe failure, timestamp
    /// overflow, or Opus failure.
    pub fn next_packet(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<EncodedOpusPacket>, AudioCaptureError> {
        let deadline = Instant::now() + timeout;
        while self.filled < self.pcm.len() {
            if self
                .child
                .try_wait()
                .map_err(|error| AudioCaptureError::new(error.to_string()))?
                .is_some()
            {
                return Err(AudioCaptureError::new("game audio capture exited"));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            let timeout_ms = remaining.as_millis().min(100);
            let timeout = PollTimeout::try_from(u32::try_from(timeout_ms).unwrap_or(100))
                .unwrap_or(PollTimeout::from(100_u16));
            let mut descriptors = [PollFd::new(self.stdout.as_fd(), PollFlags::POLLIN)];
            if poll(&mut descriptors, timeout)
                .map_err(|error| AudioCaptureError::new(error.to_string()))?
                == 0
            {
                continue;
            }
            let count = self
                .stdout
                .read(&mut self.pcm[self.filled..])
                .map_err(|error| {
                    AudioCaptureError::new(format!("game audio read failed: {error}"))
                })?;
            if count == 0 {
                return Err(AudioCaptureError::new(
                    "game audio capture closed its stream",
                ));
            }
            self.filled += count;
        }
        let mut samples = [0_i16; AUDIO_FRAME_SAMPLES_PER_CHANNEL * AUDIO_CHANNELS as usize];
        for (sample, bytes) in samples.iter_mut().zip(self.pcm.chunks_exact(2)) {
            *sample = i16::from_le_bytes([bytes[0], bytes[1]]);
        }
        let timestamp_ns = self.next_timestamp_ns.unwrap_or_else(|| {
            self.timeline_timestamp_ns
                .saturating_add(
                    u64::try_from(self.timeline_started.elapsed().as_nanos()).unwrap_or(u64::MAX),
                )
                .saturating_sub(AUDIO_FRAME_DURATION_NS)
        });
        let packet = self
            .encoder
            .encode_frame(timestamp_ns, &samples)
            .map_err(|error| AudioCaptureError::new(error.to_string()))?;
        self.next_timestamp_ns = Some(
            timestamp_ns
                .checked_add(AUDIO_FRAME_DURATION_NS)
                .ok_or_else(|| AudioCaptureError::new("game audio timestamp overflowed"))?,
        );
        self.filled = 0;
        Ok(Some(packet))
    }
}

impl Drop for SystemAudioCapture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn capture_command(source: &AudioCaptureSource) -> Result<Command, AudioCaptureError> {
    match source {
        AudioCaptureSource::PipeWire(node) => {
            let Some(pw_cat) = system_utility(PW_CAT_PATH, "pw-cat") else {
                return Err(AudioCaptureError::new(
                    "PipeWire audio capture utility is unavailable",
                ));
            };
            if node.serial == 0 {
                return Err(AudioCaptureError::new("PipeWire audio target is invalid"));
            }
            let mut command = Command::new(pw_cat);
            command.args([
                "--record",
                "--raw",
                "--rate",
                &AUDIO_SAMPLE_RATE.to_string(),
                "--channels",
                &AUDIO_CHANNELS.to_string(),
                "--format",
                "s16",
                "--latency",
                "20ms",
                "--target",
                &node.serial.to_string(),
                "-",
            ]);
            Ok(command)
        }
        AudioCaptureSource::PulseAudio { .. } => {
            let Some(parec) = system_utility(PAREC_PATH, "parec") else {
                return Err(AudioCaptureError::new(
                    "PulseAudio capture utility is unavailable",
                ));
            };
            let AudioCaptureSource::PulseAudio { sink_name } = source else {
                unreachable!();
            };
            let monitor_name = format!("{sink_name}.monitor");
            let mut command = Command::new(parec);
            command.args([
                "--record",
                "--raw",
                &format!("--rate={AUDIO_SAMPLE_RATE}"),
                &format!("--channels={AUDIO_CHANNELS}"),
                "--format=s16le",
                "--latency-msec=20",
                "--process-time-msec=20",
                &format!("--device={monitor_name}"),
                "--client-name=Redunar",
                "--stream-name=Instant Replay",
            ]);
            Ok(command)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_sink_parser_accepts_wpctl_node_name() {
        let output = br#"id 74, type PipeWire:Interface:Node
  * node.name = "alsa_output.pci-0000_0f_00.4.analog-stereo"
  * object.serial = "74"
"#;
        assert_eq!(
            parse_default_sink_name(output).as_deref(),
            Some("alsa_output.pci-0000_0f_00.4.analog-stereo")
        );
    }

    #[test]
    fn default_sink_parser_accepts_pipewire_pulse_name() {
        assert_eq!(
            parse_pactl_default_sink_name(b"alsa_output.pci-0000_0f_00.4.analog-stereo\n")
                .as_deref(),
            Some("alsa_output.pci-0000_0f_00.4.analog-stereo")
        );
    }

    #[test]
    fn pulse_capture_targets_the_resolved_default_sink_monitor() {
        let source = AudioCaptureSource::PulseAudio {
            sink_name: "alsa_output.test".to_owned(),
        };
        let command = capture_command(&source).expect("PulseAudio capture command");
        let arguments = command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(
            arguments
                .iter()
                .any(|value| value == "--device=alsa_output.test.monitor")
        );
        assert!(arguments.iter().any(|value| value == "--rate=48000"));
        assert!(arguments.iter().any(|value| value == "--channels=2"));
    }

    #[test]
    fn default_sink_parser_accepts_legacy_pactl_info() {
        let output =
            b"Server String: /run/user/1000/pulse/native\nDefault Sink: alsa_output.test\n";
        assert_eq!(
            parse_pactl_info_default_sink_name(output).as_deref(),
            Some("alsa_output.test")
        );
    }
}
