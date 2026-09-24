//! OpenGL metrics overlay rendered into a bounded CPU RGBA surface and drawn
//! with one blended textured quad. The CPU surface shares Redunar's font and
//! panel geometry, while the GL pass preserves the game's mutable state.

mod texture_renderer;

use redunar_capture::{
    OVERLAY_HARDWARE_TELEMETRY_BYTES, OverlayFailureReason, REPLAY_MENU_TELEMETRY_BYTES,
    ReplayMenuStatus, ReplayMenuTelemetry, decode_overlay_hardware_telemetry,
    decode_replay_menu_telemetry,
};
use redunar_core::{OverlayMetricSet, overlay_font};
use std::env;
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

const OVERLAY_VISIBLE_ENV: &str = "REDUNAR_OVERLAY_VISIBLE";
const OVERLAY_TELEMETRY_ENV: &str = "REDUNAR_OVERLAY_TELEMETRY";
const OVERLAY_CORNER_ENV: &str = "REDUNAR_OVERLAY_CORNER";
const OVERLAY_PALETTE_ENV: &str = "REDUNAR_OVERLAY_PALETTE";
const OVERLAY_PRESET_ENV: &str = "REDUNAR_OVERLAY_PRESET";
const OVERLAY_LAYOUT_ENV: &str = "REDUNAR_OVERLAY_LAYOUT";
const OVERLAY_BRANDING_ENV: &str = "REDUNAR_OVERLAY_BRANDING";
const OVERLAY_METRICS_ENV: &str = "REDUNAR_OVERLAY_METRICS";
const OVERLAY_OPACITY_ENV: &str = "REDUNAR_OVERLAY_OPACITY_PERCENT";

const PANEL_MARGIN: i32 = 12;
const TELEMETRY_INTERVAL_NS: u64 = 50_000_000;
const MENU_TELEMETRY_INTERVAL_NS: u64 = 8_000_000;
const REPLAY_SAVED_NOTICE_NS: u64 = 3_000_000_000;
const FRAME_HISTORY_CAPACITY: usize = 240;
const GRID_WIDTH: i32 = 294;
const GRID_HEADER_HEIGHT: i32 = 25;
const GRID_ROW_HEIGHT: i32 = 32;
const TELEMETRY_WIDTH: i32 = 300;
const TELEMETRY_HEADER_HEIGHT: i32 = 25;
const TELEMETRY_ROW_HEIGHT: i32 = 24;
const RIBBON_METRIC_WIDTH: i32 = 89;
const RIBBON_BRAND_WIDTH: i32 = 94;
const RIBBON_HEIGHT: i32 = 44;
const REPLAY_MENU_WIDTH: i32 = 520;
const REPLAY_MENU_HEIGHT: i32 = 286;
const REPLAY_MENU_SCALE_PERCENT: u8 = 160;
const REPLAY_CURSOR_WIDTH: i32 = overlay_font::GLYPH_WIDTH.cast_signed();
const REPLAY_CURSOR_HEIGHT: i32 = overlay_font::GLYPH_HEIGHT.cast_signed();
const SAVED_NOTICE_WIDTH: i32 = 340;
const SAVED_NOTICE_HEIGHT: i32 = 88;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ApiFlavor {
    Desktop,
    Embedded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Preset {
    Compact,
    Detailed,
    FpsOnly,
    Custom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Layout {
    Grid,
    Ribbon,
    Telemetry,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Palette {
    panel: [f32; 4],
    accent: [f32; 4],
    muted: [f32; 4],
    text: [f32; 4],
    divider: [f32; 4],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct MetricSnapshot {
    fps: Option<u16>,
    frame_time_tenths_ms: Option<u16>,
    one_percent_low_fps: Option<u16>,
    point_one_percent_low_fps: Option<u16>,
    cpu_percent: Option<u8>,
    cpu_temperature_c: Option<i16>,
    gpu_percent: Option<u8>,
    gpu_temperature_c: Option<i16>,
}

struct FrameHistory {
    intervals_ns: [u64; FRAME_HISTORY_CAPACITY],
    length: usize,
    next: usize,
}

impl Default for FrameHistory {
    fn default() -> Self {
        Self {
            intervals_ns: [0; FRAME_HISTORY_CAPACITY],
            length: 0,
            next: 0,
        }
    }
}

impl FrameHistory {
    fn push(&mut self, interval_ns: u64) {
        self.intervals_ns[self.next] = interval_ns;
        self.next = (self.next + 1) % FRAME_HISTORY_CAPACITY;
        self.length = self.length.saturating_add(1).min(FRAME_HISTORY_CAPACITY);
    }

    fn summarize(&self) -> MetricSnapshot {
        if self.length == 0 {
            return MetricSnapshot::default();
        }
        let intervals = &self.intervals_ns[..self.length];
        let mut sorted = [0_u64; FRAME_HISTORY_CAPACITY];
        sorted[..self.length].copy_from_slice(intervals);
        sorted[..self.length].sort_unstable_by(|left, right| right.cmp(left));
        let sum = intervals
            .iter()
            .fold(0_u128, |total, value| total + u128::from(*value));
        let average = 1_000_000_000_u128.saturating_mul(self.length as u128) / sum.max(1);
        let newest_index = if self.next == 0 {
            self.length - 1
        } else {
            self.next - 1
        };
        let one_count = self.length.div_ceil(100).max(1);
        let point_one_count = self.length.div_ceil(1_000).max(1);
        MetricSnapshot {
            fps: Some(clamp_metric(average)),
            frame_time_tenths_ms: Some(clamp_metric(u128::from(
                self.intervals_ns[newest_index] / 100_000,
            ))),
            one_percent_low_fps: Some(fps_for_slice(&sorted[..one_count])),
            point_one_percent_low_fps: Some(fps_for_slice(&sorted[..point_one_count])),
            ..MetricSnapshot::default()
        }
    }
}

struct OverlayState {
    file: Option<File>,
    last_hardware_read_ns: u64,
    last_menu_read_ns: u64,
    last_revision: u64,
    last_menu_revision: u64,
    visible: bool,
    corner: Corner,
    palette: Palette,
    preset: Preset,
    layout: Layout,
    metrics: u16,
    scale_percent: u8,
    opacity_percent: u8,
    branding_visible: bool,
    hardware: MetricSnapshot,
    menu: Option<ReplayMenuTelemetry>,
    replay_saved_revision: u16,
    replay_saved_until_ns: u64,
    history: FrameHistory,
}

impl OverlayState {
    fn from_environment() -> Self {
        let file = env::var_os(OVERLAY_TELEMETRY_ENV)
            .filter(|value| Path::new(value).is_absolute())
            .and_then(|value| File::open(value).ok());
        Self {
            file,
            last_hardware_read_ns: 0,
            last_menu_read_ns: 0,
            last_revision: 0,
            last_menu_revision: 0,
            visible: visible_from_environment(),
            corner: corner_from_environment(),
            palette: palette_from_environment(),
            preset: preset_from_environment(),
            layout: layout_from_environment(),
            metrics: metrics_from_environment(),
            scale_percent: 100,
            opacity_percent: opacity_from_environment(),
            branding_visible: branding_from_environment(),
            hardware: MetricSnapshot::default(),
            menu: None,
            replay_saved_revision: 0,
            replay_saved_until_ns: 0,
            history: FrameHistory::default(),
        }
    }

    fn refresh(&mut self, now_ns: u64) {
        let Some(file) = self.file.as_ref() else {
            return;
        };
        if self.last_hardware_read_ns == 0
            || now_ns.saturating_sub(self.last_hardware_read_ns) >= TELEMETRY_INTERVAL_NS
        {
            self.last_hardware_read_ns = now_ns;
            let mut hardware_bytes = [0_u8; OVERLAY_HARDWARE_TELEMETRY_BYTES];
            if file.read_exact_at(&mut hardware_bytes, 0).is_ok()
                && let Ok(telemetry) = decode_overlay_hardware_telemetry(&hardware_bytes)
                && telemetry.revision != self.last_revision
            {
                self.last_revision = telemetry.revision;
                if let Some(visible) = telemetry.metrics_visible {
                    self.visible = visible;
                }
                self.corner = match telemetry.corner {
                    1 => Corner::TopRight,
                    2 => Corner::BottomLeft,
                    3 => Corner::BottomRight,
                    _ => Corner::TopLeft,
                };
                self.palette = palette_from_index(telemetry.palette);
                self.preset = preset_from_index(telemetry.preset);
                self.layout = layout_from_index(telemetry.layout);
                self.metrics = telemetry.metrics;
                self.scale_percent = telemetry.scale_percent;
                self.opacity_percent = telemetry.opacity_percent;
                self.branding_visible = telemetry.branding_visible;
                self.hardware.cpu_percent = telemetry.cpu_utilization_tenths.map(round_percent);
                self.hardware.cpu_temperature_c =
                    telemetry.cpu_temperature_tenths_celsius.map(round_temp);
                self.hardware.gpu_percent = telemetry.gpu_utilization_tenths.map(round_percent);
                self.hardware.gpu_temperature_c =
                    telemetry.gpu_temperature_tenths_celsius.map(round_temp);
                if telemetry.replay_saved_revision != self.replay_saved_revision {
                    self.replay_saved_revision = telemetry.replay_saved_revision;
                    if telemetry.replay_saved_revision != 0 {
                        self.replay_saved_until_ns = now_ns.saturating_add(REPLAY_SAVED_NOTICE_NS);
                    }
                }
            }
        }
        let menu_interval = if self.menu.is_some_and(|menu| menu.visible) {
            MENU_TELEMETRY_INTERVAL_NS
        } else {
            TELEMETRY_INTERVAL_NS
        };
        if self.last_menu_read_ns == 0
            || now_ns.saturating_sub(self.last_menu_read_ns) >= menu_interval
        {
            self.last_menu_read_ns = now_ns;
            let mut menu_bytes = [0_u8; REPLAY_MENU_TELEMETRY_BYTES];
            if file
                .read_exact_at(&mut menu_bytes, OVERLAY_HARDWARE_TELEMETRY_BYTES as u64)
                .is_ok()
                && let Ok(menu) = decode_replay_menu_telemetry(&menu_bytes)
                && menu.revision != self.last_menu_revision
            {
                self.last_menu_revision = menu.revision;
                self.menu = Some(menu);
            }
        }
    }

    fn snapshot(&self) -> MetricSnapshot {
        let frame = self.history.summarize();
        MetricSnapshot {
            cpu_percent: self.hardware.cpu_percent,
            cpu_temperature_c: self.hardware.cpu_temperature_c,
            gpu_percent: self.hardware.gpu_percent,
            gpu_temperature_c: self.hardware.gpu_temperature_c,
            ..frame
        }
    }
}

static ACTIVE_REPORTED: AtomicBool = AtomicBool::new(false);
static ERROR_REPORTED: AtomicBool = AtomicBool::new(false);
static OVERLAY: LazyLock<Mutex<OverlayState>> =
    LazyLock::new(|| Mutex::new(OverlayState::from_environment()));
static RENDER_CACHE: LazyLock<Mutex<RenderCache>> =
    LazyLock::new(|| Mutex::new(RenderCache::default()));
static MENU_CACHE: LazyLock<Mutex<RenderCache>> =
    LazyLock::new(|| Mutex::new(RenderCache::default()));
static CURSOR_CACHE: LazyLock<Mutex<RenderCache>> =
    LazyLock::new(|| Mutex::new(RenderCache::default()));
static NOTICE_CACHE: LazyLock<Mutex<RenderCache>> =
    LazyLock::new(|| Mutex::new(RenderCache::default()));

pub(crate) fn visible_from_environment() -> bool {
    env::var(OVERLAY_VISIBLE_ENV).ok().as_deref() == Some("1")
}

pub(crate) fn renderer_requested_from_environment() -> bool {
    visible_from_environment() || env::var_os(OVERLAY_TELEMETRY_ENV).is_some()
}

pub(crate) fn record_interval(interval_ns: u64) {
    if let Ok(mut state) = OVERLAY.try_lock() {
        state.history.push(interval_ns);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RenderConfig {
    corner: Corner,
    palette: Palette,
    preset: Preset,
    layout: Layout,
    metrics: u16,
    scale_percent: u8,
    opacity_percent: u8,
    branding_visible: bool,
}

#[derive(Default)]
struct RenderCache {
    updated_ns: u64,
    revision: u64,
    width: i32,
    height: i32,
    metrics: u16,
    source_revision: u64,
    config: Option<RenderConfig>,
    config_menu: Option<ReplayMenuTelemetry>,
    pixels: Vec<u8>,
}

struct Canvas {
    width: i32,
    height: i32,
    pixels: Vec<u8>,
}

impl Canvas {
    fn new(width: i32, height: i32) -> Self {
        let length = usize::try_from(width.max(0))
            .unwrap_or(0)
            .saturating_mul(usize::try_from(height.max(0)).unwrap_or(0))
            .saturating_mul(4);
        Self {
            width,
            height,
            pixels: vec![0; length],
        }
    }

    fn fill_rect(&mut self, x: i32, y: i32, width: i32, height: i32, color: [f32; 4]) {
        for row in y.max(0)..y.saturating_add(height).min(self.height) {
            for column in x.max(0)..x.saturating_add(width).min(self.width) {
                self.blend_pixel(column, row, color);
            }
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "rounded panel geometry and colors are explicit"
    )]
    fn fill_rounded_rect(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        radius: i32,
        color: [f32; 4],
        border: [f32; 4],
    ) {
        let radius = radius.min(width / 2).min(height / 2).max(1);
        for row in 0..height {
            for column in 0..width {
                let dx = (radius - column - 1).max(column - (width - radius)).max(0);
                let dy = (radius - row - 1).max(row - (height - radius)).max(0);
                let distance_squared = dx * dx + dy * dy;
                if distance_squared > radius * radius {
                    continue;
                }
                let edge = distance_squared > (radius - 1) * (radius - 1)
                    || row == 0
                    || column == 0
                    || row == height - 1
                    || column == width - 1;
                self.blend_pixel(x + column, y + row, if edge { border } else { color });
            }
        }
    }

    fn blend_pixel(&mut self, x: i32, y: i32, color: [f32; 4]) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let index = (usize::try_from(y).unwrap_or(0) * usize::try_from(self.width).unwrap_or(0)
            + usize::try_from(x).unwrap_or(0))
            * 4;
        let source_alpha = color[3].clamp(0.0, 1.0);
        let destination_alpha = f32::from(self.pixels[index + 3]) / 255.0;
        let output_alpha = source_alpha + destination_alpha * (1.0 - source_alpha);
        if output_alpha <= f32::EPSILON {
            return;
        }
        for (channel, source) in color.iter().copied().enumerate().take(3) {
            let source = source.clamp(0.0, 1.0);
            let destination = f32::from(self.pixels[index + channel]) / 255.0;
            let output = source * source_alpha + destination * (1.0 - source_alpha);
            self.pixels[index + channel] = normalized_byte(output);
        }
        self.pixels[index + 3] = normalized_byte(output_alpha);
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the normalized color is clamped before conversion to one byte"
)]
fn normalized_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub(crate) fn render(context: usize, api: ApiFlavor) {
    if !renderer_requested_from_environment() {
        return;
    }
    let Ok(mut state) = OVERLAY.try_lock() else {
        return;
    };
    let now_ns = super::monotonic_ns().unwrap_or(0);
    state.refresh(now_ns);
    let metrics_visible = state.visible;
    let snapshot = state.snapshot();
    let config = RenderConfig {
        corner: state.corner,
        palette: state.palette,
        preset: state.preset,
        layout: state.layout,
        metrics: state.metrics,
        scale_percent: state.scale_percent,
        opacity_percent: state.opacity_percent,
        branding_visible: state.branding_visible,
    };
    let menu = state.menu.filter(|menu| menu.visible);
    let saved_notice =
        (state.replay_saved_until_ns > now_ns).then_some(u64::from(state.replay_saved_revision));
    drop(state);
    let Some(viewport) = texture_renderer::viewport() else {
        report_renderer_error();
        return;
    };
    if viewport[2] <= 0 || viewport[3] <= 0 {
        return;
    }
    let mut requested = false;
    let mut rendered = true;
    if metrics_visible {
        requested = true;
        rendered &= render_metrics_surface(context, api, viewport, now_ns, snapshot, config);
    }
    if let Some(menu) = menu {
        requested = true;
        rendered &= render_menu_surface(context, api, viewport, menu);
    }
    if let Some(revision) = saved_notice {
        requested = true;
        rendered &= render_saved_notice_surface(context, api, viewport, revision);
    }
    if requested && rendered {
        if !ACTIVE_REPORTED.swap(true, Ordering::Relaxed) {
            super::record_overlay_active();
        }
    } else if requested {
        report_renderer_error();
    }
}

/// Forget renderer bookkeeping immediately before a GL context is destroyed.
/// The graphics driver owns deletion of the context-local object names during
/// context teardown; removing the entry here returns Redunar's bounded context
/// budget without issuing GL calls against a context that may not be current.
pub(crate) fn destroy_context(context: usize) {
    texture_renderer::destroy_context(context);
}

fn render_metrics_surface(
    context: usize,
    api: ApiFlavor,
    viewport: [i32; 4],
    now_ns: u64,
    snapshot: MetricSnapshot,
    config: RenderConfig,
) -> bool {
    let metrics = selected_metrics(snapshot, config);
    let (logical_width, logical_height) = layout_extent(config, metrics);
    let scale_percent = effective_scale(
        viewport,
        logical_width,
        logical_height,
        config.scale_percent,
    );
    if scale_percent == 0 {
        return true;
    }
    let width = scale_value(logical_width, scale_percent).max(1);
    let height = scale_value(logical_height, scale_percent).max(1);
    let origin = panel_origin(
        viewport,
        logical_width,
        logical_height,
        scale_percent,
        config.corner,
    );
    let Ok(mut cache) = RENDER_CACHE.try_lock() else {
        return true;
    };
    let cache_needs_refresh = cache.updated_ns == 0
        || now_ns.saturating_sub(cache.updated_ns) >= TELEMETRY_INTERVAL_NS
        || cache.width != width
        || cache.height != height
        || cache.metrics != metrics
        || cache.config != Some(config);
    if cache_needs_refresh {
        let mut canvas = Canvas::new(width, height);
        render_plan(
            &mut canvas,
            snapshot,
            config,
            metrics,
            (0, 0),
            scale_percent,
        );
        cache.updated_ns = now_ns;
        cache.revision = cache.revision.wrapping_add(1).max(1);
        cache.width = width;
        cache.height = height;
        cache.metrics = metrics;
        cache.config = Some(config);
        cache.pixels = canvas.pixels;
    }
    // SAFETY: the swap hook guarantees that `context` is current. The renderer
    // owns its context-local objects and restores the GL state it changes.
    unsafe {
        texture_renderer::draw(
            context,
            api,
            texture_renderer::SURFACE_METRICS,
            viewport,
            origin,
            width,
            height,
            cache.revision,
            &cache.pixels,
        )
    }
}

fn render_menu_surface(
    context: usize,
    api: ApiFlavor,
    viewport: [i32; 4],
    menu: ReplayMenuTelemetry,
) -> bool {
    let scale = effective_scale(
        viewport,
        REPLAY_MENU_WIDTH,
        REPLAY_MENU_HEIGHT,
        REPLAY_MENU_SCALE_PERCENT,
    );
    if scale == 0 {
        return true;
    }
    let width = scale_value(REPLAY_MENU_WIDTH, scale).max(1);
    let height = scale_value(REPLAY_MENU_HEIGHT, scale).max(1);
    let origin = (
        viewport[0] + (viewport[2] - width).max(0) / 2,
        viewport[1] + (viewport[3] - height).max(0) / 2,
    );
    let Ok(mut cache) = MENU_CACHE.try_lock() else {
        return true;
    };
    let menu_key = menu_without_pointer_motion(menu);
    if cache.config_menu != Some(menu_key) || cache.width != width || cache.height != height {
        let mut canvas = Canvas::new(width, height);
        render_replay_menu(&mut canvas, menu, scale);
        cache.revision = cache.revision.wrapping_add(1).max(1);
        cache.config_menu = Some(menu_key);
        cache.width = width;
        cache.height = height;
        cache.pixels = canvas.pixels;
    }
    let menu_rendered = unsafe {
        texture_renderer::draw(
            context,
            api,
            texture_renderer::SURFACE_MENU,
            viewport,
            origin,
            width,
            height,
            cache.revision,
            &cache.pixels,
        )
    };
    drop(cache);
    menu_rendered && render_menu_cursor(context, api, viewport, origin, scale, menu)
}

fn menu_without_pointer_motion(mut menu: ReplayMenuTelemetry) -> ReplayMenuTelemetry {
    menu.revision = 0;
    menu.pointer_pressed = false;
    menu.cursor_x = 0;
    menu.cursor_y = 0;
    menu.click_revision = 0;
    menu
}

fn render_menu_cursor(
    context: usize,
    api: ApiFlavor,
    viewport: [i32; 4],
    menu_origin: (i32, i32),
    scale: u32,
    menu: ReplayMenuTelemetry,
) -> bool {
    let width = scale_value(REPLAY_CURSOR_WIDTH, scale).max(1);
    let height = scale_value(REPLAY_CURSOR_HEIGHT, scale).max(1);
    let menu_width = scale_value(REPLAY_MENU_WIDTH, scale);
    let menu_height = scale_value(REPLAY_MENU_HEIGHT, scale);
    let logical_x = i32::from(menu.cursor_x.min(10_000)) * REPLAY_MENU_WIDTH / 10_000;
    let logical_y = i32::from(menu.cursor_y.min(10_000)) * REPLAY_MENU_HEIGHT / 10_000;
    let x = scale_value(logical_x, scale);
    let y_from_top = scale_value(logical_y, scale);
    let origin = (
        menu_origin.0.saturating_add(x),
        menu_origin
            .1
            .saturating_add(menu_height - y_from_top - height),
    );
    let Ok(mut cache) = CURSOR_CACHE.try_lock() else {
        return true;
    };
    let source_revision = 1;
    if cache.source_revision != source_revision || cache.width != width || cache.height != height {
        let mut canvas = Canvas::new(width, height);
        render_pointer(&mut canvas);
        cache.revision = cache.revision.wrapping_add(1).max(1);
        cache.source_revision = source_revision;
        cache.width = width;
        cache.height = height;
        cache.pixels = canvas.pixels;
    }
    unsafe {
        texture_renderer::draw_clipped(
            context,
            api,
            texture_renderer::SURFACE_CURSOR,
            viewport,
            origin,
            width,
            height,
            cache.revision,
            &cache.pixels,
            (menu_origin.0, menu_origin.1, menu_width, menu_height),
        )
    }
}

fn render_saved_notice_surface(
    context: usize,
    api: ApiFlavor,
    viewport: [i32; 4],
    source_revision: u64,
) -> bool {
    let scale = effective_scale(viewport, SAVED_NOTICE_WIDTH, SAVED_NOTICE_HEIGHT, 100);
    if scale == 0 {
        return true;
    }
    let width = scale_value(SAVED_NOTICE_WIDTH, scale).max(1);
    let height = scale_value(SAVED_NOTICE_HEIGHT, scale).max(1);
    let origin = (viewport[0] + PANEL_MARGIN, viewport[1] + PANEL_MARGIN);
    let Ok(mut cache) = NOTICE_CACHE.try_lock() else {
        return true;
    };
    if cache.source_revision != source_revision || cache.width != width || cache.height != height {
        let mut canvas = Canvas::new(width, height);
        render_saved_notice(&mut canvas, scale);
        cache.revision = cache.revision.wrapping_add(1).max(1);
        cache.source_revision = source_revision;
        cache.width = width;
        cache.height = height;
        cache.pixels = canvas.pixels;
    }
    unsafe {
        texture_renderer::draw(
            context,
            api,
            texture_renderer::SURFACE_NOTICE,
            viewport,
            origin,
            width,
            height,
            cache.revision,
            &cache.pixels,
        )
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the fixed Replay menu geometry stays auditable"
)]
fn render_replay_menu(canvas: &mut Canvas, menu: ReplayMenuTelemetry, scale: u32) {
    let palette = palette_from_index(0);
    canvas.fill_rounded_rect(
        0,
        0,
        canvas.width,
        canvas.height,
        scale_value(12, scale).max(2),
        with_alpha(palette.panel, 0.96),
        palette.divider,
    );
    clear_scaled_rect(canvas, (0, 0), 0, 65, 520, 1, scale, palette.divider);
    for x in [173, 346] {
        clear_scaled_rect(canvas, (0, 0), x, 0, 1, 66, scale, palette.divider);
    }
    for (left, right, label) in [
        (0, 173, b"CAPTURE".as_slice()),
        (173, 346, b"QUALITY".as_slice()),
        (346, 520, b"BUFFER".as_slice()),
    ] {
        draw_text_centered(canvas, label, left, right, 13, 1, scale, palette.muted);
    }
    let capture = match menu.frame_rate {
        30 => b"30 FPS".as_slice(),
        60 => b"60 FPS".as_slice(),
        120 => b"120 FPS".as_slice(),
        _ => b"-- FPS".as_slice(),
    };
    let quality = match menu.quality {
        0 => b"EFFICIENT".as_slice(),
        1 => b"BALANCED".as_slice(),
        _ => b"HIGH".as_slice(),
    };
    let mut buffered = FixedText::<12>::default();
    match menu.status {
        ReplayMenuStatus::Unavailable => buffered.push_bytes(b"N/A"),
        ReplayMenuStatus::Inactive => buffered.push_bytes(b"OFF"),
        ReplayMenuStatus::Saving => buffered.push_bytes(b"SAVING"),
        ReplayMenuStatus::Failed => buffered.push_bytes(b"ERROR"),
        ReplayMenuStatus::Buffering => {
            buffered.push_number(Some(menu.available_seconds));
            buffered.push_bytes(b" SEC");
        }
    }
    draw_text_centered(canvas, capture, 0, 173, 32, 2, scale, palette.text);
    draw_text_centered(canvas, quality, 173, 346, 32, 2, scale, palette.text);
    draw_text_centered(
        canvas,
        buffered.as_bytes(),
        346,
        520,
        32,
        2,
        scale,
        palette.text,
    );
    draw_text_scaled(
        canvas,
        (0, 0),
        24,
        86,
        b"Instant Replay",
        2,
        scale,
        palette.text,
    );
    let overlay_shortcut = menu.overlay_shortcut.as_bytes();
    if !overlay_shortcut.is_empty() {
        draw_text_right(canvas, overlay_shortcut, 496, 91, 1, scale, palette.muted);
    }

    draw_outline(canvas, 24, 124, 472, 84, 6, scale, palette.divider);
    for column in 1..4 {
        clear_scaled_rect(
            canvas,
            (0, 0),
            24 + column * 118,
            130,
            1,
            72,
            scale,
            palette.divider,
        );
    }
    clear_scaled_rect(canvas, (0, 0), 30, 166, 460, 1, scale, palette.divider);
    let durations: [&[u8]; 8] = [
        b"15 SEC", b"30 SEC", b"1 MIN", b"2 MIN", b"3 MIN", b"5 MIN", b"10 MIN", b"15 MIN",
    ];
    for (index, label) in durations.into_iter().enumerate() {
        let left = 24 + i32::try_from(index % 4).unwrap_or(0) * 118;
        let top = 124 + i32::try_from(index / 4).unwrap_or(0) * 42;
        let selected = index == usize::from(menu.selected_duration_index.min(7));
        if selected {
            clear_scaled_rect(
                canvas,
                (0, 0),
                left + 2,
                top + 2,
                114,
                38,
                scale,
                with_alpha(palette.accent, 0.16),
            );
            clear_scaled_rect(
                canvas,
                (0, 0),
                left + 8,
                top + 39,
                102,
                2,
                scale,
                palette.accent,
            );
        } else if menu.hover_target == u8::try_from(index + 1).unwrap_or(0) {
            clear_scaled_rect(
                canvas,
                (0, 0),
                left,
                top + 40,
                118,
                2,
                scale,
                palette.accent,
            );
        }
        draw_text_centered(
            canvas,
            label,
            left,
            left + 118,
            top + 16,
            1,
            scale,
            if selected {
                palette.accent
            } else {
                palette.text
            },
        );
    }

    draw_outline(canvas, 24, 222, 180, 46, 6, scale, palette.divider);
    clear_scaled_rect(canvas, (0, 0), 114, 228, 1, 34, scale, palette.divider);
    let format_left = if menu.output_format == 0 { 24 } else { 114 };
    clear_scaled_rect(
        canvas,
        (0, 0),
        format_left + 2,
        224,
        86,
        42,
        scale,
        with_alpha(palette.accent, 0.16),
    );
    clear_scaled_rect(
        canvas,
        (0, 0),
        format_left + 8,
        265,
        74,
        2,
        scale,
        palette.accent,
    );
    draw_text_centered(canvas, b"MKV", 24, 114, 238, 1, scale, palette.text);
    draw_text_centered(canvas, b"MP4", 114, 204, 238, 1, scale, palette.text);

    let save_color = if menu.save_enabled {
        palette.accent
    } else {
        palette.divider
    };
    draw_outline(canvas, 220, 222, 276, 46, 6, scale, save_color);
    if menu.save_enabled {
        clear_scaled_rect(
            canvas,
            (0, 0),
            222,
            224,
            272,
            42,
            scale,
            with_alpha(palette.accent, 0.16),
        );
    }
    let save_labels: [&[u8]; 8] = [
        b"SAVE LAST 15 SEC",
        b"SAVE LAST 30 SEC",
        b"SAVE LAST 1 MIN",
        b"SAVE LAST 2 MIN",
        b"SAVE LAST 3 MIN",
        b"SAVE LAST 5 MIN",
        b"SAVE LAST 10 MIN",
        b"SAVE LAST 15 MIN",
    ];
    draw_text_centered(
        canvas,
        save_labels[usize::from(menu.selected_duration_index.min(7))],
        220,
        496,
        238,
        1,
        scale,
        if menu.save_enabled {
            palette.text
        } else {
            palette.muted
        },
    );
    let save_shortcut = menu.save_shortcut.as_bytes();
    if !save_shortcut.is_empty() {
        draw_text_right(canvas, save_shortcut, 488, 274, 1, scale, palette.muted);
    }
}

fn render_pointer(canvas: &mut Canvas) {
    for (raster, color) in [
        (
            overlay_font::cursor_outline_coverage_raster(),
            [0.015, 0.015, 0.018, 1.0],
        ),
        (
            overlay_font::cursor_coverage_raster(),
            [0.94, 0.95, 0.97, 1.0],
        ),
    ] {
        for row in 0..canvas.height {
            for column in 0..canvas.width {
                let coverage = resampled_coverage(raster, column, row, canvas.width, canvas.height);
                if coverage > 0.01 {
                    canvas.blend_pixel(
                        column,
                        row,
                        [color[0], color[1], color[2], color[3] * coverage],
                    );
                }
            }
        }
    }
}

fn render_saved_notice(canvas: &mut Canvas, scale: u32) {
    let palette = palette_from_index(0);
    canvas.fill_rounded_rect(
        0,
        0,
        canvas.width,
        canvas.height,
        scale_value(10, scale).max(2),
        with_alpha(palette.panel, 0.96),
        palette.divider,
    );
    clear_scaled_rect(canvas, (0, 0), 24, 37, 2, 15, scale, palette.accent);
    clear_scaled_rect(canvas, (0, 0), 24, 50, 14, 2, scale, palette.accent);
    for index in 0..13 {
        clear_scaled_rect(
            canvas,
            (0, 0),
            25 + index,
            49 - index,
            2,
            2,
            scale,
            palette.accent,
        );
    }
    draw_text_scaled(
        canvas,
        (0, 0),
        58,
        17,
        b"Moment saved.",
        2,
        scale,
        palette.text,
    );
    draw_text_scaled(
        canvas,
        (0, 0),
        58,
        54,
        b"Saved to your local library",
        1,
        scale,
        palette.muted,
    );
}

#[expect(
    clippy::too_many_arguments,
    reason = "centered text geometry is explicit"
)]
fn draw_text_centered(
    canvas: &mut Canvas,
    text: &[u8],
    left: i32,
    right: i32,
    y: i32,
    glyph_scale: i32,
    scale: u32,
    color: [f32; 4],
) {
    let width =
        i32::try_from(text.len()).unwrap_or(i32::MAX) * overlay_font::GLYPH_ADVANCE * glyph_scale;
    let x = left + (right - left - width).max(0) / 2;
    draw_text_scaled(canvas, (0, 0), x, y, text, glyph_scale, scale, color);
}

fn draw_text_right(
    canvas: &mut Canvas,
    text: &[u8],
    right: i32,
    y: i32,
    glyph_scale: i32,
    scale: u32,
    color: [f32; 4],
) {
    let width =
        i32::try_from(text.len()).unwrap_or(i32::MAX) * overlay_font::GLYPH_ADVANCE * glyph_scale;
    draw_text_scaled(
        canvas,
        (0, 0),
        right - width,
        y,
        text,
        glyph_scale,
        scale,
        color,
    );
}

#[expect(clippy::too_many_arguments, reason = "outline geometry is explicit")]
fn draw_outline(
    canvas: &mut Canvas,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    radius: i32,
    scale: u32,
    color: [f32; 4],
) {
    let x = scale_value(x, scale);
    let y = scale_value(y, scale);
    let width = scale_value(width, scale).max(1);
    let height = scale_value(height, scale).max(1);
    canvas.fill_rounded_rect(
        x,
        y,
        width,
        height,
        scale_value(radius, scale).max(2),
        [0.0; 4],
        color,
    );
}

fn report_renderer_error() {
    if !ERROR_REPORTED.swap(true, Ordering::Relaxed) {
        super::record_overlay_error(OverlayFailureReason::RequiredOpenGlFunctionsUnavailable);
    }
}

fn render_plan(
    canvas: &mut Canvas,
    snapshot: MetricSnapshot,
    config: RenderConfig,
    metrics: u16,
    origin: (i32, i32),
    scale_percent: u32,
) {
    if config.preset == Preset::FpsOnly {
        let mut label = FixedText::<12>::default();
        label.push_bytes(b"FPS ");
        label.push_number(snapshot.fps);
        // Vulkan's FPS-only preset intentionally has no background panel.
        {
            draw_text_scaled(
                canvas,
                origin,
                0,
                0,
                label.as_bytes(),
                2,
                scale_percent,
                config.palette.accent,
            );
        }
        return;
    }

    let (width, height) = layout_extent(config, metrics);
    {
        canvas.fill_rounded_rect(
            origin.0,
            origin.1,
            scale_value(width, scale_percent),
            scale_value(height, scale_percent),
            scale_value(4, scale_percent).max(2),
            with_alpha(
                config.palette.panel,
                f32::from(config.opacity_percent) / 100.0,
            ),
            with_alpha(
                config.palette.divider,
                f32::from(config.opacity_percent) / 100.0,
            ),
        );
        clear_scaled_rect(
            canvas,
            origin,
            0,
            0,
            3,
            height,
            scale_percent,
            config.palette.accent,
        );
        match config.layout {
            Layout::Grid => {
                render_grid(canvas, snapshot, config, metrics, origin, scale_percent);
            }
            Layout::Ribbon => {
                render_ribbon(canvas, snapshot, config, metrics, origin, scale_percent);
            }
            Layout::Telemetry => {
                render_telemetry(canvas, snapshot, config, metrics, origin, scale_percent);
            }
        }
    }
}

fn render_grid(
    canvas: &mut Canvas,
    snapshot: MetricSnapshot,
    config: RenderConfig,
    metrics: u16,
    origin: (i32, i32),
    scale: u32,
) {
    if config.branding_visible {
        {
            draw_text_scaled(
                canvas,
                origin,
                8,
                6,
                b"REDUNAR",
                1,
                scale,
                config.palette.accent,
            );
        }
    }
    {
        clear_scaled_rect(
            canvas,
            origin,
            3,
            24,
            GRID_WIDTH - 3,
            1,
            scale,
            config.palette.divider,
        );
    }
    let mut index = 0_usize;
    for (bit, label) in metric_labels() {
        if metrics & bit == 0 {
            continue;
        }
        let column = i32::try_from(index % 2).unwrap_or(0);
        let row = i32::try_from(index / 2).unwrap_or(0);
        let x = 10 + column * (GRID_WIDTH / 2);
        let y = GRID_HEADER_HEIGHT + row * GRID_ROW_HEIGHT + 4;
        let value = metric_value(snapshot, bit);
        {
            draw_text_scaled(canvas, origin, x, y, label, 1, scale, config.palette.muted);
            draw_text_scaled(
                canvas,
                origin,
                x,
                y + 15,
                value.as_bytes(),
                1,
                scale,
                config.palette.text,
            );
        }
        index += 1;
    }
}

fn render_ribbon(
    canvas: &mut Canvas,
    snapshot: MetricSnapshot,
    config: RenderConfig,
    metrics: u16,
    origin: (i32, i32),
    scale: u32,
) {
    let brand_width = if config.branding_visible {
        RIBBON_BRAND_WIDTH
    } else {
        0
    };
    if config.branding_visible {
        {
            draw_text_scaled(
                canvas,
                origin,
                10,
                15,
                b"REDUNAR",
                1,
                scale,
                config.palette.accent,
            );
        }
    }
    let mut index = 0_i32;
    for (bit, label) in metric_labels() {
        if metrics & bit == 0 {
            continue;
        }
        let x = brand_width + 8 + index * RIBBON_METRIC_WIDTH;
        let value = metric_value(snapshot, bit);
        {
            clear_scaled_rect(
                canvas,
                origin,
                x - 8,
                6,
                1,
                RIBBON_HEIGHT - 12,
                scale,
                config.palette.divider,
            );
            draw_text_scaled(canvas, origin, x, 6, label, 1, scale, config.palette.muted);
            draw_text_scaled(
                canvas,
                origin,
                x,
                22,
                value.as_bytes(),
                1,
                scale,
                config.palette.text,
            );
        }
        index += 1;
    }
}

fn render_telemetry(
    canvas: &mut Canvas,
    snapshot: MetricSnapshot,
    config: RenderConfig,
    metrics: u16,
    origin: (i32, i32),
    scale: u32,
) {
    let count = metric_count(metrics);
    if config.branding_visible {
        {
            draw_text_scaled(
                canvas,
                origin,
                8,
                6,
                b"REDUNAR",
                1,
                scale,
                config.palette.accent,
            );
        }
    }
    let heading = b"FRAME METRICS";
    let heading_width = i32::try_from(heading.len()).unwrap_or(0) * overlay_font::GLYPH_ADVANCE;
    draw_text_scaled(
        canvas,
        origin,
        TELEMETRY_WIDTH - 8 - heading_width,
        6,
        heading,
        1,
        scale,
        config.palette.muted,
    );
    {
        clear_scaled_rect(
            canvas,
            origin,
            3,
            24,
            TELEMETRY_WIDTH - 3,
            1,
            scale,
            config.palette.divider,
        );
    }
    let mut row = 0_i32;
    for (bit, label) in metric_labels() {
        if metrics & bit == 0 {
            continue;
        }
        let y = 31 + row * TELEMETRY_ROW_HEIGHT;
        let value = metric_value(snapshot, bit);
        let value_width =
            i32::try_from(value.as_bytes().len()).unwrap_or(0) * overlay_font::GLYPH_ADVANCE;
        {
            draw_text_scaled(canvas, origin, 8, y, label, 1, scale, config.palette.muted);
            draw_text_scaled(
                canvas,
                origin,
                TELEMETRY_WIDTH - 8 - value_width,
                y,
                value.as_bytes(),
                1,
                scale,
                config.palette.text,
            );
        }
        row += 1;
        if usize::try_from(row).unwrap_or(usize::MAX) < count {
            clear_scaled_rect(
                canvas,
                origin,
                8,
                y + TELEMETRY_ROW_HEIGHT - 7,
                TELEMETRY_WIDTH - 16,
                1,
                scale,
                config.palette.divider,
            );
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "scaled rectangle geometry is explicit"
)]
fn clear_scaled_rect(
    canvas: &mut Canvas,
    origin: (i32, i32),
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    scale_percent: u32,
    color: [f32; 4],
) {
    canvas.fill_rect(
        origin.0.saturating_add(scale_value(x, scale_percent)),
        origin.1.saturating_add(scale_value(y, scale_percent)),
        scale_value(width, scale_percent).max(1),
        scale_value(height, scale_percent).max(1),
        color,
    );
}

#[expect(
    clippy::too_many_arguments,
    reason = "text placement and scale are explicit"
)]
fn draw_text_scaled(
    canvas: &mut Canvas,
    origin: (i32, i32),
    x: i32,
    y: i32,
    text: &[u8],
    glyph_scale: i32,
    scale_percent: u32,
    color: [f32; 4],
) {
    let glyph_width = scale_value(
        i32::try_from(overlay_font::GLYPH_WIDTH).unwrap_or(0) * glyph_scale,
        scale_percent,
    )
    .max(1);
    let glyph_height = scale_value(
        i32::try_from(overlay_font::GLYPH_HEIGHT).unwrap_or(0) * glyph_scale,
        scale_percent,
    )
    .max(1);
    for (glyph_index, byte) in text.iter().copied().enumerate() {
        let glyph_x = origin.0
            + scale_value(
                x + i32::try_from(glyph_index).unwrap_or(i32::MAX)
                    * overlay_font::GLYPH_ADVANCE
                    * glyph_scale,
                scale_percent,
            );
        let glyph_y = origin.1 + scale_value(y, scale_percent);
        let raster = overlay_font::coverage_raster(byte);
        for row in 0..glyph_height {
            for column in 0..glyph_width {
                let coverage = resampled_coverage(raster, column, row, glyph_width, glyph_height);
                if coverage <= 0.01 {
                    continue;
                }
                canvas.blend_pixel(
                    glyph_x + column,
                    glyph_y + row,
                    [color[0], color[1], color[2], color[3] * coverage],
                );
            }
        }
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "bounded glyph coordinates are converted to match Vulkan's normalized raster sampling"
)]
fn resampled_coverage(
    raster: [u32; overlay_font::COVERAGE_WORDS],
    column: i32,
    row: i32,
    width: i32,
    height: i32,
) -> f32 {
    let source_x = (column as f32 + 0.5) * overlay_font::COVERAGE_WIDTH as f32 / width as f32 - 0.5;
    let source_y = (row as f32 + 0.5) * overlay_font::COVERAGE_HEIGHT as f32 / height as f32 - 0.5;
    let base_x = source_x.floor() as i32;
    let base_y = source_y.floor() as i32;
    let fraction_x = source_x - base_x as f32;
    let fraction_y = source_y - base_y as f32;
    let upper = coverage_sample(raster, base_x, base_y)
        + (coverage_sample(raster, base_x + 1, base_y) - coverage_sample(raster, base_x, base_y))
            * fraction_x;
    let lower = coverage_sample(raster, base_x, base_y + 1)
        + (coverage_sample(raster, base_x + 1, base_y + 1)
            - coverage_sample(raster, base_x, base_y + 1))
            * fraction_x;
    (upper + (lower - upper) * fraction_y).powf(0.72)
}

fn coverage_sample(raster: [u32; overlay_font::COVERAGE_WORDS], x: i32, y: i32) -> f32 {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return 0.0;
    };
    f32::from(overlay_font::coverage_pixel(raster, x, y)) / 3.0
}

fn scale_value(value: i32, percent: u32) -> i32 {
    i32::try_from(i64::from(value).saturating_mul(i64::from(percent)) / 100).unwrap_or(i32::MAX)
}

fn effective_scale(viewport: [i32; 4], width: i32, height: i32, requested: u8) -> u32 {
    let available_width = viewport[2].saturating_sub(PANEL_MARGIN * 2).max(0);
    let available_height = viewport[3].saturating_sub(PANEL_MARGIN * 2).max(0);
    let width_limit = u32::try_from(available_width)
        .unwrap_or(0)
        .saturating_mul(100)
        / u32::try_from(width.max(1)).unwrap_or(1);
    let height_limit = u32::try_from(available_height)
        .unwrap_or(0)
        .saturating_mul(100)
        / u32::try_from(height.max(1)).unwrap_or(1);
    u32::from(requested).min(width_limit).min(height_limit)
}

fn panel_origin(
    viewport: [i32; 4],
    width: i32,
    height: i32,
    scale_percent: u32,
    corner: Corner,
) -> (i32, i32) {
    let physical_width = scale_value(width, scale_percent);
    let physical_height = scale_value(height, scale_percent);
    let left = viewport[0] + PANEL_MARGIN;
    let right = viewport[0] + viewport[2] - physical_width - PANEL_MARGIN;
    let bottom = viewport[1] + PANEL_MARGIN;
    let top = viewport[1] + viewport[3] - physical_height - PANEL_MARGIN;
    match corner {
        Corner::TopLeft => (left, top),
        Corner::TopRight => (right, top),
        Corner::BottomLeft => (left, bottom),
        Corner::BottomRight => (right, bottom),
    }
}

fn selected_metrics(snapshot: MetricSnapshot, config: RenderConfig) -> u16 {
    if config.preset == Preset::FpsOnly {
        return OverlayMetricSet::FPS;
    }
    let mut metrics = match config.preset {
        Preset::Compact => OverlayMetricSet::COMPACT.bits(),
        Preset::Detailed => OverlayMetricSet::DETAILED.bits(),
        Preset::Custom => config.metrics,
        Preset::FpsOnly => OverlayMetricSet::FPS,
    };
    if snapshot.cpu_percent.is_none() {
        metrics &= !OverlayMetricSet::CPU_LOAD;
    }
    if snapshot.cpu_temperature_c.is_none() {
        metrics &= !OverlayMetricSet::CPU_TEMPERATURE;
    }
    if snapshot.gpu_percent.is_none() {
        metrics &= !OverlayMetricSet::GPU_LOAD;
    }
    if snapshot.gpu_temperature_c.is_none() {
        metrics &= !OverlayMetricSet::GPU_TEMPERATURE;
    }
    metrics
}

fn layout_extent(config: RenderConfig, metrics: u16) -> (i32, i32) {
    if config.preset == Preset::FpsOnly {
        return (
            116,
            i32::try_from(overlay_font::GLYPH_HEIGHT).unwrap_or(12) * 2,
        );
    }
    let count = i32::try_from(metric_count(metrics)).unwrap_or(0);
    match config.layout {
        Layout::Grid => (
            GRID_WIDTH,
            GRID_HEADER_HEIGHT + ((count + 1) / 2) * GRID_ROW_HEIGHT,
        ),
        Layout::Ribbon => (
            if config.branding_visible {
                RIBBON_BRAND_WIDTH
            } else {
                0
            } + count * RIBBON_METRIC_WIDTH,
            RIBBON_HEIGHT,
        ),
        Layout::Telemetry => (
            TELEMETRY_WIDTH,
            TELEMETRY_HEADER_HEIGHT + count * TELEMETRY_ROW_HEIGHT,
        ),
    }
}

fn metric_count(metrics: u16) -> usize {
    metric_labels()
        .into_iter()
        .filter(|(bit, _)| metrics & *bit != 0)
        .count()
}

fn metric_labels() -> [(u16, &'static [u8]); 8] {
    [
        (OverlayMetricSet::FPS, b"FPS"),
        (OverlayMetricSet::FRAME_TIME, b"FRAME TIME"),
        (OverlayMetricSet::ONE_PERCENT_LOW, b"1% LOW"),
        (OverlayMetricSet::POINT_ONE_PERCENT_LOW, b"0.1% LOW"),
        (OverlayMetricSet::GPU_LOAD, b"GPU LOAD"),
        (OverlayMetricSet::GPU_TEMPERATURE, b"GPU TEMP"),
        (OverlayMetricSet::CPU_LOAD, b"CPU LOAD"),
        (OverlayMetricSet::CPU_TEMPERATURE, b"CPU TEMP"),
    ]
}

fn metric_value(snapshot: MetricSnapshot, metric: u16) -> FixedText<20> {
    let mut value = FixedText::default();
    match metric {
        OverlayMetricSet::FPS => {
            value.push_number(snapshot.fps);
            value.push_bytes(b" FPS");
        }
        OverlayMetricSet::FRAME_TIME => {
            value.push_tenths(snapshot.frame_time_tenths_ms);
            value.push_bytes(b" MS");
        }
        OverlayMetricSet::ONE_PERCENT_LOW => {
            value.push_number(snapshot.one_percent_low_fps);
            value.push_bytes(b" FPS");
        }
        OverlayMetricSet::POINT_ONE_PERCENT_LOW => {
            value.push_number(snapshot.point_one_percent_low_fps);
            value.push_bytes(b" FPS");
        }
        OverlayMetricSet::GPU_LOAD => {
            value.push_number(snapshot.gpu_percent.map(u16::from));
            value.push(b'%');
        }
        OverlayMetricSet::GPU_TEMPERATURE => {
            value.push_number(
                snapshot
                    .gpu_temperature_c
                    .and_then(|v| u16::try_from(v).ok()),
            );
            value.push_bytes(b"^C");
        }
        OverlayMetricSet::CPU_LOAD => {
            value.push_number(snapshot.cpu_percent.map(u16::from));
            value.push(b'%');
        }
        OverlayMetricSet::CPU_TEMPERATURE => {
            value.push_number(
                snapshot
                    .cpu_temperature_c
                    .and_then(|v| u16::try_from(v).ok()),
            );
            value.push_bytes(b"^C");
        }
        _ => value.push_bytes(b"--"),
    }
    value
}

fn corner_from_environment() -> Corner {
    match env::var(OVERLAY_CORNER_ENV).ok().as_deref() {
        Some("top-right") => Corner::TopRight,
        Some("bottom-left") => Corner::BottomLeft,
        Some("bottom-right") => Corner::BottomRight,
        _ => Corner::TopLeft,
    }
}

fn preset_from_environment() -> Preset {
    match env::var(OVERLAY_PRESET_ENV).ok().as_deref() {
        Some("detailed") => Preset::Detailed,
        Some("fps-only") => Preset::FpsOnly,
        Some("custom") => Preset::Custom,
        _ => Preset::Compact,
    }
}

const fn preset_from_index(index: u8) -> Preset {
    match index {
        1 => Preset::Detailed,
        2 => Preset::FpsOnly,
        3 => Preset::Custom,
        _ => Preset::Compact,
    }
}

fn layout_from_environment() -> Layout {
    match env::var(OVERLAY_LAYOUT_ENV).ok().as_deref() {
        Some("ribbon") => Layout::Ribbon,
        Some("telemetry") => Layout::Telemetry,
        _ => Layout::Grid,
    }
}

const fn layout_from_index(index: u8) -> Layout {
    match index {
        1 => Layout::Ribbon,
        2 => Layout::Telemetry,
        _ => Layout::Grid,
    }
}

fn metrics_from_environment() -> u16 {
    env::var(OVERLAY_METRICS_ENV)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|bits| *bits != 0 && *bits & !OverlayMetricSet::KNOWN == 0)
        .unwrap_or_else(|| OverlayMetricSet::COMPACT.bits())
}

fn opacity_from_environment() -> u8 {
    env::var(OVERLAY_OPACITY_ENV)
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|value| *value <= 100)
        .unwrap_or(50)
}

fn with_alpha(mut color: [f32; 4], alpha: f32) -> [f32; 4] {
    color[3] = alpha;
    color
}

fn branding_from_environment() -> bool {
    env::var(OVERLAY_BRANDING_ENV).ok().as_deref() != Some("0")
}

fn palette_from_environment() -> Palette {
    let index = match env::var(OVERLAY_PALETTE_ENV).ok().as_deref() {
        Some("glacier") => 1,
        Some("ember") => 2,
        Some("mint") => 3,
        Some("mono") => 4,
        Some("amethyst") => 5,
        Some("solar") => 6,
        Some("rose") => 7,
        _ => 0,
    };
    palette_from_index(index)
}

const fn palette_from_index(index: u8) -> Palette {
    match index {
        1 => Palette::new([0.027, 0.067, 0.086, 1.0], [0.31, 0.78, 0.96, 1.0]),
        2 => Palette::new([0.071, 0.051, 0.031, 1.0], [0.98, 0.47, 0.22, 1.0]),
        3 => Palette::new([0.027, 0.067, 0.047, 1.0], [0.33, 0.88, 0.59, 1.0]),
        4 => Palette::new([0.035, 0.035, 0.035, 1.0], [0.78, 0.78, 0.78, 1.0]),
        5 => Palette::new([0.055, 0.039, 0.078, 1.0], [0.68, 0.48, 0.95, 1.0]),
        6 => Palette::new([0.071, 0.063, 0.024, 1.0], [0.95, 0.83, 0.36, 1.0]),
        7 => Palette::new([0.078, 0.035, 0.063, 1.0], [1.0, 0.51, 0.68, 1.0]),
        _ => Palette::new([0.035, 0.035, 0.035, 1.0], [0.88, 0.20, 0.24, 1.0]),
    }
}

impl Palette {
    const fn new(panel: [f32; 4], accent: [f32; 4]) -> Self {
        Self {
            panel,
            accent,
            muted: [0.72, 0.72, 0.72, 1.0],
            text: [0.96, 0.96, 0.96, 1.0],
            divider: [0.22, 0.22, 0.22, 1.0],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FixedText<const N: usize> {
    bytes: [u8; N],
    length: usize,
}

impl<const N: usize> Default for FixedText<N> {
    fn default() -> Self {
        Self {
            bytes: [0; N],
            length: 0,
        }
    }
}

impl<const N: usize> FixedText<N> {
    fn push(&mut self, byte: u8) {
        if self.length < N {
            self.bytes[self.length] = byte;
            self.length += 1;
        }
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.push(*byte);
        }
    }

    fn push_number(&mut self, value: Option<u16>) {
        let Some(mut value) = value else {
            self.push_bytes(b"--");
            return;
        };
        let mut digits = [0_u8; 5];
        let mut count = 0;
        loop {
            digits[count] = b'0' + u8::try_from(value % 10).unwrap_or(0);
            count += 1;
            value /= 10;
            if value == 0 || count == digits.len() {
                break;
            }
        }
        for index in (0..count).rev() {
            self.push(digits[index]);
        }
    }

    fn push_tenths(&mut self, value: Option<u16>) {
        let Some(value) = value else {
            self.push_bytes(b"--");
            return;
        };
        self.push_number(Some(value / 10));
        self.push(b'.');
        self.push(b'0' + u8::try_from(value % 10).unwrap_or(0));
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
}

fn fps_for_slice(intervals_ns: &[u64]) -> u16 {
    let sum = intervals_ns
        .iter()
        .fold(0_u128, |total, value| total + u128::from(*value));
    clamp_metric(1_000_000_000_u128.saturating_mul(intervals_ns.len() as u128) / sum.max(1))
}

fn clamp_metric(value: u128) -> u16 {
    u16::try_from(value.min(9_999)).unwrap_or(9_999)
}

fn round_percent(tenths: u16) -> u8 {
    u8::try_from((tenths.saturating_add(5) / 10).min(100)).unwrap_or(100)
}

fn round_temp(tenths: u16) -> i16 {
    i16::try_from(tenths.saturating_add(5) / 10).unwrap_or(i16::MAX)
}

#[cfg(test)]
mod tests {
    use super::{
        Canvas, Corner, FixedText, FrameHistory, Layout, MetricSnapshot, OverlayState, Preset,
        RenderConfig, draw_text_scaled, layout_extent, palette_from_index, panel_origin,
        render_plan, render_replay_menu, render_saved_notice,
    };
    use redunar_capture::{
        OVERLAY_HARDWARE_TELEMETRY_BYTES, OverlayHardwareTelemetry, REPLAY_MENU_TELEMETRY_BYTES,
        ReplayMenuStatus, ReplayMenuTelemetry, ReplayShortcutLabel,
        encode_overlay_hardware_telemetry, encode_replay_menu_telemetry,
    };
    use std::fs::{self, File};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn fixed_metric_text_formats_without_allocating() {
        let mut text = FixedText::<12>::default();
        text.push_bytes(b"FPS ");
        text.push_number(Some(60));
        assert_eq!(text.as_bytes(), b"FPS 60");
    }

    #[test]
    fn downsampled_font_contains_every_metric_label_character() {
        for byte in b"FPS FRAME TIME LOW GPU LOAD TEMP CPU REDUNAR 0.1%" {
            if *byte != b' ' {
                let raster = redunar_core::overlay_font::coverage_raster(*byte);
                assert!(
                    raster.iter().any(|word| *word != 0),
                    "missing glyph {}",
                    char::from(*byte)
                );
            }
        }
    }

    #[test]
    fn fractional_scale_expands_glyph_strokes_with_their_advance() {
        fn ink_width(canvas: &Canvas) -> usize {
            let columns = (0..canvas.width)
                .filter(|column| {
                    (0..canvas.height).any(|row| {
                        let index = (usize::try_from(row).unwrap()
                            * usize::try_from(canvas.width).unwrap()
                            + usize::try_from(*column).unwrap())
                            * 4;
                        canvas.pixels[index + 3] != 0
                    })
                })
                .collect::<Vec<_>>();
            usize::try_from(columns.last().unwrap() - columns.first().unwrap() + 1).unwrap()
        }

        let mut base = Canvas::new(32, 32);
        draw_text_scaled(&mut base, (0, 0), 0, 0, b"8", 1, 100, [1.0; 4]);
        let mut fractional = Canvas::new(32, 32);
        draw_text_scaled(&mut fractional, (0, 0), 0, 0, b"8", 1, 135, [1.0; 4]);

        assert!(ink_width(&fractional) > ink_width(&base));
    }

    fn has_rgb(canvas: &Canvas, rows: std::ops::Range<i32>, rgb: [u8; 3]) -> bool {
        rows.flat_map(|row| (0..canvas.width).map(move |column| (column, row)))
            .any(|(column, row)| {
                let index = (usize::try_from(row).unwrap()
                    * usize::try_from(canvas.width).unwrap()
                    + usize::try_from(column).unwrap())
                    * 4;
                canvas.pixels[index..index + 3] == rgb
            })
    }

    #[test]
    fn ribbon_places_labels_above_values() {
        let palette = palette_from_index(0);
        let config = RenderConfig {
            corner: Corner::BottomRight,
            palette,
            preset: Preset::Compact,
            layout: Layout::Ribbon,
            metrics: 0,
            scale_percent: 100,
            opacity_percent: 65,
            branding_visible: false,
        };
        let snapshot = MetricSnapshot {
            fps: Some(144),
            frame_time_tenths_ms: Some(69),
            ..MetricSnapshot::default()
        };
        let metrics =
            redunar_core::OverlayMetricSet::FPS | redunar_core::OverlayMetricSet::FRAME_TIME;
        let (width, height) = layout_extent(config, metrics);
        let mut canvas = Canvas::new(width, height);

        render_plan(&mut canvas, snapshot, config, metrics, (0, 0), 100);

        let muted = palette.muted.map(super::normalized_byte);
        let text = palette.text.map(super::normalized_byte);
        assert!(has_rgb(&canvas, 6..18, [muted[0], muted[1], muted[2]]));
        assert!(!has_rgb(&canvas, 6..18, [text[0], text[1], text[2]]));
        assert!(has_rgb(&canvas, 22..34, [text[0], text[1], text[2]]));
    }

    #[test]
    fn telemetry_has_vulkan_header_and_row_separators() {
        let palette = palette_from_index(0);
        let config = RenderConfig {
            corner: Corner::BottomRight,
            palette,
            preset: Preset::Compact,
            layout: Layout::Telemetry,
            metrics: 0,
            scale_percent: 100,
            opacity_percent: 65,
            branding_visible: false,
        };
        let snapshot = MetricSnapshot {
            fps: Some(144),
            frame_time_tenths_ms: Some(69),
            ..MetricSnapshot::default()
        };
        let metrics =
            redunar_core::OverlayMetricSet::FPS | redunar_core::OverlayMetricSet::FRAME_TIME;
        let (width, height) = layout_extent(config, metrics);
        let mut canvas = Canvas::new(width, height);

        render_plan(&mut canvas, snapshot, config, metrics, (0, 0), 100);

        let muted = palette.muted.map(super::normalized_byte);
        let divider = palette.divider.map(super::normalized_byte);
        let width = usize::try_from(width).unwrap();
        assert!(has_rgb(&canvas, 6..18, [muted[0], muted[1], muted[2]]));
        assert_eq!(&canvas.pixels[(24 * width + 20) * 4..][..3], &divider[..3]);
        assert_eq!(&canvas.pixels[(48 * width + 20) * 4..][..3], &divider[..3]);
    }

    #[test]
    fn panel_origin_honors_viewport_offset_and_corner() {
        let viewport = [10, 20, 640, 360];
        assert_eq!(
            panel_origin(viewport, 160, 36, 100, Corner::TopLeft),
            (22, 332)
        );
        assert_eq!(
            panel_origin(viewport, 160, 36, 100, Corner::BottomRight),
            (478, 32)
        );
    }

    #[test]
    fn fps_only_has_no_panel_sized_background() {
        let config = RenderConfig {
            corner: Corner::TopLeft,
            palette: palette_from_index(0),
            preset: Preset::FpsOnly,
            layout: Layout::Grid,
            metrics: 1,
            scale_percent: 100,
            opacity_percent: 50,
            branding_visible: true,
        };
        assert_eq!(layout_extent(config, 1), (116, 24));
    }

    #[test]
    fn frame_history_calculates_average_and_low_metrics() {
        let mut history = FrameHistory::default();
        for _ in 0..99 {
            history.push(10_000_000);
        }
        history.push(20_000_000);
        let snapshot = history.summarize();
        assert_eq!(snapshot.fps, Some(99));
        assert_eq!(snapshot.one_percent_low_fps, Some(50));
        assert_eq!(snapshot.frame_time_tenths_ms, Some(200));
    }

    #[test]
    fn telemetry_refresh_applies_live_visibility_corner_and_palette() {
        let path = std::env::temp_dir().join(format!(
            "redunar-opengl-overlay-{}-{}",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut bytes = [0_u8; OVERLAY_HARDWARE_TELEMETRY_BYTES];
        encode_overlay_hardware_telemetry(
            OverlayHardwareTelemetry {
                revision: 2,
                corner: 3,
                preset: 2,
                layout: 0,
                palette: 7,
                metrics: 1,
                opacity_percent: 50,
                scale_percent: 100,
                replay_saved_revision: 0,
                metrics_visible: Some(true),
                branding_visible: true,
                cpu_utilization_tenths: None,
                cpu_temperature_tenths_celsius: None,
                gpu_utilization_tenths: None,
                gpu_temperature_tenths_celsius: None,
            },
            &mut bytes,
        )
        .expect("encode telemetry");
        fs::write(&path, bytes).expect("write telemetry");
        let mut state = OverlayState {
            file: Some(File::open(&path).expect("open telemetry")),
            last_hardware_read_ns: 0,
            last_menu_read_ns: 0,
            last_revision: 0,
            last_menu_revision: 0,
            visible: false,
            corner: Corner::TopLeft,
            palette: palette_from_index(0),
            preset: Preset::Compact,
            layout: Layout::Grid,
            metrics: 1,
            scale_percent: 100,
            opacity_percent: 50,
            branding_visible: false,
            hardware: MetricSnapshot::default(),
            menu: None,
            replay_saved_revision: 0,
            replay_saved_until_ns: 0,
            history: FrameHistory::default(),
        };

        state.refresh(1);

        assert!(state.visible);
        assert_eq!(state.corner, Corner::BottomRight);
        assert_eq!(state.palette, palette_from_index(7));
        assert_eq!(state.preset, Preset::FpsOnly);
        assert_eq!(state.scale_percent, 100);
        fs::remove_file(path).expect("remove telemetry");
    }

    #[test]
    fn replay_menu_and_saved_notice_are_independent_from_metric_visibility() {
        let path = std::env::temp_dir().join(format!(
            "redunar-opengl-replay-overlay-{}-{}",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut bytes = vec![0_u8; OVERLAY_HARDWARE_TELEMETRY_BYTES + REPLAY_MENU_TELEMETRY_BYTES];
        encode_overlay_hardware_telemetry(
            OverlayHardwareTelemetry {
                revision: 2,
                corner: 0,
                preset: 0,
                layout: 0,
                palette: 0,
                metrics: 1,
                opacity_percent: 50,
                scale_percent: 100,
                replay_saved_revision: 3,
                metrics_visible: Some(false),
                branding_visible: true,
                cpu_utilization_tenths: None,
                cpu_temperature_tenths_celsius: None,
                gpu_utilization_tenths: None,
                gpu_temperature_tenths_celsius: None,
            },
            &mut bytes[..OVERLAY_HARDWARE_TELEMETRY_BYTES],
        )
        .expect("encode hardware telemetry");
        let menu = ReplayMenuTelemetry {
            revision: 4,
            visible: true,
            pointer_pressed: false,
            cursor_x: 5_000,
            cursor_y: 5_000,
            hover_target: 2,
            pressed_target: 0,
            selected_duration_index: 1,
            status: ReplayMenuStatus::Buffering,
            available_seconds: 27,
            click_revision: 0,
            frame_rate: 60,
            quality: 1,
            output_format: 0,
            save_enabled: true,
            overlay_shortcut: ReplayShortcutLabel::from_shortcut("Ctrl+Shift+R"),
            save_shortcut: ReplayShortcutLabel::from_shortcut("Ctrl+F9"),
        };
        encode_replay_menu_telemetry(menu, &mut bytes[OVERLAY_HARDWARE_TELEMETRY_BYTES..])
            .expect("encode Replay menu telemetry");
        fs::write(&path, bytes).expect("write telemetry");
        let mut state = OverlayState {
            file: Some(File::open(&path).expect("open telemetry")),
            last_hardware_read_ns: 0,
            last_menu_read_ns: 0,
            last_revision: 0,
            last_menu_revision: 0,
            visible: true,
            corner: Corner::TopLeft,
            palette: palette_from_index(0),
            preset: Preset::Compact,
            layout: Layout::Grid,
            metrics: 1,
            scale_percent: 100,
            opacity_percent: 50,
            branding_visible: true,
            hardware: MetricSnapshot::default(),
            menu: None,
            replay_saved_revision: 0,
            replay_saved_until_ns: 0,
            history: FrameHistory::default(),
        };

        state.refresh(100);

        assert!(!state.visible);
        assert_eq!(state.menu, Some(menu));
        assert_eq!(state.replay_saved_revision, 3);
        assert_eq!(
            state.replay_saved_until_ns,
            100 + super::REPLAY_SAVED_NOTICE_NS
        );
        let mut menu_canvas = Canvas::new(super::REPLAY_MENU_WIDTH, super::REPLAY_MENU_HEIGHT);
        render_replay_menu(&mut menu_canvas, menu, 100);
        assert!(menu_canvas.pixels.iter().any(|byte| *byte != 0));
        let mut notice_canvas = Canvas::new(super::SAVED_NOTICE_WIDTH, super::SAVED_NOTICE_HEIGHT);
        render_saved_notice(&mut notice_canvas, 100);
        assert!(notice_canvas.pixels.iter().any(|byte| *byte != 0));
        fs::remove_file(path).expect("remove telemetry");
    }
}
