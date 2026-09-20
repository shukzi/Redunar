use std::error::Error;
use std::fmt;

pub const OVERLAY_HARDWARE_TELEMETRY_BYTES: usize = 48;
pub const REPLAY_MENU_TELEMETRY_BYTES: usize = 128;
pub const REPLAY_SHORTCUT_LABEL_BYTES: usize = 40;

const MAGIC: [u8; 8] = *b"RDOVL001";
const VERSION: u16 = 4;
const CPU_UTILIZATION_PRESENT: u16 = 1 << 0;
const CPU_TEMPERATURE_PRESENT: u16 = 1 << 1;
const GPU_UTILIZATION_PRESENT: u16 = 1 << 2;
const GPU_TEMPERATURE_PRESENT: u16 = 1 << 3;
const KNOWN_FLAGS: u16 = CPU_UTILIZATION_PRESENT
    | CPU_TEMPERATURE_PRESENT
    | GPU_UTILIZATION_PRESENT
    | GPU_TEMPERATURE_PRESENT;
const MAX_UTILIZATION_TENTHS: u16 = 1_000;
const MAX_TEMPERATURE_TENTHS_CELSIUS: u16 = 2_000;
const REPLAY_MENU_MAGIC: [u8; 8] = *b"RDRPM001";
const REPLAY_MENU_VERSION: u16 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ReplayMenuStatus {
    Unavailable,
    Inactive,
    Buffering,
    Saving,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplayMenuTelemetry {
    pub revision: u64,
    pub visible: bool,
    pub pointer_pressed: bool,
    /// Normalized inclusive 0..=10,000 panel-space position.
    pub cursor_x: u16,
    pub cursor_y: u16,
    /// 0 none, 1..=8 duration, 9 save.
    pub hover_target: u8,
    pub pressed_target: u8,
    pub selected_duration_index: u8,
    pub status: ReplayMenuStatus,
    pub available_seconds: u16,
    pub click_revision: u16,
    pub frame_rate: u8,
    pub quality: u8,
    pub output_format: u8,
    pub save_enabled: bool,
    pub overlay_shortcut: ReplayShortcutLabel,
    pub save_shortcut: ReplayShortcutLabel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplayShortcutLabel {
    bytes: [u8; REPLAY_SHORTCUT_LABEL_BYTES],
    length: u8,
}

impl Default for ReplayShortcutLabel {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl ReplayShortcutLabel {
    pub const EMPTY: Self = Self {
        bytes: [0; REPLAY_SHORTCUT_LABEL_BYTES],
        length: 0,
    };

    /// Build one bounded display label from a validated shortcut chord.
    /// Modifiers and the key are uppercased and separated for legibility.
    #[must_use]
    pub fn from_shortcut(shortcut: &str) -> Self {
        let mut label = Self::EMPTY;
        for (index, part) in shortcut
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .enumerate()
        {
            if index > 0 {
                label.push_bytes(b" + ");
            }
            for byte in part.bytes() {
                label.push(byte.to_ascii_uppercase());
            }
        }
        label
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.length)]
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.push(*byte);
        }
    }

    fn push(&mut self, byte: u8) {
        let index = usize::from(self.length);
        if index < self.bytes.len() && (byte.is_ascii_graphic() || byte == b' ') {
            self.bytes[index] = byte;
            self.length = self.length.saturating_add(1);
        }
    }
}

/// Encode one daemon-owned Replay menu snapshot into its fixed private ABI.
///
/// # Errors
///
/// Returns an error when a field is outside its bounded domain or `output`
/// cannot hold the complete snapshot.
pub fn encode_replay_menu_telemetry(
    value: ReplayMenuTelemetry,
    output: &mut [u8],
) -> Result<usize, OverlayTelemetryError> {
    validate_replay_menu(value)?;
    if output.len() < REPLAY_MENU_TELEMETRY_BYTES {
        return Err(OverlayTelemetryError::new(
            "Replay menu output buffer is too small",
        ));
    }
    let output = &mut output[..REPLAY_MENU_TELEMETRY_BYTES];
    output.fill(0);
    output[..8].copy_from_slice(&REPLAY_MENU_MAGIC);
    write_u16(output, 8, REPLAY_MENU_VERSION);
    output[10] = u8::from(value.visible);
    output[11] = u8::from(value.pointer_pressed);
    write_u16(output, 12, value.cursor_x);
    write_u16(output, 14, value.cursor_y);
    write_u64(output, 16, value.revision);
    output[24] = value.hover_target;
    output[25] = value.pressed_target;
    output[26] = value.selected_duration_index;
    output[27] = value.status as u8;
    write_u16(output, 28, value.available_seconds);
    write_u16(output, 30, value.click_revision);
    output[32] = value.frame_rate;
    output[33] = value.quality;
    output[34] = value.output_format;
    output[35] = u8::from(value.save_enabled);
    output[36] = value.overlay_shortcut.length;
    output[37] = value.save_shortcut.length;
    output[40..80].copy_from_slice(&value.overlay_shortcut.bytes);
    output[80..120].copy_from_slice(&value.save_shortcut.bytes);
    write_u64(output, 120, value.revision);
    Ok(REPLAY_MENU_TELEMETRY_BYTES)
}

/// Decode one complete Replay menu snapshot, rejecting torn or non-canonical data.
///
/// # Errors
///
/// Returns an error when the buffer has the wrong size/version, contains a
/// torn revision pair, or carries a value outside the bounded protocol.
pub fn decode_replay_menu_telemetry(
    input: &[u8],
) -> Result<ReplayMenuTelemetry, OverlayTelemetryError> {
    if input.len() != REPLAY_MENU_TELEMETRY_BYTES || input[..8] != REPLAY_MENU_MAGIC {
        return Err(OverlayTelemetryError::new(
            "Replay menu telemetry is invalid",
        ));
    }
    if read_u16(input, 8) != REPLAY_MENU_VERSION {
        return Err(OverlayTelemetryError::new(
            "Replay menu version is unsupported",
        ));
    }
    let status = match input[27] {
        0 => ReplayMenuStatus::Unavailable,
        1 => ReplayMenuStatus::Inactive,
        2 => ReplayMenuStatus::Buffering,
        3 => ReplayMenuStatus::Saving,
        4 => ReplayMenuStatus::Failed,
        _ => return Err(OverlayTelemetryError::new("Replay menu status is invalid")),
    };
    let value = ReplayMenuTelemetry {
        revision: read_u64(input, 16),
        visible: input[10] == 1,
        pointer_pressed: input[11] == 1,
        cursor_x: read_u16(input, 12),
        cursor_y: read_u16(input, 14),
        hover_target: input[24],
        pressed_target: input[25],
        selected_duration_index: input[26],
        status,
        available_seconds: read_u16(input, 28),
        click_revision: read_u16(input, 30),
        frame_rate: input[32],
        quality: input[33],
        output_format: input[34],
        save_enabled: input[35] == 1,
        overlay_shortcut: decode_shortcut_label(input[36], &input[40..80])?,
        save_shortcut: decode_shortcut_label(input[37], &input[80..120])?,
    };
    if input[10] > 1 || input[11] > 1 || input[35] > 1 || input[38..40] != [0; 2] {
        return Err(OverlayTelemetryError::new(
            "Replay menu encoding is not canonical",
        ));
    }
    if read_u64(input, 120) != value.revision {
        return Err(OverlayTelemetryError::new(
            "Replay menu update is incomplete",
        ));
    }
    validate_replay_menu(value)?;
    Ok(value)
}

fn validate_replay_menu(value: ReplayMenuTelemetry) -> Result<(), OverlayTelemetryError> {
    if value.revision == 0
        || !value.revision.is_multiple_of(2)
        || value.cursor_x > 10_000
        || value.cursor_y > 10_000
        || value.hover_target > 11
        || value.pressed_target > 11
        || value.selected_duration_index > 7
        || value.available_seconds > 900
        || !matches!(value.frame_rate, 30 | 60 | 120)
        || value.quality > 2
        || value.output_format > 1
        || (!value.pointer_pressed && value.pressed_target != 0)
    {
        return Err(OverlayTelemetryError::new(
            "Replay menu configuration is invalid",
        ));
    }
    Ok(())
}

fn decode_shortcut_label(
    length: u8,
    bytes: &[u8],
) -> Result<ReplayShortcutLabel, OverlayTelemetryError> {
    let length = usize::from(length);
    if length > REPLAY_SHORTCUT_LABEL_BYTES
        || bytes.len() != REPLAY_SHORTCUT_LABEL_BYTES
        || bytes[..length]
            .iter()
            .any(|byte| !byte.is_ascii_graphic() && *byte != b' ')
        || bytes[length..].iter().any(|byte| *byte != 0)
    {
        return Err(OverlayTelemetryError::new(
            "Replay shortcut label is invalid",
        ));
    }
    let mut label = ReplayShortcutLabel::EMPTY;
    label.bytes.copy_from_slice(bytes);
    label.length = u8::try_from(length).unwrap_or(0);
    Ok(label)
}

/// One bounded daemon-to-layer hardware update.
///
/// Values use tenths instead of floats so the private file format is stable
/// across processes. A revision must be positive and even. Writers first
/// publish the corresponding odd revision, then this complete even snapshot;
/// readers reject mismatched or odd revisions rather than accepting a torn
/// update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayHardwareTelemetry {
    pub revision: u64,
    pub corner: u8,
    pub preset: u8,
    pub layout: u8,
    pub palette: u8,
    pub metrics: u16,
    pub opacity_percent: u8,
    pub scale_percent: u8,
    /// Monotonic daemon event edge for a durably committed Replay clip.
    /// Zero means no clip has completed in this capture session.
    pub replay_saved_revision: u16,
    /// None preserves launch visibility; Some is a session-only override.
    pub metrics_visible: Option<bool>,
    pub branding_visible: bool,
    pub cpu_utilization_tenths: Option<u16>,
    pub cpu_temperature_tenths_celsius: Option<u16>,
    pub gpu_utilization_tenths: Option<u16>,
    pub gpu_temperature_tenths_celsius: Option<u16>,
}

/// Encode a fixed-size hardware snapshot without allocation.
///
/// # Errors
///
/// Returns [`OverlayTelemetryError`] for an odd/zero revision, an impossible
/// metric, or insufficient output storage.
pub fn encode_overlay_hardware_telemetry(
    telemetry: OverlayHardwareTelemetry,
    output: &mut [u8],
) -> Result<usize, OverlayTelemetryError> {
    validate(&telemetry)?;
    if output.len() < OVERLAY_HARDWARE_TELEMETRY_BYTES {
        return Err(OverlayTelemetryError::new(
            "overlay telemetry output buffer is too small",
        ));
    }

    let output = &mut output[..OVERLAY_HARDWARE_TELEMETRY_BYTES];
    output.fill(0);
    output[..8].copy_from_slice(&MAGIC);
    write_u16(output, 8, VERSION);
    let mut flags = 0_u16;
    write_optional(
        output,
        24,
        telemetry.cpu_utilization_tenths,
        CPU_UTILIZATION_PRESENT,
        &mut flags,
    );
    write_optional(
        output,
        26,
        telemetry.cpu_temperature_tenths_celsius,
        CPU_TEMPERATURE_PRESENT,
        &mut flags,
    );
    write_optional(
        output,
        28,
        telemetry.gpu_utilization_tenths,
        GPU_UTILIZATION_PRESENT,
        &mut flags,
    );
    write_optional(
        output,
        30,
        telemetry.gpu_temperature_tenths_celsius,
        GPU_TEMPERATURE_PRESENT,
        &mut flags,
    );
    write_u16(output, 10, flags);
    output[12] = telemetry.corner;
    output[13] = telemetry.preset;
    write_u16(output, 14, telemetry.metrics);
    output[32] = telemetry.opacity_percent;
    output[33] = telemetry.scale_percent;
    write_u16(output, 34, telemetry.replay_saved_revision);
    output[36] = match telemetry.metrics_visible {
        None => 0,
        Some(true) => 1,
        Some(false) => 2,
    };
    output[37] = telemetry.layout;
    output[38] = telemetry.palette;
    output[39] = u8::from(telemetry.branding_visible);
    write_u64(output, 16, telemetry.revision);
    write_u64(output, 40, telemetry.revision);
    Ok(OVERLAY_HARDWARE_TELEMETRY_BYTES)
}

/// Decode one exact fixed-size snapshot and reject torn or non-canonical data.
///
/// # Errors
///
/// Returns [`OverlayTelemetryError`] for a wrong size, magic/version mismatch,
/// non-zero reserved bytes, unknown flags, a torn revision, or invalid values.
pub fn decode_overlay_hardware_telemetry(
    input: &[u8],
) -> Result<OverlayHardwareTelemetry, OverlayTelemetryError> {
    if input.len() != OVERLAY_HARDWARE_TELEMETRY_BYTES {
        return Err(OverlayTelemetryError::new(
            "overlay telemetry has invalid size",
        ));
    }
    if input[..8] != MAGIC {
        return Err(OverlayTelemetryError::new(
            "overlay telemetry has invalid magic",
        ));
    }
    if read_u16(input, 8) != VERSION {
        return Err(OverlayTelemetryError::new(
            "overlay telemetry version is unsupported",
        ));
    }
    let flags = read_u16(input, 10);
    if flags & !KNOWN_FLAGS != 0 {
        return Err(OverlayTelemetryError::new(
            "overlay telemetry flags are unsupported",
        ));
    }
    let revision = read_u64(input, 16);
    if revision == 0 || !revision.is_multiple_of(2) || revision != read_u64(input, 40) {
        return Err(OverlayTelemetryError::new(
            "overlay telemetry update is incomplete",
        ));
    }
    let telemetry = OverlayHardwareTelemetry {
        revision,
        corner: input[12],
        preset: input[13],
        layout: input[37],
        palette: input[38],
        metrics: read_u16(input, 14),
        opacity_percent: input[32],
        scale_percent: input[33],
        replay_saved_revision: read_u16(input, 34),
        metrics_visible: match input[36] {
            0 => None,
            1 => Some(true),
            2 => Some(false),
            _ => return Err(OverlayTelemetryError::new("invalid overlay visibility")),
        },
        branding_visible: match input[39] {
            0 => false,
            1 => true,
            _ => return Err(OverlayTelemetryError::new("invalid branding visibility")),
        },
        cpu_utilization_tenths: read_optional(input, 24, flags, CPU_UTILIZATION_PRESENT),
        cpu_temperature_tenths_celsius: read_optional(input, 26, flags, CPU_TEMPERATURE_PRESENT),
        gpu_utilization_tenths: read_optional(input, 28, flags, GPU_UTILIZATION_PRESENT),
        gpu_temperature_tenths_celsius: read_optional(input, 30, flags, GPU_TEMPERATURE_PRESENT),
    };
    validate(&telemetry)?;
    Ok(telemetry)
}

fn validate(telemetry: &OverlayHardwareTelemetry) -> Result<(), OverlayTelemetryError> {
    if telemetry.revision == 0 || !telemetry.revision.is_multiple_of(2) {
        return Err(OverlayTelemetryError::new(
            "overlay telemetry revision must be positive and even",
        ));
    }
    if telemetry.corner > 3
        || telemetry.preset > 3
        || telemetry.layout > 2
        || telemetry.palette > 7
        || telemetry.metrics == 0
        || telemetry.opacity_percent > 100
        || !(50..=200).contains(&telemetry.scale_percent)
        || !telemetry.scale_percent.is_multiple_of(5)
    {
        return Err(OverlayTelemetryError::new(
            "overlay configuration is invalid",
        ));
    }
    for utilization in [
        telemetry.cpu_utilization_tenths,
        telemetry.gpu_utilization_tenths,
    ]
    .into_iter()
    .flatten()
    {
        if utilization > MAX_UTILIZATION_TENTHS {
            return Err(OverlayTelemetryError::new(
                "overlay utilization is outside 0 to 100 percent",
            ));
        }
    }
    for temperature in [
        telemetry.cpu_temperature_tenths_celsius,
        telemetry.gpu_temperature_tenths_celsius,
    ]
    .into_iter()
    .flatten()
    {
        if temperature > MAX_TEMPERATURE_TENTHS_CELSIUS {
            return Err(OverlayTelemetryError::new(
                "overlay temperature is outside the supported range",
            ));
        }
    }
    Ok(())
}

fn write_optional(
    output: &mut [u8],
    offset: usize,
    value: Option<u16>,
    flag: u16,
    flags: &mut u16,
) {
    if let Some(value) = value {
        *flags |= flag;
        write_u16(output, offset, value);
    }
}

fn read_optional(input: &[u8], offset: usize, flags: u16, flag: u16) -> Option<u16> {
    (flags & flag != 0).then(|| read_u16(input, offset))
}

fn write_u16(output: &mut [u8], offset: usize, value: u16) {
    output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(output: &mut [u8], offset: usize, value: u64) {
    output[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn read_u16(input: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(input[offset..offset + 2].try_into().expect("validated u16"))
}

fn read_u64(input: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(input[offset..offset + 8].try_into().expect("validated u64"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverlayTelemetryError {
    message: &'static str,
}

impl OverlayTelemetryError {
    const fn new(message: &'static str) -> Self {
        Self { message }
    }
}

impl fmt::Display for OverlayTelemetryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl Error for OverlayTelemetryError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn telemetry() -> OverlayHardwareTelemetry {
        OverlayHardwareTelemetry {
            revision: 2,
            corner: 0,
            preset: 0,
            layout: 0,
            palette: 0,
            metrics: 1,
            opacity_percent: 50,
            scale_percent: 100,
            replay_saved_revision: 7,
            metrics_visible: None,
            branding_visible: true,
            cpu_utilization_tenths: Some(487),
            cpu_temperature_tenths_celsius: Some(624),
            gpu_utilization_tenths: Some(991),
            gpu_temperature_tenths_celsius: None,
        }
    }

    #[test]
    fn session_visibility_round_trips_and_rejects_invalid_values() {
        let mut bytes = [0; OVERLAY_HARDWARE_TELEMETRY_BYTES];
        for visible in [None, Some(false), Some(true)] {
            let mut value = telemetry();
            value.metrics_visible = visible;
            encode_overlay_hardware_telemetry(value, &mut bytes).unwrap();
            assert_eq!(decode_overlay_hardware_telemetry(&bytes).unwrap(), value);
        }
        bytes[36] = 3;
        assert!(decode_overlay_hardware_telemetry(&bytes).is_err());
    }

    #[test]
    fn fixed_snapshot_round_trips_without_allocation() {
        let mut bytes = [0; OVERLAY_HARDWARE_TELEMETRY_BYTES];
        assert_eq!(
            encode_overlay_hardware_telemetry(telemetry(), &mut bytes),
            Ok(OVERLAY_HARDWARE_TELEMETRY_BYTES)
        );
        assert_eq!(decode_overlay_hardware_telemetry(&bytes), Ok(telemetry()));
    }

    #[test]
    fn missing_values_remain_distinct_from_zero() {
        let telemetry = OverlayHardwareTelemetry {
            revision: 4,
            corner: 1,
            preset: 2,
            layout: 2,
            palette: 7,
            metrics: 3,
            opacity_percent: 75,
            scale_percent: 125,
            replay_saved_revision: 0,
            metrics_visible: None,
            branding_visible: false,
            cpu_utilization_tenths: Some(0),
            cpu_temperature_tenths_celsius: None,
            gpu_utilization_tenths: None,
            gpu_temperature_tenths_celsius: Some(0),
        };
        let mut bytes = [0; OVERLAY_HARDWARE_TELEMETRY_BYTES];
        encode_overlay_hardware_telemetry(telemetry, &mut bytes).expect("encode telemetry");
        assert_eq!(decode_overlay_hardware_telemetry(&bytes), Ok(telemetry));
    }

    #[test]
    fn torn_unknown_and_impossible_snapshots_are_rejected() {
        let mut bytes = [0; OVERLAY_HARDWARE_TELEMETRY_BYTES];
        encode_overlay_hardware_telemetry(telemetry(), &mut bytes).expect("encode telemetry");

        let mut torn = bytes;
        write_u64(&mut torn, 40, 4);
        assert!(decode_overlay_hardware_telemetry(&torn).is_err());

        let mut odd = bytes;
        write_u64(&mut odd, 16, 3);
        write_u64(&mut odd, 40, 3);
        assert!(decode_overlay_hardware_telemetry(&odd).is_err());

        let mut unknown_flags = bytes;
        write_u16(&mut unknown_flags, 10, KNOWN_FLAGS | (1 << 15));
        assert!(decode_overlay_hardware_telemetry(&unknown_flags).is_err());

        let mut impossible = bytes;
        write_u16(&mut impossible, 24, MAX_UTILIZATION_TENTHS + 1);
        assert!(decode_overlay_hardware_telemetry(&impossible).is_err());
    }

    #[test]
    fn encoding_rejects_invalid_revision_values_and_small_buffers() {
        let mut invalid = telemetry();
        invalid.revision = 1;
        assert!(
            encode_overlay_hardware_telemetry(invalid, &mut [0; OVERLAY_HARDWARE_TELEMETRY_BYTES])
                .is_err()
        );
        assert!(encode_overlay_hardware_telemetry(telemetry(), &mut [0; 8]).is_err());
    }

    #[test]
    fn replay_menu_snapshot_round_trips_and_rejects_tearing() {
        let value = ReplayMenuTelemetry {
            revision: 8,
            visible: true,
            pointer_pressed: true,
            cursor_x: 5_000,
            cursor_y: 7_500,
            hover_target: 7,
            pressed_target: 7,
            selected_duration_index: 1,
            status: ReplayMenuStatus::Buffering,
            available_seconds: 28,
            click_revision: 3,
            frame_rate: 120,
            quality: 2,
            output_format: 1,
            save_enabled: true,
            overlay_shortcut: ReplayShortcutLabel::from_shortcut("Ctrl+Shift+R"),
            save_shortcut: ReplayShortcutLabel::from_shortcut("Ctrl+F9"),
        };
        let mut bytes = [0; REPLAY_MENU_TELEMETRY_BYTES];
        assert_eq!(
            encode_replay_menu_telemetry(value, &mut bytes),
            Ok(bytes.len())
        );
        assert_eq!(decode_replay_menu_telemetry(&bytes), Ok(value));
        write_u64(&mut bytes, 120, 10);
        assert!(decode_replay_menu_telemetry(&bytes).is_err());
        let mut invalid_bool = [0; REPLAY_MENU_TELEMETRY_BYTES];
        encode_replay_menu_telemetry(value, &mut invalid_bool).unwrap();
        invalid_bool[10] = 2;
        assert!(decode_replay_menu_telemetry(&invalid_bool).is_err());
    }
}
