//! Startup-only DRM topology. A display card (for example simpledrm) is not
//! evidence of another render GPU. Missing access never erases a render GPU
//! from attribution decisions.

use super::{is_drm_card, read_trimmed};
use redunar_nvidia_nvml::NvidiaReadiness;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) struct DrmTopology {
    pub cards: Vec<PathBuf>,
    render_devices: BTreeSet<PathBuf>,
    nvidia_devices: BTreeSet<PathBuf>,
    has_nvidia: bool,
    vendors: BTreeMap<PathBuf, u32>,
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
            vendors: BTreeMap::new(),
            incomplete,
        };
        for entry in &entries {
            if is_render_node(entry) {
                let device = entry.join("device");
                // Unknown identity still counts as a render GPU. Treating it
                // as absent could incorrectly enable a hybrid NVIDIA path.
                topology.render_devices.insert(device_identity(&device));
                if let Some(vendor) = vendor_id(&device) {
                    topology.vendors.insert(device_identity(&device), vendor);
                }
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
                if let Some(vendor) = vendor_id(&device) {
                    topology.vendors.insert(identity.clone(), vendor);
                }
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
            // An incomplete scan is not proof of a uniquely owned NVIDIA device.
            // Keep AMD readings but never guess NVIDIA attribution.
            NvidiaReadiness::Unavailable
        } else if !self.has_nvidia {
            NvidiaReadiness::NoDevice
        } else if self.nvidia_devices.is_empty() {
            NvidiaReadiness::NoRenderDevice
        } else if self.nvidia_devices.len() != 1
            || self
                .render_devices
                .iter()
                .any(|device| !matches!(self.vendors.get(device), Some(0x1002 | 0x8086 | 0x10de)))
        {
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

/// One physical render GPU matching a capture vendor. Enumeration order and
/// display-only devices have no authority over this selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DrmRenderDevice {
    pub card: String,
    pub render_node_index: u32,
}

/// Resolve a sole physical GPU for a vendor without opening a device. An
/// unknown render GPU might be another device of that vendor: fail closed.
/// UUID matching at encoder startup remains required before importing frames.
#[must_use]
pub fn unique_render_device(sys_root: &Path, vendor: u32) -> Option<DrmRenderDevice> {
    let topology = DrmTopology::discover(sys_root);
    if topology.incomplete
        || topology.render_devices.len() > 16
        || topology
            .render_devices
            .iter()
            .any(|device| !matches!(topology.vendors.get(device), Some(0x1002 | 0x8086 | 0x10de)))
    {
        return None;
    }
    let mut matching = topology
        .render_devices
        .iter()
        .filter(|device| topology.vendors.get(*device) == Some(&vendor));
    let device = matching.next()?;
    if matching.next().is_some() {
        return None;
    }
    let card = topology
        .cards
        .iter()
        .find(|card| &device_identity(&card.join("device")) == device)?;
    let (nodes, incomplete) = drm_entries(&card.join("device/drm"));
    if incomplete {
        return None;
    }
    let render_node_index = nodes
        .iter()
        .filter(|node| is_render_node(node))
        .filter_map(|node| {
            node.file_name()?
                .to_str()?
                .strip_prefix("renderD")?
                .parse::<u32>()
                .ok()
        })
        .min()?;
    Some(DrmRenderDevice {
        card: card.file_name()?.to_str()?.to_owned(),
        render_node_index,
    })
}

fn vendor_id(device: &Path) -> Option<u32> {
    let vendor = read_trimmed(device.join("vendor"))?;
    u32::from_str_radix(vendor.trim_start_matches("0x"), 16)
        .ok()
        .filter(|vendor| *vendor > 0 && u16::try_from(*vendor).is_ok())
}
