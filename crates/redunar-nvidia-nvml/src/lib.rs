//! Optional, read-only NVIDIA telemetry for the beta hardware monitor.
//!
//! The installed driver's NVML library is loaded once per attempt, never via a
//! subprocess. Missing readings stay absent. Driver loss invalidates the whole
//! cached session, with bounded recovery using the original PCI identity.

mod diagnostics;
mod library;

pub use diagnostics::{
    NvidiaDiagnostics, NvidiaReadiness, NvidiaSensor, NvidiaSensorStatus, NvidiaSensors, NvmlError,
    NvmlFailure, NvmlSymbol,
};

use library::{Device, DriverLoader, Functions, Library, LibraryLoader, Memory, Utilization};
use redunar_core::GpuSnapshot;
use std::ffi::CString;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

const RECOVERY_DELAYS: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(30),
    Duration::from_mins(2),
];

/// One monitor-owned NVML adapter. Startup failure is retained as diagnostics
/// so the sampler can continue CPU/AMD monitoring and retry without rebuilding
/// its cached hardware topology. No native handles escape this type.
pub struct NvidiaNvml {
    loader: Box<dyn LibraryLoader>,
    session: Option<Session>,
    target: Option<Target>,
    device_name: Option<String>,
    diagnostics: NvidiaDiagnostics,
    next_retry: Option<Instant>,
}

struct Target {
    index: usize,
    card: String,
    address: CString,
}

struct Session {
    _library: Box<dyn Library>,
    functions: Functions,
    device: Device,
    initialized: bool,
}

// The session and device move into one monitor worker. They are never shared
// with UI/game code, and the library stays loaded through shutdown.
unsafe impl Send for NvidiaNvml {}

impl std::fmt::Debug for NvidiaNvml {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NvidiaNvml")
            .field("diagnostics", &self.diagnostics)
            .finish_non_exhaustive()
    }
}

impl NvidiaNvml {
    /// Start a monitor adapter, retaining safe failures and bounded recovery.
    /// The caller must withhold ambiguous render-device attribution first.
    #[must_use]
    pub fn open(sys_root: &Path, gpus: &mut [GpuSnapshot]) -> Self {
        Self::with_loader(sys_root, gpus, Box::new(DriverLoader), Instant::now())
    }

    fn with_loader(
        sys_root: &Path,
        gpus: &mut [GpuSnapshot],
        loader: Box<dyn LibraryLoader>,
        now: Instant,
    ) -> Self {
        for gpu in &mut *gpus {
            if is_nvidia(gpu) {
                clear_metrics(gpu);
            }
        }
        let mut adapter = Self {
            loader,
            session: None,
            target: None,
            device_name: None,
            diagnostics: NvidiaDiagnostics::withheld(NvidiaReadiness::NoDevice),
            next_retry: None,
        };
        let mut indices = gpus
            .iter()
            .enumerate()
            .filter_map(|(index, gpu)| is_nvidia(gpu).then_some(index));
        let Some(index) = indices.next() else {
            return adapter;
        };
        if indices.next().is_some() {
            adapter.diagnostics.readiness = NvidiaReadiness::AmbiguousTopology;
            return adapter;
        }
        let device = sys_root
            .join("class/drm")
            .join(&gpus[index].card)
            .join("device");
        let Some(address) = pci_slot_name(&device).and_then(|value| CString::new(value).ok())
        else {
            adapter.fail(NvmlFailure::PciIdentityUnavailable, now);
            return adapter;
        };
        adapter.target = Some(Target {
            index,
            card: gpus[index].card.clone(),
            address,
        });
        adapter.connect(&mut gpus[index], now);
        adapter
    }

    #[must_use]
    pub const fn diagnostics(&self) -> &NvidiaDiagnostics {
        &self.diagnostics
    }

    /// Sample cached functions; retries use the cached PCI address after 5,
    /// 30, and 120 seconds, at most three times across this adapter's lifetime.
    /// No rediscovery or subprocess runs on a tick.
    pub fn sample(&mut self, index: usize, gpu: &mut GpuSnapshot) {
        self.sample_at(index, gpu, Instant::now());
    }

    fn sample_at(&mut self, index: usize, gpu: &mut GpuSnapshot, now: Instant) {
        if !is_nvidia(gpu) {
            return;
        }
        clear_metrics(gpu);
        if self
            .target
            .as_ref()
            .is_none_or(|target| target.index != index || target.card != gpu.card)
        {
            return;
        }
        if self.session.is_none() && self.next_retry.is_some_and(|deadline| now >= deadline) {
            self.next_retry = None;
            self.diagnostics.recovery_attempts += 1;
            self.connect(gpu, now);
        }
        if let Some(name) = &self.device_name {
            gpu.model.clone_from(name);
        }
        let Some(session) = &self.session else {
            return;
        };
        match session.sample(gpu, &mut self.diagnostics.sensors) {
            Ok(()) => self.update_readiness(),
            Err(failure) => {
                // Values gathered before device loss cannot describe a valid
                // sample. Clear them all before releasing the stale handle.
                clear_metrics(gpu);
                self.close_session();
                self.fail(failure, now);
            }
        }
    }

    fn connect(&mut self, gpu: &mut GpuSnapshot, now: Instant) {
        clear_metrics(gpu);
        let Some(target) = &self.target else { return };
        let result = Session::open(&*self.loader, &target.address);
        match result {
            Ok(session) => {
                self.diagnostics.sensors = session.sensor_status();
                match session.name() {
                    Ok(name) => {
                        if let Some(name) = name {
                            gpu.model.clone_from(&name);
                            self.device_name = Some(name);
                        }
                        self.diagnostics.sensors.name = if session.functions.get_name.is_some() {
                            NvidiaSensorStatus::Available
                        } else {
                            NvidiaSensorStatus::MissingSymbol
                        };
                    }
                    Err(status) => {
                        self.diagnostics.sensors.name = status;
                        if let NvidiaSensorStatus::Unavailable(error) = status
                            && error.invalidates_session()
                        {
                            self.diagnostics.cleanup_failure =
                                session.close().or(self.diagnostics.cleanup_failure);
                            self.fail(
                                NvmlFailure::SensorRead {
                                    sensor: NvidiaSensor::Name,
                                    error,
                                },
                                now,
                            );
                            return;
                        }
                    }
                }
                self.session = Some(session);
                self.next_retry = None;
                self.diagnostics.failure = None;
                self.diagnostics.retry_delay = None;
                self.update_readiness();
            }
            Err((failure, cleanup_failure)) => {
                self.diagnostics.cleanup_failure =
                    cleanup_failure.or(self.diagnostics.cleanup_failure);
                self.fail(failure, now);
            }
        }
    }

    fn update_readiness(&mut self) {
        self.diagnostics.readiness = if self.diagnostics.sensors.degraded() {
            NvidiaReadiness::Degraded
        } else {
            NvidiaReadiness::Ready
        };
    }

    fn fail(&mut self, failure: NvmlFailure, now: Instant) {
        // Once the session is gone, earlier successful readings no longer
        // indicate current availability. Preserve missing-symbol information,
        // but invalidate every function whose handle became stale.
        if let NvmlFailure::SensorRead { error, .. } = failure {
            let sensors = &mut self.diagnostics.sensors;
            for status in [
                &mut sensors.name,
                &mut sensors.utilization,
                &mut sensors.temperature,
                &mut sensors.clock,
                &mut sensors.memory,
                &mut sensors.power,
            ] {
                if *status != NvidiaSensorStatus::MissingSymbol {
                    *status = NvidiaSensorStatus::Unavailable(error);
                }
            }
        } else {
            self.diagnostics.sensors = NvidiaSensors::default();
        }
        self.diagnostics.failure = Some(failure);
        self.next_retry = None;
        self.diagnostics.retry_delay = None;
        self.diagnostics.readiness = NvidiaReadiness::Unavailable;
        if !failure.can_retry() {
            return;
        }
        if let Some(delay) = RECOVERY_DELAYS.get(usize::from(self.diagnostics.recovery_attempts)) {
            self.next_retry = Some(now + *delay);
            self.diagnostics.retry_delay = Some(*delay);
            self.diagnostics.readiness = NvidiaReadiness::Recovering;
        } else {
            self.diagnostics.readiness = NvidiaReadiness::RecoveryExhausted;
        }
    }

    fn close_session(&mut self) {
        if let Some(session) = self.session.take() {
            self.diagnostics.cleanup_failure = session.close().or(self.diagnostics.cleanup_failure);
        }
    }

    /// Release the library and cancel recovery. Repeated shutdown is harmless.
    /// Cleanup failures remain typed diagnostics even though handles are gone.
    pub fn shutdown(&mut self) {
        self.next_retry = None;
        self.close_session();
        self.diagnostics.retry_delay = None;
        self.diagnostics.readiness = NvidiaReadiness::Stopped;
        self.diagnostics.sensors = NvidiaSensors::default();
    }
}

impl Drop for NvidiaNvml {
    fn drop(&mut self) {
        self.close_session();
    }
}

impl Session {
    fn open(
        loader: &dyn LibraryLoader,
        address: &CString,
    ) -> Result<Self, (NvmlFailure, Option<NvmlError>)> {
        let library = loader.load().map_err(|failure| (failure, None))?;
        let functions = Functions::load(&*library).map_err(|failure| (failure, None))?;
        // SAFETY: required symbols are resolved while their library is owned.
        let result = unsafe { (functions.init)() };
        if result != 0 {
            return Err((
                NvmlFailure::Initialization(NvmlError::from_code(result)),
                None,
            ));
        }
        let mut session = Self {
            _library: library,
            functions,
            device: std::ptr::null_mut(),
            initialized: true,
        };
        // SAFETY: initialization succeeded, the validated PCI string is
        // NUL-terminated, and the output handle storage is valid.
        let result = unsafe { (functions.get_handle)(address.as_ptr(), &raw mut session.device) };
        let failure = if result != 0 {
            Some(NvmlFailure::DeviceLookup(NvmlError::from_code(result)))
        } else if session.device.is_null() {
            Some(NvmlFailure::DeviceHandleMissing)
        } else {
            None
        };
        if let Some(failure) = failure {
            return Err((failure, session.close()));
        }
        Ok(session)
    }

    fn sensor_status(&self) -> NvidiaSensors {
        fn initial(present: bool) -> NvidiaSensorStatus {
            if present {
                NvidiaSensorStatus::NotSampled
            } else {
                NvidiaSensorStatus::MissingSymbol
            }
        }
        NvidiaSensors {
            name: initial(self.functions.get_name.is_some()),
            utilization: initial(self.functions.get_utilization.is_some()),
            temperature: initial(self.functions.get_temperature.is_some()),
            clock: initial(self.functions.get_clock.is_some()),
            memory: initial(self.functions.get_memory.is_some()),
            power: initial(self.functions.get_power.is_some()),
        }
    }

    fn name(&self) -> Result<Option<String>, NvidiaSensorStatus> {
        let Some(read) = self.functions.get_name else {
            return Ok(None);
        };
        let mut buffer = [0_u8; 96];
        // SAFETY: device belongs to this initialized session; the byte buffer
        // has exactly the length supplied to NVML.
        let result = unsafe { read(self.device, buffer.as_mut_ptr().cast(), 96) };
        if result != 0 {
            return Err(NvidiaSensorStatus::Unavailable(NvmlError::from_code(
                result,
            )));
        }
        let end = buffer
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(NvidiaSensorStatus::InvalidValue)?;
        let name = std::str::from_utf8(&buffer[..end])
            .map_err(|_| NvidiaSensorStatus::InvalidValue)?
            .trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err(NvidiaSensorStatus::InvalidValue);
        }
        Ok(Some(name.to_owned()))
    }

    fn sample(
        &self,
        gpu: &mut GpuSnapshot,
        sensors: &mut NvidiaSensors,
    ) -> Result<(), NvmlFailure> {
        // SAFETY: every optional function is checked first; each call uses
        // an initialized, PCI-matched handle and ABI-sized output storage.
        gpu.utilization_percent = read_sensor(
            self.functions.get_utilization,
            &mut sensors.utilization,
            NvidiaSensor::Utilization,
            |read| {
                let mut value = Utilization::default();
                let result = unsafe { read(self.device, &raw mut value) };
                (result, (value.gpu <= 100).then_some(f64::from(value.gpu)))
            },
        )?;
        gpu.temperature_celsius = read_sensor(
            self.functions.get_temperature,
            &mut sensors.temperature,
            NvidiaSensor::Temperature,
            |read| {
                let mut value = 0;
                let result = unsafe { read(self.device, 0, &raw mut value) };
                (result, (value <= 150).then_some(f64::from(value)))
            },
        )?;
        gpu.clock_mhz = read_sensor(
            self.functions.get_clock,
            &mut sensors.clock,
            NvidiaSensor::Clock,
            |read| {
                let mut value = 0;
                let result = unsafe { read(self.device, 0, &raw mut value) };
                (result, (value > 0).then_some(f64::from(value)))
            },
        )?;
        let memory = read_sensor(
            self.functions.get_memory,
            &mut sensors.memory,
            NvidiaSensor::Memory,
            |read| {
                let mut value = Memory::default();
                let result = unsafe { read(self.device, &raw mut value) };
                (
                    result,
                    (value.total > 0 && value.used <= value.total && value.free <= value.total)
                        .then_some(value),
                )
            },
        )?;
        gpu.vram_used_bytes = memory.map(|value| value.used);
        gpu.vram_total_bytes = memory.map(|value| value.total);
        gpu.power_watts = read_sensor(
            self.functions.get_power,
            &mut sensors.power,
            NvidiaSensor::Power,
            |read| {
                let mut value = 0;
                let result = unsafe { read(self.device, &raw mut value) };
                (result, Some(f64::from(value) / 1_000.0))
            },
        )?;
        Ok(())
    }

    fn close(mut self) -> Option<NvmlError> {
        self.release()
    }

    fn release(&mut self) -> Option<NvmlError> {
        if !self.initialized {
            return None;
        }
        self.initialized = false;
        // SAFETY: pair one successful init with one shutdown while loaded,
        // even when lookup failed or the driver invalidated its handles.
        let result = unsafe { (self.functions.shutdown)() };
        (result != 0).then(|| NvmlError::from_code(result))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.release();
    }
}

fn read_sensor<F: Copy, T>(
    function: Option<F>,
    status: &mut NvidiaSensorStatus,
    sensor: NvidiaSensor,
    read: impl FnOnce(F) -> (i32, Option<T>),
) -> Result<Option<T>, NvmlFailure> {
    let Some(function) = function else {
        *status = NvidiaSensorStatus::MissingSymbol;
        return Ok(None);
    };
    let (result, value) = read(function);
    if result != 0 {
        let error = NvmlError::from_code(result);
        *status = NvidiaSensorStatus::Unavailable(error);
        if error.invalidates_session() {
            return Err(NvmlFailure::SensorRead { sensor, error });
        }
        return Ok(None);
    }
    *status = if value.is_some() {
        NvidiaSensorStatus::Available
    } else {
        NvidiaSensorStatus::InvalidValue
    };
    Ok(value)
}

fn clear_metrics(gpu: &mut GpuSnapshot) {
    gpu.utilization_percent = None;
    gpu.temperature_celsius = None;
    gpu.clock_mhz = None;
    gpu.vram_used_bytes = None;
    gpu.vram_total_bytes = None;
    gpu.power_watts = None;
    gpu.performance_level = None;
}

#[must_use]
pub fn is_nvidia(gpu: &GpuSnapshot) -> bool {
    gpu.vendor_id
        .trim_start_matches("0x")
        .eq_ignore_ascii_case("10de")
}

fn pci_slot_name(device: &Path) -> Option<String> {
    let uevent = fs::read_to_string(device.join("uevent")).ok()?;
    let mut addresses = uevent
        .lines()
        .filter_map(|line| line.strip_prefix("PCI_SLOT_NAME="));
    let address = addresses.next()?;
    if addresses.next().is_some() {
        return None;
    }
    let bytes = address.as_bytes();
    if bytes.len() != 12
        || !bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b':',
            10 => *byte == b'.',
            _ => byte.is_ascii_hexdigit(),
        })
        || u8::from_str_radix(&address[8..10], 16).ok()? > 31
        || !(b'0'..=b'7').contains(&bytes[11])
    {
        return None;
    }
    Some(address.to_owned())
}

#[cfg(test)]
mod tests;
