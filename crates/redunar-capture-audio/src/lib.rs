//! Bounded game-owned audio capture foundations for Instant Replay.
//!
//! Redunar identifies one `PipeWire` game playback node by process ownership,
//! captures fixed 48 kHz stereo PCM from that node, and encodes independent
//! 20 ms Opus packets. Prefer the game-owned stream; when none is identified,
//! output-monitor fallback retains audio and can include other applications.

mod node;
mod opus;
mod source;

pub use node::{
    GameAudioNode, GameAudioNodeError, discover_game_audio_node, discover_output_monitor_node,
};
pub use opus::{EncodedOpusPacket, OpusEncoder, OpusEncoderError, OpusStreamDescription};
pub use source::{
    AudioCaptureError, PipeWireGameAudioCapture, discover_pipewire_game_node, process_tree,
};

pub const AUDIO_SAMPLE_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: u8 = 2;
pub const AUDIO_FRAME_SAMPLES_PER_CHANNEL: usize = 960;
pub const AUDIO_FRAME_DURATION_NS: u64 = 20_000_000;
pub const AUDIO_PCM_FRAME_BYTES: usize =
    AUDIO_FRAME_SAMPLES_PER_CHANNEL * 2 * AUDIO_CHANNELS as usize;
