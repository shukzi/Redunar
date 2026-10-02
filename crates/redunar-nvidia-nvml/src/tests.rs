//! The mock library resolves the same function table as production. No test
//! opens the installed NVML library or a real GPU. Each test thread owns its
//! driver state, including init/shutdown counters and sensor return codes.

use super::*;
use std::cell::RefCell;
use std::ffi::{CStr, c_char, c_void};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
struct MockDriver {
    load_failure: bool,
    missing_symbols: Vec<&'static [u8]>,
    init_result: i32,
    handle_result: i32,
    null_handle: bool,
    shutdown_result: i32,
    sensor_results: [i32; 6],
    invalid_values: bool,
    loads: usize,
    init_calls: usize,
    handle_addresses: Vec<String>,
    sensor_calls: [usize; 6],
    shutdown_calls: usize,
    unloads: usize,
    events: Vec<&'static str>,
}

thread_local! {
    static DRIVER: RefCell<MockDriver> = RefCell::new(MockDriver::default());
}

fn driver<T>(read: impl FnOnce(&mut MockDriver) -> T) -> T {
    DRIVER.with(|state| read(&mut state.borrow_mut()))
}

struct MockLoader;

impl LibraryLoader for MockLoader {
    fn load(&self) -> Result<Box<dyn Library>, NvmlFailure> {
        driver(|state| {
            state.loads += 1;
            state.events.push("load");
            if state.load_failure {
                Err(NvmlFailure::LibraryUnavailable)
            } else {
                Ok(Box::new(MockLibrary) as Box<dyn Library>)
            }
        })
    }
}

struct MockLibrary;

// SAFETY: each mock symbol implements precisely the NVML signature resolved
// by Functions. The functions are static and the test state outlives sessions.
unsafe impl Library for MockLibrary {
    fn symbol(&self, name: &CStr) -> *mut c_void {
        if driver(|state| state.missing_symbols.contains(&name.to_bytes())) {
            return std::ptr::null_mut();
        }
        match name.to_bytes() {
            b"nvmlInit_v2" => init as *mut c_void,
            b"nvmlShutdown" => shutdown as *mut c_void,
            b"nvmlDeviceGetHandleByPciBusId_v2" => get_handle as *mut c_void,
            b"nvmlDeviceGetName" => get_name as *mut c_void,
            b"nvmlDeviceGetUtilizationRates" => utilization as *mut c_void,
            b"nvmlDeviceGetTemperature" => temperature as *mut c_void,
            b"nvmlDeviceGetClockInfo" => clock as *mut c_void,
            b"nvmlDeviceGetMemoryInfo" => memory as *mut c_void,
            b"nvmlDeviceGetPowerUsage" => power as *mut c_void,
            _ => std::ptr::null_mut(),
        }
    }
}

impl Drop for MockLibrary {
    fn drop(&mut self) {
        driver(|state| {
            state.unloads += 1;
            state.events.push("unload");
        });
    }
}

unsafe extern "C" fn init() -> i32 {
    driver(|state| {
        state.init_calls += 1;
        state.events.push("init");
        state.init_result
    })
}

unsafe extern "C" fn shutdown() -> i32 {
    driver(|state| {
        state.shutdown_calls += 1;
        state.events.push("shutdown");
        state.shutdown_result
    })
}

unsafe extern "C" fn get_handle(address: *const c_char, output: *mut Device) -> i32 {
    // SAFETY: Session supplies a NUL-terminated address and output storage.
    let address = unsafe { CStr::from_ptr(address) }
        .to_str()
        .expect("PCI address");
    let (result, null) = driver(|state| {
        state.handle_addresses.push(address.to_owned());
        (state.handle_result, state.null_handle)
    });
    // The adapter never dereferences this opaque handle.
    unsafe {
        *output = if null {
            std::ptr::null_mut()
        } else {
            std::ptr::dangling_mut::<u8>().cast()
        }
    };
    result
}

fn sensor_result(index: usize) -> (i32, bool) {
    driver(|state| {
        state.sensor_calls[index] += 1;
        (state.sensor_results[index], state.invalid_values)
    })
}

unsafe extern "C" fn get_name(_: Device, output: *mut c_char, length: u32) -> i32 {
    let (result, invalid) = sensor_result(0);
    let bytes = if invalid {
        b"bad\nname\0".as_slice()
    } else {
        b"NVIDIA Mock GPU\0".as_slice()
    };
    assert!(bytes.len() <= length as usize);
    // SAFETY: caller supplies a writable buffer of `length` bytes.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), output.cast(), bytes.len()) };
    result
}

unsafe extern "C" fn utilization(_: Device, output: *mut Utilization) -> i32 {
    let (result, invalid) = sensor_result(1);
    // SAFETY: caller supplies ABI-sized writable output storage.
    unsafe {
        *output = Utilization {
            gpu: if invalid { 101 } else { 76 },
            memory: 33,
        }
    };
    result
}

unsafe extern "C" fn temperature(_: Device, sensor: u32, output: *mut u32) -> i32 {
    assert_eq!(sensor, 0);
    let (result, invalid) = sensor_result(2);
    unsafe { *output = if invalid { 151 } else { 63 } };
    result
}

unsafe extern "C" fn clock(_: Device, clock_type: u32, output: *mut u32) -> i32 {
    assert_eq!(clock_type, 0);
    let (result, invalid) = sensor_result(3);
    unsafe { *output = if invalid { 0 } else { 2_145 } };
    result
}

unsafe extern "C" fn memory(_: Device, output: *mut Memory) -> i32 {
    let (result, invalid) = sensor_result(4);
    unsafe {
        *output = Memory {
            total: 8_589_934_592,
            free: 6_442_450_944,
            used: if invalid {
                8_589_934_593
            } else {
                2_147_483_648
            },
        }
    };
    result
}

unsafe extern "C" fn power(_: Device, output: *mut u32) -> i32 {
    let (result, _) = sensor_result(5);
    unsafe { *output = 123_456 };
    result
}

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    sys: PathBuf,
    gpus: Vec<GpuSnapshot>,
    now: Instant,
}

impl Fixture {
    fn new() -> Self {
        driver(|state| *state = MockDriver::default());
        let sys = std::env::temp_dir().join(format!(
            "redunar-nvml-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(sys.join("class/drm/card2/device")).expect("fixture device");
        fs::write(
            sys.join("class/drm/card2/device/uevent"),
            "PCI_SLOT_NAME=0000:04:00.0\n",
        )
        .expect("PCI fixture");
        Self {
            sys,
            now: Instant::now(),
            gpus: vec![GpuSnapshot {
                card: "card2".to_owned(),
                vendor_id: "0x10de".to_owned(),
                model: "NVIDIA GPU".to_owned(),
                device_id: None,
                driver: Some("nvidia".to_owned()),
                utilization_percent: None,
                temperature_celsius: None,
                clock_mhz: None,
                vram_used_bytes: None,
                vram_total_bytes: None,
                power_watts: None,
                performance_level: None,
            }],
        }
    }

    fn open(&mut self) -> NvidiaNvml {
        NvidiaNvml::with_loader(&self.sys, &mut self.gpus, Box::new(MockLoader), self.now)
    }

    fn sample(&mut self, adapter: &mut NvidiaNvml, seconds: u64) {
        adapter.sample_at(
            0,
            &mut self.gpus[0],
            self.now + Duration::from_secs(seconds),
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.sys).expect("remove fixture");
    }
}

fn assert_absent(gpu: &GpuSnapshot) {
    assert_eq!(gpu.utilization_percent, None);
    assert_eq!(gpu.temperature_celsius, None);
    assert_eq!(gpu.clock_mhz, None);
    assert_eq!(gpu.vram_used_bytes, None);
    assert_eq!(gpu.vram_total_bytes, None);
    assert_eq!(gpu.power_watts, None);
    assert_eq!(gpu.performance_level, None);
}

#[test]
fn resolves_mock_library_by_pci_and_preserves_sensor_units() {
    let mut fixture = Fixture::new();
    let mut adapter = fixture.open();
    fixture.sample(&mut adapter, 0);
    let gpu = &fixture.gpus[0];
    assert_eq!(gpu.model, "NVIDIA Mock GPU");
    assert_eq!(gpu.utilization_percent, Some(76.0));
    assert_eq!(gpu.temperature_celsius, Some(63.0));
    assert_eq!(gpu.clock_mhz, Some(2_145.0));
    assert_eq!(gpu.vram_used_bytes, Some(2_147_483_648));
    assert_eq!(gpu.vram_total_bytes, Some(8_589_934_592));
    assert_eq!(gpu.power_watts, Some(123.456));
    assert_eq!(adapter.diagnostics().readiness, NvidiaReadiness::Ready);
    assert_eq!(
        driver(|state| state.handle_addresses.clone()),
        ["0000:04:00.0"]
    );
    adapter.shutdown();
    adapter.shutdown();
    drop(adapter);
    assert_eq!(
        driver(|state| (state.shutdown_calls, state.unloads)),
        (1, 1)
    );
    assert_eq!(
        driver(|state| state.events.clone()),
        ["load", "init", "shutdown", "unload"]
    );
}

#[test]
fn missing_library_recovers_only_on_bounded_backoff() {
    let mut fixture = Fixture::new();
    driver(|state| state.load_failure = true);
    let mut adapter = fixture.open();
    assert_eq!(
        adapter.diagnostics().failure,
        Some(NvmlFailure::LibraryUnavailable)
    );
    for second in 0..5 {
        fixture.sample(&mut adapter, second);
    }
    assert_eq!(driver(|state| state.loads), 1);
    fixture.sample(&mut adapter, 5);
    assert_eq!(adapter.diagnostics().recovery_attempts, 1);
    assert_eq!(
        adapter.diagnostics().retry_delay,
        Some(Duration::from_secs(30))
    );
    fixture.sample(&mut adapter, 34);
    assert_eq!(driver(|state| state.loads), 2);
    fixture.sample(&mut adapter, 35);
    fixture.sample(&mut adapter, 155);
    for second in [156, 1_000, 100_000] {
        fixture.sample(&mut adapter, second);
    }
    assert_eq!(driver(|state| state.loads), 4);
    assert_eq!(adapter.diagnostics().recovery_attempts, 3);
    assert_eq!(
        adapter.diagnostics().readiness,
        NvidiaReadiness::RecoveryExhausted
    );
    assert_absent(&fixture.gpus[0]);
}

#[test]
fn missing_required_symbols_unload_without_initialization() {
    for (symbol, expected) in [
        (b"nvmlInit_v2".as_slice(), NvmlSymbol::Init),
        (b"nvmlShutdown".as_slice(), NvmlSymbol::Shutdown),
        (
            b"nvmlDeviceGetHandleByPciBusId_v2".as_slice(),
            NvmlSymbol::PciHandle,
        ),
    ] {
        let mut fixture = Fixture::new();
        driver(|state| state.missing_symbols.push(symbol));
        let mut adapter = fixture.open();
        assert_eq!(
            adapter.diagnostics().failure,
            Some(NvmlFailure::RequiredSymbolMissing(expected))
        );
        fixture.sample(&mut adapter, 1_000);
        assert_eq!(
            driver(|state| (
                state.loads,
                state.init_calls,
                state.shutdown_calls,
                state.unloads
            )),
            (1, 0, 0, 1)
        );
    }
}

#[test]
fn failed_init_does_not_shutdown_an_unowned_session() {
    let mut fixture = Fixture::new();
    driver(|state| state.init_result = 9);
    let mut adapter = fixture.open();
    assert_eq!(
        adapter.diagnostics().failure,
        Some(NvmlFailure::Initialization(NvmlError::DriverNotLoaded))
    );
    assert_eq!(
        driver(|state| (state.init_calls, state.shutdown_calls, state.unloads)),
        (1, 0, 1)
    );
    driver(|state| state.init_result = 0);
    fixture.sample(&mut adapter, 5);
    assert_eq!(adapter.diagnostics().readiness, NvidiaReadiness::Ready);
    assert_eq!(fixture.gpus[0].temperature_celsius, Some(63.0));
}

#[test]
fn failed_and_null_handles_shutdown_then_unload() {
    for null in [false, true] {
        let mut fixture = Fixture::new();
        driver(|state| {
            state.null_handle = null;
            state.handle_result = if null { 0 } else { 4 };
        });
        let adapter = fixture.open();
        assert_eq!(
            adapter.diagnostics().failure,
            Some(if null {
                NvmlFailure::DeviceHandleMissing
            } else {
                NvmlFailure::DeviceLookup(NvmlError::NoPermission)
            })
        );
        assert_eq!(
            driver(|state| (state.shutdown_calls, state.unloads, state.sensor_calls)),
            (1, 1, [0; 6])
        );
        assert_absent(&fixture.gpus[0]);
    }
}

#[test]
fn every_optional_symbol_stays_absent_without_poisoning_other_sensors() {
    for (symbol, sensor) in [
        (b"nvmlDeviceGetName".as_slice(), NvidiaSensor::Name),
        (
            b"nvmlDeviceGetUtilizationRates".as_slice(),
            NvidiaSensor::Utilization,
        ),
        (
            b"nvmlDeviceGetTemperature".as_slice(),
            NvidiaSensor::Temperature,
        ),
        (b"nvmlDeviceGetClockInfo".as_slice(), NvidiaSensor::Clock),
        (b"nvmlDeviceGetMemoryInfo".as_slice(), NvidiaSensor::Memory),
        (b"nvmlDeviceGetPowerUsage".as_slice(), NvidiaSensor::Power),
    ] {
        let mut fixture = Fixture::new();
        driver(|state| state.missing_symbols.push(symbol));
        let mut adapter = fixture.open();
        fixture.sample(&mut adapter, 0);
        let gpu = &fixture.gpus[0];
        let sensors = adapter.diagnostics().sensors;
        assert_eq!(
            gpu.utilization_percent,
            (sensor != NvidiaSensor::Utilization).then_some(76.0)
        );
        assert_eq!(
            gpu.temperature_celsius,
            (sensor != NvidiaSensor::Temperature).then_some(63.0)
        );
        assert_eq!(
            gpu.clock_mhz,
            (sensor != NvidiaSensor::Clock).then_some(2_145.0)
        );
        assert_eq!(
            gpu.vram_total_bytes,
            (sensor != NvidiaSensor::Memory).then_some(8_589_934_592)
        );
        assert_eq!(
            gpu.vram_used_bytes,
            (sensor != NvidiaSensor::Memory).then_some(2_147_483_648)
        );
        assert_eq!(
            gpu.power_watts,
            (sensor != NvidiaSensor::Power).then_some(123.456)
        );
        let status = match sensor {
            NvidiaSensor::Name => {
                assert_eq!(gpu.model, "NVIDIA GPU");
                sensors.name
            }
            NvidiaSensor::Utilization => sensors.utilization,
            NvidiaSensor::Temperature => sensors.temperature,
            NvidiaSensor::Clock => sensors.clock,
            NvidiaSensor::Memory => sensors.memory,
            NvidiaSensor::Power => sensors.power,
        };
        assert_eq!(status, NvidiaSensorStatus::MissingSymbol);
        assert_eq!(adapter.diagnostics().readiness, NvidiaReadiness::Degraded);
        assert_eq!(driver(|state| state.loads), 1);
        drop(adapter);
    }
}

#[test]
fn unsupported_sensors_and_invalid_values_never_retain_stale_metrics() {
    let mut fixture = Fixture::new();
    let mut adapter = fixture.open();
    fixture.sample(&mut adapter, 0);
    driver(|state| state.sensor_results = [3; 6]);
    fixture.sample(&mut adapter, 1);
    assert_absent(&fixture.gpus[0]);
    assert_eq!(
        adapter.diagnostics().sensors.memory,
        NvidiaSensorStatus::Unavailable(NvmlError::NotSupported)
    );
    driver(|state| {
        state.sensor_results = [0; 6];
        state.invalid_values = true;
    });
    fixture.sample(&mut adapter, 2);
    assert_eq!(fixture.gpus[0].utilization_percent, None);
    assert_eq!(fixture.gpus[0].temperature_celsius, None);
    assert_eq!(fixture.gpus[0].clock_mhz, None);
    assert_eq!(fixture.gpus[0].vram_used_bytes, None);
    assert_eq!(fixture.gpus[0].vram_total_bytes, None);
    assert_eq!(
        adapter.diagnostics().sensors.memory,
        NvidiaSensorStatus::InvalidValue
    );
    assert_eq!(fixture.gpus[0].power_watts, Some(123.456));
    assert_eq!(driver(|state| state.loads), 1);
}

#[test]
fn device_loss_clears_partial_sample_and_reopens_the_same_cached_pci_device() {
    let mut fixture = Fixture::new();
    let mut adapter = fixture.open();
    fixture.sample(&mut adapter, 0);
    driver(|state| state.sensor_results[2] = 15);
    fixture.sample(&mut adapter, 1);
    assert_absent(&fixture.gpus[0]);
    assert_eq!(adapter.diagnostics().readiness, NvidiaReadiness::Recovering);
    assert_eq!(
        adapter.diagnostics().failure,
        Some(NvmlFailure::SensorRead {
            sensor: NvidiaSensor::Temperature,
            error: NvmlError::DeviceLost
        })
    );
    assert_eq!(
        driver(|state| (state.shutdown_calls, state.unloads)),
        (1, 1)
    );
    let sensors = adapter.diagnostics().sensors;
    for status in [
        sensors.name,
        sensors.utilization,
        sensors.temperature,
        sensors.clock,
        sensors.memory,
        sensors.power,
    ] {
        assert_eq!(
            status,
            NvidiaSensorStatus::Unavailable(NvmlError::DeviceLost)
        );
    }
    fixture.sample(&mut adapter, 5);
    assert_eq!(driver(|state| state.loads), 1);
    // No re-enumeration, including if the fixture identity disappears.
    fs::remove_file(fixture.sys.join("class/drm/card2/device/uevent")).expect("remove sysfs");
    driver(|state| state.sensor_results[2] = 0);
    fixture.sample(&mut adapter, 6);
    assert_eq!(adapter.diagnostics().readiness, NvidiaReadiness::Ready);
    assert_eq!(adapter.diagnostics().recovery_attempts, 1);
    assert_eq!(
        driver(|state| state.handle_addresses.clone()),
        ["0000:04:00.0", "0000:04:00.0"]
    );
    assert_eq!(fixture.gpus[0].utilization_percent, Some(76.0));
    adapter.shutdown();
    assert_eq!(
        driver(|state| (state.shutdown_calls, state.unloads)),
        (2, 2)
    );
}

#[test]
fn cleanup_failure_is_typed_and_does_not_double_shutdown_or_restart_after_stop() {
    let mut fixture = Fixture::new();
    let mut adapter = fixture.open();
    driver(|state| {
        state.sensor_results[5] = 15;
        state.shutdown_result = 999;
    });
    fixture.sample(&mut adapter, 1);
    assert_eq!(
        adapter.diagnostics().cleanup_failure,
        Some(NvmlError::Unknown)
    );
    adapter.shutdown();
    fixture.sample(&mut adapter, 1_000);
    adapter.shutdown();
    drop(adapter);
    assert_eq!(
        driver(|state| (state.loads, state.shutdown_calls, state.unloads)),
        (1, 1, 1)
    );
    assert_absent(&fixture.gpus[0]);
}

#[test]
fn successful_recovery_does_not_reset_lifetime_retry_budget() {
    let mut fixture = Fixture::new();
    let mut adapter = fixture.open();
    let mut second = 0;
    for (index, delay) in [5, 30, 120].into_iter().enumerate() {
        second += 1;
        driver(|state| state.sensor_results[1] = 15);
        fixture.sample(&mut adapter, second);
        assert_absent(&fixture.gpus[0]);
        driver(|state| state.sensor_results[1] = 0);
        second += delay;
        fixture.sample(&mut adapter, second);
        assert_eq!(
            adapter.diagnostics().recovery_attempts,
            u8::try_from(index + 1).expect("three attempts")
        );
        assert_eq!(adapter.diagnostics().readiness, NvidiaReadiness::Ready);
    }
    driver(|state| state.sensor_results[1] = 15);
    fixture.sample(&mut adapter, second + 1);
    assert_eq!(
        adapter.diagnostics().readiness,
        NvidiaReadiness::RecoveryExhausted
    );
    fixture.sample(&mut adapter, 100_000);
    assert_absent(&fixture.gpus[0]);
    assert_eq!(
        driver(|state| (state.loads, state.shutdown_calls, state.unloads)),
        (4, 4, 4)
    );
}

#[test]
fn driver_loss_during_name_lookup_releases_the_initialized_session() {
    let mut fixture = Fixture::new();
    driver(|state| state.sensor_results[0] = 15);
    let mut adapter = fixture.open();
    assert_eq!(
        adapter.diagnostics().failure,
        Some(NvmlFailure::SensorRead {
            sensor: NvidiaSensor::Name,
            error: NvmlError::DeviceLost
        })
    );
    assert_eq!(
        driver(|state| (state.shutdown_calls, state.unloads)),
        (1, 1)
    );
    fixture.sample(&mut adapter, 1);
    assert_absent(&fixture.gpus[0]);
}

#[test]
fn drop_without_explicit_shutdown_releases_nvml_and_library_once() {
    let mut fixture = Fixture::new();
    let adapter = fixture.open();
    drop(adapter);
    assert_eq!(
        driver(|state| (state.shutdown_calls, state.unloads)),
        (1, 1)
    );
}

#[test]
fn cached_name_survives_cloned_snapshot_samples_after_recovery() {
    let mut fixture = Fixture::new();
    let original = fixture.gpus[0].clone();
    let mut adapter = fixture.open();
    driver(|state| state.sensor_results[1] = 15);
    fixture.sample(&mut adapter, 1);
    driver(|state| state.sensor_results[1] = 0);
    fixture.sample(&mut adapter, 6);
    let mut cloned = original;
    adapter.sample_at(0, &mut cloned, fixture.now + Duration::from_secs(7));
    assert_eq!(cloned.model, "NVIDIA Mock GPU");
    assert_eq!(cloned.utilization_percent, Some(76.0));
}

#[test]
fn invalid_or_duplicate_pci_identity_never_loads_driver_and_diagnostics_are_safe() {
    for value in [
        "private path",
        "0000:04:ff.0",
        "0000:04:00.8",
        "0000:04:00.0\nPCI_SLOT_NAME=0000:05:00.0",
    ] {
        let mut fixture = Fixture::new();
        fs::write(
            fixture.sys.join("class/drm/card2/device/uevent"),
            format!("PCI_SLOT_NAME={value}\n"),
        )
        .expect("invalid PCI");
        let adapter = fixture.open();
        assert_eq!(
            adapter.diagnostics().failure,
            Some(NvmlFailure::PciIdentityUnavailable)
        );
        assert_eq!(driver(|state| state.loads), 0);
        let debug = format!("{adapter:?}");
        assert!(!debug.contains(value));
        assert!(!debug.contains("card2"));
        assert!(!debug.contains("NVIDIA GPU"));
    }
}

#[test]
fn no_nvidia_and_ambiguous_nvidia_never_load_a_library() {
    let mut fixture = Fixture::new();
    fixture.gpus[0].vendor_id = "0x1002".to_owned();
    assert_eq!(
        fixture.open().diagnostics().readiness,
        NvidiaReadiness::NoDevice
    );
    fixture.gpus[0].vendor_id = "0x10de".to_owned();
    fixture.gpus.push(fixture.gpus[0].clone());
    assert_eq!(
        fixture.open().diagnostics().readiness,
        NvidiaReadiness::AmbiguousTopology
    );
    assert_eq!(driver(|state| state.loads), 0);
}
