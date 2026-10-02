//! Bounded, non-identifying monitor diagnostics. These types deliberately do
//! not retain library errors, paths, PCI addresses, handles, or device names.

use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NvidiaReadiness {
    BetaDisabled,
    NoDevice,
    NoRenderDevice,
    AmbiguousTopology,
    Ready,
    Degraded,
    Unavailable,
    Recovering,
    RecoveryExhausted,
    Stopped,
}

impl NvidiaReadiness {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BetaDisabled => "beta-disabled",
            Self::NoDevice => "no-device",
            Self::NoRenderDevice => "no-render-device",
            Self::AmbiguousTopology => "ambiguous-topology",
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Unavailable => "unavailable",
            Self::Recovering => "recovering",
            Self::RecoveryExhausted => "recovery-exhausted",
            Self::Stopped => "stopped",
        }
    }
}

/// Known NVML return categories; undocumented codes collapse to `Unknown`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NvmlError {
    Uninitialized,
    InvalidArgument,
    NotSupported,
    NoPermission,
    NotFound,
    DriverNotLoaded,
    Timeout,
    DeviceLost,
    ResetRequired,
    DriverVersionMismatch,
    NoData,
    Unknown,
}

impl NvmlError {
    pub(crate) const fn from_code(code: i32) -> Self {
        match code {
            1 => Self::Uninitialized,
            2 => Self::InvalidArgument,
            3 => Self::NotSupported,
            4 => Self::NoPermission,
            6 => Self::NotFound,
            9 => Self::DriverNotLoaded,
            10 => Self::Timeout,
            15 => Self::DeviceLost,
            16 => Self::ResetRequired,
            18 => Self::DriverVersionMismatch,
            21 => Self::NoData,
            _ => Self::Unknown,
        }
    }

    pub(crate) const fn invalidates_session(self) -> bool {
        matches!(
            self,
            Self::Uninitialized
                | Self::DriverNotLoaded
                | Self::DeviceLost
                | Self::ResetRequired
                | Self::DriverVersionMismatch
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NvmlSymbol {
    Init,
    Shutdown,
    PciHandle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NvidiaSensor {
    Name,
    Utilization,
    Temperature,
    Clock,
    Memory,
    Power,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NvmlFailure {
    LibraryUnavailable,
    RequiredSymbolMissing(NvmlSymbol),
    Initialization(NvmlError),
    PciIdentityUnavailable,
    DeviceLookup(NvmlError),
    DeviceHandleMissing,
    SensorRead {
        sensor: NvidiaSensor,
        error: NvmlError,
    },
}

impl NvmlFailure {
    pub(crate) const fn can_retry(self) -> bool {
        match self {
            Self::PciIdentityUnavailable | Self::RequiredSymbolMissing(_) => false,
            Self::Initialization(error) | Self::DeviceLookup(error) => {
                !matches!(error, NvmlError::InvalidArgument | NvmlError::NotSupported)
            }
            _ => true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NvidiaSensorStatus {
    #[default]
    NotSampled,
    Available,
    MissingSymbol,
    Unavailable(NvmlError),
    InvalidValue,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NvidiaSensors {
    pub name: NvidiaSensorStatus,
    pub utilization: NvidiaSensorStatus,
    pub temperature: NvidiaSensorStatus,
    pub clock: NvidiaSensorStatus,
    pub memory: NvidiaSensorStatus,
    pub power: NvidiaSensorStatus,
}

impl NvidiaSensors {
    pub(crate) fn degraded(self) -> bool {
        [
            self.name,
            self.utilization,
            self.temperature,
            self.clock,
            self.memory,
            self.power,
        ]
        .iter()
        .any(|status| {
            !matches!(
                status,
                NvidiaSensorStatus::Available | NvidiaSensorStatus::NotSampled
            )
        })
    }
}

/// Safe to log on changes. `retry_delay` is the scheduled backoff, not a
/// per-tick countdown. Recovery attempts are capped for the monitor's lifetime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NvidiaDiagnostics {
    pub readiness: NvidiaReadiness,
    pub failure: Option<NvmlFailure>,
    pub sensors: NvidiaSensors,
    pub recovery_attempts: u8,
    pub retry_delay: Option<Duration>,
    pub cleanup_failure: Option<NvmlError>,
}

impl NvidiaDiagnostics {
    #[must_use]
    pub const fn withheld(readiness: NvidiaReadiness) -> Self {
        Self {
            readiness,
            failure: None,
            sensors: NvidiaSensors {
                name: NvidiaSensorStatus::NotSampled,
                utilization: NvidiaSensorStatus::NotSampled,
                temperature: NvidiaSensorStatus::NotSampled,
                clock: NvidiaSensorStatus::NotSampled,
                memory: NvidiaSensorStatus::NotSampled,
                power: NvidiaSensorStatus::NotSampled,
            },
            recovery_attempts: 0,
            retry_delay: None,
            cleanup_failure: None,
        }
    }
}
