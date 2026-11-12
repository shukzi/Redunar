//! Read-only CPU frequency and package energy sampling through cached paths.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(super) struct CpuMetrics {
    clocks: Vec<PathBuf>,
    packages: Vec<Package>,
}
#[derive(Debug)]
struct Package {
    energy: PathBuf,
    range: Option<u64>,
    previous: Option<(u64, Instant)>,
}
impl CpuMetrics {
    pub(super) fn discover(sys: &Path) -> Self {
        let mut clocks = Vec::new();
        if let Ok(entries) = std::fs::read_dir(sys.join("devices/system/cpu/cpufreq")) {
            for entry in entries.take(4096).flatten() {
                if entry.file_name().to_string_lossy().starts_with("policy") {
                    clocks.push(entry.path().join("scaling_cur_freq"));
                }
            }
        }
        let mut packages = Vec::new();
        let mut package_names = std::collections::BTreeSet::new();
        let mut ambiguous_packages = false;
        if let Ok(entries) = std::fs::read_dir(sys.join("class/powercap")) {
            for entry in entries.take(256).flatten() {
                let path = entry.path();
                let Some(name) = std::fs::read_to_string(path.join("name"))
                    .ok()
                    .map(|name| name.trim().to_owned())
                    .filter(|name| name.starts_with("package-"))
                else {
                    continue;
                };
                // Different powercap controllers can expose the same package.
                // An ambiguous total is unavailable rather than counted twice.
                ambiguous_packages |= !package_names.insert(name);
                packages.push(Package {
                    energy: path.join("energy_uj"),
                    range: read_counter(path.join("max_energy_range_uj")).filter(|v| *v > 0),
                    previous: None,
                });
            }
        }
        if ambiguous_packages {
            packages.clear();
        }
        Self { clocks, packages }
    }
    pub(super) fn requires_privileged_reader(&self) -> bool {
        self.packages.iter().any(|package| {
            std::fs::File::open(&package.energy)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied)
        })
    }
    #[allow(dead_code)] // Used by the separately compiled privileged endpoint.
    pub(super) fn power_only(&mut self) {
        self.clocks.clear();
    }
    pub(super) fn sample(&mut self) -> (Option<f64>, Option<f64>) {
        self.sample_at(Instant::now())
    }
    fn sample_at(&mut self, now: Instant) -> (Option<f64>, Option<f64>) {
        let clock = self
            .clocks
            .iter()
            .filter_map(read_counter)
            .filter_map(|v| u32::try_from(v).ok())
            .filter(|v| *v > 0 && *v <= 20_000_000)
            .max()
            .map(|v| f64::from(v) / 1000.0);
        let power = if self.packages.is_empty() {
            None
        } else {
            // Sample every package even if one is missing, to refresh all baselines.
            let mut total = Some(0.0);
            for package in &mut self.packages {
                let value = package.sample(now);
                total = total.zip(value).map(|(sum, watts)| sum + watts);
            }
            total
        };
        (clock, power)
    }
}
impl Package {
    fn sample(&mut self, now: Instant) -> Option<f64> {
        let range = self.range?;
        let Some(current) = read_counter(&self.energy).filter(|v| *v < range) else {
            self.previous = None;
            return None;
        };
        let (previous, time) = self.previous.replace((current, now))?;
        let elapsed = now.checked_duration_since(time)?;
        if elapsed.is_zero() || elapsed > Duration::from_secs(5) {
            return None;
        }
        let delta = if current >= previous {
            current - previous
        } else {
            range.checked_sub(previous)?.checked_add(current)?
        };
        let delta = u32::try_from(delta).ok()?;
        let watts = f64::from(delta) / 1_000_000.0 / elapsed.as_secs_f64();
        (watts <= 2000.0).then_some(watts)
    }
}

// Kernel counters are short decimal values. Read into a bounded stack buffer so
// malformed inputs cannot allocate without a limit. Parsing needs no heap buffer.
fn read_counter(path: impl AsRef<Path>) -> Option<u64> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut bytes = [0; 65];
    let mut length = 0;
    loop {
        let count = file.read(&mut bytes[length..]).ok()?;
        if count == 0 {
            break;
        }
        length += count;
        if length == bytes.len() {
            return None;
        }
    }
    std::str::from_utf8(&bytes[..length])
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    #[test]
    fn numeric_sensor_reads_reject_oversized_and_malformed_values() {
        let path = std::env::temp_dir().join(format!(
            "redunar-counter-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        for (contents, expected) in [
            ("5225000\n".to_owned(), Some(5_225_000)),
            ("0".to_owned(), Some(0)),
            ("-1".to_owned(), None),
            ("NaN".to_owned(), None),
            ("1".repeat(65), None),
            (format!("{}1", " ".repeat(65)), None),
        ] {
            std::fs::write(&path, contents).unwrap();
            assert_eq!(read_counter(&path), expected);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    #[ignore = "synthetic latency measurement; run explicitly in an optimized build"]
    fn cached_cpu_sampling_latency() {
        let root =
            std::env::temp_dir().join(format!("redunar-counter-perf-{}", std::process::id()));
        for index in 0..32 {
            let policy = root.join(format!("devices/system/cpu/cpufreq/policy{index}"));
            std::fs::create_dir_all(&policy).unwrap();
            std::fs::write(policy.join("scaling_cur_freq"), "5225000\n").unwrap();
        }
        for index in 0..2 {
            let package = root.join(format!("class/powercap/intel-rapl:{index}"));
            std::fs::create_dir_all(&package).unwrap();
            std::fs::write(package.join("name"), format!("package-{index}")).unwrap();
            std::fs::write(package.join("energy_uj"), "100000000").unwrap();
            std::fs::write(package.join("max_energy_range_uj"), "1000000000").unwrap();
        }
        let mut sampler = CpuMetrics::discover(&root);
        let mut durations = Vec::with_capacity(1000);
        for _ in 0..100 {
            std::hint::black_box(sampler.sample());
        }
        for _ in 0..1000 {
            let start = Instant::now();
            assert_eq!(std::hint::black_box(sampler.sample()).0, Some(5225.0));
            durations.push(start.elapsed().as_nanos());
        }
        durations.sort_unstable();
        println!(
            "cached 32-policy / 2-package file fixture, 1000 samples: p50={} ns, p95={} ns, p99={} ns",
            durations[499], durations[949], durations[989]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn package_totals_reject_duplicates_and_incomplete_discovery() {
        let root = std::env::temp_dir().join(format!(
            "redunar-cpu-metrics-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let package = root.join("class/powercap/intel-rapl:0");
        let other = root.join("class/powercap/intel-rapl-mmio:0");
        for path in [&package, &other] {
            std::fs::create_dir_all(path).unwrap();
            std::fs::write(path.join("name"), "package-0").unwrap();
            std::fs::write(path.join("max_energy_range_uj"), "1000000000").unwrap();
            std::fs::write(path.join("energy_uj"), "100000000").unwrap();
        }
        let mut duplicate = CpuMetrics::discover(&root);
        let now = Instant::now();
        assert!(duplicate.packages.is_empty());
        assert_eq!(duplicate.sample_at(now).1, None);
        std::fs::write(other.join("name"), "package-1").unwrap();
        std::fs::write(other.join("max_energy_range_uj"), "unreadable").unwrap();
        let mut incomplete = CpuMetrics::discover(&root);
        assert_eq!(incomplete.packages.len(), 2);
        assert_eq!(incomplete.sample_at(now).1, None);
        std::fs::write(package.join("energy_uj"), "165000000").unwrap();
        assert_eq!(incomplete.sample_at(now + Duration::from_secs(1)).1, None);
        std::fs::write(other.join("max_energy_range_uj"), "1000000000").unwrap();
        let mut complete = CpuMetrics::discover(&root);
        assert_eq!(complete.sample_at(now).1, None);
        std::fs::write(package.join("energy_uj"), "230000000").unwrap();
        std::fs::write(other.join("energy_uj"), "165000000").unwrap();
        assert_eq!(
            complete.sample_at(now + Duration::from_secs(1)).1,
            Some(130.0)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clocks_and_package_power_handle_missing_wrap_and_stale_samples() {
        let root = std::env::temp_dir().join(format!(
            "redunar-cpu-metrics-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let clock = root.join("devices/system/cpu/cpufreq/policy0");
        let package = root.join("class/powercap/intel-rapl:0");
        std::fs::create_dir_all(&clock).unwrap();
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(clock.join("scaling_cur_freq"), "5225000").unwrap();
        std::fs::write(package.join("name"), "package-0").unwrap();
        std::fs::write(package.join("max_energy_range_uj"), "1000000000").unwrap();
        let energy = package.join("energy_uj");
        std::fs::write(&energy, "950000000").unwrap();
        let mut sampler = CpuMetrics::discover(&root);
        let now = Instant::now();
        assert_eq!(sampler.sample_at(now), (Some(5225.0), None));
        std::fs::write(&energy, "15000000").unwrap();
        assert_eq!(
            sampler.sample_at(now + Duration::from_secs(1)),
            (Some(5225.0), Some(65.0))
        );
        std::fs::write(&energy, "invalid").unwrap();
        assert_eq!(sampler.sample_at(now + Duration::from_secs(2)).1, None);
        std::fs::write(&energy, "80000000").unwrap();
        assert_eq!(sampler.sample_at(now + Duration::from_secs(3)).1, None);
        std::fs::write(&energy, "145000000").unwrap();
        assert_eq!(sampler.sample_at(now + Duration::from_secs(10)).1, None);
        std::fs::write(clock.join("scaling_cur_freq"), "-1").unwrap();
        assert_eq!(sampler.sample_at(now + Duration::from_secs(11)).0, None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
