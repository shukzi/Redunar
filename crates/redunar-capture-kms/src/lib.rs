//! Clean-room KMS Replay capture planning.
//!
//! This crate is deliberately read-only today. It discovers bounded connector
//! metadata and produces diagnostic plans, but it does not open DRM devices,
//! export framebuffers, initialize an encoder, or alter a game process.

mod discovery;
mod drm;
mod plan;
mod probe;

pub use discovery::{KmsDiscoveryError, KmsMode, KmsOutput, discover_outputs_at};
pub use drm::{KmsDrmCapture, KmsDrmError, KmsExportedFrame};
pub use plan::{KmsCapturePlan, KmsPlanError};
pub use probe::{KmsProbe, KmsProbeBlocker, diagnostic_requested};
