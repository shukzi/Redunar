use crate::{
    AUDIO_CHANNELS, AUDIO_FRAME_DURATION_NS, AUDIO_FRAME_SAMPLES_PER_CHANNEL,
    AUDIO_PCM_FRAME_BYTES, AUDIO_SAMPLE_RATE, EncodedOpusPacket, GameAudioNode, OpusEncoder,
    discover_game_audio_node, discover_output_monitor_node,
};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{Read, Take};
use std::os::fd::AsFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

const PW_DUMP_PATH: &str = "/usr/bin/pw-dump";
const PW_CAT_PATH: &str = "/usr/bin/pw-cat";
const MAX_REGISTRY_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PROCESSES: usize = 8_192;
const MAX_PROCESS_DEPTH: usize = 32;

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

/// Return the bounded process tree rooted at the game renderer process.
///
/// Missing or unreadable `/proc` entries are ignored because game processes
/// may exit during discovery. The root PID remains included.
#[must_use]
pub fn process_tree(root_pid: u32) -> BTreeSet<u32> {
    let mut result = BTreeSet::from([root_pid]);
    if root_pid == 0 {
        return result;
    }
    let mut parents = BTreeMap::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return result;
    };
    for entry in entries.flatten().take(MAX_PROCESSES) {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        if let Ok(stat) = fs::read_to_string(entry.path().join("stat"))
            && let Some(parent) = stat_parent_pid(&stat)
        {
            parents.insert(pid, parent);
        }
    }
    for _ in 0..MAX_PROCESS_DEPTH {
        let before = result.len();
        for (&pid, &parent) in &parents {
            if result.contains(&parent) {
                result.insert(pid);
            }
        }
        if result.len() == before {
            break;
        }
    }
    result
}

/// Query the local `PipeWire` registry once and select exactly one audio node
/// owned by the current game process tree.
///
/// # Errors
///
/// Returns [`AudioCaptureError`] when the fixed system utility is unavailable,
/// its bounded output is malformed, or process ownership is ambiguous.
pub fn discover_pipewire_game_node(
    process_ids: &BTreeSet<u32>,
) -> Result<Option<GameAudioNode>, AudioCaptureError> {
    if !Path::new(PW_DUMP_PATH).is_file() {
        return Err(AudioCaptureError::new(
            "PipeWire registry utility is unavailable",
        ));
    }
    let mut child = Command::new(PW_DUMP_PATH)
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
    discover_game_audio_node(&bytes, process_ids)
        .and_then(|node| {
            node.map_or_else(
                || discover_output_monitor_node(&bytes),
                |found| Ok(Some(found)),
            )
        })
        .map_err(|error| AudioCaptureError::new(error.to_string()))
}

pub struct PipeWireGameAudioCapture {
    child: Child,
    stdout: ChildStdout,
    encoder: OpusEncoder,
    pcm: [u8; AUDIO_PCM_FRAME_BYTES],
    filled: usize,
    timeline_timestamp_ns: u64,
    timeline_started: Instant,
    next_timestamp_ns: Option<u64>,
}

impl PipeWireGameAudioCapture {
    /// Start one long-lived raw `PipeWire` capture stream for an exact node.
    ///
    /// # Errors
    ///
    /// Returns [`AudioCaptureError`] when `pw-cat`, its pipe, or `libopus`
    /// cannot be initialized. Node discovery may supply the output-monitor
    /// fallback when no game-owned stream can be identified.
    pub fn start(
        node: &GameAudioNode,
        timeline_timestamp_ns: u64,
        timeline_started: Instant,
    ) -> Result<Self, AudioCaptureError> {
        if !Path::new(PW_CAT_PATH).is_file() || node.serial == 0 {
            return Err(AudioCaptureError::new(
                "PipeWire audio capture utility is unavailable",
            ));
        }
        let encoder =
            OpusEncoder::open().map_err(|error| AudioCaptureError::new(error.to_string()))?;
        let mut command = Command::new(PW_CAT_PATH);
        command
            .args([
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
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            ;
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
        let mut child = command
            .spawn()
            .map_err(|error| {
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

impl Drop for PipeWireGameAudioCapture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn stat_parent_pid(stat: &str) -> Option<u32> {
    let end = stat.rfind(')')?;
    stat.get(end + 1..)?.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_parser_handles_spaces_inside_process_name() {
        assert_eq!(stat_parent_pid("42 (game name) S 17 0 0"), Some(17));
    }

    #[test]
    fn process_tree_always_contains_the_requested_root() {
        assert!(process_tree(std::process::id()).contains(&std::process::id()));
    }
}
