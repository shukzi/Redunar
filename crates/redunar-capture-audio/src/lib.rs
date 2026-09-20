//! Bounded system-output audio capture for Instant Replay.
//!
//! Redunar records the default output monitor: the same mixed system audio the
//! user hears. It uses PulseAudio (including PipeWire's Pulse server) when
//! available and direct PipeWire otherwise, then produces fixed 48 kHz stereo
//! PCM and independent 20 ms Opus packets.

mod node;
mod opus;
mod source;

pub use node::{
    GameAudioNode, GameAudioNodeError, discover_game_audio_node, discover_output_monitor_node,
};
pub use opus::{EncodedOpusPacket, OpusEncoder, OpusEncoderError, OpusStreamDescription};
pub use source::{
    AudioCaptureError, AudioCaptureSource, SystemAudioCapture, discover_system_audio_source,
    discover_system_audio_sources,
};

pub const AUDIO_SAMPLE_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: u8 = 2;
pub const AUDIO_FRAME_SAMPLES_PER_CHANNEL: usize = 960;
pub const AUDIO_FRAME_DURATION_NS: u64 = 20_000_000;
pub const AUDIO_PCM_FRAME_BYTES: usize =
    AUDIO_FRAME_SAMPLES_PER_CHANNEL * 2 * AUDIO_CHANNELS as usize;
