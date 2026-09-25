//! Optional, read-only NVIDIA telemetry for the beta hardware monitor.
//!
//! NVML is supplied by the installed NVIDIA driver. Resolve its small public
//! C ABI at runtime so standard Redunar builds do not require that driver.
//! Failure to load the library or a sensor leaves only that reading absent.

use redunar_core::GpuSnapshot;
use std::ffi::{CStr, CString, c_char, c_void};
use std::fs;
use std::mem;
use std::path::Path;

type Device = *mut c_void;
type Init = unsafe extern "C" fn() -> i32;
type Shutdown = unsafe extern "C" fn() -> i32;
type GetHandle = unsafe extern "C" fn(*const c_char, *mut Device) -> i32;
type GetName = unsafe extern "C" fn(Device, *mut c_char, u32) -> i32;
type GetUtilization = unsafe extern "C" fn(Device, *mut Utilization) -> i32;
type GetTemperature = unsafe extern "C" fn(Device, u32, *mut u32) -> i32;
type GetClock = unsafe extern "C" fn(Device, u32, *mut u32) -> i32;
type GetMemory = unsafe extern "C" fn(Device, *mut Memory) -> i32;
type GetPower = unsafe extern "C" fn(Device, *mut u32) -> i32;

#[repr(C)]
#[derive(Default)]
struct Utilization {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
#[derive(Default)]
struct Memory {
    total: u64,
    free: u64,
    used: u64,
}

/// A single monitor-owned NVML session. Device handles are cached by the
/// sysfs PCI address rather than by an enumeration index.
pub struct NvidiaNvml {
    library: *mut c_void,
    shutdown: Shutdown,
    devices: Vec<Option<Device>>,
    get_name: Option<GetName>,
    get_utilization: Option<GetUtilization>,
    get_temperature: Option<GetTemperature>,
    get_clock: Option<GetClock>,
    get_memory: Option<GetMemory>,
    get_power: Option<GetPower>,
}

// The monitor moves this privately owned session into exactly one worker;
// NVML handles are never published to the UI or shared with game code.
unsafe impl Send for NvidiaNvml {}

impl std::fmt::Debug for NvidiaNvml {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NvidiaNvml")
            .field("device_count", &self.devices.len())
            .finish_non_exhaustive()
    }
}

impl NvidiaNvml {
    pub fn open(sys_root: &Path, gpus: &mut [GpuSnapshot]) -> Option<Self> {
        if !gpus.iter().any(is_nvidia) {
            return None;
        }
        // SAFETY: the library name is a fixed literal and every symbol below
        // is resolved before use. Missing drivers leave metrics unavailable.
        let library = unsafe {
            libc::dlopen(
                c"libnvidia-ml.so.1".as_ptr(),
                libc::RTLD_NOW | libc::RTLD_LOCAL,
            )
        };
        if library.is_null() {
            return None;
        }
        // SAFETY: function types match the documented NVML C ABI. Required
        // functions are checked before initialization and all calls below.
        let required = || unsafe {
            Some((
                symbol::<Init>(library, c"nvmlInit_v2")?,
                symbol::<Shutdown>(library, c"nvmlShutdown")?,
                symbol::<GetHandle>(library, c"nvmlDeviceGetHandleByPciBusId_v2")?,
            ))
        };
        let Some((init, shutdown, get_handle)) = required() else {
            // SAFETY: the handle came from dlopen above and is not retained.
            unsafe { libc::dlclose(library) };
            return None;
        };
        // SAFETY: initialization precedes every device and sensor call.
        if unsafe { init() } != 0 {
            unsafe { libc::dlclose(library) };
            return None;
        }
        let mut session = Self {
            library,
            shutdown,
            devices: Vec::with_capacity(gpus.len()),
            get_name: unsafe { symbol(library, c"nvmlDeviceGetName") },
            get_utilization: unsafe { symbol(library, c"nvmlDeviceGetUtilizationRates") },
            get_temperature: unsafe { symbol(library, c"nvmlDeviceGetTemperature") },
            get_clock: unsafe { symbol(library, c"nvmlDeviceGetClockInfo") },
            get_memory: unsafe { symbol(library, c"nvmlDeviceGetMemoryInfo") },
            get_power: unsafe { symbol(library, c"nvmlDeviceGetPowerUsage") },
        };
        for gpu in gpus {
            let handle = is_nvidia(gpu)
                .then(|| pci_slot_name(&sys_root.join("class/drm").join(&gpu.card).join("device")))
                .flatten()
                .and_then(|address| CString::new(address).ok())
                .and_then(|address| {
                    let mut handle = std::ptr::null_mut();
                    // SAFETY: address is NUL-terminated and handle storage is
                    // valid for this call; a failed lookup is not retained.
                    (unsafe { get_handle(address.as_ptr(), &raw mut handle) } == 0
                        && !handle.is_null())
                    .then_some(handle)
                });
            if let Some(device) = handle
                && let Some(name) = session.name(device)
            {
                gpu.model = name;
            }
            session.devices.push(handle);
        }
        Some(session)
    }

    pub fn sample(&self, index: usize, gpu: &mut GpuSnapshot) {
        let Some(device) = self.devices.get(index).copied().flatten() else {
            return;
        };
        gpu.utilization_percent = self.get_utilization.and_then(|read| {
            let mut value = Utilization::default();
            (unsafe { read(device, &raw mut value) } == 0 && value.gpu <= 100)
                .then_some(f64::from(value.gpu))
        });
        gpu.temperature_celsius = self.get_temperature.and_then(|read| {
            let mut value = 0;
            (unsafe { read(device, 0, &raw mut value) } == 0 && value <= 150)
                .then_some(f64::from(value))
        });
        gpu.clock_mhz = self.get_clock.and_then(|read| {
            let mut value = 0;
            (unsafe { read(device, 0, &raw mut value) } == 0 && value > 0)
                .then_some(f64::from(value))
        });
        if let Some(read) = self.get_memory {
            let mut value = Memory::default();
            if unsafe { read(device, &raw mut value) } == 0 && value.used <= value.total {
                gpu.vram_used_bytes = Some(value.used);
                gpu.vram_total_bytes = Some(value.total);
            } else {
                gpu.vram_used_bytes = None;
                gpu.vram_total_bytes = None;
            }
        }
        gpu.power_watts = self.get_power.and_then(|read| {
            let mut milliwatts = 0;
            (unsafe { read(device, &raw mut milliwatts) } == 0)
                .then_some(f64::from(milliwatts) / 1_000.0)
        });
    }

    fn name(&self, device: Device) -> Option<String> {
        let mut buffer = [0_u8; 96];
        let read = self.get_name?;
        if unsafe {
            read(
                device,
                buffer.as_mut_ptr().cast(),
                u32::try_from(buffer.len()).ok()?,
            )
        } != 0
        {
            return None;
        }
        let end = buffer.iter().position(|byte| *byte == 0)?;
        let name = CStr::from_bytes_with_nul(&buffer[..=end])
            .ok()?
            .to_str()
            .ok()?
            .trim();
        (!name.is_empty()).then(|| name.to_owned())
    }
}

impl Drop for NvidiaNvml {
    fn drop(&mut self) {
        // SAFETY: the session was initialized once, owns the library handle,
        // and no device handles escape it.
        unsafe {
            (self.shutdown)();
            libc::dlclose(self.library);
        }
    }
}

#[must_use]
pub fn is_nvidia(gpu: &GpuSnapshot) -> bool {
    gpu.vendor_id
        .trim_start_matches("0x")
        .eq_ignore_ascii_case("10de")
}

fn pci_slot_name(device: &Path) -> Option<String> {
    let uevent = fs::read_to_string(device.join("uevent")).ok()?;
    let address = uevent
        .lines()
        .find_map(|line| line.strip_prefix("PCI_SLOT_NAME="))?;
    let bytes = address.as_bytes();
    if bytes.len() != 12
        || !bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b':',
            10 => *byte == b'.',
            _ => byte.is_ascii_hexdigit(),
        })
    {
        return None;
    }
    Some(address.to_owned())
}

unsafe fn symbol<T: Copy>(library: *mut c_void, name: &CStr) -> Option<T> {
    // SAFETY: name is NUL-terminated, the library remains loaded for the
    // returned pointer's lifetime, and callers specify the exact ABI type.
    let pointer = unsafe { libc::dlsym(library, name.as_ptr()) };
    if pointer.is_null() || mem::size_of::<T>() != mem::size_of::<*mut c_void>() {
        return None;
    }
    Some(unsafe { mem::transmute_copy(&pointer) })
}
