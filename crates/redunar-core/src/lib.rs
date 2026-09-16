//! Core domain types shared by every Redunar frontend and service.

mod game;
pub mod overlay_font;

pub use game::{
    EffectiveGameProfile, GameCatalog, GameId, GameIdentityError, GameLaunchConfig, GameMatchRule,
    GameProcess, GameRecord, GameResolution, GlobalGameProfile, Inheritable, OverlayCorner,
    OverlayMetricSet, OverlayMetricSetError, OverlayOpacity, OverlayOpacityError, OverlayPreset,
    OverlayScale, PerGameProfile, ReplayDuration, ReplayFrameRate, ReplayQuality, ReplaySettings,
    ReplayStorageLimit, resolve_game,
};

use std::error::Error;
use std::fmt;

/// A complete read-only view of the performance-relevant host state.
#[derive(Clone, Debug, PartialEq)]
pub struct SystemSnapshot {
    /// RAM unavailable for allocation, excluding reclaimable cache.
    pub memory: Option<MemorySnapshot>,
    pub cpu: CpuSnapshot,
    pub gpus: Vec<GpuSnapshot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemorySnapshot {
    pub used_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CpuSnapshot {
    pub vendor: String,
    pub model: String,
    pub logical_cpus: usize,
    pub temperature_celsius: Option<f64>,
    /// Aggregate utilization across all logical CPUs. This is unavailable
    /// until a stateful platform sampler has observed two valid readings.
    pub utilization_percent: Option<f64>,
    pub scaling_driver: Option<String>,
    pub governor: Option<String>,
    pub energy_performance_preference: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuSnapshot {
    pub card: String,
    pub vendor_id: String,
    pub device_id: Option<String>,
    pub model: String,
    pub driver: Option<String>,
    pub temperature_celsius: Option<f64>,
    pub utilization_percent: Option<f64>,
    pub clock_mhz: Option<f64>,
    pub vram_used_bytes: Option<u64>,
    pub vram_total_bytes: Option<u64>,
    pub power_watts: Option<f64>,
    pub performance_level: Option<String>,
}

/// Read-only hardware probes implement this boundary. It also makes safe fake
/// backends possible for tests and UI development.
pub trait HardwareProbe {
    /// Capture the current performance-relevant hardware state.
    ///
    /// # Errors
    ///
    /// Returns a [`ProbeError`] when a required source cannot be read or its
    /// contents cannot produce a coherent snapshot. Optional sensors and
    /// unsupported devices are represented as missing data instead.
    fn snapshot(&self) -> Result<SystemSnapshot, ProbeError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbeError {
    message: String,
}

impl ProbeError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ProbeError {}
