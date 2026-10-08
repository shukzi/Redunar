//! Discover one CPU package sensor once; live sampling uses its cached path.
use super::{read_millivalue, read_trimmed, sorted_directories};
use std::path::{Path, PathBuf};

pub(super) fn discover(sys_root: &Path) -> Option<PathBuf> {
    sorted_directories(&sys_root.join("class/hwmon"))
        .into_iter()
        .find_map(|directory| {
            let name = read_trimmed(directory.join("name"))?;
            match name.as_str() {
                "k10temp" | "zenpower" => readable_input(&directory, 1),
                // Select the documented package label, never an arbitrary
                // motherboard/GPU sensor or a single Intel core temperature.
                "coretemp" => (1..=128).find_map(|index| {
                    let label = read_trimmed(directory.join(format!("temp{index}_label")))?;
                    label.strip_prefix("Package id ")?.parse::<u32>().ok()?;
                    readable_input(&directory, index)
                }),
                _ => None,
            }
        })
}

fn readable_input(directory: &Path, index: u32) -> Option<PathBuf> {
    let path = directory.join(format!("temp{index}_input"));
    read_millivalue(&path).filter(|value| value.is_finite())?;
    Some(path)
}
