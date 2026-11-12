//! Shared bounded hardware values and labels for both overlay renderers.
use crate::OverlayMetricSet as M;
use std::fmt::Write;

pub const LABELS: [(u32, &[u8]); 17] = [
    (M::FPS, b"FPS"),
    (M::FRAME_TIME, b"FRAME TIME"),
    (M::ONE_PERCENT_LOW, b"1% LOW"),
    (M::POINT_ONE_PERCENT_LOW, b"0.1% LOW"),
    (M::GRAPHICS_API, b"API"),
    (M::GPU_MODEL, b"GPU MODEL"),
    (M::GPU_LOAD, b"GPU LOAD"),
    (M::GPU_TEMPERATURE, b"GPU TEMP"),
    (M::GPU_POWER, b"GPU POWER"),
    (M::GPU_CLOCK, b"GPU CLOCK"),
    (M::VRAM, b"VRAM"),
    (M::CPU_MODEL, b"CPU MODEL"),
    (M::CPU_LOAD, b"CPU LOAD"),
    (M::CPU_TEMPERATURE, b"CPU TEMP"),
    (M::CPU_POWER, b"CPU POWER"),
    (M::CPU_CLOCK, b"CPU CLOCK"),
    (M::RAM, b"RAM"),
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HardwareDetails {
    pub cpu_model: MetricText,
    pub gpu_model: MetricText,
    pub cpu_clock_mhz: Option<u32>,
    pub gpu_clock_mhz: Option<u32>,
    pub cpu_power_tenths: Option<u32>,
    pub gpu_power_tenths: Option<u32>,
    pub ram_mib: Option<u32>,
    pub vram_mib: Option<u32>,
}

impl HardwareDetails {
    #[must_use]
    pub fn value(self, metric: u32, api: &str) -> MetricText {
        let mut text = MetricText::default();
        let (value, unit, tenths) = match metric {
            M::CPU_MODEL => return self.cpu_model.or_missing(),
            M::GPU_MODEL => return self.gpu_model.or_missing(),
            M::GRAPHICS_API => return MetricText::new(api),
            M::CPU_CLOCK => (self.cpu_clock_mhz, " MHz", false),
            M::GPU_CLOCK => (self.gpu_clock_mhz, " MHz", false),
            M::CPU_POWER => (self.cpu_power_tenths, " W", true),
            M::GPU_POWER => (self.gpu_power_tenths, " W", true),
            M::RAM => (self.ram_mib, " MiB", false),
            M::VRAM => (self.vram_mib, " MiB", false),
            _ => return MetricText::new("--"),
        };
        if let Some(value) = value {
            if tenths {
                let _ = write!(text, "{}.{:01}", value / 10, value % 10);
            } else {
                let _ = write!(text, "{value}");
            }
        } else {
            let _ = text.write_str("--");
        }
        let _ = text.write_str(unit);
        text
    }
}

/// Display-safe ASCII with a fixed maximum length. Long identities end in dots.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MetricText(pub [u8; 32]);
impl MetricText {
    #[must_use]
    pub fn new(value: &str) -> Self {
        let mut text = Self::default();
        let _ = text.write_str(value);
        text
    }
    /// Remove trademark and nominal-frequency suffixes from hardware labels.
    /// Keep the model number visible and emit only glyphs the overlay supports.
    #[must_use]
    pub fn model_name(value: &str) -> Self {
        let normalized = value
            .replace("(R)", "")
            .replace("(TM)", "")
            .replace(['®', '™'], "");
        let model = normalized.split(" @ ").next().unwrap_or(&normalized);
        let mut text = Self::default();
        for word in model.split_whitespace() {
            if word == "CPU" || word == "Processor" {
                continue;
            }
            if !text.as_bytes().is_empty() {
                let _ = text.write_str(" ");
            }
            for byte in word.bytes() {
                if byte.is_ascii_alphanumeric() || b"-./+".contains(&byte) {
                    let _ = text.write_char(char::from(byte));
                } else if byte.is_ascii() {
                    let _ = text.write_str(" ");
                }
            }
        }
        text
    }
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0[..self.0.iter().position(|b| *b == 0).unwrap_or(32)]
    }
    fn or_missing(self) -> Self {
        if self.as_bytes().is_empty() {
            Self::new("--")
        } else {
            self
        }
    }
}
impl Write for MetricText {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        let mut index = self.as_bytes().len();
        for byte in value.bytes().filter(u8::is_ascii) {
            if !byte.is_ascii_graphic() && byte != b' ' {
                continue;
            }
            if index == self.0.len() {
                self.0[29..].copy_from_slice(b"...");
                break;
            }
            self.0[index] = byte;
            index += 1;
        }
        Ok(())
    }
}

#[must_use]
pub const fn ribbon_width(metric: u32) -> u32 {
    match metric {
        M::CPU_MODEL | M::GPU_MODEL => 304,
        _ if metric >= 256 => 128,
        _ => 89,
    }
}
#[must_use]
pub const fn telemetry_width(metrics: u32) -> u32 {
    if metrics & (M::CPU_MODEL | M::GPU_MODEL) != 0 {
        400
    } else {
        300
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hardware_labels_preserve_model_numbers_and_use_renderable_glyphs() {
        let name = MetricText::model_name("12th Gen Intel(R) Core(TM) i7-12700K @ 3.60GHz");
        assert_eq!(name.as_bytes(), b"12th Gen Intel Core i7-12700K");
        let amd = MetricText::model_name("AMD Ryzen 7 9800X3D 8-Core Processor");
        assert_eq!(amd.as_bytes(), b"AMD Ryzen 7 9800X3D 8-Core");
        let gpu = MetricText::model_name("AMD Radeon [RX 7900 XTX]");
        for byte in gpu.as_bytes().iter().copied().filter(|b| *b != b' ') {
            assert!(
                crate::overlay_font::coverage_raster(byte)
                    .iter()
                    .any(|v| *v != 0)
            );
        }
        assert!(
            MetricText::model_name(&"LONG ".repeat(40))
                .as_bytes()
                .ends_with(b"...")
        );
    }
}
