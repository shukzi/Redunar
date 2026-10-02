//! The dynamically loaded driver ABI. All function pointers stay behind the
//! monitor adapter and expire before their owning library is released.

use crate::{NvmlFailure, NvmlSymbol};
use std::ffi::{CStr, c_char, c_void};
use std::mem;

pub(crate) type Device = *mut c_void;
pub(crate) type Init = unsafe extern "C" fn() -> i32;
pub(crate) type Shutdown = unsafe extern "C" fn() -> i32;
pub(crate) type GetHandle = unsafe extern "C" fn(*const c_char, *mut Device) -> i32;
pub(crate) type GetName = unsafe extern "C" fn(Device, *mut c_char, u32) -> i32;
pub(crate) type GetUtilization = unsafe extern "C" fn(Device, *mut Utilization) -> i32;
pub(crate) type GetTemperature = unsafe extern "C" fn(Device, u32, *mut u32) -> i32;
pub(crate) type GetClock = unsafe extern "C" fn(Device, u32, *mut u32) -> i32;
pub(crate) type GetMemory = unsafe extern "C" fn(Device, *mut Memory) -> i32;
pub(crate) type GetPower = unsafe extern "C" fn(Device, *mut u32) -> i32;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Utilization {
    pub gpu: u32,
    pub memory: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Memory {
    pub total: u64,
    pub free: u64,
    pub used: u64,
}

/// # Safety
/// Symbols must use the documented NVML ABI and remain callable until drop.
/// Each library owns one load reference, including on partial startup failure.
pub(crate) unsafe trait Library: Send {
    fn symbol(&self, name: &CStr) -> *mut c_void;
}

pub(crate) trait LibraryLoader: Send {
    fn load(&self) -> Result<Box<dyn Library>, NvmlFailure>;
}

pub(crate) struct DriverLoader;

impl LibraryLoader for DriverLoader {
    fn load(&self) -> Result<Box<dyn Library>, NvmlFailure> {
        // SAFETY: use only the fixed installed-driver soname; no environment
        // or application-provided path becomes an NVML library argument.
        let handle = unsafe {
            libc::dlopen(
                c"libnvidia-ml.so.1".as_ptr(),
                libc::RTLD_NOW | libc::RTLD_LOCAL,
            )
        };
        if handle.is_null() {
            return Err(NvmlFailure::LibraryUnavailable);
        }
        Ok(Box::new(DriverLibrary(handle)))
    }
}

struct DriverLibrary(*mut c_void);

// Dynamic-loader handles may move between threads. Calls remain owned by one
// monitor, and the load reference survives every resolved function pointer.
unsafe impl Send for DriverLibrary {}

// SAFETY: the installed driver exports the public NVML ABI. The loader owns
// its reference until all session calls and shutdown have finished.
unsafe impl Library for DriverLibrary {
    fn symbol(&self, name: &CStr) -> *mut c_void {
        // SAFETY: name is NUL-terminated and this handle is still loaded.
        unsafe { libc::dlsym(self.0, name.as_ptr()) }
    }
}

impl Drop for DriverLibrary {
    fn drop(&mut self) {
        // SAFETY: release exactly the reference acquired by dlopen.
        unsafe { libc::dlclose(self.0) };
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Functions {
    pub init: Init,
    pub shutdown: Shutdown,
    pub get_handle: GetHandle,
    pub get_name: Option<GetName>,
    pub get_utilization: Option<GetUtilization>,
    pub get_temperature: Option<GetTemperature>,
    pub get_clock: Option<GetClock>,
    pub get_memory: Option<GetMemory>,
    pub get_power: Option<GetPower>,
}

impl Functions {
    pub fn load(library: &dyn Library) -> Result<Self, NvmlFailure> {
        // SAFETY: each signature matches its public NVML symbol. Library's
        // contract keeps these callable until the session releases its owner.
        unsafe {
            Ok(Self {
                init: symbol(library, c"nvmlInit_v2")
                    .ok_or(NvmlFailure::RequiredSymbolMissing(NvmlSymbol::Init))?,
                shutdown: symbol(library, c"nvmlShutdown")
                    .ok_or(NvmlFailure::RequiredSymbolMissing(NvmlSymbol::Shutdown))?,
                get_handle: symbol(library, c"nvmlDeviceGetHandleByPciBusId_v2")
                    .ok_or(NvmlFailure::RequiredSymbolMissing(NvmlSymbol::PciHandle))?,
                get_name: symbol(library, c"nvmlDeviceGetName"),
                get_utilization: symbol(library, c"nvmlDeviceGetUtilizationRates"),
                get_temperature: symbol(library, c"nvmlDeviceGetTemperature"),
                get_clock: symbol(library, c"nvmlDeviceGetClockInfo"),
                get_memory: symbol(library, c"nvmlDeviceGetMemoryInfo"),
                get_power: symbol(library, c"nvmlDeviceGetPowerUsage"),
            })
        }
    }
}

unsafe fn symbol<T: Copy>(library: &dyn Library, name: &CStr) -> Option<T> {
    let pointer = library.symbol(name);
    if pointer.is_null() || mem::size_of::<T>() != mem::size_of::<*mut c_void>() {
        return None;
    }
    // SAFETY: caller selects the symbol's exact ABI; the library guarantees
    // that its address is valid for the session lifetime.
    Some(unsafe { mem::transmute_copy(&pointer) })
}
