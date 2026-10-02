//! Startup-only DRM topology. A display card (for example simpledrm) is not
//! evidence of another render GPU. Missing access never erases a render GPU
//! from attribution decisions.

use super::{is_drm_card, read_trimmed};
use redunar_nvidia_nvml::NvidiaReadiness;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) struct DrmTopology {
    pub cards: Vec<PathBuf>,
    render_devices: BTreeSet<PathBuf>,
    nvidia_devices: BTreeSet<PathBuf>,
    has_nvidia: bool,
    incomplete: bool,
}

impl DrmTopology {
    pub fn discover(sys_root: &Path) -> Self {
        let root = sys_root.join("class/drm");
        let (entries, incomplete) = drm_entries(&root);
        let mut topology = Self {
            cards: entries
                .iter()
                .filter(|path| is_drm_card(path))
                .cloned()
                .collect(),
            render_devices: BTreeSet::new(),
            nvidia_devices: BTreeSet::new(),
            has_nvidia: false,
            incomplete,
        };
        for entry in &entries {
            if is_render_node(entry) {
                let device = entry.join("device");
                // Unknown identity still counts as a render GPU. Treating it
                // as absent could incorrectly enable a hybrid NVIDIA path.
                topology.render_devices.insert(device_identity(&device));
                if is_nvidia_device(&device) {
                    topology.has_nvidia = true;
                    topology.nvidia_devices.insert(device_identity(&device));
                }
            }
        }
        for card in &topology.cards {
            let device = card.join("device");
            let nvidia = is_nvidia_device(&device);
            topology.has_nvidia |= nvidia;
            // The per-device DRM directory is the kernel's render capability
            // signal even if /dev/dri is not available to this process.
            let (nodes, incomplete) = drm_entries(&device.join("drm"));
            topology.incomplete |= incomplete;
            if nodes.iter().any(|path| is_render_node(path)) {
                let identity = device_identity(&device);
                topology.render_devices.insert(identity.clone());
                if nvidia {
                    topology.nvidia_devices.insert(identity);
                }
            }
        }
        topology
    }

    pub fn nvidia_readiness(&self, beta: bool) -> NvidiaReadiness {
        if !beta {
            NvidiaReadiness::BetaDisabled
        } else if self.incomplete {
            // An incomplete scan is not proof of a single render device.
            // Keep AMD readings but never guess NVIDIA attribution.
            NvidiaReadiness::Unavailable
        } else if !self.has_nvidia {
            NvidiaReadiness::NoDevice
        } else if self.nvidia_devices.is_empty() {
            NvidiaReadiness::NoRenderDevice
        } else if self.render_devices.len() != 1 || self.nvidia_devices.len() != 1 {
            NvidiaReadiness::AmbiguousTopology
        } else {
            NvidiaReadiness::Ready
        }
    }

    pub fn is_nvidia_render_card(&self, card: &Path) -> bool {
        self.nvidia_devices
            .contains(&device_identity(&card.join("device")))
    }
}

fn drm_entries(root: &Path) -> (Vec<PathBuf>, bool) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => return (Vec::new(), error.kind() != std::io::ErrorKind::NotFound),
    };
    let mut paths = Vec::new();
    let mut incomplete = false;
    for entry in entries {
        match entry {
            Ok(entry) => paths.push(entry.path()),
            Err(_) => incomplete = true,
        }
    }
    paths.sort();
    (paths, incomplete)
}

fn device_identity(device: &Path) -> PathBuf {
    fs::canonicalize(device).unwrap_or_else(|_| device.to_path_buf())
}

fn is_render_node(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.strip_prefix("renderD").is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
}

fn is_nvidia_device(device: &Path) -> bool {
    read_trimmed(device.join("vendor"))
        .is_some_and(|vendor| vendor.trim_start_matches("0x").eq_ignore_ascii_case("10de"))
}
