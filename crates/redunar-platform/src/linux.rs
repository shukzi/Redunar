use redunar_core::{CpuSnapshot, GpuSnapshot, HardwareProbe, ProbeError, SystemSnapshot};
use redunar_nvidia_nvml::{NvidiaNvml, is_nvidia};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct LinuxHardwareProbe {
    proc_root: PathBuf,
    sys_root: PathBuf,
    allow_nvidia_beta: bool,
}

/// Stateful aggregate CPU utilization sampler backed by `/proc/stat`.
///
/// Sampling performs one small file read. The caller controls the interval;
/// Redunar's live monitor normally calls this once per second.
#[derive(Clone, Debug)]
pub struct LinuxCpuUtilizationSampler {
    stat_path: PathBuf,
    previous: Option<CpuTimes>,
}

/// A low-overhead telemetry session with hardware paths discovered once.
///
/// `sample` does not enumerate `/sys`: it clones the stable hardware identity
/// captured at construction and refreshes values through cached exact paths.
#[derive(Debug)]
pub struct LinuxTelemetrySampler {
    snapshot: SystemSnapshot,
    memory_path: PathBuf,
    cpu_utilization: LinuxCpuUtilizationSampler,
    cpu_paths: CpuTelemetryPaths,
    gpu_paths: Vec<GpuTelemetryPaths>,
    nvidia_nvml: Option<NvidiaNvml>,
}

#[derive(Clone, Debug)]
struct CpuTelemetryPaths {
    temperature: Option<PathBuf>,
    governor: PathBuf,
    energy_performance_preference: PathBuf,
}

#[derive(Clone, Debug)]
struct GpuTelemetryPaths {
    temperature: Option<PathBuf>,
    utilization: PathBuf,
    clock: Option<PathBuf>,
    vram_used: PathBuf,
    vram_total: PathBuf,
    power: Option<PathBuf>,
    performance_level: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CpuTimes {
    counters: [u64; 8],
    total: u64,
    idle: u64,
}

impl Default for LinuxCpuUtilizationSampler {
    fn default() -> Self {
        Self::new("/proc")
    }
}

impl LinuxCpuUtilizationSampler {
    #[must_use]
    pub fn new(proc_root: impl Into<PathBuf>) -> Self {
        Self {
            stat_path: proc_root.into().join("stat"),
            previous: None,
        }
    }

    /// Read aggregate CPU utilization since the previous valid sample.
    ///
    /// The first reading establishes a baseline and returns `None`. Malformed
    /// data is ignored without losing that baseline. If kernel counters reset
    /// or wrap, the new reading becomes the baseline for the next call.
    #[must_use]
    pub fn sample(&mut self) -> Option<f64> {
        let contents = fs::read_to_string(&self.stat_path).ok()?;
        let current = parse_cpu_times(&contents)?;
        let previous = self.previous.replace(current)?;

        if current
            .counters
            .iter()
            .zip(previous.counters)
            .any(|(current, previous)| *current < previous)
        {
            return None;
        }

        let total_delta = current.total.checked_sub(previous.total)?;
        let idle_delta = current.idle.checked_sub(previous.idle)?;
        if total_delta == 0 || idle_delta > total_delta {
            return None;
        }

        let busy_delta = total_delta - idle_delta;
        // At the intended one-second cadence these deltas are tiny. Reject an
        // implausibly long interval instead of silently losing integer
        // precision while converting full-width kernel counters to `f64`.
        let busy_delta = u32::try_from(busy_delta).ok()?;
        let total_delta = u32::try_from(total_delta).ok()?;
        Some((f64::from(busy_delta) * 100.0 / f64::from(total_delta)).clamp(0.0, 100.0))
    }
}

impl LinuxTelemetrySampler {
    /// Discover supported hardware and cache all telemetry paths.
    ///
    /// # Errors
    ///
    /// Returns a [`ProbeError`] when the CPU identity source cannot be read.
    pub fn new(
        proc_root: impl Into<PathBuf>,
        sys_root: impl Into<PathBuf>,
    ) -> Result<Self, ProbeError> {
        Self::with_nvidia_beta(proc_root, sys_root, false)
    }

    /// Discover NVIDIA identity and optional driver telemetry only for an
    /// explicit beta session. Missing NVML never hides CPU or AMD readings.
    ///
    /// # Errors
    ///
    /// Returns a [`ProbeError`] when required CPU identity is unavailable.
    pub fn with_nvidia_beta(
        proc_root: impl Into<PathBuf>,
        sys_root: impl Into<PathBuf>,
        allow_nvidia_beta: bool,
    ) -> Result<Self, ProbeError> {
        let proc_root = proc_root.into();
        let sys_root = sys_root.into();
        let probe = LinuxHardwareProbe::new(&proc_root, &sys_root)
            .with_nvidia_beta_enabled(allow_nvidia_beta);
        let mut snapshot = probe.snapshot()?;
        let nvidia_nvml = allow_nvidia_beta
            .then(|| NvidiaNvml::open(&sys_root, &mut snapshot.gpus))
            .flatten();
        let cpufreq = sys_root.join("devices/system/cpu/cpu0/cpufreq");
        let cpu_paths = CpuTelemetryPaths {
            temperature: find_cpu_temperature_path(&sys_root),
            governor: cpufreq.join("scaling_governor"),
            energy_performance_preference: cpufreq.join("energy_performance_preference"),
        };
        let gpu_paths = snapshot
            .gpus
            .iter()
            .map(|gpu| discover_gpu_telemetry_paths(&sys_root, &gpu.card))
            .collect();

        Ok(Self {
            snapshot,
            memory_path: proc_root.join("meminfo"),
            cpu_utilization: LinuxCpuUtilizationSampler::new(proc_root),
            cpu_paths,
            gpu_paths,
            nvidia_nvml,
        })
    }

    /// Refresh dynamic metrics through cached paths only.
    #[must_use]
    pub fn sample(&mut self) -> SystemSnapshot {
        let mut snapshot = self.snapshot.clone();
        snapshot.memory = crate::memory::read_memory(&self.memory_path);
        snapshot.cpu.temperature_celsius = self
            .cpu_paths
            .temperature
            .as_ref()
            .and_then(read_millivalue);
        snapshot.cpu.utilization_percent = self.cpu_utilization.sample();
        snapshot.cpu.governor = read_trimmed(&self.cpu_paths.governor);
        snapshot.cpu.energy_performance_preference =
            read_trimmed(&self.cpu_paths.energy_performance_preference);

        for (index, (gpu, paths)) in snapshot.gpus.iter_mut().zip(&self.gpu_paths).enumerate() {
            if is_nvidia(gpu) {
                if let Some(nvml) = &self.nvidia_nvml {
                    nvml.sample(index, gpu);
                }
                continue;
            }
            gpu.temperature_celsius = paths.temperature.as_ref().and_then(read_millivalue);
            gpu.utilization_percent = read_number(&paths.utilization);
            gpu.clock_mhz = paths
                .clock
                .as_ref()
                .and_then(read_number)
                .map(|value| value / 1_000_000.0);
            gpu.vram_used_bytes = read_u64(&paths.vram_used);
            gpu.vram_total_bytes = read_u64(&paths.vram_total);
            gpu.power_watts = paths
                .power
                .as_ref()
                .and_then(read_number)
                .map(|value| value / 1_000_000.0);
            gpu.performance_level = read_trimmed(&paths.performance_level);
        }

        snapshot
    }
}

impl Default for LinuxHardwareProbe {
    fn default() -> Self {
        Self::new("/proc", "/sys")
    }
}

impl LinuxHardwareProbe {
    #[must_use]
    pub fn new(proc_root: impl Into<PathBuf>, sys_root: impl Into<PathBuf>) -> Self {
        Self {
            proc_root: proc_root.into(),
            sys_root: sys_root.into(),
            allow_nvidia_beta: false,
        }
    }

    /// Include NVIDIA GPU identity only when Beta access was enabled before
    /// this native service started.
    #[must_use]
    pub fn with_nvidia_beta_enabled(mut self, enabled: bool) -> Self {
        self.allow_nvidia_beta = enabled;
        self
    }

    fn cpu_snapshot(&self) -> Result<CpuSnapshot, ProbeError> {
        let cpuinfo_path = self.proc_root.join("cpuinfo");
        let cpuinfo = fs::read_to_string(&cpuinfo_path).map_err(|error| {
            ProbeError::new(format!(
                "could not read {}: {error}",
                cpuinfo_path.display()
            ))
        })?;

        let vendor = first_cpuinfo_value(&cpuinfo, "vendor_id").unwrap_or("Unknown");
        let model = first_cpuinfo_value(&cpuinfo, "model name").unwrap_or("Unknown CPU");
        let logical_cpus = cpuinfo
            .lines()
            .filter(|line| cpuinfo_key(line) == Some("processor"))
            .count();
        let cpufreq = self.sys_root.join("devices/system/cpu/cpu0/cpufreq");

        Ok(CpuSnapshot {
            vendor: vendor.to_owned(),
            model: model.to_owned(),
            logical_cpus,
            temperature_celsius: self.cpu_temperature(),
            utilization_percent: None,
            scaling_driver: read_trimmed(cpufreq.join("scaling_driver")),
            governor: read_trimmed(cpufreq.join("scaling_governor")),
            energy_performance_preference: read_trimmed(
                cpufreq.join("energy_performance_preference"),
            ),
        })
    }

    fn cpu_temperature(&self) -> Option<f64> {
        find_cpu_temperature_path(&self.sys_root).and_then(read_millivalue)
    }

    fn gpu_snapshots(&self) -> Vec<GpuSnapshot> {
        let drm_root = self.sys_root.join("class/drm");
        let cards: Vec<_> = sorted_directories(&drm_root)
            .into_iter()
            .filter(|directory| is_drm_card(directory))
            .collect();
        // Until a launched game's render device is tied to a PCI address,
        // showing one card's numbers on a hybrid machine would mislabel them.
        let has_nvidia = cards.iter().any(|card| {
            read_trimmed(card.join("device/vendor"))
                .is_some_and(|vendor| vendor.trim_start_matches("0x").eq_ignore_ascii_case("10de"))
        });
        if self.allow_nvidia_beta && has_nvidia && cards.len() != 1 {
            return Vec::new();
        }
        cards
            .into_iter()
            .filter_map(|card_path| Self::gpu_snapshot(&card_path, self.allow_nvidia_beta))
            .collect()
    }

    fn gpu_snapshot(card_path: &Path, allow_nvidia_beta: bool) -> Option<GpuSnapshot> {
        let card = card_path.file_name()?.to_str()?.to_owned();
        let device = card_path.join("device");
        let vendor_id = read_trimmed(device.join("vendor"))?;
        let device_id = read_trimmed(device.join("device"));
        let subsystem_vendor_id = read_trimmed(device.join("subsystem_vendor"));
        let subsystem_device_id = read_trimmed(device.join("subsystem_device"));

        let nvidia = vendor_id
            .trim_start_matches("0x")
            .eq_ignore_ascii_case("10de");
        if !(is_supported_gpu_vendor(&vendor_id) || allow_nvidia_beta && nvidia) {
            return None;
        }

        let hwmon = sorted_directories(&device.join("hwmon"))
            .into_iter()
            .find(|directory| {
                read_trimmed(directory.join("name")).is_some_and(|name| name == "amdgpu")
            });
        let driver = fs::read_link(device.join("driver"))
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .or_else(|| {
                hwmon
                    .as_ref()
                    .and_then(|directory| read_trimmed(directory.join("name")))
            });

        let temperature_celsius = hwmon
            .as_ref()
            .and_then(|directory| read_millivalue(directory.join("temp1_input")));
        let clock_mhz = hwmon
            .as_ref()
            .and_then(|directory| read_number(directory.join("freq1_input")))
            .map(|value| value / 1_000_000.0);
        let power_watts = hwmon
            .as_ref()
            .and_then(|directory| read_number(directory.join("power1_average")))
            .map(|value| value / 1_000_000.0);

        let model = if nvidia {
            device_id.as_ref().map_or_else(
                || "NVIDIA GPU".to_owned(),
                |id| format!("NVIDIA GPU ({id})"),
            )
        } else {
            gpu_model(
                device_id.as_deref(),
                subsystem_vendor_id.as_deref(),
                subsystem_device_id.as_deref(),
            )
        };
        Some(GpuSnapshot {
            card,
            vendor_id,
            model,
            device_id,
            driver,
            temperature_celsius: (!nvidia).then_some(temperature_celsius).flatten(),
            utilization_percent: (!nvidia)
                .then(|| read_number(device.join("gpu_busy_percent")))
                .flatten(),
            clock_mhz: (!nvidia).then_some(clock_mhz).flatten(),
            vram_used_bytes: (!nvidia)
                .then(|| read_u64(device.join("mem_info_vram_used")))
                .flatten(),
            vram_total_bytes: (!nvidia)
                .then(|| read_u64(device.join("mem_info_vram_total")))
                .flatten(),
            power_watts: (!nvidia).then_some(power_watts).flatten(),
            performance_level: (!nvidia)
                .then(|| read_trimmed(device.join("power_dpm_force_performance_level")))
                .flatten(),
        })
    }
}

impl HardwareProbe for LinuxHardwareProbe {
    fn snapshot(&self) -> Result<SystemSnapshot, ProbeError> {
        Ok(SystemSnapshot {
            memory: crate::memory::read_memory(&self.proc_root.join("meminfo")),
            cpu: self.cpu_snapshot()?,
            gpus: self.gpu_snapshots(),
        })
    }
}

fn first_cpuinfo_value<'a>(cpuinfo: &'a str, key: &str) -> Option<&'a str> {
    cpuinfo.lines().find_map(|line| {
        let (candidate, value) = line.split_once(':')?;
        (candidate.trim() == key).then(|| value.trim())
    })
}

fn cpuinfo_key(line: &str) -> Option<&str> {
    line.split_once(':').map(|(key, _)| key.trim())
}

fn parse_cpu_times(stat: &str) -> Option<CpuTimes> {
    let aggregate = stat.lines().find(|line| {
        line.strip_prefix("cpu")
            .and_then(|suffix| suffix.chars().next())
            .is_some_and(char::is_whitespace)
    })?;
    let mut fields = aggregate.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }

    // user, nice, system and idle are required by proc_stat(5). The next
    // four fields are present on modern kernels. Guest counters are excluded
    // because Linux already includes them in user and nice.
    let mut counters = [0_u64; 8];
    for counter in counters.iter_mut().take(4) {
        *counter = fields.next()?.parse().ok()?;
    }
    for counter in counters.iter_mut().skip(4) {
        let Some(value) = fields.next() else {
            break;
        };
        *counter = value.parse().ok()?;
    }

    let total = counters
        .iter()
        .try_fold(0_u64, |sum, counter| sum.checked_add(*counter))?;
    let idle = counters[3].checked_add(counters[4])?;
    Some(CpuTimes {
        counters,
        total,
        idle,
    })
}

fn find_cpu_temperature_path(sys_root: &Path) -> Option<PathBuf> {
    sorted_directories(&sys_root.join("class/hwmon"))
        .into_iter()
        .find_map(|directory| {
            let name = read_trimmed(directory.join("name"))?;
            matches!(name.as_str(), "k10temp" | "zenpower").then(|| directory.join("temp1_input"))
        })
}

fn discover_gpu_telemetry_paths(sys_root: &Path, card: &str) -> GpuTelemetryPaths {
    let device = sys_root.join("class/drm").join(card).join("device");
    let hwmon = sorted_directories(&device.join("hwmon"))
        .into_iter()
        .find(|directory| read_trimmed(directory.join("name")).as_deref() == Some("amdgpu"));

    GpuTelemetryPaths {
        temperature: hwmon.as_ref().map(|path| path.join("temp1_input")),
        utilization: device.join("gpu_busy_percent"),
        clock: hwmon.as_ref().map(|path| path.join("freq1_input")),
        vram_used: device.join("mem_info_vram_used"),
        vram_total: device.join("mem_info_vram_total"),
        power: hwmon.as_ref().map(|path| path.join("power1_average")),
        performance_level: device.join("power_dpm_force_performance_level"),
    }
}

fn is_drm_card(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.strip_prefix("card").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
    }) && path.join("device").is_dir()
}

fn sorted_directories(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut directories: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    directories.sort();
    directories
}

fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_owned())
}

fn read_number(path: impl AsRef<Path>) -> Option<f64> {
    read_trimmed(path)?.parse().ok()
}

fn read_u64(path: impl AsRef<Path>) -> Option<u64> {
    read_trimmed(path)?.parse().ok()
}

fn read_millivalue(path: impl AsRef<Path>) -> Option<f64> {
    read_number(path).map(|value| value / 1_000.0)
}

fn is_supported_gpu_vendor(vendor_id: &str) -> bool {
    matches!(
        vendor_id
            .trim_start_matches("0x")
            .to_ascii_lowercase()
            .as_str(),
        "1002"
    )
}

fn gpu_model(
    device_id: Option<&str>,
    subsystem_vendor_id: Option<&str>,
    subsystem_device_id: Option<&str>,
) -> String {
    let vendor = "AMD Radeon";
    let raw_device_id = device_id;
    let normalized_device_id =
        raw_device_id.map(|id| id.trim_start_matches("0x").to_ascii_lowercase());
    let subsystem_vendor_id =
        subsystem_vendor_id.map(|id| id.trim_start_matches("0x").to_ascii_lowercase());
    let subsystem_device_id =
        subsystem_device_id.map(|id| id.trim_start_matches("0x").to_ascii_lowercase());
    if normalized_device_id.as_deref() == Some("73bf")
        && subsystem_vendor_id.as_deref() == Some("1458")
        && subsystem_device_id.as_deref() == Some("2328")
    {
        // Navi 21's PCI device ID is shared by the RX 6800, RX 6800 XT, and
        // RX 6900 XT. This Gigabyte subsystem pair identifies the RX 6800 XT
        // board while preserving the generic architecture fallback below.
        return "AMD Radeon RX 6800 XT".to_owned();
    }
    match (vendor, normalized_device_id.as_deref()) {
        (_, Some("73bf")) => "AMD Radeon Navi 21".to_owned(),
        (_, Some(_)) => format!("{vendor} ({})", raw_device_id.unwrap_or_default()),
        (_, None) => vendor.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("redunar-platform-{}-{id}", std::process::id()));
            fs::create_dir_all(&root).expect("create fixture root");
            Self { root }
        }

        fn write(&self, relative: &str, value: &str) {
            let path = self.root.join(relative);
            fs::create_dir_all(path.parent().expect("fixture parent"))
                .expect("create fixture parents");
            let mut file = fs::File::create(path).expect("create fixture file");
            file.write_all(value.as_bytes())
                .expect("write fixture file");
        }

        fn probe(&self) -> LinuxHardwareProbe {
            LinuxHardwareProbe::new(self.root.join("proc"), self.root.join("sys"))
        }

        fn telemetry_sampler(&self) -> LinuxTelemetrySampler {
            LinuxTelemetrySampler::new(self.root.join("proc"), self.root.join("sys"))
                .expect("discover telemetry fixture")
        }

        fn remove(&self, relative: &str) {
            fs::remove_file(self.root.join(relative)).expect("remove fixture sensor");
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).expect("remove fixture");
        }
    }

    fn amd_fixture() -> Fixture {
        let fixture = Fixture::new();
        fixture.write(
            "proc/cpuinfo",
            concat!(
                "processor : 0\n",
                "vendor_id : AuthenticAMD\n",
                "model name : AMD Ryzen Test CPU\n\n",
                "processor : 1\n",
                "vendor_id : AuthenticAMD\n",
                "model name : AMD Ryzen Test CPU\n",
            ),
        );
        fixture.write(
            "sys/devices/system/cpu/cpu0/cpufreq/scaling_driver",
            "amd-pstate-epp\n",
        );
        fixture.write(
            "sys/devices/system/cpu/cpu0/cpufreq/scaling_governor",
            "powersave\n",
        );
        fixture.write(
            "sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference",
            "balance_performance\n",
        );
        for policy in [0, 1] {
            fixture.write(
                &format!("sys/devices/system/cpu/cpufreq/policy{policy}/scaling_governor"),
                "powersave\n",
            );
            fixture.write(
                &format!(
                    "sys/devices/system/cpu/cpufreq/policy{policy}/energy_performance_preference"
                ),
                "balance_performance\n",
            );
        }
        fixture.write("sys/class/hwmon/hwmon4/name", "k10temp\n");
        fixture.write("sys/class/hwmon/hwmon4/temp1_input", "42250\n");
        fixture.write("sys/class/drm/card1/device/vendor", "0x1002\n");
        fixture.write("sys/class/drm/card1/device/device", "0x73bf\n");
        fixture.write("sys/class/drm/card1/device/gpu_busy_percent", "96\n");
        fixture.write(
            "sys/class/drm/card1/device/mem_info_vram_used",
            "12025908428\n",
        );
        fixture.write(
            "sys/class/drm/card1/device/mem_info_vram_total",
            "17179869184\n",
        );
        fixture.write(
            "sys/class/drm/card1/device/power_dpm_force_performance_level",
            "auto\n",
        );
        fixture.write("sys/class/drm/card1/device/hwmon/hwmon2/name", "amdgpu\n");
        fixture.write(
            "sys/class/drm/card1/device/hwmon/hwmon2/temp1_input",
            "62000\n",
        );
        fixture.write(
            "sys/class/drm/card1/device/hwmon/hwmon2/freq1_input",
            "2480000000\n",
        );
        fixture.write(
            "sys/class/drm/card1/device/hwmon/hwmon2/power1_average",
            "210000000\n",
        );
        fixture
    }

    #[test]
    fn detects_amd_cpu_and_gpu_from_fixture() {
        let fixture = amd_fixture();
        let snapshot = fixture.probe().snapshot().expect("probe succeeds");

        assert_eq!(snapshot.cpu.model, "AMD Ryzen Test CPU");
        assert_eq!(snapshot.cpu.logical_cpus, 2);
        assert_eq!(snapshot.cpu.temperature_celsius, Some(42.25));
        assert_eq!(snapshot.cpu.utilization_percent, None);
        assert_eq!(
            snapshot.cpu.scaling_driver.as_deref(),
            Some("amd-pstate-epp")
        );
        assert_eq!(snapshot.gpus.len(), 1);

        let gpu = &snapshot.gpus[0];
        assert_eq!(gpu.model, "AMD Radeon Navi 21");
        assert_eq!(gpu.temperature_celsius, Some(62.0));
        assert_eq!(gpu.utilization_percent, Some(96.0));
        assert_eq!(gpu.clock_mhz, Some(2_480.0));
        assert_eq!(gpu.power_watts, Some(210.0));
        assert_eq!(gpu.driver.as_deref(), Some("amdgpu"));
    }

    #[test]
    fn disambiguates_gigabyte_rx_6800_xt_from_shared_navi_21_device_id() {
        assert_eq!(
            gpu_model(Some("0x73bf"), Some("0x1458"), Some("0x2328")),
            "AMD Radeon RX 6800 XT"
        );
        assert_eq!(gpu_model(Some("0x73bf"), None, None), "AMD Radeon Navi 21");
    }

    #[test]
    fn ignores_non_amd_drm_cards() {
        let fixture = amd_fixture();
        fixture.write("sys/class/drm/card0/device/vendor", "0x8086\n");
        fixture.write("sys/class/drm/card0/device/device", "0x1234\n");

        let snapshot = fixture.probe().snapshot().expect("probe succeeds");
        assert_eq!(snapshot.gpus.len(), 1);
        assert_eq!(snapshot.gpus[0].card, "card1");
    }

    #[test]
    fn ignores_unsupported_drm_cards() {
        let fixture = amd_fixture();
        fixture.write("sys/class/drm/card2/device/vendor", "0x8086\n");
        fixture.write("sys/class/drm/card2/device/device", "0x1234\n");
        fixture.write("sys/class/drm/card2/device/hwmon/hwmon3/name", "i915\n");
        fixture.write(
            "sys/class/drm/card2/device/hwmon/hwmon3/temp1_input",
            "58000\n",
        );

        let snapshot = fixture.probe().snapshot().expect("probe succeeds");
        assert!(snapshot.gpus.iter().all(|gpu| gpu.card != "card2"));
    }

    #[test]
    fn cpu_sampler_calculates_utilization_from_aggregate_deltas() {
        let fixture = amd_fixture();
        fixture.write(
            "proc/stat",
            "cpu  100 20 30 400 10 5 5 0 0 0\ncpu0 1 2 3 4\n",
        );
        let mut sampler = LinuxCpuUtilizationSampler::new(fixture.root.join("proc"));

        assert_eq!(sampler.sample(), None);
        fixture.write("proc/stat", "cpu  140 20 40 440 10 5 5 0 0 0\n");

        // 50 busy ticks out of 90 total ticks.
        assert_eq!(sampler.sample(), Some(5000.0 / 90.0));
    }

    #[test]
    fn cpu_sampler_recovers_after_counter_reset() {
        let fixture = amd_fixture();
        fixture.write("proc/stat", "cpu 100 0 0 100 0 0 0 0\n");
        let mut sampler = LinuxCpuUtilizationSampler::new(fixture.root.join("proc"));
        assert_eq!(sampler.sample(), None);

        fixture.write("proc/stat", "cpu 10 0 0 10 0 0 0 0\n");
        assert_eq!(sampler.sample(), None);
        fixture.write("proc/stat", "cpu 20 0 0 20 0 0 0 0\n");
        assert_eq!(sampler.sample(), Some(50.0));
    }

    #[test]
    fn cpu_sampler_rejects_partial_counter_reset() {
        let fixture = amd_fixture();
        fixture.write("proc/stat", "cpu 100 0 0 100 0 0 0 0\n");
        let mut sampler = LinuxCpuUtilizationSampler::new(fixture.root.join("proc"));
        assert_eq!(sampler.sample(), None);

        // The total increased, but the user counter reset.
        fixture.write("proc/stat", "cpu 20 0 0 300 0 0 0 0\n");
        assert_eq!(sampler.sample(), None);
        fixture.write("proc/stat", "cpu 30 0 0 310 0 0 0 0\n");
        assert_eq!(sampler.sample(), Some(50.0));
    }

    #[test]
    fn cpu_sampler_ignores_malformed_reading_without_losing_baseline() {
        let fixture = amd_fixture();
        fixture.write("proc/stat", "cpu 100 0 0 100 0 0 0 0\n");
        let mut sampler = LinuxCpuUtilizationSampler::new(fixture.root.join("proc"));
        assert_eq!(sampler.sample(), None);

        fixture.write("proc/stat", "cpu invalid data\n");
        assert_eq!(sampler.sample(), None);
        fixture.write("proc/stat", "cpu 130 0 0 110 0 0 0 0\n");
        assert_eq!(sampler.sample(), Some(75.0));
    }

    #[test]
    fn cpu_sampler_rejects_zero_delta_and_overflow() {
        assert_eq!(parse_cpu_times("cpu 18446744073709551615 1 0 0\n"), None);

        let fixture = amd_fixture();
        fixture.write("proc/stat", "cpu 10 0 0 10\n");
        let mut sampler = LinuxCpuUtilizationSampler::new(fixture.root.join("proc"));
        assert_eq!(sampler.sample(), None);
        assert_eq!(sampler.sample(), None);
    }

    #[test]
    fn telemetry_sampler_updates_cached_dynamic_paths() {
        let fixture = amd_fixture();
        fixture.write("proc/stat", "cpu 100 0 0 100 0 0 0 0\n");
        let mut sampler = fixture.telemetry_sampler();
        assert_eq!(sampler.sample().cpu.utilization_percent, None);

        // These lexically earlier sensors would win a fresh discovery. The
        // live sampler must continue reading the exact paths it cached.
        fixture.write("sys/class/hwmon/hwmon0/name", "k10temp\n");
        fixture.write("sys/class/hwmon/hwmon0/temp1_input", "99000\n");
        fixture.write("sys/class/drm/card1/device/hwmon/hwmon0/name", "amdgpu\n");
        fixture.write(
            "sys/class/drm/card1/device/hwmon/hwmon0/temp1_input",
            "99000\n",
        );

        fixture.write("proc/stat", "cpu 150 0 0 150 0 0 0 0\n");
        fixture.write("sys/class/hwmon/hwmon4/temp1_input", "51000\n");
        fixture.write(
            "sys/devices/system/cpu/cpu0/cpufreq/scaling_governor",
            "performance\n",
        );
        fixture.write(
            "sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference",
            "performance\n",
        );
        fixture.write("sys/class/drm/card1/device/gpu_busy_percent", "81\n");
        fixture.write(
            "sys/class/drm/card1/device/mem_info_vram_used",
            "8589934592\n",
        );
        fixture.write(
            "sys/class/drm/card1/device/mem_info_vram_total",
            "17179869184\n",
        );
        fixture.write(
            "sys/class/drm/card1/device/power_dpm_force_performance_level",
            "high\n",
        );
        fixture.write(
            "sys/class/drm/card1/device/hwmon/hwmon2/temp1_input",
            "70000\n",
        );
        fixture.write(
            "sys/class/drm/card1/device/hwmon/hwmon2/freq1_input",
            "2200000000\n",
        );
        fixture.write(
            "sys/class/drm/card1/device/hwmon/hwmon2/power1_average",
            "180000000\n",
        );

        let snapshot = sampler.sample();
        assert_eq!(snapshot.cpu.model, "AMD Ryzen Test CPU");
        assert_eq!(snapshot.cpu.temperature_celsius, Some(51.0));
        assert_eq!(snapshot.cpu.utilization_percent, Some(50.0));
        assert_eq!(snapshot.cpu.governor.as_deref(), Some("performance"));
        assert_eq!(
            snapshot.cpu.energy_performance_preference.as_deref(),
            Some("performance")
        );

        let gpu = &snapshot.gpus[0];
        assert_eq!(gpu.model, "AMD Radeon Navi 21");
        assert_eq!(gpu.temperature_celsius, Some(70.0));
        assert_eq!(gpu.utilization_percent, Some(81.0));
        assert_eq!(gpu.clock_mhz, Some(2_200.0));
        assert_eq!(gpu.vram_used_bytes, Some(8_589_934_592));
        assert_eq!(gpu.vram_total_bytes, Some(17_179_869_184));
        assert_eq!(gpu.power_watts, Some(180.0));
        assert_eq!(gpu.performance_level.as_deref(), Some("high"));
    }

    #[test]
    fn telemetry_sampler_degrades_when_cached_sensors_disappear() {
        let fixture = amd_fixture();
        fixture.write("proc/stat", "cpu 100 0 0 100 0 0 0 0\n");
        let mut sampler = fixture.telemetry_sampler();
        let _ = sampler.sample();

        for path in [
            "proc/stat",
            "sys/class/hwmon/hwmon4/temp1_input",
            "sys/devices/system/cpu/cpu0/cpufreq/scaling_governor",
            "sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference",
            "sys/class/drm/card1/device/gpu_busy_percent",
            "sys/class/drm/card1/device/mem_info_vram_used",
            "sys/class/drm/card1/device/mem_info_vram_total",
            "sys/class/drm/card1/device/power_dpm_force_performance_level",
            "sys/class/drm/card1/device/hwmon/hwmon2/temp1_input",
            "sys/class/drm/card1/device/hwmon/hwmon2/freq1_input",
            "sys/class/drm/card1/device/hwmon/hwmon2/power1_average",
        ] {
            fixture.remove(path);
        }

        let snapshot = sampler.sample();
        assert_eq!(snapshot.cpu.model, "AMD Ryzen Test CPU");
        assert_eq!(snapshot.cpu.temperature_celsius, None);
        assert_eq!(snapshot.cpu.utilization_percent, None);
        assert_eq!(snapshot.cpu.governor, None);
        assert_eq!(snapshot.cpu.energy_performance_preference, None);
        assert_eq!(
            snapshot.cpu.scaling_driver.as_deref(),
            Some("amd-pstate-epp")
        );

        let gpu = &snapshot.gpus[0];
        assert_eq!(gpu.model, "AMD Radeon Navi 21");
        assert_eq!(gpu.temperature_celsius, None);
        assert_eq!(gpu.utilization_percent, None);
        assert_eq!(gpu.clock_mhz, None);
        assert_eq!(gpu.vram_used_bytes, None);
        assert_eq!(gpu.vram_total_bytes, None);
        assert_eq!(gpu.power_watts, None);
        assert_eq!(gpu.performance_level, None);
    }
}
