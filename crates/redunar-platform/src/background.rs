use crate::{apply_priority, read_priority};
use std::fs;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BackgroundLoad {
    pub process_count: u32,
    pub known_interferers: Vec<String>,
    pub known_pids: Vec<(i32, String)>,
}

#[must_use]
pub fn observe_background_load() -> BackgroundLoad {
    let mut load = BackgroundLoad::default();
    let Ok(entries) = fs::read_dir("/proc") else {
        return load;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .filter(|v| v.bytes().all(|b| b.is_ascii_digit()))
        else {
            continue;
        };
        load.process_count = load.process_count.saturating_add(1);
        let Ok(comm) = fs::read_to_string(format!("/proc/{pid}/comm")) else {
            continue;
        };
        let comm = comm.trim();
        if [
            "firefox",
            "chrome",
            "chromium",
            "steamwebhelper",
            "baloo_file",
            "tracker-miner-f",
        ]
        .iter()
        .any(|known| comm.eq_ignore_ascii_case(known))
            && load.known_interferers.len() < 32
        {
            load.known_interferers.push(comm.to_owned());
            if let Ok(pid) = pid.parse::<i32>() {
                load.known_pids.push((pid, comm.to_owned()));
            }
        }
    }
    load.known_interferers.sort();
    load.known_interferers.dedup();
    load
}

#[derive(Default)]
pub struct BackgroundCoordinator {
    restored: Vec<(i32, i32)>,
}

impl BackgroundCoordinator {
    pub fn lower_known_processes(&mut self, load: &BackgroundLoad) {
        for &(pid, _) in &load.known_pids {
            if let Ok(previous) = read_priority(pid)
                && apply_priority(pid, 5).is_ok()
            {
                self.restored.push((pid, previous));
            }
        }
    }
    pub fn restore(&mut self) {
        for (pid, priority) in self.restored.drain(..) {
            let _ = apply_priority(pid, priority);
        }
    }
}

impl Drop for BackgroundCoordinator {
    fn drop(&mut self) {
        self.restore();
    }
}
