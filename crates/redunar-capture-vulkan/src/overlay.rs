//! Bounded Vulkan overlay renderer.
//!
//! Panel transparency and glyphs use tiny Redunar-authored, descriptor-free
//! graphics pipelines with embedded SPIR-V. Each glyph receives one expanded
//! 108-byte high-resolution coverage raster, avoiding driver-sensitive rectangle arrays and font
//! layout differences in injected games.
//! All resources are created at queue/swapchain discovery and cached.
//! Presentation only resets and records an already allocated command buffer,
//! checks a fence without waiting, and submits once before the application's
//! present.

#![expect(
    clippy::large_types_passed_by_value,
    reason = "the copied Vulkan function table avoids aliased borrows across renderer state and contains only function pointers"
)]

// This private ABI module intentionally mirrors a broad Vulkan command/type
// surface. Keeping the names unprefixed makes the safety review correspond to
// Vulkan's specification names without a second layer of aliases.
#[allow(clippy::wildcard_imports)]
use crate::ffi::*;
use redunar_capture::{
    OVERLAY_HARDWARE_TELEMETRY_BYTES, OverlayFailureReason, OverlayHardwareTelemetry,
    REPLAY_MENU_TELEMETRY_BYTES, ReplayMenuStatus, ReplayMenuTelemetry, ReplayShortcutLabel,
    decode_overlay_hardware_telemetry, decode_replay_menu_telemetry,
};
use redunar_core::overlay_font;
use std::collections::BTreeMap;
use std::env;
use std::ffi::c_char;
use std::fs::File;
use std::mem;
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};

mod saved_notice;

const OVERLAY_VISIBLE_ENV: &str = "REDUNAR_OVERLAY_VISIBLE";
const REPLAY_PRODUCTION_ENV: &str = "REDUNAR_REPLAY_PRODUCTION";
const OVERLAY_DIAGNOSTIC_ENV: &str = "REDUNAR_OVERLAY_DIAGNOSTIC";
const OVERLAY_TELEMETRY_ENV: &str = "REDUNAR_OVERLAY_TELEMETRY";
const OVERLAY_PRESET_ENV: &str = "REDUNAR_OVERLAY_PRESET";
const OVERLAY_LAYOUT_ENV: &str = "REDUNAR_OVERLAY_LAYOUT";
const OVERLAY_PALETTE_ENV: &str = "REDUNAR_OVERLAY_PALETTE";
const OVERLAY_BRANDING_ENV: &str = "REDUNAR_OVERLAY_BRANDING";
const OVERLAY_CORNER_ENV: &str = "REDUNAR_OVERLAY_CORNER";
const OVERLAY_METRICS_ENV: &str = "REDUNAR_OVERLAY_METRICS";
const OVERLAY_OPACITY_ENV: &str = "REDUNAR_OVERLAY_OPACITY_PERCENT";
// Poll the tiny cached telemetry block at 4 Hz so session visibility controls
// respond promptly. Unchanged revisions never rebuild the rendering plan.
const TELEMETRY_INTERVAL_NS: u64 = 250_000_000;
// Poll the Replay menu block at 20 Hz while it is closed. Opening the menu is
// detected within one poll; once visible or animating, the reader switches to
// per-presentation polling so the drawn pointer tracks the daemon revision.
const MENU_POLL_IDLE_NS: u64 = 50_000_000;
const FRAME_HISTORY_CAPACITY: usize = 240;
const SUMMARY_INTERVAL_NS: u64 = 250_000_000;
const REPLAY_MENU_ANIMATION_NS: u64 = 135_000_000;
const REPLAY_SAVED_NOTICE_NS: u64 = 3_000_000_000;
const MAX_REASONABLE_INTERVAL_NS: u64 = 2_000_000_000;
const MAX_QUEUE_FAMILIES: usize = 64;
const MAX_DEVICES: usize = 8;
const MAX_QUEUES_PER_DEVICE: usize = 8;
const MAX_SWAPCHAINS_PER_DEVICE: usize = 8;
const MAX_SWAPCHAIN_IMAGES: usize = 16;
const MAX_PRESENT_SWAPCHAINS: usize = 8;
const MAX_PRESENT_WAIT_SEMAPHORES: usize = 16;
const MAX_QUEUE_CONTEXTS: usize = 16;
const MAX_ACCENT_RECTS: usize = 96;
const MAX_TEXT_GLYPHS: usize = 384;

const REPLAY_MENU_WIDTH: u32 = 520;
const REPLAY_MENU_HEIGHT: u32 = 286;
const REPLAY_MENU_FACTS_HEIGHT: u32 = 66;
const REPLAY_MENU_DURATION_X: i32 = 24;
const REPLAY_MENU_DURATION_Y: i32 = 124;
const REPLAY_MENU_DURATION_WIDTH: u32 = 472;
const REPLAY_MENU_DURATION_HEIGHT: u32 = 84;
const REPLAY_MENU_SAVE_X: i32 = 220;
const REPLAY_MENU_SAVE_Y: i32 = 222;
const REPLAY_MENU_SAVE_WIDTH: u32 = 276;
const REPLAY_MENU_SAVE_HEIGHT: u32 = 46;
const REPLAY_MENU_FORMAT_X: i32 = 24;
const REPLAY_MENU_FORMAT_Y: i32 = 222;
const REPLAY_MENU_FORMAT_WIDTH: u32 = 180;
const REPLAY_MENU_FORMAT_HEIGHT: u32 = 46;
const REPLAY_MENU_CORNER_RADIUS: u8 = 12;
const REPLAY_MENU_CONTROL_RADIUS: u32 = 6;
/// Panel shader palette slot for the Replay menu's near-black surface. The
/// eight metric palettes keep slots 0..=7; the menu never follows them.
const MENU_PANEL_PALETTE: u32 = 8;
/// The Replay menu's near-black surface and selected-control fill, converted
/// from the approved reference surface (#090909 and #21090b).
#[cfg(test)]
const MENU_PANEL_COLOR: [f32; 4] = [0.003, 0.003, 0.003, 1.0];
const MENU_SELECTED_FILL: [f32; 4] = [0.015, 0.003, 0.003, 1.0];
const METRIC_PANEL_RADIUS: u8 = 4;
const RIBBON_BRAND_WIDTH: u32 = 94;
const RIBBON_METRIC_WIDTH: u32 = 89;
/// Ribbon labels use a tighter advance than the fixed 9 px cell so the
/// longest label stays clear of the next cell divider.
const RIBBON_LABEL_ADVANCE: i32 = 8;
const RIBBON_HEIGHT: u32 = 44;
const TELEMETRY_WIDTH: u32 = 300;
const TELEMETRY_ROW_HEIGHT: u32 = 24;

const MIN_OVERLAY_WIDTH: u32 = PANEL_WIDTH + 24;
const MIN_OVERLAY_HEIGHT: u32 = DETAILED_PANEL_HEIGHT + 24;
const PANEL_WIDTH: u32 = 294;
const PANEL_BASE_HEIGHT: u32 = 25;
// Keep six to seven logical pixels around each text canvas. The injected
// overlay is read peripherally during play, so rows need breathing room even
// at the smallest configured scale.
const PRIMARY_ROW_HEIGHT: u32 = 38;
const SECONDARY_ROW_HEIGHT: u32 = 26;
const DETAILED_PANEL_HEIGHT: u32 =
    PANEL_BASE_HEIGHT + PRIMARY_ROW_HEIGHT + 2 * SECONDARY_ROW_HEIGHT;
const OVERLAY_MARGIN: i32 = 12;
#[cfg(test)]
const BITMAP_GLYPH_HEIGHT: u32 = overlay_font::GLYPH_HEIGHT;
const FPS_ONLY_WIDTH: u32 = 116;
const FIRST_METRIC_ROW_Y: i32 = 31;
const ROW_DIVIDER_OFFSET: i32 = 7;
const METRIC_FPS: u16 = 1 << 0;
const METRIC_FRAME_TIME: u16 = 1 << 1;
const METRIC_ONE_PERCENT_LOW: u16 = 1 << 2;
const METRIC_POINT_ONE_PERCENT_LOW: u16 = 1 << 3;
const METRIC_CPU_LOAD: u16 = 1 << 4;
const METRIC_CPU_TEMPERATURE: u16 = 1 << 5;
const METRIC_GPU_LOAD: u16 = 1 << 6;
const METRIC_GPU_TEMPERATURE: u16 = 1 << 7;
const METRICS_KNOWN: u16 = METRIC_FPS
    | METRIC_FRAME_TIME
    | METRIC_ONE_PERCENT_LOW
    | METRIC_POINT_ONE_PERCENT_LOW
    | METRIC_CPU_LOAD
    | METRIC_CPU_TEMPERATURE
    | METRIC_GPU_LOAD
    | METRIC_GPU_TEMPERATURE;
const METRICS_COMPACT: u16 = METRIC_FPS
    | METRIC_FRAME_TIME
    | METRIC_CPU_LOAD
    | METRIC_CPU_TEMPERATURE
    | METRIC_GPU_LOAD
    | METRIC_GPU_TEMPERATURE;

const GET_SWAPCHAIN_IMAGES: &[u8] = b"vkGetSwapchainImagesKHR\0";
const CREATE_SEMAPHORE: &[u8] = b"vkCreateSemaphore\0";
const DESTROY_SEMAPHORE: &[u8] = b"vkDestroySemaphore\0";
const CREATE_FENCE: &[u8] = b"vkCreateFence\0";
const DESTROY_FENCE: &[u8] = b"vkDestroyFence\0";
const RESET_FENCES: &[u8] = b"vkResetFences\0";
const GET_FENCE_STATUS: &[u8] = b"vkGetFenceStatus\0";
const CREATE_COMMAND_POOL: &[u8] = b"vkCreateCommandPool\0";
const DESTROY_COMMAND_POOL: &[u8] = b"vkDestroyCommandPool\0";
const ALLOCATE_COMMAND_BUFFERS: &[u8] = b"vkAllocateCommandBuffers\0";
const RESET_COMMAND_BUFFER: &[u8] = b"vkResetCommandBuffer\0";
const BEGIN_COMMAND_BUFFER: &[u8] = b"vkBeginCommandBuffer\0";
const END_COMMAND_BUFFER: &[u8] = b"vkEndCommandBuffer\0";
const QUEUE_SUBMIT: &[u8] = b"vkQueueSubmit\0";
const CREATE_IMAGE_VIEW: &[u8] = b"vkCreateImageView\0";
const DESTROY_IMAGE_VIEW: &[u8] = b"vkDestroyImageView\0";
const CREATE_RENDER_PASS: &[u8] = b"vkCreateRenderPass\0";
const DESTROY_RENDER_PASS: &[u8] = b"vkDestroyRenderPass\0";
const CREATE_FRAMEBUFFER: &[u8] = b"vkCreateFramebuffer\0";
const DESTROY_FRAMEBUFFER: &[u8] = b"vkDestroyFramebuffer\0";
const CMD_BEGIN_RENDER_PASS: &[u8] = b"vkCmdBeginRenderPass\0";
const CMD_END_RENDER_PASS: &[u8] = b"vkCmdEndRenderPass\0";
const CMD_CLEAR_ATTACHMENTS: &[u8] = b"vkCmdClearAttachments\0";
const CREATE_SHADER_MODULE: &[u8] = b"vkCreateShaderModule\0";
const DESTROY_SHADER_MODULE: &[u8] = b"vkDestroyShaderModule\0";
const CREATE_PIPELINE_LAYOUT: &[u8] = b"vkCreatePipelineLayout\0";
const DESTROY_PIPELINE_LAYOUT: &[u8] = b"vkDestroyPipelineLayout\0";
const CREATE_GRAPHICS_PIPELINES: &[u8] = b"vkCreateGraphicsPipelines\0";
const DESTROY_PIPELINE: &[u8] = b"vkDestroyPipeline\0";
const CMD_BIND_PIPELINE: &[u8] = b"vkCmdBindPipeline\0";
const CMD_SET_VIEWPORT: &[u8] = b"vkCmdSetViewport\0";
const CMD_SET_SCISSOR: &[u8] = b"vkCmdSetScissor\0";
const CMD_SET_BLEND_CONSTANTS: &[u8] = b"vkCmdSetBlendConstants\0";
const CMD_DRAW: &[u8] = b"vkCmdDraw\0";

#[repr(align(4))]
struct AlignedShader<const N: usize>([u8; N]);

static PANEL_VERTEX_SHADER: AlignedShader<1164> =
    AlignedShader(*include_bytes!("shaders/panel.vert.spv"));
static PANEL_FRAGMENT_SHADER: AlignedShader<4652> =
    AlignedShader(*include_bytes!("shaders/panel.frag.spv"));
static GLYPH_VERTEX_SHADER: AlignedShader<1448> =
    AlignedShader(*include_bytes!("shaders/glyph.vert.spv"));
static GLYPH_FRAGMENT_SHADER: AlignedShader<10736> =
    AlignedShader(*include_bytes!("shaders/glyph.frag.spv"));

/// Function table used only by the optional renderer. If even one required
/// command is unavailable, device creation remains successful and overlay
/// setup is skipped for that device.
#[derive(Clone, Copy)]
pub(crate) struct DeviceFunctions {
    get_swapchain_images: PfnGetSwapchainImagesKhr,
    create_semaphore: PfnCreateSemaphore,
    destroy_semaphore: PfnDestroySemaphore,
    create_fence: PfnCreateFence,
    destroy_fence: PfnDestroyFence,
    reset_fences: PfnResetFences,
    get_fence_status: PfnGetFenceStatus,
    create_command_pool: PfnCreateCommandPool,
    destroy_command_pool: PfnDestroyCommandPool,
    allocate_command_buffers: PfnAllocateCommandBuffers,
    reset_command_buffer: PfnResetCommandBuffer,
    begin_command_buffer: PfnBeginCommandBuffer,
    end_command_buffer: PfnEndCommandBuffer,
    queue_submit: PfnQueueSubmit,
    create_image_view: PfnCreateImageView,
    destroy_image_view: PfnDestroyImageView,
    create_render_pass: PfnCreateRenderPass,
    destroy_render_pass: PfnDestroyRenderPass,
    create_framebuffer: PfnCreateFramebuffer,
    destroy_framebuffer: PfnDestroyFramebuffer,
    cmd_begin_render_pass: PfnCmdBeginRenderPass,
    cmd_end_render_pass: PfnCmdEndRenderPass,
    cmd_clear_attachments: PfnCmdClearAttachments,
    create_shader_module: PfnCreateShaderModule,
    destroy_shader_module: PfnDestroyShaderModule,
    create_pipeline_layout: PfnCreatePipelineLayout,
    destroy_pipeline_layout: PfnDestroyPipelineLayout,
    create_graphics_pipelines: PfnCreateGraphicsPipelines,
    destroy_pipeline: PfnDestroyPipeline,
    cmd_bind_pipeline: PfnCmdBindPipeline,
    cmd_set_viewport: PfnCmdSetViewport,
    cmd_set_scissor: PfnCmdSetScissor,
    cmd_set_blend_constants: PfnCmdSetBlendConstants,
    cmd_push_constants: PfnCmdPushConstants,
    cmd_draw: PfnCmdDraw,
}

impl DeviceFunctions {
    pub(crate) unsafe fn load(next: PfnGetDeviceProcAddr, device: VkDevice) -> Option<Self> {
        macro_rules! load {
            ($name:expr, $ty:ty) => {{
                // SAFETY: each lookup uses the exact command name corresponding
                // to the destination Vulkan PFN type.
                let raw = unsafe { next(device, $name.as_ptr().cast::<c_char>()) }?;
                // SAFETY: Vulkan exposes all commands through one generic PFN;
                // the command name above fixes the concrete ABI signature.
                unsafe { mem::transmute::<unsafe extern "system" fn(), $ty>(raw) }
            }};
        }

        Some(Self {
            get_swapchain_images: load!(GET_SWAPCHAIN_IMAGES, PfnGetSwapchainImagesKhr),
            create_semaphore: load!(CREATE_SEMAPHORE, PfnCreateSemaphore),
            destroy_semaphore: load!(DESTROY_SEMAPHORE, PfnDestroySemaphore),
            create_fence: load!(CREATE_FENCE, PfnCreateFence),
            destroy_fence: load!(DESTROY_FENCE, PfnDestroyFence),
            reset_fences: load!(RESET_FENCES, PfnResetFences),
            get_fence_status: load!(GET_FENCE_STATUS, PfnGetFenceStatus),
            create_command_pool: load!(CREATE_COMMAND_POOL, PfnCreateCommandPool),
            destroy_command_pool: load!(DESTROY_COMMAND_POOL, PfnDestroyCommandPool),
            allocate_command_buffers: load!(ALLOCATE_COMMAND_BUFFERS, PfnAllocateCommandBuffers),
            reset_command_buffer: load!(RESET_COMMAND_BUFFER, PfnResetCommandBuffer),
            begin_command_buffer: load!(BEGIN_COMMAND_BUFFER, PfnBeginCommandBuffer),
            end_command_buffer: load!(END_COMMAND_BUFFER, PfnEndCommandBuffer),
            queue_submit: load!(QUEUE_SUBMIT, PfnQueueSubmit),
            create_image_view: load!(CREATE_IMAGE_VIEW, PfnCreateImageView),
            destroy_image_view: load!(DESTROY_IMAGE_VIEW, PfnDestroyImageView),
            create_render_pass: load!(CREATE_RENDER_PASS, PfnCreateRenderPass),
            destroy_render_pass: load!(DESTROY_RENDER_PASS, PfnDestroyRenderPass),
            create_framebuffer: load!(CREATE_FRAMEBUFFER, PfnCreateFramebuffer),
            destroy_framebuffer: load!(DESTROY_FRAMEBUFFER, PfnDestroyFramebuffer),
            cmd_begin_render_pass: load!(CMD_BEGIN_RENDER_PASS, PfnCmdBeginRenderPass),
            cmd_end_render_pass: load!(CMD_END_RENDER_PASS, PfnCmdEndRenderPass),
            cmd_clear_attachments: load!(CMD_CLEAR_ATTACHMENTS, PfnCmdClearAttachments),
            create_shader_module: load!(CREATE_SHADER_MODULE, PfnCreateShaderModule),
            destroy_shader_module: load!(DESTROY_SHADER_MODULE, PfnDestroyShaderModule),
            create_pipeline_layout: load!(CREATE_PIPELINE_LAYOUT, PfnCreatePipelineLayout),
            destroy_pipeline_layout: load!(DESTROY_PIPELINE_LAYOUT, PfnDestroyPipelineLayout),
            create_graphics_pipelines: load!(CREATE_GRAPHICS_PIPELINES, PfnCreateGraphicsPipelines),
            destroy_pipeline: load!(DESTROY_PIPELINE, PfnDestroyPipeline),
            cmd_bind_pipeline: load!(CMD_BIND_PIPELINE, PfnCmdBindPipeline),
            cmd_set_viewport: load!(CMD_SET_VIEWPORT, PfnCmdSetViewport),
            cmd_set_scissor: load!(CMD_SET_SCISSOR, PfnCmdSetScissor),
            cmd_set_blend_constants: load!(CMD_SET_BLEND_CONSTANTS, PfnCmdSetBlendConstants),
            cmd_push_constants: load!(b"vkCmdPushConstants\0", PfnCmdPushConstants),
            cmd_draw: load!(CMD_DRAW, PfnCmdDraw),
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct OverlaySnapshot {
    pub(crate) fps: Option<u16>,
    pub(crate) frame_time_tenths_ms: Option<u16>,
    pub(crate) one_percent_low_fps: Option<u16>,
    pub(crate) point_one_percent_low_fps: Option<u16>,
    pub(crate) cpu_percent: Option<u8>,
    pub(crate) cpu_temperature_c: Option<i16>,
    pub(crate) gpu_percent: Option<u8>,
    pub(crate) gpu_temperature_c: Option<i16>,
}

struct FrameHistory {
    intervals_ns: [u64; FRAME_HISTORY_CAPACITY],
    length: usize,
    next: usize,
    previous_present_ns: Option<u64>,
    last_summary_ns: u64,
    snapshot: OverlaySnapshot,
}

impl Default for FrameHistory {
    fn default() -> Self {
        Self {
            intervals_ns: [0; FRAME_HISTORY_CAPACITY],
            length: 0,
            next: 0,
            previous_present_ns: None,
            last_summary_ns: 0,
            snapshot: OverlaySnapshot::default(),
        }
    }
}

impl FrameHistory {
    fn record(&mut self, now_ns: u64) -> bool {
        let Some(previous) = self.previous_present_ns.replace(now_ns) else {
            return false;
        };
        let Some(interval) = now_ns.checked_sub(previous) else {
            return false;
        };
        if interval == 0 || interval > MAX_REASONABLE_INTERVAL_NS {
            return false;
        }
        self.intervals_ns[self.next] = interval;
        self.next = (self.next + 1) % FRAME_HISTORY_CAPACITY;
        self.length = self.length.saturating_add(1).min(FRAME_HISTORY_CAPACITY);

        if self.last_summary_ns != 0
            && now_ns.saturating_sub(self.last_summary_ns) < SUMMARY_INTERVAL_NS
        {
            return false;
        }
        self.last_summary_ns = now_ns;
        let frame_fields = summarize(&self.intervals_ns[..self.length]);
        let external = self.snapshot;
        self.snapshot = OverlaySnapshot {
            cpu_percent: external.cpu_percent,
            cpu_temperature_c: external.cpu_temperature_c,
            gpu_percent: external.gpu_percent,
            gpu_temperature_c: external.gpu_temperature_c,
            ..frame_fields
        };
        true
    }
}

fn summarize(intervals_ns: &[u64]) -> OverlaySnapshot {
    if intervals_ns.is_empty() {
        return OverlaySnapshot::default();
    }
    let mut sorted = [0_u64; FRAME_HISTORY_CAPACITY];
    sorted[..intervals_ns.len()].copy_from_slice(intervals_ns);
    sorted[..intervals_ns.len()].sort_unstable_by(|left, right| right.cmp(left));

    let sum = intervals_ns
        .iter()
        .fold(0_u128, |total, value| total + u128::from(*value));
    let average =
        u128::from(1_000_000_000_u64).saturating_mul(intervals_ns.len() as u128) / sum.max(1);
    let one_count = intervals_ns.len().div_ceil(100).max(1);
    let point_one_count = intervals_ns.len().div_ceil(1_000).max(1);
    let newest = intervals_ns[intervals_ns.len() - 1];

    OverlaySnapshot {
        fps: Some(clamp_metric(average)),
        frame_time_tenths_ms: Some(clamp_metric(u128::from(newest) / 100_000)),
        one_percent_low_fps: Some(fps_for_slice(&sorted[..one_count])),
        point_one_percent_low_fps: Some(fps_for_slice(&sorted[..point_one_count])),
        ..OverlaySnapshot::default()
    }
}

fn fps_for_slice(intervals_ns: &[u64]) -> u16 {
    let sum = intervals_ns
        .iter()
        .fold(0_u128, |total, value| total + u128::from(*value));
    clamp_metric(
        u128::from(1_000_000_000_u64).saturating_mul(intervals_ns.len() as u128) / sum.max(1),
    )
}

fn clamp_metric(value: u128) -> u16 {
    u16::try_from(value.min(9_999)).unwrap_or(9_999)
}

#[derive(Clone, Copy)]
struct RectBatch {
    rects: [VkClearRect; MAX_ACCENT_RECTS],
    length: usize,
}

impl Default for RectBatch {
    fn default() -> Self {
        Self {
            rects: [empty_clear_rect(); MAX_ACCENT_RECTS],
            length: 0,
        }
    }
}

impl RectBatch {
    fn push(&mut self, x: i32, y: i32, width: u32, height: u32) {
        if self.length == self.rects.len() || width == 0 || height == 0 {
            return;
        }
        self.rects[self.length] = clear_rect(x, y, width, height);
        self.length += 1;
    }

    fn as_slice(&self) -> &[VkClearRect] {
        &self.rects[..self.length]
    }

    fn scaled_translated(mut self, percent: u8, x: i32, y: i32) -> Self {
        for rect in &mut self.rects[..self.length] {
            *rect = scaled_translated_rect(*rect, percent, x, y);
        }
        self
    }
}

#[derive(Clone, Copy)]
struct GlyphInstance {
    x: i32,
    y: i32,
    scale: u8,
    byte: u8,
    ui_font: bool,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GlyphPushConstants {
    words: [u32; overlay_font::COVERAGE_WORDS],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PanelPushConstants {
    bounds: [f32; 4],
    radius: f32,
    palette: u32,
}

impl PanelPushConstants {
    #[expect(
        clippy::cast_precision_loss,
        reason = "bounded swapchain panel coordinates are exactly representable as Vulkan push-constant floats"
    )]
    fn from_rect(rect: VkRect2d, radius: u32, palette: u32) -> Self {
        Self {
            bounds: [
                rect.offset.x as f32,
                rect.offset.y as f32,
                rect.extent.width as f32,
                rect.extent.height as f32,
            ],
            radius: radius as f32,
            palette,
        }
    }
}

impl GlyphPushConstants {
    const fn from_raster(raster: [u32; overlay_font::COVERAGE_WORDS]) -> Self {
        Self { words: raster }
    }

    #[cfg(test)]
    const fn raster(self) -> [u32; overlay_font::COVERAGE_WORDS] {
        self.words
    }
}

const EMPTY_GLYPH_INSTANCE: GlyphInstance = GlyphInstance {
    x: 0,
    y: 0,
    scale: 1,
    byte: 0,
    ui_font: false,
};

#[derive(Clone, Copy)]
struct GlyphBatch {
    glyphs: [GlyphInstance; MAX_TEXT_GLYPHS],
    length: usize,
}

impl Default for GlyphBatch {
    fn default() -> Self {
        Self {
            glyphs: [EMPTY_GLYPH_INSTANCE; MAX_TEXT_GLYPHS],
            length: 0,
        }
    }
}

impl GlyphBatch {
    fn push(&mut self, instance: GlyphInstance) {
        if self.length < self.glyphs.len() {
            self.glyphs[self.length] = instance;
            self.length += 1;
        }
    }

    fn as_slice(&self) -> &[GlyphInstance] {
        &self.glyphs[..self.length]
    }
}

#[derive(Clone, Copy)]
struct OverlayPlan {
    panel: Option<VkClearRect>,
    panel_radius: u8,
    /// Panel shader palette index. The Replay menu uses its own near-black
    /// surface (index 8) instead of a metric palette panel.
    panel_palette: u32,
    logo_base: RectBatch,
    logo_dark_red: RectBatch,
    accent: RectBatch,
    dividers: RectBatch,
    cursor_shadow: RectBatch,
    cursor: RectBatch,
    pointer_shadow: Option<GlyphInstance>,
    pointer: Option<GlyphInstance>,
    accent_glyphs: GlyphBatch,
    muted_glyphs: GlyphBatch,
    text_glyphs: GlyphBatch,
    highlight_glyphs: GlyphBatch,
    width: u32,
    height: u32,
    corner: OverlayCorner,
    opacity_percent: u8,
    scale_percent: u8,
    placement: OverlayPlacement,
    animation_progress: u16,
    dim_percent: u8,
    palette: OverlayPalette,
    colors: OverlayColors,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OverlayPlacement {
    Corner,
    ReplayMenu,
    SavedNotice,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ReplayMenuView {
    animation_progress: u16,
    status: u8,
    available_seconds: u16,
    capture_fps: u16,
    quality: u8,
    format: u8,
    selected_duration: u8,
    hover_target: u8,
    cursor_x: u16,
    cursor_y: u16,
    save_enabled: bool,
    overlay_shortcut: ReplayShortcutLabel,
    save_shortcut: ReplayShortcutLabel,
}

impl ReplayMenuView {
    /// Map one decoded daemon snapshot onto the bounded menu geometry. The
    /// status byte keeps the protocol numbering; the renderer's label match
    /// treats Buffering as the active replay state.
    fn from_telemetry(menu: ReplayMenuTelemetry, animation_progress: u16) -> Self {
        Self {
            animation_progress,
            status: match menu.status {
                ReplayMenuStatus::Unavailable => 0,
                ReplayMenuStatus::Inactive => 1,
                ReplayMenuStatus::Buffering => 2,
                ReplayMenuStatus::Saving => 3,
                ReplayMenuStatus::Failed => 4,
            },
            available_seconds: menu.available_seconds,
            capture_fps: u16::from(menu.frame_rate),
            quality: menu.quality,
            format: menu.output_format,
            selected_duration: menu.selected_duration_index,
            hover_target: menu.hover_target,
            cursor_x: menu.cursor_x,
            cursor_y: menu.cursor_y,
            save_enabled: menu.save_enabled,
            overlay_shortcut: menu.overlay_shortcut,
            save_shortcut: menu.save_shortcut,
        }
    }
}

impl OverlayPlan {
    fn hidden() -> Self {
        Self {
            panel: None,
            panel_radius: METRIC_PANEL_RADIUS,
            panel_palette: OverlayPalette::Redunar.code(),
            logo_base: RectBatch::default(),
            logo_dark_red: RectBatch::default(),
            accent: RectBatch::default(),
            dividers: RectBatch::default(),
            cursor_shadow: RectBatch::default(),
            cursor: RectBatch::default(),
            pointer_shadow: None,
            pointer: None,
            accent_glyphs: GlyphBatch::default(),
            muted_glyphs: GlyphBatch::default(),
            text_glyphs: GlyphBatch::default(),
            highlight_glyphs: GlyphBatch::default(),
            width: 1,
            height: 1,
            corner: OverlayCorner::TopLeft,
            opacity_percent: 0,
            scale_percent: 100,
            placement: OverlayPlacement::Corner,
            animation_progress: 0,
            dim_percent: 0,
            palette: OverlayPalette::Redunar,
            colors: OverlayPalette::Redunar.colors(),
        }
    }

    fn is_empty(&self) -> bool {
        self.panel.is_none()
            && self.logo_base.length == 0
            && self.logo_dark_red.length == 0
            && self.accent.length == 0
            && self.dividers.length == 0
            && self.cursor_shadow.length == 0
            && self.cursor.length == 0
            && self.pointer_shadow.is_none()
            && self.pointer.is_none()
            && self.accent_glyphs.length == 0
            && self.muted_glyphs.length == 0
            && self.text_glyphs.length == 0
            && self.highlight_glyphs.length == 0
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the fixed metric-plan construction keeps row geometry and its bounded glyph batches together"
    )]
    fn new(snapshot: OverlaySnapshot, config: OverlayConfig) -> Self {
        if config.preset == OverlayPreset::FpsOnly {
            let accent = RectBatch::default();
            let mut accent_glyphs = GlyphBatch::default();
            let text_glyphs = GlyphBatch::default();
            let mut row = FixedText::<12>::default();
            row.push_bytes(b"FPS ");
            row.push_metric_unpadded(snapshot.fps, 3);
            push_text_scaled(&mut accent_glyphs, row.as_bytes(), 0, 0, 2);
            return Self {
                panel: None,
                panel_radius: 0,
                panel_palette: config.palette.code(),
                logo_base: RectBatch::default(),
                logo_dark_red: RectBatch::default(),
                accent,
                dividers: RectBatch::default(),
                cursor_shadow: RectBatch::default(),
                cursor: RectBatch::default(),
                pointer_shadow: None,
                pointer: None,
                accent_glyphs,
                muted_glyphs: GlyphBatch::default(),
                text_glyphs,
                highlight_glyphs: GlyphBatch::default(),
                width: FPS_ONLY_WIDTH,
                height: overlay_font::GLYPH_HEIGHT.saturating_mul(2),
                corner: config.corner,
                opacity_percent: config.opacity_percent,
                scale_percent: config.scale_percent,
                placement: OverlayPlacement::Corner,
                animation_progress: u16::MAX,
                dim_percent: 0,
                palette: config.palette,
                colors: config.palette.colors(),
            };
        }

        let mut metrics = match config.preset {
            OverlayPreset::Compact => METRICS_COMPACT,
            OverlayPreset::FpsOnly => METRIC_FPS,
            OverlayPreset::Detailed => METRICS_KNOWN,
            OverlayPreset::Custom => config.metrics,
        };
        // Hardware labels are omitted until the shared monitor has supplied a
        // real value. A missing sensor is not rendered as a synthetic zero or
        // a row of placeholders.
        if snapshot.cpu_percent.is_none() {
            metrics &= !METRIC_CPU_LOAD;
        }
        if snapshot.cpu_temperature_c.is_none() {
            metrics &= !METRIC_CPU_TEMPERATURE;
        }
        if snapshot.gpu_percent.is_none() {
            metrics &= !METRIC_GPU_LOAD;
        }
        if snapshot.gpu_temperature_c.is_none() {
            metrics &= !METRIC_GPU_TEMPERATURE;
        }
        let frame_row = metrics & (METRIC_FPS | METRIC_FRAME_TIME) != 0;
        let low_row = metrics & (METRIC_ONE_PERCENT_LOW | METRIC_POINT_ONE_PERCENT_LOW) != 0;
        let hardware_row = metrics
            & (METRIC_CPU_LOAD | METRIC_CPU_TEMPERATURE | METRIC_GPU_LOAD | METRIC_GPU_TEMPERATURE)
            != 0;
        match config.layout {
            OverlayLayout::Ribbon => return Self::ribbon(snapshot, metrics, config),
            OverlayLayout::Telemetry => return Self::telemetry(snapshot, metrics, config),
            OverlayLayout::Grid => {}
        }
        let panel_height = PANEL_BASE_HEIGHT
            .saturating_add(u32::from(frame_row).saturating_mul(PRIMARY_ROW_HEIGHT))
            .saturating_add(
                (u32::from(low_row) + u32::from(hardware_row)).saturating_mul(SECONDARY_ROW_HEIGHT),
            );
        let mut plan = Self {
            panel: Some(clear_rect(0, 0, PANEL_WIDTH, panel_height)),
            panel_radius: METRIC_PANEL_RADIUS,
            panel_palette: config.palette.code(),
            logo_base: RectBatch::default(),
            logo_dark_red: RectBatch::default(),
            accent: RectBatch::default(),
            dividers: RectBatch::default(),
            cursor_shadow: RectBatch::default(),
            cursor: RectBatch::default(),
            pointer_shadow: None,
            pointer: None,
            accent_glyphs: GlyphBatch::default(),
            muted_glyphs: GlyphBatch::default(),
            text_glyphs: GlyphBatch::default(),
            highlight_glyphs: GlyphBatch::default(),
            width: PANEL_WIDTH,
            height: panel_height,
            corner: config.corner,
            opacity_percent: config.opacity_percent,
            scale_percent: config.scale_percent,
            placement: OverlayPlacement::Corner,
            animation_progress: u16::MAX,
            dim_percent: 0,
            palette: config.palette,
            colors: config.palette.colors(),
        };
        plan.accent.push(0, 0, 3, panel_height);
        push_panel_grid(&mut plan.dividers, panel_height);
        if config.branding_visible {
            push_text(&mut plan.accent_glyphs, b"REDUNAR", 8, 6);
        }

        let mut y = FIRST_METRIC_ROW_Y;
        if frame_row {
            push_frame_metrics(
                &mut plan.text_glyphs,
                &mut plan.muted_glyphs,
                snapshot,
                metrics,
                y,
            );
            y = y.saturating_add(i32::try_from(PRIMARY_ROW_HEIGHT).unwrap_or(i32::MAX));
            if low_row || hardware_row {
                plan.dividers
                    .push(12, y - ROW_DIVIDER_OFFSET, PANEL_WIDTH - 24, 1);
            }
        }

        if low_row {
            push_low_metrics(&mut plan.muted_glyphs, snapshot, metrics, y);
            y = y.saturating_add(i32::try_from(SECONDARY_ROW_HEIGHT).unwrap_or(i32::MAX));
            if hardware_row {
                plan.dividers
                    .push(12, y - ROW_DIVIDER_OFFSET, PANEL_WIDTH - 24, 1);
            }
        }

        if hardware_row {
            push_hardware_metrics(&mut plan.muted_glyphs, snapshot, metrics, y);
        }
        plan
    }

    fn metric_panel(width: u32, height: u32, config: OverlayConfig) -> Self {
        let mut plan = Self {
            panel: Some(clear_rect(0, 0, width, height)),
            panel_radius: METRIC_PANEL_RADIUS,
            panel_palette: config.palette.code(),
            logo_base: RectBatch::default(),
            logo_dark_red: RectBatch::default(),
            accent: RectBatch::default(),
            dividers: RectBatch::default(),
            cursor_shadow: RectBatch::default(),
            cursor: RectBatch::default(),
            pointer_shadow: None,
            pointer: None,
            accent_glyphs: GlyphBatch::default(),
            muted_glyphs: GlyphBatch::default(),
            text_glyphs: GlyphBatch::default(),
            highlight_glyphs: GlyphBatch::default(),
            width,
            height,
            corner: config.corner,
            opacity_percent: config.opacity_percent,
            scale_percent: config.scale_percent,
            placement: OverlayPlacement::Corner,
            animation_progress: u16::MAX,
            dim_percent: 0,
            palette: config.palette,
            colors: config.palette.colors(),
        };
        plan.accent.push(0, 0, 3, height);
        plan
    }

    fn ribbon(snapshot: OverlaySnapshot, metrics: u16, config: OverlayConfig) -> Self {
        let metric_count = u32::try_from(
            metric_labels()
                .into_iter()
                .filter(|(bit, _)| metrics & *bit != 0)
                .count(),
        )
        .unwrap_or(0);
        let brand_width = if config.branding_visible {
            RIBBON_BRAND_WIDTH
        } else {
            0
        };
        let width = brand_width.saturating_add(metric_count.saturating_mul(RIBBON_METRIC_WIDTH));
        let mut plan = Self::metric_panel(width, RIBBON_HEIGHT, config);
        if config.branding_visible {
            push_text(&mut plan.accent_glyphs, b"REDUNAR", 10, 15);
        }
        let mut index = 0_i32;
        for (bit, label) in metric_labels() {
            if metrics & bit == 0 {
                continue;
            }
            let left = i32::try_from(brand_width).unwrap_or(94)
                + 8
                + index * i32::try_from(RIBBON_METRIC_WIDTH).unwrap_or(89);
            plan.dividers.push(left - 8, 6, 1, RIBBON_HEIGHT - 12);
            push_text_advance(&mut plan.muted_glyphs, label, left, 6, RIBBON_LABEL_ADVANCE);
            let value = metric_value(snapshot, bit);
            push_text(&mut plan.text_glyphs, value.as_bytes(), left, 22);
            index += 1;
        }
        plan
    }

    fn telemetry(snapshot: OverlaySnapshot, metrics: u16, config: OverlayConfig) -> Self {
        let count = u32::try_from(
            metric_labels()
                .into_iter()
                .filter(|(bit, _)| metrics & *bit != 0)
                .count(),
        )
        .unwrap_or(0);
        let height = PANEL_BASE_HEIGHT + count.saturating_mul(TELEMETRY_ROW_HEIGHT);
        let mut plan = Self::metric_panel(TELEMETRY_WIDTH, height, config);
        if config.branding_visible {
            push_text(&mut plan.accent_glyphs, b"REDUNAR", 8, 6);
        }
        let mut heading = FixedText::<16>::default();
        heading.push_bytes(b"FRAME METRICS");
        let heading_x = i32::try_from(TELEMETRY_WIDTH).unwrap_or(300)
            - 8
            - i32::try_from(heading.length).unwrap_or(0) * overlay_font::GLYPH_ADVANCE;
        push_text(&mut plan.muted_glyphs, heading.as_bytes(), heading_x, 6);
        plan.dividers.push(3, 24, TELEMETRY_WIDTH - 3, 1);
        let mut row = 0_i32;
        for (bit, label) in metric_labels() {
            if metrics & bit == 0 {
                continue;
            }
            let y = 31 + row * i32::try_from(TELEMETRY_ROW_HEIGHT).unwrap_or(24);
            push_text(&mut plan.muted_glyphs, label, 8, y);
            let value = metric_value(snapshot, bit);
            let x = i32::try_from(TELEMETRY_WIDTH).unwrap_or(300)
                - 8
                - i32::try_from(value.length).unwrap_or(0) * overlay_font::GLYPH_ADVANCE;
            push_text(&mut plan.text_glyphs, value.as_bytes(), x, y);
            row += 1;
            if u32::try_from(row).unwrap_or(0) < count {
                plan.dividers.push(
                    8,
                    y + i32::try_from(TELEMETRY_ROW_HEIGHT).unwrap_or(24) - 7,
                    TELEMETRY_WIDTH - 16,
                    1,
                );
            }
        }
        plan
    }

    fn replay_menu(menu: ReplayMenuView) -> Self {
        let mut plan = Self {
            panel: Some(clear_rect(0, 0, REPLAY_MENU_WIDTH, REPLAY_MENU_HEIGHT)),
            panel_radius: REPLAY_MENU_CORNER_RADIUS,
            panel_palette: MENU_PANEL_PALETTE,
            logo_base: RectBatch::default(),
            logo_dark_red: RectBatch::default(),
            accent: RectBatch::default(),
            dividers: RectBatch::default(),
            cursor_shadow: RectBatch::default(),
            cursor: RectBatch::default(),
            pointer_shadow: None,
            pointer: None,
            accent_glyphs: GlyphBatch::default(),
            muted_glyphs: GlyphBatch::default(),
            text_glyphs: GlyphBatch::default(),
            highlight_glyphs: GlyphBatch::default(),
            width: REPLAY_MENU_WIDTH,
            height: REPLAY_MENU_HEIGHT,
            corner: OverlayCorner::TopLeft,
            opacity_percent: 96,
            // Match the desktop design's visual weight at common 1080p and
            // 1440p game resolutions. `effective_scale_percent` still fits
            // this down on smaller swapchains.
            scale_percent: 160,
            placement: OverlayPlacement::ReplayMenu,
            animation_progress: menu.animation_progress,
            dim_percent: u8::try_from(
                u32::from(menu.animation_progress) * 45 / u32::from(u16::MAX),
            )
            .unwrap_or(45),
            palette: OverlayPalette::Redunar,
            colors: OverlayColors::replay_menu(),
        };
        push_replay_menu_grid(&mut plan, menu);
        push_replay_menu_text(&mut plan, menu);
        let cursor_x = normalized_coordinate(menu.cursor_x, REPLAY_MENU_WIDTH);
        let cursor_y = normalized_coordinate(menu.cursor_y, REPLAY_MENU_HEIGHT);
        push_replay_pointer(&mut plan, cursor_x, cursor_y);
        plan
    }
}

fn push_replay_pointer(plan: &mut OverlayPlan, x: i32, y: i32) {
    let pointer = GlyphInstance {
        x,
        y,
        scale: 1,
        byte: u8::MAX,
        ui_font: false,
    };
    plan.pointer = Some(pointer);
    plan.pointer_shadow = Some(GlyphInstance {
        byte: u8::MAX - 1,
        ..pointer
    });
}

fn normalized_coordinate(value: u16, extent: u32) -> i32 {
    i32::try_from(u32::from(value.min(10_000)).saturating_mul(extent) / 10_000).unwrap_or(0)
}

#[cfg(test)]
fn replay_menu_hit_target(x: i32, y: i32) -> u8 {
    if x >= REPLAY_MENU_DURATION_X
        && x < REPLAY_MENU_DURATION_X + i32::try_from(REPLAY_MENU_DURATION_WIDTH).unwrap_or(0)
        && y >= REPLAY_MENU_DURATION_Y
        && y < REPLAY_MENU_DURATION_Y + i32::try_from(REPLAY_MENU_DURATION_HEIGHT).unwrap_or(0)
    {
        let column = (x - REPLAY_MENU_DURATION_X) / 118;
        let row = (y - REPLAY_MENU_DURATION_Y) / 42;
        return u8::try_from(row * 4 + column + 1).unwrap_or(0);
    }
    if x >= REPLAY_MENU_SAVE_X
        && x < REPLAY_MENU_SAVE_X + i32::try_from(REPLAY_MENU_SAVE_WIDTH).unwrap_or(0)
        && (REPLAY_MENU_SAVE_Y
            ..REPLAY_MENU_SAVE_Y + i32::try_from(REPLAY_MENU_SAVE_HEIGHT).unwrap_or(0))
            .contains(&y)
    {
        return 9;
    }
    if (REPLAY_MENU_FORMAT_X..REPLAY_MENU_FORMAT_X + 90).contains(&x)
        && (REPLAY_MENU_FORMAT_Y
            ..REPLAY_MENU_FORMAT_Y + i32::try_from(REPLAY_MENU_FORMAT_HEIGHT).unwrap_or(0))
            .contains(&y)
    {
        return 10;
    }
    if (REPLAY_MENU_FORMAT_X + 90..REPLAY_MENU_FORMAT_X + 180).contains(&x)
        && (REPLAY_MENU_FORMAT_Y
            ..REPLAY_MENU_FORMAT_Y + i32::try_from(REPLAY_MENU_FORMAT_HEIGHT).unwrap_or(0))
            .contains(&y)
    {
        return 11;
    }
    0
}

#[allow(clippy::too_many_lines)]
fn push_replay_menu_grid(plan: &mut OverlayPlan, menu: ReplayMenuView) {
    let width = REPLAY_MENU_WIDTH;
    let facts_bottom = REPLAY_MENU_FACTS_HEIGHT;
    plan.dividers
        .push(0, i32::try_from(facts_bottom).unwrap_or(138), width, 1);
    for x in [173, 346] {
        plan.dividers.push(x, 0, 1, REPLAY_MENU_FACTS_HEIGHT);
    }
    let cell_width = REPLAY_MENU_DURATION_WIDTH / 4;
    let cell_height = REPLAY_MENU_DURATION_HEIGHT / 2;
    push_rounded_outline(
        &mut plan.dividers,
        REPLAY_MENU_DURATION_X,
        REPLAY_MENU_DURATION_Y,
        REPLAY_MENU_DURATION_WIDTH,
        REPLAY_MENU_DURATION_HEIGHT,
        REPLAY_MENU_CONTROL_RADIUS,
    );
    for column in 1..4 {
        let x = REPLAY_MENU_DURATION_X + i32::try_from(column * cell_width).unwrap_or_default();
        plan.dividers.push(
            x,
            REPLAY_MENU_DURATION_Y + i32::try_from(REPLAY_MENU_CONTROL_RADIUS).unwrap_or(0),
            1,
            REPLAY_MENU_DURATION_HEIGHT - REPLAY_MENU_CONTROL_RADIUS * 2,
        );
    }
    plan.dividers.push(
        REPLAY_MENU_DURATION_X + i32::try_from(REPLAY_MENU_CONTROL_RADIUS).unwrap_or(0),
        REPLAY_MENU_DURATION_Y + i32::try_from(cell_height).unwrap_or(0),
        REPLAY_MENU_DURATION_WIDTH - REPLAY_MENU_CONTROL_RADIUS * 2,
        1,
    );
    let selected = u32::from(menu.selected_duration.min(7));
    let selected_x = REPLAY_MENU_DURATION_X + i32::try_from(selected % 4 * cell_width).unwrap_or(0);
    let selected_y =
        REPLAY_MENU_DURATION_Y + i32::try_from(selected / 4 * cell_height).unwrap_or(0);
    plan.logo_dark_red.push(
        selected_x + 2,
        selected_y + 2,
        cell_width - 4,
        cell_height - 4,
    );
    plan.accent.push(
        selected_x + 8,
        selected_y + i32::try_from(cell_height).unwrap_or(0) - 3,
        cell_width - 16,
        2,
    );
    if menu.save_enabled {
        plan.logo_dark_red.push(
            REPLAY_MENU_SAVE_X + 2,
            REPLAY_MENU_SAVE_Y + 2,
            REPLAY_MENU_SAVE_WIDTH - 4,
            REPLAY_MENU_SAVE_HEIGHT - 4,
        );
        push_rounded_outline(
            &mut plan.accent,
            REPLAY_MENU_SAVE_X,
            REPLAY_MENU_SAVE_Y,
            REPLAY_MENU_SAVE_WIDTH,
            REPLAY_MENU_SAVE_HEIGHT,
            REPLAY_MENU_CONTROL_RADIUS,
        );
    } else {
        push_rounded_outline(
            &mut plan.dividers,
            REPLAY_MENU_SAVE_X,
            REPLAY_MENU_SAVE_Y,
            REPLAY_MENU_SAVE_WIDTH,
            REPLAY_MENU_SAVE_HEIGHT,
            REPLAY_MENU_CONTROL_RADIUS,
        );
    }

    push_rounded_outline(
        &mut plan.dividers,
        REPLAY_MENU_FORMAT_X,
        REPLAY_MENU_FORMAT_Y,
        REPLAY_MENU_FORMAT_WIDTH,
        REPLAY_MENU_FORMAT_HEIGHT,
        REPLAY_MENU_CONTROL_RADIUS,
    );
    plan.dividers.push(
        REPLAY_MENU_FORMAT_X + 90,
        REPLAY_MENU_FORMAT_Y + 6,
        1,
        REPLAY_MENU_FORMAT_HEIGHT - 12,
    );
    let format_left = if menu.format == 0 {
        REPLAY_MENU_FORMAT_X
    } else {
        REPLAY_MENU_FORMAT_X + 90
    };
    plan.logo_dark_red.push(
        format_left + 2,
        REPLAY_MENU_FORMAT_Y + 2,
        86,
        REPLAY_MENU_FORMAT_HEIGHT - 4,
    );
    plan.accent
        .push(format_left + 8, REPLAY_MENU_FORMAT_Y + 43, 74, 2);
    if (1..=8).contains(&menu.hover_target) && menu.hover_target != menu.selected_duration + 1 {
        let hover = u32::from(menu.hover_target - 1);
        plan.accent.push(
            REPLAY_MENU_DURATION_X + i32::try_from(hover % 4 * cell_width).unwrap_or(0),
            REPLAY_MENU_DURATION_Y
                + i32::try_from(hover / 4 * cell_height + cell_height).unwrap_or(0)
                - 2,
            cell_width,
            2,
        );
    }
    if matches!(menu.hover_target, 10 | 11) && menu.hover_target != menu.format + 10 {
        let x = REPLAY_MENU_FORMAT_X + i32::from(menu.hover_target - 10) * 90;
        plan.accent.push(
            x + 8,
            REPLAY_MENU_FORMAT_Y + i32::try_from(REPLAY_MENU_FORMAT_HEIGHT).unwrap_or(0) - 3,
            74,
            2,
        );
    }
}

#[allow(clippy::too_many_lines)]
fn push_replay_menu_text(plan: &mut OverlayPlan, menu: ReplayMenuView) {
    let mut buffered = FixedText::<12>::default();
    match menu.status {
        0 => buffered.push_bytes(b"N/A"),
        1 => buffered.push_bytes(b"OFF"),
        3 => buffered.push_bytes(b"SAVING"),
        4 => buffered.push_bytes(b"ERROR"),
        _ => {
            buffered.push_unsigned(menu.available_seconds, 1);
            buffered.push_bytes(b" SEC");
        }
    }

    for (left, right, label) in [
        (0, 173, b"CAPTURE".as_slice()),
        (173, 346, b"QUALITY".as_slice()),
        (346, 520, b"BUFFER".as_slice()),
    ] {
        push_text_centered_tracked(&mut plan.muted_glyphs, label, left, right, 11);
    }
    let capture = match menu.capture_fps {
        30 => b"30 FPS".as_slice(),
        60 => b"60 FPS".as_slice(),
        120 => b"120 FPS".as_slice(),
        _ => b"-- FPS".as_slice(),
    };
    push_ui_text_centered(&mut plan.text_glyphs, capture, 0, 173, 30, 2);
    let quality = match menu.quality {
        0 => b"EFFICIENT".as_slice(),
        1 => b"BALANCED".as_slice(),
        _ => b"HIGH".as_slice(),
    };
    push_ui_text_centered(&mut plan.text_glyphs, quality, 173, 346, 30, 2);
    push_ui_text_centered(&mut plan.text_glyphs, buffered.as_bytes(), 346, 520, 30, 2);

    push_ui_text_scaled(&mut plan.text_glyphs, b"Instant Replay", 24, 77, 2);
    let durations: [&[u8]; 8] = [
        b"15 SEC", b"30 SEC", b"1 MIN", b"2 MIN", b"3 MIN", b"5 MIN", b"10 MIN", b"15 MIN",
    ];
    for (index, label) in durations.into_iter().enumerate() {
        let left = REPLAY_MENU_DURATION_X + i32::try_from(index % 4).unwrap_or(0) * 118;
        let right = left + 118;
        let y = 139 + i32::try_from(index / 4).unwrap_or(0) * 42;
        if index == usize::from(menu.selected_duration.min(7)) {
            push_text_centered_tracked(&mut plan.highlight_glyphs, label, left, right, y);
        } else {
            push_text_centered_tracked(&mut plan.text_glyphs, label, left, right, y);
        }
    }
    let save_label = match menu.selected_duration.min(7) {
        0 => b"SAVE LAST 15 SEC".as_slice(),
        1 => b"SAVE LAST 30 SEC".as_slice(),
        2 => b"SAVE LAST 1 MIN".as_slice(),
        3 => b"SAVE LAST 2 MIN".as_slice(),
        4 => b"SAVE LAST 3 MIN".as_slice(),
        5 => b"SAVE LAST 5 MIN".as_slice(),
        6 => b"SAVE LAST 10 MIN".as_slice(),
        _ => b"SAVE LAST 15 MIN".as_slice(),
    };
    push_ui_text_centered_tracked(
        if menu.save_enabled {
            &mut plan.highlight_glyphs
        } else {
            &mut plan.muted_glyphs
        },
        save_label,
        REPLAY_MENU_SAVE_X,
        REPLAY_MENU_SAVE_X + i32::try_from(REPLAY_MENU_SAVE_WIDTH).unwrap_or(0),
        237,
        1,
    );
    push_ui_text_centered_tracked(&mut plan.text_glyphs, b"MKV", 24, 114, 237, 1);
    push_ui_text_centered_tracked(&mut plan.text_glyphs, b"MP4", 114, 204, 237, 1);
}

fn push_rounded_outline(
    batch: &mut RectBatch,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    radius: u32,
) {
    let radius = radius.min(width / 2).min(height / 2).max(2);
    let inset = radius.div_ceil(2).max(2);
    let right = x + i32::try_from(width).unwrap_or(0) - 1;
    let bottom = y + i32::try_from(height).unwrap_or(0) - 1;
    let inset_i32 = i32::try_from(inset).unwrap_or(0);
    let shoulder = inset - 1;
    let shoulder_i32 = i32::try_from(shoulder).unwrap_or(0);
    batch.push(x + inset_i32, y, width - inset * 2, 1);
    batch.push(x + inset_i32, bottom, width - inset * 2, 1);
    batch.push(x, y + shoulder_i32, 1, height - shoulder * 2);
    batch.push(right, y + shoulder_i32, 1, height - shoulder * 2);
    batch.push(x + 1, y + 1, shoulder, 1);
    batch.push(right - inset_i32 + 1, y + 1, shoulder, 1);
    batch.push(x + 1, bottom - 1, shoulder, 1);
    batch.push(right - inset_i32 + 1, bottom - 1, shoulder, 1);
}

fn push_panel_grid(dividers: &mut RectBatch, panel_height: u32) {
    // A complete one-pixel frame makes the injected renderer use the same
    // connected-grid language as the desktop surfaces. These bounded clears
    // avoid geometry buffers and add no per-frame allocations.
    let panel_bottom = i32::try_from(panel_height).unwrap_or(i32::MAX) - 1;
    let panel_right = i32::try_from(PANEL_WIDTH).unwrap_or(i32::MAX) - 1;
    let header_bottom = i32::try_from(PANEL_BASE_HEIGHT).unwrap_or(i32::MAX) - 1;
    dividers.push(0, 0, PANEL_WIDTH, 1);
    dividers.push(0, panel_bottom, PANEL_WIDTH, 1);
    dividers.push(0, 0, 1, panel_height);
    dividers.push(panel_right, 0, 1, panel_height);
    dividers.push(3, header_bottom, PANEL_WIDTH - 3, 1);
}

fn push_frame_metrics(
    values: &mut GlyphBatch,
    labels: &mut GlyphBatch,
    snapshot: OverlaySnapshot,
    metrics: u16,
    y: i32,
) {
    if metrics & METRIC_FPS != 0 {
        let mut value = FixedText::<6>::default();
        value.push_metric_unpadded(snapshot.fps, 3);
        push_text_scaled(values, value.as_bytes(), 12, y, 2);
        let label_x = 12_i32.saturating_add(
            i32::try_from(value.length)
                .unwrap_or(4)
                .saturating_mul(overlay_font::GLYPH_ADVANCE.saturating_mul(2)),
        );
        push_text(labels, b"FPS", label_x + 4, y + 9);
    }
    if metrics & METRIC_FRAME_TIME != 0 {
        let mut value = FixedText::<8>::default();
        value.push_tenths(snapshot.frame_time_tenths_ms);
        let value_width = i32::try_from(value.length)
            .unwrap_or(4)
            .saturating_mul(overlay_font::GLYPH_ADVANCE.saturating_mul(2));
        let start_x = 184_i32.min(
            i32::try_from(PANEL_WIDTH)
                .unwrap_or(i32::MAX)
                .saturating_sub(12 + value_width + 4 + overlay_font::GLYPH_ADVANCE * 2),
        );
        push_text_scaled(values, value.as_bytes(), start_x, y, 2);
        let label_x = start_x.saturating_add(value_width);
        push_text(labels, b"MS", label_x + 4, y + 9);
    }
}

fn metric_labels() -> [(u16, &'static [u8]); 8] {
    [
        (METRIC_FPS, b"FPS"),
        (METRIC_FRAME_TIME, b"FRAME TIME"),
        (METRIC_ONE_PERCENT_LOW, b"1% LOW"),
        (METRIC_POINT_ONE_PERCENT_LOW, b"0.1% LOW"),
        (METRIC_GPU_LOAD, b"GPU LOAD"),
        (METRIC_GPU_TEMPERATURE, b"GPU TEMP"),
        (METRIC_CPU_LOAD, b"CPU LOAD"),
        (METRIC_CPU_TEMPERATURE, b"CPU TEMP"),
    ]
}

fn metric_value(snapshot: OverlaySnapshot, metric: u16) -> FixedText<16> {
    let mut value = FixedText::<16>::default();
    match metric {
        METRIC_FPS => {
            value.push_metric_unpadded(snapshot.fps, 3);
            value.push_bytes(b" FPS");
        }
        METRIC_FRAME_TIME => {
            value.push_tenths(snapshot.frame_time_tenths_ms);
            value.push_bytes(b" MS");
        }
        METRIC_ONE_PERCENT_LOW => {
            value.push_metric_unpadded(snapshot.one_percent_low_fps, 3);
            value.push_bytes(b" FPS");
        }
        METRIC_POINT_ONE_PERCENT_LOW => {
            value.push_metric_unpadded(snapshot.point_one_percent_low_fps, 3);
            value.push_bytes(b" FPS");
        }
        METRIC_GPU_LOAD => {
            value.push_metric_unpadded(snapshot.gpu_percent.map(u16::from), 2);
            value.push(b'%');
        }
        METRIC_GPU_TEMPERATURE => {
            value.push_metric_unpadded(
                snapshot
                    .gpu_temperature_c
                    .and_then(|v| u16::try_from(v).ok()),
                2,
            );
            value.push_bytes(b"^C");
        }
        METRIC_CPU_LOAD => {
            value.push_metric_unpadded(snapshot.cpu_percent.map(u16::from), 2);
            value.push(b'%');
        }
        METRIC_CPU_TEMPERATURE => {
            value.push_metric_unpadded(
                snapshot
                    .cpu_temperature_c
                    .and_then(|v| u16::try_from(v).ok()),
                2,
            );
            value.push_bytes(b"^C");
        }
        _ => value.push_bytes(b"--"),
    }
    value
}

fn push_low_metrics(glyphs: &mut GlyphBatch, snapshot: OverlaySnapshot, metrics: u16, y: i32) {
    if metrics & METRIC_ONE_PERCENT_LOW != 0 {
        let mut left = FixedText::<16>::default();
        left.push_bytes(b"1% LOW ");
        left.push_metric_unpadded(snapshot.one_percent_low_fps, 3);
        push_text(glyphs, left.as_bytes(), 8, y);
    }
    if metrics & METRIC_POINT_ONE_PERCENT_LOW != 0 {
        let mut right = FixedText::<16>::default();
        right.push_bytes(b"0.1% LOW ");
        right.push_metric_unpadded(snapshot.point_one_percent_low_fps, 3);
        push_text(glyphs, right.as_bytes(), right_aligned_x(&right, 8), y);
    }
}

fn push_hardware_metrics(glyphs: &mut GlyphBatch, snapshot: OverlaySnapshot, metrics: u16, y: i32) {
    let mut cpu = FixedText::<16>::default();
    push_device_metrics(
        &mut cpu,
        b"CPU",
        metrics & METRIC_CPU_LOAD != 0,
        metrics & METRIC_CPU_TEMPERATURE != 0,
        snapshot.cpu_percent,
        snapshot.cpu_temperature_c,
    );
    if !cpu.as_bytes().is_empty() {
        push_text(glyphs, cpu.as_bytes(), 8, y);
    }
    let mut gpu = FixedText::<16>::default();
    push_device_metrics(
        &mut gpu,
        b"GPU",
        metrics & METRIC_GPU_LOAD != 0,
        metrics & METRIC_GPU_TEMPERATURE != 0,
        snapshot.gpu_percent,
        snapshot.gpu_temperature_c,
    );
    if !gpu.as_bytes().is_empty() {
        push_text(glyphs, gpu.as_bytes(), right_aligned_x(&gpu, 8), y);
    }
}

fn right_aligned_x<const N: usize>(text: &FixedText<N>, margin: i32) -> i32 {
    let width = i32::try_from(text.length)
        .unwrap_or(i32::MAX)
        .saturating_mul(overlay_font::GLYPH_ADVANCE);
    i32::try_from(PANEL_WIDTH)
        .unwrap_or(i32::MAX)
        .saturating_sub(margin)
        .saturating_sub(width)
        .max(margin)
}

fn push_device_metrics<const N: usize>(
    row: &mut FixedText<N>,
    label: &[u8],
    show_load: bool,
    show_temperature: bool,
    load: Option<u8>,
    temperature: Option<i16>,
) {
    if !show_load && !show_temperature {
        return;
    }
    separate_metric(row);
    row.push_bytes(label);
    if show_load {
        row.push(b' ');
        row.push_metric(load.map(u16::from), 2);
        row.push(b'%');
    }
    if show_temperature {
        row.push(if show_load { b'~' } else { b' ' });
        row.push_metric(temperature.and_then(|value| u16::try_from(value).ok()), 2);
        row.push(b'^');
        row.push(b'C');
    }
}

fn separate_metric<const N: usize>(row: &mut FixedText<N>) {
    if row.length > 0 {
        row.push_bytes(b"  ");
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum OverlayPreset {
    #[default]
    Compact,
    FpsOnly,
    Detailed,
    Custom,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum OverlayLayout {
    #[default]
    Grid,
    Ribbon,
    Telemetry,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum OverlayPalette {
    #[default]
    Redunar,
    Glacier,
    Ember,
    Mint,
    Mono,
    Amethyst,
    Solar,
    Rose,
}

impl OverlayPalette {
    const fn code(self) -> u32 {
        match self {
            Self::Redunar => 0,
            Self::Glacier => 1,
            Self::Ember => 2,
            Self::Mint => 3,
            Self::Mono => 4,
            Self::Amethyst => 5,
            Self::Solar => 6,
            Self::Rose => 7,
        }
    }

    const fn colors(self) -> OverlayColors {
        match self {
            Self::Redunar => OverlayColors::new(
                [0.807, 0.063, 0.078, 1.0],
                [0.356, 0.356, 0.392, 1.0],
                [0.888, 0.871, 0.847, 1.0],
                [0.03, 0.03, 0.037, 1.0],
            ),
            Self::Glacier => OverlayColors::new(
                [0.112, 0.571, 0.791, 1.0],
                [0.342, 0.479, 0.521, 1.0],
                [0.913, 0.956, 0.973, 1.0],
                [0.022, 0.051, 0.063, 1.0],
            ),
            Self::Ember => OverlayColors::new(
                [0.888, 0.381, 0.068, 1.0],
                [0.539, 0.418, 0.296, 1.0],
                [1.0, 0.93, 0.871, 1.0],
                [0.06, 0.04, 0.024, 1.0],
            ),
            Self::Mint => OverlayColors::new(
                [0.171, 0.672, 0.356, 1.0],
                [0.332, 0.491, 0.392, 1.0],
                [0.888, 1.0, 0.93, 1.0],
                [0.022, 0.054, 0.033, 1.0],
            ),
            Self::Mono => OverlayColors::new(
                [0.807, 0.807, 0.807, 1.0],
                [0.407, 0.407, 0.407, 1.0],
                [0.93, 0.93, 0.93, 1.0],
                [0.04, 0.04, 0.04, 1.0],
            ),
            Self::Amethyst => OverlayColors::new(
                [0.434, 0.262, 1.0, 1.0],
                [0.462, 0.392, 0.552, 1.0],
                [0.956, 0.93, 1.0, 1.0],
                [0.047, 0.033, 0.068, 1.0],
            ),
            Self::Solar => OverlayColors::new(
                [0.888, 0.658, 0.107, 1.0],
                [0.539, 0.479, 0.275, 1.0],
                [1.0, 0.973, 0.847, 1.0],
                [0.063, 0.054, 0.022, 1.0],
            ),
            Self::Rose => OverlayColors::new(
                [1.0, 0.223, 0.418, 1.0],
                [0.552, 0.366, 0.434, 1.0],
                [1.0, 0.93, 0.956, 1.0],
                [0.068, 0.03, 0.044, 1.0],
            ),
        }
    }

    const fn panel_color(self) -> [f32; 4] {
        match self {
            Self::Redunar | Self::Mono => [0.003, 0.003, 0.003, 1.0],
            Self::Glacier => [0.002, 0.006, 0.008, 1.0],
            Self::Ember => [0.006, 0.004, 0.002, 1.0],
            Self::Mint => [0.002, 0.006, 0.004, 1.0],
            Self::Amethyst => [0.004, 0.003, 0.007, 1.0],
            Self::Solar => [0.006, 0.005, 0.002, 1.0],
            Self::Rose => [0.007, 0.003, 0.005, 1.0],
        }
    }
}

#[derive(Clone, Copy)]
struct OverlayColors {
    accent: [f32; 4],
    muted: [f32; 4],
    text: [f32; 4],
    divider: [f32; 4],
    /// Bright selection text. Palettes reuse their accent; the Replay menu
    /// keeps a dedicated crimson highlight beside its control red.
    highlight: [f32; 4],
}

impl OverlayColors {
    const fn new(accent: [f32; 4], muted: [f32; 4], text: [f32; 4], divider: [f32; 4]) -> Self {
        Self {
            accent,
            muted,
            text,
            divider,
            highlight: accent,
        }
    }

    /// The Replay menu's fixed Redunar control colors, converted from the
    /// approved reference surface: #262930 grid, #eb2933 control red, and
    /// #ff5a63 selected text on the #21090b selection fill.
    const fn replay_menu() -> Self {
        Self {
            accent: [0.831, 0.022, 0.033, 1.0],
            muted: [0.275, 0.242, 0.279, 1.0],
            text: [0.888, 0.871, 0.847, 1.0],
            divider: [0.019, 0.022, 0.03, 1.0],
            highlight: [1.0, 0.102, 0.125, 1.0],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum OverlayCorner {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OverlayConfig {
    preset: OverlayPreset,
    layout: OverlayLayout,
    palette: OverlayPalette,
    branding_visible: bool,
    corner: OverlayCorner,
    metrics: u16,
    opacity_percent: u8,
    scale_percent: u8,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            preset: OverlayPreset::default(),
            layout: OverlayLayout::default(),
            palette: OverlayPalette::default(),
            branding_visible: true,
            corner: OverlayCorner::default(),
            metrics: METRICS_COMPACT,
            opacity_percent: 50,
            scale_percent: 100,
        }
    }
}

fn parse_overlay_branding(value: Option<&str>) -> bool {
    !matches!(value, Some("0"))
}

impl OverlayConfig {
    fn from_environment() -> Self {
        Self {
            preset: parse_overlay_preset(env::var(OVERLAY_PRESET_ENV).ok().as_deref()),
            layout: parse_overlay_layout(env::var(OVERLAY_LAYOUT_ENV).ok().as_deref()),
            palette: parse_overlay_palette(env::var(OVERLAY_PALETTE_ENV).ok().as_deref()),
            branding_visible: parse_overlay_branding(
                env::var(OVERLAY_BRANDING_ENV).ok().as_deref(),
            ),
            corner: parse_overlay_corner(env::var(OVERLAY_CORNER_ENV).ok().as_deref()),
            metrics: parse_overlay_metrics(env::var(OVERLAY_METRICS_ENV).ok().as_deref()),
            opacity_percent: parse_overlay_opacity(env::var(OVERLAY_OPACITY_ENV).ok().as_deref()),
            scale_percent: 100,
        }
    }
}

fn parse_overlay_opacity(value: Option<&str>) -> u8 {
    value
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|percent| *percent <= 100)
        .unwrap_or(50)
}

fn parse_overlay_preset(value: Option<&str>) -> OverlayPreset {
    match value {
        Some("fps-only") => OverlayPreset::FpsOnly,
        Some("detailed") => OverlayPreset::Detailed,
        Some("custom") => OverlayPreset::Custom,
        _ => OverlayPreset::Compact,
    }
}

fn parse_overlay_layout(value: Option<&str>) -> OverlayLayout {
    match value {
        Some("ribbon") => OverlayLayout::Ribbon,
        Some("telemetry") => OverlayLayout::Telemetry,
        _ => OverlayLayout::Grid,
    }
}

fn parse_overlay_palette(value: Option<&str>) -> OverlayPalette {
    match value {
        Some("glacier") => OverlayPalette::Glacier,
        Some("ember") => OverlayPalette::Ember,
        Some("mint") => OverlayPalette::Mint,
        Some("mono") => OverlayPalette::Mono,
        Some("amethyst") => OverlayPalette::Amethyst,
        Some("solar") => OverlayPalette::Solar,
        Some("rose") => OverlayPalette::Rose,
        _ => OverlayPalette::Redunar,
    }
}

fn parse_overlay_metrics(value: Option<&str>) -> u16 {
    value
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|bits| *bits != 0 && *bits & !METRICS_KNOWN == 0)
        .unwrap_or(METRICS_COMPACT)
}

fn parse_overlay_corner(value: Option<&str>) -> OverlayCorner {
    match value {
        Some("top-right") => OverlayCorner::TopRight,
        Some("bottom-left") => OverlayCorner::BottomLeft,
        Some("bottom-right") => OverlayCorner::BottomRight,
        _ => OverlayCorner::TopLeft,
    }
}

#[derive(Clone, Copy)]
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

    fn push_metric(&mut self, value: Option<u16>, minimum_digits: usize) {
        let Some(value) = value else {
            for _ in 0..minimum_digits {
                self.push(b'-');
            }
            return;
        };
        self.push_unsigned(value, minimum_digits);
    }

    fn push_metric_unpadded(&mut self, value: Option<u16>, missing_digits: usize) {
        let Some(value) = value else {
            for _ in 0..missing_digits {
                self.push(b'-');
            }
            return;
        };
        self.push_unsigned(value, 1);
    }

    fn push_tenths(&mut self, value: Option<u16>) {
        let Some(value) = value else {
            self.push_bytes(b"--.-");
            return;
        };
        self.push_unsigned(value / 10, 1);
        self.push(b'.');
        self.push(b'0' + u8::try_from(value % 10).unwrap_or(0));
    }

    fn push_unsigned(&mut self, value: u16, minimum_digits: usize) {
        let mut digits = [0_u8; 5];
        let mut remaining = value;
        let mut count = 0;
        loop {
            digits[count] = b'0' + u8::try_from(remaining % 10).unwrap_or(0);
            count += 1;
            remaining /= 10;
            if remaining == 0 {
                break;
            }
        }
        for _ in count..minimum_digits {
            self.push(b'0');
        }
        for index in (0..count).rev() {
            self.push(digits[index]);
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
}

fn push_text(batch: &mut GlyphBatch, text: &[u8], start_x: i32, start_y: i32) {
    push_text_scaled(batch, text, start_x, start_y, 1);
}

fn push_text_scaled(batch: &mut GlyphBatch, text: &[u8], start_x: i32, start_y: i32, scale: u8) {
    for (character_index, byte) in text.iter().enumerate() {
        let x = start_x.saturating_add(
            i32::try_from(character_index)
                .unwrap_or(i32::MAX)
                .saturating_mul(overlay_font::GLYPH_ADVANCE.saturating_mul(i32::from(scale))),
        );
        if overlay_font::coverage_raster(*byte)
            .iter()
            .any(|word| *word != 0)
        {
            batch.push(GlyphInstance {
                x,
                y: start_y,
                scale,
                byte: *byte,
                ui_font: false,
            });
        }
    }
}

/// Letter-spaced monospace text. The reference menu sets quiet labels with
/// tracking; one extra logical pixel per advance reproduces that density on
/// the fixed 9 px cell without changing the glyph raster.
fn push_text_tracked(batch: &mut GlyphBatch, text: &[u8], start_x: i32, start_y: i32) {
    push_text_advance(
        batch,
        text,
        start_x,
        start_y,
        overlay_font::GLYPH_ADVANCE.saturating_add(1),
    );
}

/// Monospace text with an explicit logical advance. Ribbon cells are 89 px
/// wide, so their quiet labels use an 8 px advance to keep the longest label
/// (FRAME TIME) clear of the next cell divider.
fn push_text_advance(
    batch: &mut GlyphBatch,
    text: &[u8],
    start_x: i32,
    start_y: i32,
    advance: i32,
) {
    for (character_index, byte) in text.iter().enumerate() {
        let x = start_x.saturating_add(
            i32::try_from(character_index)
                .unwrap_or(i32::MAX)
                .saturating_mul(advance),
        );
        if overlay_font::coverage_raster(*byte)
            .iter()
            .any(|word| *word != 0)
        {
            batch.push(GlyphInstance {
                x,
                y: start_y,
                scale: 1,
                byte: *byte,
                ui_font: false,
            });
        }
    }
}

fn push_text_centered_tracked(batch: &mut GlyphBatch, text: &[u8], left: i32, right: i32, y: i32) {
    let width = i32::try_from(text.len())
        .unwrap_or(i32::MAX)
        .saturating_mul(overlay_font::GLYPH_ADVANCE.saturating_add(1))
        .saturating_sub(1);
    let x = left.saturating_add((right - left - width).max(0) / 2);
    push_text_tracked(batch, text, x, y);
}

fn push_ui_text(batch: &mut GlyphBatch, text: &[u8], start_x: i32, start_y: i32) {
    push_ui_text_scaled(batch, text, start_x, start_y, 1);
}

fn push_ui_text_scaled(batch: &mut GlyphBatch, text: &[u8], start_x: i32, start_y: i32, scale: u8) {
    let mut x = start_x;
    for byte in text {
        if overlay_font::ui_coverage_raster(*byte)
            .iter()
            .any(|word| *word != 0)
        {
            batch.push(GlyphInstance {
                x,
                y: start_y,
                scale,
                byte: *byte,
                ui_font: true,
            });
        }
        x = x
            .saturating_add(overlay_font::ui_advance_width(*byte).saturating_mul(i32::from(scale)));
    }
}

fn push_ui_text_centered(
    batch: &mut GlyphBatch,
    text: &[u8],
    left: i32,
    right: i32,
    y: i32,
    scale: u8,
) {
    let width = ui_text_width(text, scale);
    let x = left.saturating_add((right - left - width).max(0) / 2);
    push_ui_text_scaled(batch, text, x, y, scale);
}

fn ui_text_width(text: &[u8], scale: u8) -> i32 {
    text.iter().fold(0_i32, |width, byte| {
        width.saturating_add(overlay_font::ui_advance_width(*byte).saturating_mul(i32::from(scale)))
    })
}

fn ui_text_width_tracked(text: &[u8], scale: u8) -> i32 {
    text.iter()
        .fold(0_i32, |width, byte| {
            width.saturating_add(
                overlay_font::ui_advance_width(*byte)
                    .saturating_mul(i32::from(scale))
                    .saturating_add(1),
            )
        })
        .saturating_sub(1)
}

fn push_ui_text_centered_tracked(
    batch: &mut GlyphBatch,
    text: &[u8],
    left: i32,
    right: i32,
    y: i32,
    scale: u8,
) {
    let width = ui_text_width_tracked(text, scale);
    let x = left.saturating_add((right - left - width).max(0) / 2);
    let mut cursor = x;
    for byte in text {
        if overlay_font::ui_coverage_raster(*byte)
            .iter()
            .any(|word| *word != 0)
        {
            batch.push(GlyphInstance {
                x: cursor,
                y,
                scale,
                byte: *byte,
                ui_font: true,
            });
        }
        cursor = cursor.saturating_add(
            overlay_font::ui_advance_width(*byte)
                .saturating_mul(i32::from(scale))
                .saturating_add(1),
        );
    }
}

const fn empty_clear_rect() -> VkClearRect {
    clear_rect(0, 0, 0, 0)
}

const fn clear_rect(x: i32, y: i32, width: u32, height: u32) -> VkClearRect {
    VkClearRect {
        rect: VkRect2d {
            offset: VkOffset2d { x, y },
            extent: VkExtent2d { width, height },
        },
        base_array_layer: 0,
        layer_count: 1,
    }
}

#[derive(Clone, Copy)]
struct SwapchainImage {
    view: VkImageView,
    framebuffer: VkFramebuffer,
}

const EMPTY_SWAPCHAIN_IMAGE: SwapchainImage = SwapchainImage {
    view: 0,
    framebuffer: 0,
};

struct SwapchainState {
    render_pass: VkRenderPass,
    panel_pipeline_layout: VkPipelineLayout,
    panel_pipeline: VkPipeline,
    glyph_pipeline_layout: VkPipelineLayout,
    glyph_pipeline: VkPipeline,
    extent: VkExtent2d,
    srgb_attachment: bool,
    images: [SwapchainImage; MAX_SWAPCHAIN_IMAGES],
    image_count: usize,
}

#[derive(Clone, Copy, Default)]
struct FrameContext {
    command_buffer_address: usize,
    semaphore: VkSemaphore,
    fence: VkFence,
    primary_swapchain: VkSwapchainKhr,
    primary_image_index: u32,
    referenced_swapchains: [VkSwapchainKhr; MAX_PRESENT_SWAPCHAINS],
    referenced_image_indices: [u32; MAX_PRESENT_SWAPCHAINS],
    reference_count: usize,
    recorded_plan_revision: u64,
    assigned: bool,
    usable: bool,
}

struct QueueState {
    device_key: usize,
    queue_address: usize,
    command_pool: VkCommandPool,
    contexts: [FrameContext; MAX_QUEUE_CONTEXTS],
}

struct DeviceState {
    device_address: usize,
    functions: DeviceFunctions,
    graphics_queue_families: u64,
    queue_count: usize,
    swapchains: BTreeMap<VkSwapchainKhr, SwapchainState>,
}

struct RendererState {
    devices: BTreeMap<usize, DeviceState>,
    queues: BTreeMap<usize, QueueState>,
    history: FrameHistory,
    plan: OverlayPlan,
    saved_notice: OverlayPlan,
    menu: OverlayPlan,
    menu_animation: ReplayMenuAnimation,
    menu_visible: bool,
    menu_snapshot: Option<ReplayMenuTelemetry>,
    plan_revision: u64,
    config: OverlayConfig,
    telemetry: TelemetryReader,
    replay_saved_revision: u16,
    replay_saved_until_ns: u64,
    replay_saved_visible: bool,
    metrics_visible: bool,
}

impl Default for RendererState {
    fn default() -> Self {
        let config = OverlayConfig::from_environment();
        Self {
            devices: BTreeMap::new(),
            queues: BTreeMap::new(),
            history: FrameHistory::default(),
            plan: if visible_from_environment() {
                OverlayPlan::new(OverlaySnapshot::default(), config)
            } else {
                OverlayPlan::hidden()
            },
            saved_notice: OverlayPlan::hidden(),
            menu: OverlayPlan::hidden(),
            menu_animation: ReplayMenuAnimation::default(),
            menu_visible: false,
            menu_snapshot: None,
            plan_revision: 1,
            config,
            telemetry: TelemetryReader::from_environment(),
            replay_saved_revision: 0,
            replay_saved_until_ns: 0,
            replay_saved_visible: false,
            metrics_visible: visible_from_environment(),
        }
    }
}

impl RendererState {
    fn update_saved_notice(&mut self, revision: Option<u16>, now_ns: u64) -> bool {
        if let Some(revision) =
            revision.filter(|value| *value != 0 && *value != self.replay_saved_revision)
        {
            self.replay_saved_revision = revision;
            self.replay_saved_until_ns = now_ns.saturating_add(REPLAY_SAVED_NOTICE_NS);
        }
        let visible = now_ns < self.replay_saved_until_ns;
        let changed = visible != self.replay_saved_visible;
        self.replay_saved_visible = visible;
        if changed {
            self.saved_notice = if visible {
                saved_notice::plan()
            } else {
                OverlayPlan::hidden()
            };
        }
        changed
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ReplayMenuAnimation {
    progress: u16,
    target: u16,
    last_update_ns: u64,
}

impl ReplayMenuAnimation {
    fn set_visible(&mut self, visible: bool, now_ns: u64) {
        self.advance(now_ns);
        self.target = if visible { u16::MAX } else { 0 };
        self.last_update_ns = now_ns;
    }

    fn advance(&mut self, now_ns: u64) -> bool {
        if self.progress == self.target {
            self.last_update_ns = now_ns;
            return false;
        }
        let elapsed = if self.last_update_ns == 0 {
            0
        } else {
            now_ns.saturating_sub(self.last_update_ns)
        };
        self.last_update_ns = now_ns;
        if elapsed == 0 {
            return false;
        }
        let step = elapsed
            .saturating_mul(u64::from(u16::MAX))
            .checked_div(REPLAY_MENU_ANIMATION_NS)
            .unwrap_or(u64::from(u16::MAX))
            .max(1);
        let step = u16::try_from(step.min(u64::from(u16::MAX))).unwrap_or(u16::MAX);
        let previous = self.progress;
        self.progress = if self.target > self.progress {
            self.progress.saturating_add(step).min(self.target)
        } else {
            self.progress.saturating_sub(step).max(self.target)
        };
        self.progress != previous
    }
}

struct TelemetryReader {
    file: Option<File>,
    last_read_ns: u64,
    last_revision: u64,
    last_menu_read_ns: u64,
    last_menu_revision: u64,
}

impl TelemetryReader {
    fn from_environment() -> Self {
        let file = env::var_os(OVERLAY_TELEMETRY_ENV)
            .filter(|value| Path::new(value).is_absolute())
            .and_then(|value| File::open(value).ok());
        Self {
            file,
            last_read_ns: 0,
            last_revision: 0,
            last_menu_read_ns: 0,
            last_menu_revision: 0,
        }
    }

    /// Read both telemetry blocks. The hardware block keeps its 4 Hz budget
    /// because it only feeds slow-moving metric rows. The Replay menu block
    /// carries pointer motion, so the caller requests eager polling whenever
    /// the menu is visible or animating; while hidden, a 20 Hz poll bounds
    /// open detection without adding a read to every presentation.
    fn refresh(
        &mut self,
        now_ns: u64,
        menu_eager: bool,
    ) -> (
        Option<OverlayHardwareTelemetry>,
        Option<ReplayMenuTelemetry>,
    ) {
        let Some(file) = self.file.as_ref() else {
            return (None, None);
        };
        let hardware_due = self.last_read_ns == 0
            || now_ns.saturating_sub(self.last_read_ns) >= TELEMETRY_INTERVAL_NS;
        let menu_due = menu_eager
            || self.last_menu_read_ns == 0
            || now_ns.saturating_sub(self.last_menu_read_ns) >= MENU_POLL_IDLE_NS;
        if !hardware_due && !menu_due {
            return (None, None);
        }
        let mut hardware = None;
        let mut menu = None;
        if menu_due {
            self.last_menu_read_ns = now_ns;
            let mut bytes = [0_u8; REPLAY_MENU_TELEMETRY_BYTES];
            if file
                .read_exact_at(&mut bytes, OVERLAY_HARDWARE_TELEMETRY_BYTES as u64)
                .is_ok()
                && let Ok(telemetry) = decode_replay_menu_telemetry(&bytes)
                && telemetry.revision != self.last_menu_revision
            {
                self.last_menu_revision = telemetry.revision;
                menu = Some(telemetry);
            }
        }
        if hardware_due {
            self.last_read_ns = now_ns;
            let mut bytes = [0_u8; OVERLAY_HARDWARE_TELEMETRY_BYTES];
            if file.read_exact_at(&mut bytes, 0).is_ok()
                && let Ok(telemetry) = decode_overlay_hardware_telemetry(&bytes)
                && telemetry.revision != self.last_revision
            {
                self.last_revision = telemetry.revision;
                hardware = Some(telemetry);
            }
        }
        (hardware, menu)
    }
}

fn hardware_snapshot(telemetry: OverlayHardwareTelemetry) -> OverlaySnapshot {
    OverlaySnapshot {
        cpu_percent: telemetry.cpu_utilization_tenths.map(round_percent),
        cpu_temperature_c: telemetry
            .cpu_temperature_tenths_celsius
            .map(round_temperature),
        gpu_percent: telemetry.gpu_utilization_tenths.map(round_percent),
        gpu_temperature_c: telemetry
            .gpu_temperature_tenths_celsius
            .map(round_temperature),
        ..OverlaySnapshot::default()
    }
}

fn round_percent(tenths: u16) -> u8 {
    u8::try_from((tenths.saturating_add(5) / 10).min(100)).unwrap_or(100)
}

fn round_temperature(tenths: u16) -> i16 {
    i16::try_from(tenths.saturating_add(5) / 10).unwrap_or(i16::MAX)
}

static RENDERER: LazyLock<Mutex<RendererState>> =
    LazyLock::new(|| Mutex::new(RendererState::default()));
static ACTIVE_DEVICE_COUNT: AtomicUsize = AtomicUsize::new(0);
static DIAGNOSTIC_PREPARE_REPORTED: AtomicBool = AtomicBool::new(false);

pub(crate) const fn max_queue_families() -> usize {
    MAX_QUEUE_FAMILIES
}

pub(crate) fn visible_from_environment() -> bool {
    env::var(OVERLAY_VISIBLE_ENV).ok().as_deref() == Some("1")
}

pub(crate) fn renderer_requested_from_environment() -> bool {
    visible_from_environment()
        || env::var(REPLAY_PRODUCTION_ENV).ok().as_deref() == Some("1")
        || env::var_os(OVERLAY_TELEMETRY_ENV).is_some()
}

fn diagnostic(stage: &str, result: VkResult) {
    if env::var(OVERLAY_DIAGNOSTIC_ENV).ok().as_deref() == Some("1") {
        eprintln!("Redunar overlay diagnostic: {stage} failed with Vulkan result {result}");
    }
}

fn diagnostic_prepare(reason: &str) {
    if env::var(OVERLAY_DIAGNOSTIC_ENV).ok().as_deref() == Some("1")
        && !DIAGNOSTIC_PREPARE_REPORTED.swap(true, Ordering::Relaxed)
    {
        eprintln!("Redunar overlay diagnostic: present preparation stopped at {reason}");
    }
}

pub(crate) fn device_created(
    device_key: usize,
    device: VkDevice,
    functions: Option<DeviceFunctions>,
    graphics_queue_families: u64,
) {
    if env::var(OVERLAY_DIAGNOSTIC_ENV).ok().as_deref() == Some("1") {
        eprintln!(
            "Redunar overlay diagnostic: device observed (visible={}, graphics_families={graphics_queue_families:#x}, functions={})",
            renderer_requested_from_environment(),
            functions.is_some()
        );
    }
    if !renderer_requested_from_environment() || graphics_queue_families == 0 {
        if renderer_requested_from_environment()
            && graphics_queue_families == 0
            && ACTIVE_DEVICE_COUNT.load(Ordering::Relaxed) == 0
        {
            crate::producer::record_overlay_error(OverlayFailureReason::GraphicsQueueUnavailable);
        }
        return;
    }
    let Some(functions) = functions else {
        if ACTIVE_DEVICE_COUNT.load(Ordering::Relaxed) == 0 {
            crate::producer::record_overlay_error(
                OverlayFailureReason::RequiredVulkanFunctionsUnavailable,
            );
        }
        return;
    };
    let mut state = lock_renderer();
    if state.devices.len() >= MAX_DEVICES && !state.devices.contains_key(&device_key) {
        if ACTIVE_DEVICE_COUNT.load(Ordering::Relaxed) == 0 {
            crate::producer::record_overlay_error(OverlayFailureReason::RendererCapacityReached);
        }
        return;
    }
    let replaced = state.devices.insert(
        device_key,
        DeviceState {
            device_address: device.addr(),
            functions,
            graphics_queue_families,
            queue_count: 0,
            swapchains: BTreeMap::new(),
        },
    );
    if replaced.is_none() {
        ACTIVE_DEVICE_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn device_destroyed(device_key: usize) {
    let mut state = lock_renderer();
    let queue_keys: [usize; MAX_QUEUES_PER_DEVICE] = {
        let mut keys = [0; MAX_QUEUES_PER_DEVICE];
        let mut count = 0;
        for (key, queue) in &state.queues {
            if queue.device_key == device_key && count < keys.len() {
                keys[count] = *key;
                count += 1;
            }
        }
        // Zero cannot be a valid dispatchable queue address. Leave the unused
        // suffix as a sentinel to keep teardown allocation-free.
        keys
    };
    let Some(mut device) = state.devices.remove(&device_key) else {
        return;
    };
    ACTIVE_DEVICE_COUNT.fetch_sub(1, Ordering::Relaxed);
    for key in queue_keys.into_iter().filter(|key| *key != 0) {
        if let Some(queue) = state.queues.remove(&key) {
            unsafe { destroy_queue_resources(&device, &queue) };
        }
    }
    let swapchains = mem::take(&mut device.swapchains);
    for (_, swapchain) in swapchains {
        unsafe { destroy_swapchain_resources(&device, &swapchain) };
    }
}

pub(crate) fn queue_observed(device_key: usize, queue: VkQueue, family_index: u32) {
    if env::var(OVERLAY_DIAGNOSTIC_ENV).ok().as_deref() == Some("1") {
        eprintln!(
            "Redunar overlay diagnostic: queue observed (device={device_key:#x}, family={family_index})"
        );
    }
    if queue.is_null() || family_index >= 64 {
        return;
    }
    let queue_address = queue.addr();
    let mut state = lock_renderer();
    if state.queues.contains_key(&queue_address) {
        return;
    }
    let Some(device) = state.devices.get(&device_key) else {
        return;
    };
    if device.queue_count >= MAX_QUEUES_PER_DEVICE
        || device.graphics_queue_families & (1_u64 << family_index) == 0
    {
        return;
    }
    // SAFETY: the device and graphics family were captured from successful
    // Vulkan creation; failures clean up partial resources and disable only
    // this optional queue renderer.
    let Some(queue_state) =
        (unsafe { create_queue_resources(device_key, device, queue, family_index) })
    else {
        return;
    };
    state.queues.insert(queue_address, queue_state);
    if let Some(device) = state.devices.get_mut(&device_key) {
        device.queue_count += 1;
    }
}

pub(crate) unsafe fn swapchain_created(
    device_key: usize,
    create_info: *const VkSwapchainCreateInfoKhr,
    swapchain: VkSwapchainKhr,
) {
    if env::var(OVERLAY_DIAGNOSTIC_ENV).ok().as_deref() == Some("1") && !create_info.is_null() {
        // SAFETY: the hook receives the application create info and checks
        // only fixed scalar fields before forwarding ownership elsewhere.
        let diagnostic_info = unsafe { &*create_info };
        eprintln!(
            "Redunar overlay diagnostic: swapchain observed (format={}, usage={:#x}, extent={}x{})",
            diagnostic_info.image_format,
            diagnostic_info.image_usage,
            diagnostic_info.image_extent.width,
            diagnostic_info.image_extent.height
        );
    }
    if create_info.is_null() || swapchain == 0 {
        return;
    }
    // SAFETY: this hook is called only after the next layer successfully
    // consumed the application-provided create info.
    let info = unsafe { &*create_info };
    if info.s_type != VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR
        || info.flags & VK_SWAPCHAIN_CREATE_PROTECTED_BIT_KHR != 0
        || info.image_format == VK_FORMAT_UNDEFINED
        || info.image_array_layers != 1
        || info.image_usage & VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT == 0
        || info.image_extent.width < MIN_OVERLAY_WIDTH
        || info.image_extent.height < MIN_OVERLAY_HEIGHT
    {
        return;
    }

    let mut state = lock_renderer();
    let Some(device) = state.devices.get(&device_key) else {
        return;
    };
    if device.swapchains.len() >= MAX_SWAPCHAINS_PER_DEVICE {
        return;
    }
    // SAFETY: all resources are created on the device that owns the successful
    // swapchain. Partial failures are torn down before returning.
    let Some(resources) = (unsafe { create_swapchain_resources(device, info, swapchain) }) else {
        return;
    };
    if let Some(device) = state.devices.get_mut(&device_key) {
        device.swapchains.insert(swapchain, resources);
    }
}

pub(crate) fn swapchain_destroyed(device_key: usize, swapchain: VkSwapchainKhr) {
    let mut state = lock_renderer();
    for queue in state
        .queues
        .values_mut()
        .filter(|queue| queue.device_key == device_key)
    {
        for context in &mut queue.contexts {
            if context.referenced_swapchains[..context.reference_count].contains(&swapchain) {
                context.assigned = false;
                context.reference_count = 0;
            }
        }
    }
    let Some(device) = state.devices.get_mut(&device_key) else {
        return;
    };
    if let Some(resources) = device.swapchains.remove(&swapchain) {
        // SAFETY: Vulkan requires the application to finish use of swapchain
        // images before destroying the swapchain. Redunar owns these dependent
        // views/framebuffers and tears them down before forwarding destruction.
        unsafe { destroy_swapchain_resources(device, &resources) };
    }
}

/// Prepare one overlay submission and return the semaphore that the forwarded
/// present must wait on. Any unsupported or busy state returns `None`, leaving
/// the original `VkPresentInfoKHR` completely unchanged.
#[expect(
    clippy::too_many_lines,
    reason = "presentation preparation keeps queue, swapchain, and cached-plan ownership in one auditable transaction"
)]
pub(crate) unsafe fn prepare_present(
    queue: VkQueue,
    present_info: *const VkPresentInfoKhr,
) -> Option<VkSemaphore> {
    if ACTIVE_DEVICE_COUNT.load(Ordering::Relaxed) == 0 || queue.is_null() || present_info.is_null()
    {
        diagnostic_prepare("missing active device, queue, or present info");
        return None;
    }
    let Ok(mut state) = RENDERER.try_lock() else {
        return None;
    };
    // SAFETY: Vulkan requires a valid present-info structure for this call;
    // the wrapper forwards the same pointer to the next layer.
    let present = unsafe { &*present_info };
    if present.s_type != VK_STRUCTURE_TYPE_PRESENT_INFO_KHR
        || present.swapchain_count == 0
        || usize::try_from(present.swapchain_count).ok()? > MAX_PRESENT_SWAPCHAINS
        || usize::try_from(present.wait_semaphore_count).ok()? > MAX_PRESENT_WAIT_SEMAPHORES
        || present.swapchains.is_null()
        || present.image_indices.is_null()
        || (present.wait_semaphore_count > 0 && present.wait_semaphores.is_null())
    {
        return None;
    }

    let queue_address = queue.addr();
    let Some(device_key) = state
        .queues
        .get(&queue_address)
        .map(|queue| queue.device_key)
    else {
        diagnostic_prepare("unregistered presentation queue");
        return None;
    };
    if state.plan.is_empty() && state.saved_notice.is_empty() && state.menu.is_empty() {
        return None;
    }
    let plan_revision = state.plan_revision;
    let Some(device) = state.devices.get(&device_key) else {
        diagnostic_prepare("unregistered presentation device");
        return None;
    };
    let device_address = device.device_address;
    let functions = device.functions;
    let mut targets = [RenderTarget::default(); MAX_PRESENT_SWAPCHAINS];
    let mut target_count = 0;
    for index in 0..usize::try_from(present.swapchain_count).ok()? {
        // SAFETY: the application supplies arrays with `swapchain_count`
        // entries under the Vulkan presentation contract.
        let swapchain = unsafe { *present.swapchains.add(index) };
        // SAFETY: same array-length contract as `pSwapchains`.
        let image_index = unsafe { *present.image_indices.add(index) };
        let Some(resources) = device.swapchains.get(&swapchain) else {
            continue;
        };
        let Ok(image_offset) = usize::try_from(image_index) else {
            continue;
        };
        let Some(image) = resources.images.get(image_offset) else {
            continue;
        };
        if image_offset >= resources.image_count || image.framebuffer == 0 {
            continue;
        }
        targets[target_count] = RenderTarget {
            swapchain,
            image_index,
            render_pass: resources.render_pass,
            panel_pipeline_layout: resources.panel_pipeline_layout,
            panel_pipeline: resources.panel_pipeline,
            glyph_pipeline_layout: resources.glyph_pipeline_layout,
            glyph_pipeline: resources.glyph_pipeline,
            framebuffer: image.framebuffer,
            extent: resources.extent,
            srgb_attachment: resources.srgb_attachment,
        };
        target_count += 1;
    }
    if target_count == 0 {
        diagnostic_prepare("missing swapchain render resources");
        return None;
    }

    let RendererState {
        queues,
        plan,
        saved_notice,
        menu,
        ..
    } = &mut *state;
    let plans = [&*plan, &*saved_notice, &*menu];
    let queue_state = queues.get_mut(&queue_address)?;
    let device_handle = device_address as VkDevice;
    // SAFETY: all handles/functions in these records belong to this queue's
    // live device. The routine performs only nonblocking readiness checks.
    let submitted = unsafe {
        submit_overlay(
            device_handle,
            functions,
            queue_state,
            present,
            &targets[..target_count],
            &plans,
            plan_revision,
        )
    };
    if submitted.is_none() {
        diagnostic_prepare("queue submission");
    } else if env::var(OVERLAY_DIAGNOSTIC_ENV).ok().as_deref() == Some("1")
        && !DIAGNOSTIC_PREPARE_REPORTED.swap(true, Ordering::Relaxed)
    {
        eprintln!("Redunar overlay diagnostic: first submission succeeded");
    }
    submitted
}

pub(crate) fn presentation_finished(now_ns: u64) {
    if ACTIVE_DEVICE_COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    let Ok(mut state) = RENDERER.try_lock() else {
        return;
    };
    // The menu pointer must not wait for the slow hardware poll while the
    // panel is on screen or finishing its enter/exit animation.
    let menu_eager =
        state.menu_visible || state.menu_animation.progress != state.menu_animation.target;
    let (telemetry, menu) = state.telemetry.refresh(now_ns, menu_eager);
    state.update_presentation(telemetry, menu, now_ns);
}

impl RendererState {
    fn update_presentation(
        &mut self,
        telemetry: Option<OverlayHardwareTelemetry>,
        menu: Option<ReplayMenuTelemetry>,
        now_ns: u64,
    ) {
        if let Some(telemetry) = telemetry {
            let hardware = hardware_snapshot(telemetry);
            self.history.snapshot.cpu_percent = hardware.cpu_percent;
            self.history.snapshot.cpu_temperature_c = hardware.cpu_temperature_c;
            self.history.snapshot.gpu_percent = hardware.gpu_percent;
            self.history.snapshot.gpu_temperature_c = hardware.gpu_temperature_c;
            let config = OverlayConfig {
                corner: match telemetry.corner {
                    1 => OverlayCorner::TopRight,
                    2 => OverlayCorner::BottomLeft,
                    3 => OverlayCorner::BottomRight,
                    _ => OverlayCorner::TopLeft,
                },
                preset: match telemetry.preset {
                    1 => OverlayPreset::Detailed,
                    2 => OverlayPreset::FpsOnly,
                    3 => OverlayPreset::Custom,
                    _ => OverlayPreset::Compact,
                },
                layout: match telemetry.layout {
                    1 => OverlayLayout::Ribbon,
                    2 => OverlayLayout::Telemetry,
                    _ => OverlayLayout::Grid,
                },
                palette: match telemetry.palette {
                    1 => OverlayPalette::Glacier,
                    2 => OverlayPalette::Ember,
                    3 => OverlayPalette::Mint,
                    4 => OverlayPalette::Mono,
                    5 => OverlayPalette::Amethyst,
                    6 => OverlayPalette::Solar,
                    7 => OverlayPalette::Rose,
                    _ => OverlayPalette::Redunar,
                },
                branding_visible: telemetry.branding_visible,
                metrics: telemetry.metrics,
                opacity_percent: telemetry.opacity_percent,
                scale_percent: telemetry.scale_percent,
            };
            self.config = config;
            if let Some(visible) = telemetry.metrics_visible {
                self.metrics_visible = visible;
            }
        }
        let replay_notice_changed =
            self.update_saved_notice(telemetry.map(|value| value.replay_saved_revision), now_ns);
        let menu_changed = self.update_replay_menu(menu, now_ns);
        if self.history.record(now_ns)
            || telemetry.is_some()
            || replay_notice_changed
            || menu_changed
        {
            self.plan = if self.metrics_visible {
                OverlayPlan::new(self.history.snapshot, self.config)
            } else {
                OverlayPlan::hidden()
            };
            self.plan_revision = self.plan_revision.saturating_add(1);
        }
    }

    /// Advance the Replay menu state machine for this presentation. The
    /// daemon publishes one revision per visible change (toggle, pointer,
    /// hover, readiness), so unchanged periods cost no rebuild and never
    /// bump the plan revision the recorded command buffers are cached by.
    fn update_replay_menu(&mut self, menu: Option<ReplayMenuTelemetry>, now_ns: u64) -> bool {
        if let Some(menu) = menu {
            self.menu_visible = menu.visible;
            self.menu_snapshot = Some(menu);
            self.menu_animation.set_visible(menu.visible, now_ns);
        }
        let advanced = self.menu_animation.advance(now_ns);
        let progress = self.menu_animation.progress;
        if progress == 0 && !self.menu_visible {
            // Fully closed: release all menu render work so presentations
            // with only metrics (or nothing) keep the original fast path.
            if self.menu.is_empty() {
                return false;
            }
            self.menu = OverlayPlan::hidden();
            return true;
        }
        if menu.is_none() && !advanced {
            return false;
        }
        let Some(snapshot) = self.menu_snapshot else {
            return false;
        };
        self.menu = OverlayPlan::replay_menu(ReplayMenuView::from_telemetry(snapshot, progress));
        true
    }
}

#[derive(Clone, Copy)]
struct RenderTarget {
    swapchain: VkSwapchainKhr,
    image_index: u32,
    render_pass: VkRenderPass,
    panel_pipeline_layout: VkPipelineLayout,
    panel_pipeline: VkPipeline,
    glyph_pipeline_layout: VkPipelineLayout,
    glyph_pipeline: VkPipeline,
    framebuffer: VkFramebuffer,
    extent: VkExtent2d,
    srgb_attachment: bool,
}

impl Default for RenderTarget {
    fn default() -> Self {
        Self {
            swapchain: 0,
            image_index: 0,
            render_pass: 0,
            panel_pipeline_layout: 0,
            panel_pipeline: 0,
            glyph_pipeline_layout: 0,
            glyph_pipeline: 0,
            framebuffer: 0,
            extent: VkExtent2d {
                width: 0,
                height: 0,
            },
            srgb_attachment: false,
        }
    }
}

unsafe fn create_queue_resources(
    device_key: usize,
    device: &DeviceState,
    queue: VkQueue,
    family_index: u32,
) -> Option<QueueState> {
    let device_handle = device.device_address as VkDevice;
    let functions = device.functions;
    let pool_info = VkCommandPoolCreateInfo {
        s_type: VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT,
        queue_family_index: family_index,
    };
    let mut command_pool = 0;
    // SAFETY: output storage and create info are valid for this live device.
    if unsafe {
        (functions.create_command_pool)(
            device_handle,
            &raw const pool_info,
            std::ptr::null(),
            &raw mut command_pool,
        )
    } != VK_SUCCESS
    {
        return None;
    }

    let allocate_info = VkCommandBufferAllocateInfo {
        s_type: VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        p_next: std::ptr::null(),
        command_pool,
        level: VK_COMMAND_BUFFER_LEVEL_PRIMARY,
        command_buffer_count: u32::try_from(MAX_QUEUE_CONTEXTS).ok()?,
    };
    let mut command_buffers = [std::ptr::null_mut(); MAX_QUEUE_CONTEXTS];
    // SAFETY: the command pool is live and the fixed output array matches the
    // requested count.
    if unsafe {
        (functions.allocate_command_buffers)(
            device_handle,
            &raw const allocate_info,
            command_buffers.as_mut_ptr(),
        )
    } != VK_SUCCESS
    {
        // SAFETY: the pool was created above and owns any partial allocation.
        unsafe { (functions.destroy_command_pool)(device_handle, command_pool, std::ptr::null()) };
        return None;
    }

    let semaphore_info = VkSemaphoreCreateInfo {
        s_type: VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
    };
    let fence_info = VkFenceCreateInfo {
        s_type: VK_STRUCTURE_TYPE_FENCE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: VK_FENCE_CREATE_SIGNALED_BIT,
    };
    let mut contexts = [FrameContext::default(); MAX_QUEUE_CONTEXTS];
    for (index, context) in contexts.iter_mut().enumerate() {
        context.command_buffer_address = command_buffers[index].addr();
        // SAFETY: fixed create infos and outputs belong to the live device.
        let semaphore_result = unsafe {
            (functions.create_semaphore)(
                device_handle,
                &raw const semaphore_info,
                std::ptr::null(),
                &raw mut context.semaphore,
            )
        };
        if semaphore_result != VK_SUCCESS {
            // SAFETY: cleanup visits only handles successfully created so far.
            unsafe { destroy_partial_contexts(device_handle, functions, &contexts, command_pool) };
            return None;
        }
        // SAFETY: fixed create infos and outputs belong to the live device.
        let fence_result = unsafe {
            (functions.create_fence)(
                device_handle,
                &raw const fence_info,
                std::ptr::null(),
                &raw mut context.fence,
            )
        };
        if fence_result != VK_SUCCESS {
            // SAFETY: cleanup visits only nonzero handles.
            unsafe { destroy_partial_contexts(device_handle, functions, &contexts, command_pool) };
            return None;
        }
        context.usable = true;
    }

    Some(QueueState {
        device_key,
        queue_address: queue.addr(),
        command_pool,
        contexts,
    })
}

unsafe fn destroy_partial_contexts(
    device: VkDevice,
    functions: DeviceFunctions,
    contexts: &[FrameContext],
    command_pool: VkCommandPool,
) {
    for context in contexts {
        if context.fence != 0 {
            // SAFETY: nonzero handles were created by this device.
            unsafe { (functions.destroy_fence)(device, context.fence, std::ptr::null()) };
        }
        if context.semaphore != 0 {
            // SAFETY: nonzero handles were created by this device.
            unsafe { (functions.destroy_semaphore)(device, context.semaphore, std::ptr::null()) };
        }
    }
    // SAFETY: the command pool was created by this device and owns all command
    // buffers, including any partially allocated set.
    unsafe { (functions.destroy_command_pool)(device, command_pool, std::ptr::null()) };
}

unsafe fn destroy_queue_resources(device: &DeviceState, queue: &QueueState) {
    let handle = device.device_address as VkDevice;
    // Vulkan requires applications to finish queue work before device
    // destruction. We never wait here; teardown follows that ownership rule.
    for context in queue.contexts {
        if context.fence != 0 {
            // SAFETY: handles belong to this live device.
            unsafe { (device.functions.destroy_fence)(handle, context.fence, std::ptr::null()) };
        }
        if context.semaphore != 0 {
            // SAFETY: handles belong to this live device.
            unsafe {
                (device.functions.destroy_semaphore)(handle, context.semaphore, std::ptr::null());
            };
        }
    }
    if queue.command_pool != 0 {
        // SAFETY: the pool and its command buffers belong to this device.
        unsafe {
            (device.functions.destroy_command_pool)(handle, queue.command_pool, std::ptr::null());
        };
    }
}

// Swapchain setup is one transactional sequence: every partial failure routes
// through the same teardown path. Keeping it contiguous makes ownership easier
// to audit than splitting live Vulkan handles across helper return values.
#[allow(clippy::too_many_lines)]
unsafe fn create_swapchain_resources(
    device: &DeviceState,
    create_info: &VkSwapchainCreateInfoKhr,
    swapchain: VkSwapchainKhr,
) -> Option<SwapchainState> {
    let handle = device.device_address as VkDevice;
    let functions = device.functions;
    let attachment = VkAttachmentDescription {
        flags: 0,
        format: create_info.image_format,
        samples: VK_SAMPLE_COUNT_1_BIT,
        load_op: VK_ATTACHMENT_LOAD_OP_LOAD,
        store_op: VK_ATTACHMENT_STORE_OP_STORE,
        stencil_load_op: VK_ATTACHMENT_LOAD_OP_DONT_CARE,
        stencil_store_op: VK_ATTACHMENT_STORE_OP_DONT_CARE,
        initial_layout: VK_IMAGE_LAYOUT_PRESENT_SRC_KHR,
        final_layout: VK_IMAGE_LAYOUT_PRESENT_SRC_KHR,
    };
    let color_reference = VkAttachmentReference {
        attachment: 0,
        layout: VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
    };
    let subpass = VkSubpassDescription {
        flags: 0,
        pipeline_bind_point: VK_PIPELINE_BIND_POINT_GRAPHICS,
        input_attachment_count: 0,
        input_attachments: std::ptr::null(),
        color_attachment_count: 1,
        color_attachments: &raw const color_reference,
        resolve_attachments: std::ptr::null(),
        depth_stencil_attachment: std::ptr::null(),
        preserve_attachment_count: 0,
        preserve_attachments: std::ptr::null(),
    };
    let dependencies = [
        VkSubpassDependency {
            src_subpass: VK_SUBPASS_EXTERNAL,
            dst_subpass: 0,
            src_stage_mask: VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
            dst_stage_mask: VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
            src_access_mask: 0,
            dst_access_mask: VK_ACCESS_COLOR_ATTACHMENT_READ_BIT
                | VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
            dependency_flags: VK_DEPENDENCY_BY_REGION_BIT,
        },
        VkSubpassDependency {
            src_subpass: 0,
            dst_subpass: VK_SUBPASS_EXTERNAL,
            src_stage_mask: VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
            dst_stage_mask: VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT,
            src_access_mask: VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
            dst_access_mask: 0,
            dependency_flags: VK_DEPENDENCY_BY_REGION_BIT,
        },
    ];
    let render_pass_info = VkRenderPassCreateInfo {
        s_type: VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        attachment_count: 1,
        attachments: &raw const attachment,
        subpass_count: 1,
        subpasses: &raw const subpass,
        dependency_count: u32::try_from(dependencies.len()).ok()?,
        dependencies: dependencies.as_ptr(),
    };
    let mut resources = SwapchainState {
        render_pass: 0,
        panel_pipeline_layout: 0,
        panel_pipeline: 0,
        glyph_pipeline_layout: 0,
        glyph_pipeline: 0,
        extent: create_info.image_extent,
        srgb_attachment: matches!(
            create_info.image_format,
            VK_FORMAT_R8G8B8A8_SRGB | VK_FORMAT_B8G8R8A8_SRGB
        ),
        images: [EMPTY_SWAPCHAIN_IMAGE; MAX_SWAPCHAIN_IMAGES],
        image_count: 0,
    };
    // SAFETY: create info points to stack data valid for the complete call.
    let render_pass_result = unsafe {
        (functions.create_render_pass)(
            handle,
            &raw const render_pass_info,
            std::ptr::null(),
            &raw mut resources.render_pass,
        )
    };
    if render_pass_result != VK_SUCCESS {
        diagnostic("render-pass creation", render_pass_result);
        return None;
    }
    if let Some((layout, pipeline)) =
        unsafe { create_overlay_pipeline(handle, functions, resources.render_pass, false) }
    {
        resources.panel_pipeline_layout = layout;
        resources.panel_pipeline = pipeline;
    }
    let Some((layout, pipeline)) =
        (unsafe { create_overlay_pipeline(handle, functions, resources.render_pass, true) })
    else {
        // Glyph rendering is required. Failing closed here is preferable to
        // presenting corrupted or misleading metrics inside the game.
        unsafe { destroy_swapchain_resources(device, &resources) };
        return None;
    };
    resources.glyph_pipeline_layout = layout;
    resources.glyph_pipeline = pipeline;

    let mut image_count = 0_u32;
    // SAFETY: count query uses the successful live swapchain.
    let image_count_result = unsafe {
        (functions.get_swapchain_images)(
            handle,
            swapchain,
            &raw mut image_count,
            std::ptr::null_mut(),
        )
    };
    if image_count_result != VK_SUCCESS
        || image_count == 0
        || usize::try_from(image_count).ok()? > MAX_SWAPCHAIN_IMAGES
    {
        diagnostic("swapchain image count", image_count_result);
        // SAFETY: resources contains only owned nonzero handles.
        unsafe { destroy_swapchain_resources(device, &resources) };
        return None;
    }
    let mut images = [0; MAX_SWAPCHAIN_IMAGES];
    let mut supplied_count = u32::try_from(images.len()).ok()?;
    // SAFETY: the output array has `supplied_count` entries.
    let images_result = unsafe {
        (functions.get_swapchain_images)(
            handle,
            swapchain,
            &raw mut supplied_count,
            images.as_mut_ptr(),
        )
    };
    if images_result != VK_SUCCESS || supplied_count != image_count {
        diagnostic("swapchain image retrieval", images_result);
        // SAFETY: resources contains only owned nonzero handles.
        unsafe { destroy_swapchain_resources(device, &resources) };
        return None;
    }

    for (index, image) in images
        .iter()
        .copied()
        .take(usize::try_from(image_count).ok()?)
        .enumerate()
    {
        let view_info = VkImageViewCreateInfo {
            s_type: VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            image,
            view_type: VK_IMAGE_VIEW_TYPE_2D,
            format: create_info.image_format,
            components: VkComponentMapping {
                r: VK_COMPONENT_SWIZZLE_IDENTITY,
                g: VK_COMPONENT_SWIZZLE_IDENTITY,
                b: VK_COMPONENT_SWIZZLE_IDENTITY,
                a: VK_COMPONENT_SWIZZLE_IDENTITY,
            },
            subresource_range: VkImageSubresourceRange {
                aspect_mask: VK_IMAGE_ASPECT_COLOR_BIT,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            },
        };
        // SAFETY: the swapchain image belongs to this device and the output is
        // valid stack storage.
        let image_view_result = unsafe {
            (functions.create_image_view)(
                handle,
                &raw const view_info,
                std::ptr::null(),
                &raw mut resources.images[index].view,
            )
        };
        if image_view_result != VK_SUCCESS {
            diagnostic("swapchain image-view creation", image_view_result);
            // SAFETY: cleanup handles partial creation.
            unsafe { destroy_swapchain_resources(device, &resources) };
            return None;
        }
        let framebuffer_info = VkFramebufferCreateInfo {
            s_type: VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            render_pass: resources.render_pass,
            attachment_count: 1,
            attachments: &raw const resources.images[index].view,
            width: create_info.image_extent.width,
            height: create_info.image_extent.height,
            layers: 1,
        };
        // SAFETY: the render pass and view are live and compatible by format.
        let framebuffer_result = unsafe {
            (functions.create_framebuffer)(
                handle,
                &raw const framebuffer_info,
                std::ptr::null(),
                &raw mut resources.images[index].framebuffer,
            )
        };
        if framebuffer_result != VK_SUCCESS {
            diagnostic("overlay framebuffer creation", framebuffer_result);
            // SAFETY: cleanup handles partial creation.
            unsafe { destroy_swapchain_resources(device, &resources) };
            return None;
        }
        resources.image_count += 1;
    }
    if env::var(OVERLAY_DIAGNOSTIC_ENV).ok().as_deref() == Some("1") {
        eprintln!("Redunar overlay diagnostic: swapchain resources ready ({image_count} images)");
    }
    Some(resources)
}

/// Build one descriptor-free panel or glyph pipeline. Glyphs use one fixed
/// 108-byte scalar push constant and never expose font arrays to the driver.
#[allow(clippy::too_many_lines)]
unsafe fn create_overlay_pipeline(
    device: VkDevice,
    functions: DeviceFunctions,
    render_pass: VkRenderPass,
    glyph: bool,
) -> Option<(VkPipelineLayout, VkPipeline)> {
    unsafe fn shader_module<const N: usize>(
        device: VkDevice,
        functions: DeviceFunctions,
        shader: &AlignedShader<N>,
    ) -> Option<VkShaderModule> {
        if !N.is_multiple_of(mem::size_of::<u32>()) {
            return None;
        }
        let info = VkShaderModuleCreateInfo {
            s_type: VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            code_size: N,
            // The wrapper type guarantees the address has u32 alignment.
            code: std::ptr::from_ref(shader).cast::<u32>(),
        };
        let mut module = 0;
        // SAFETY: embedded SPIR-V storage is four-byte aligned and remains
        // alive for the complete Vulkan call.
        let result = unsafe {
            (functions.create_shader_module)(
                device,
                &raw const info,
                std::ptr::null(),
                &raw mut module,
            )
        };
        if result == VK_SUCCESS {
            Some(module)
        } else {
            diagnostic("shader-module creation", result);
            None
        }
    }

    #[repr(C)]
    struct PushConstantRange {
        stage_flags: u32,
        offset: u32,
        size: u32,
    }

    let vertex = if glyph {
        unsafe { shader_module(device, functions, &GLYPH_VERTEX_SHADER) }?
    } else {
        unsafe { shader_module(device, functions, &PANEL_VERTEX_SHADER) }?
    };
    let fragment = if glyph {
        unsafe { shader_module(device, functions, &GLYPH_FRAGMENT_SHADER) }
    } else {
        unsafe { shader_module(device, functions, &PANEL_FRAGMENT_SHADER) }
    };
    let Some(fragment) = fragment else {
        // SAFETY: the vertex module was created by this device.
        unsafe { (functions.destroy_shader_module)(device, vertex, std::ptr::null()) };
        return None;
    };

    let push_constant = PushConstantRange {
        stage_flags: VK_SHADER_STAGE_FRAGMENT_BIT,
        offset: 0,
        size: u32::try_from(if glyph {
            mem::size_of::<GlyphPushConstants>()
        } else {
            mem::size_of::<PanelPushConstants>()
        })
        .ok()?,
    };
    let layout_info = VkPipelineLayoutCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        set_layout_count: 0,
        set_layouts: std::ptr::null(),
        push_constant_range_count: 1,
        push_constant_ranges: std::ptr::from_ref(&push_constant).cast(),
    };
    let mut layout = 0;
    // SAFETY: the descriptor-free layout create info is complete.
    let layout_result = unsafe {
        (functions.create_pipeline_layout)(
            device,
            &raw const layout_info,
            std::ptr::null(),
            &raw mut layout,
        )
    };
    if layout_result != VK_SUCCESS {
        diagnostic("pipeline-layout creation", layout_result);
        // SAFETY: both shader modules belong to this device.
        unsafe {
            (functions.destroy_shader_module)(device, fragment, std::ptr::null());
            (functions.destroy_shader_module)(device, vertex, std::ptr::null());
        }
        return None;
    }

    let entry = b"main\0";
    let stages = [
        VkPipelineShaderStageCreateInfo {
            s_type: VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            stage: VK_SHADER_STAGE_VERTEX_BIT,
            module: vertex,
            name: entry.as_ptr().cast::<c_char>(),
            specialization_info: std::ptr::null(),
        },
        VkPipelineShaderStageCreateInfo {
            s_type: VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            stage: VK_SHADER_STAGE_FRAGMENT_BIT,
            module: fragment,
            name: entry.as_ptr().cast::<c_char>(),
            specialization_info: std::ptr::null(),
        },
    ];
    let vertex_input = VkPipelineVertexInputStateCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        vertex_binding_description_count: 0,
        vertex_binding_descriptions: std::ptr::null(),
        vertex_attribute_description_count: 0,
        vertex_attribute_descriptions: std::ptr::null(),
    };
    let input_assembly = VkPipelineInputAssemblyStateCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        topology: VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
        primitive_restart_enable: 0,
    };
    let viewport_state = VkPipelineViewportStateCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        viewport_count: 1,
        viewports: std::ptr::null(),
        scissor_count: 1,
        scissors: std::ptr::null(),
    };
    let rasterization = VkPipelineRasterizationStateCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        depth_clamp_enable: 0,
        rasterizer_discard_enable: 0,
        polygon_mode: VK_POLYGON_MODE_FILL,
        cull_mode: VK_CULL_MODE_NONE,
        front_face: VK_FRONT_FACE_COUNTER_CLOCKWISE,
        depth_bias_enable: 0,
        depth_bias_constant_factor: 0.0,
        depth_bias_clamp: 0.0,
        depth_bias_slope_factor: 0.0,
        line_width: 1.0,
    };
    let multisample = VkPipelineMultisampleStateCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        rasterization_samples: VK_SAMPLE_COUNT_1_BIT,
        sample_shading_enable: 0,
        min_sample_shading: 0.0,
        sample_mask: std::ptr::null(),
        alpha_to_coverage_enable: 0,
        alpha_to_one_enable: 0,
    };
    let blend_attachment = VkPipelineColorBlendAttachmentState {
        blend_enable: 1,
        src_color_blend_factor: if glyph {
            VK_BLEND_FACTOR_CONSTANT_COLOR
        } else {
            VK_BLEND_FACTOR_CONSTANT_ALPHA
        },
        dst_color_blend_factor: if glyph {
            VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA
        } else {
            VK_BLEND_FACTOR_ONE_MINUS_CONSTANT_ALPHA
        },
        color_blend_op: VK_BLEND_OP_ADD,
        src_alpha_blend_factor: VK_BLEND_FACTOR_ONE,
        dst_alpha_blend_factor: VK_BLEND_FACTOR_ZERO,
        alpha_blend_op: VK_BLEND_OP_ADD,
        // Preserve the game's swapchain alpha; Redunar blends RGB only.
        color_write_mask: VK_COLOR_COMPONENT_R_BIT
            | VK_COLOR_COMPONENT_G_BIT
            | VK_COLOR_COMPONENT_B_BIT,
    };
    let blend_state = VkPipelineColorBlendStateCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        logic_op_enable: 0,
        logic_op: VK_LOGIC_OP_COPY,
        attachment_count: 1,
        attachments: &raw const blend_attachment,
        blend_constants: [0.5; 4],
    };
    let dynamic_states = [
        VK_DYNAMIC_STATE_VIEWPORT,
        VK_DYNAMIC_STATE_SCISSOR,
        VK_DYNAMIC_STATE_BLEND_CONSTANTS,
    ];
    let dynamic_state = VkPipelineDynamicStateCreateInfo {
        s_type: VK_STRUCTURE_TYPE_PIPELINE_DYNAMIC_STATE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        dynamic_state_count: u32::try_from(dynamic_states.len()).ok()?,
        dynamic_states: dynamic_states.as_ptr(),
    };
    let info = VkGraphicsPipelineCreateInfo {
        s_type: VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        stage_count: u32::try_from(stages.len()).ok()?,
        stages: stages.as_ptr(),
        vertex_input_state: &raw const vertex_input,
        input_assembly_state: &raw const input_assembly,
        tessellation_state: std::ptr::null(),
        viewport_state: &raw const viewport_state,
        rasterization_state: &raw const rasterization,
        multisample_state: &raw const multisample,
        depth_stencil_state: std::ptr::null(),
        color_blend_state: &raw const blend_state,
        dynamic_state: &raw const dynamic_state,
        layout,
        render_pass,
        subpass: 0,
        base_pipeline_handle: 0,
        base_pipeline_index: -1,
    };
    let mut pipeline = 0;
    // SAFETY: all referenced pipeline state and compatible render-pass objects
    // remain valid for the complete call.
    let result = unsafe {
        (functions.create_graphics_pipelines)(
            device,
            0,
            1,
            &raw const info,
            std::ptr::null(),
            &raw mut pipeline,
        )
    };
    // Shader modules are no longer needed after pipeline creation, including
    // the failure path.
    unsafe {
        (functions.destroy_shader_module)(device, fragment, std::ptr::null());
        (functions.destroy_shader_module)(device, vertex, std::ptr::null());
    }
    if result == VK_SUCCESS {
        Some((layout, pipeline))
    } else {
        diagnostic("graphics-pipeline creation", result);
        // SAFETY: the layout belongs to this device and no pipeline owns it.
        unsafe { (functions.destroy_pipeline_layout)(device, layout, std::ptr::null()) };
        None
    }
}

unsafe fn destroy_swapchain_resources(device: &DeviceState, resources: &SwapchainState) {
    let handle = device.device_address as VkDevice;
    for image in resources.images.iter().take(resources.image_count) {
        if image.framebuffer != 0 {
            // SAFETY: framebuffer belongs to this live device.
            unsafe {
                (device.functions.destroy_framebuffer)(handle, image.framebuffer, std::ptr::null());
            };
        }
        if image.view != 0 {
            // SAFETY: view belongs to this live device.
            unsafe { (device.functions.destroy_image_view)(handle, image.view, std::ptr::null()) };
        }
    }
    if resources.panel_pipeline != 0 {
        // SAFETY: pipeline belongs to this live device.
        unsafe {
            (device.functions.destroy_pipeline)(handle, resources.panel_pipeline, std::ptr::null());
        }
    }
    if resources.panel_pipeline_layout != 0 {
        // SAFETY: pipeline layout belongs to this live device.
        unsafe {
            (device.functions.destroy_pipeline_layout)(
                handle,
                resources.panel_pipeline_layout,
                std::ptr::null(),
            );
        }
    }
    if resources.glyph_pipeline != 0 {
        // SAFETY: pipeline belongs to this live device.
        unsafe {
            (device.functions.destroy_pipeline)(handle, resources.glyph_pipeline, std::ptr::null());
        }
    }
    if resources.glyph_pipeline_layout != 0 {
        // SAFETY: pipeline layout belongs to this live device.
        unsafe {
            (device.functions.destroy_pipeline_layout)(
                handle,
                resources.glyph_pipeline_layout,
                std::ptr::null(),
            );
        }
    }
    if resources.render_pass != 0 {
        // SAFETY: render pass belongs to this live device.
        unsafe {
            (device.functions.destroy_render_pass)(handle, resources.render_pass, std::ptr::null());
        };
    }
}

unsafe fn submit_overlay(
    device: VkDevice,
    functions: DeviceFunctions,
    queue: &mut QueueState,
    present: &VkPresentInfoKhr,
    targets: &[RenderTarget],
    plans: &[&OverlayPlan; 3],
    plan_revision: u64,
) -> Option<VkSemaphore> {
    let primary = targets.first()?;
    let context = queue.contexts.iter_mut().find(|context| {
        context.usable
            && ((!context.assigned)
                || (context.primary_swapchain == primary.swapchain
                    && context.primary_image_index == primary.image_index))
    })?;
    // A fence check is a nonblocking readiness query. Matching the context to
    // a reacquired swapchain image also guarantees the prior binary semaphore
    // was consumed by presentation before it is signaled again.
    // SAFETY: the fence is a live renderer-owned device object.
    if unsafe { (functions.get_fence_status)(device, context.fence) } != VK_SUCCESS {
        return None;
    }
    // SAFETY: the fence is signaled and belongs to this live device.
    if unsafe { (functions.reset_fences)(device, 1, &raw const context.fence) } != VK_SUCCESS {
        context.usable = false;
        return None;
    }
    let command_buffer = context.command_buffer_address as VkCommandBuffer;
    let recording_matches = context.assigned
        && context.recorded_plan_revision == plan_revision
        && context.reference_count == targets.len()
        && context.referenced_swapchains[..context.reference_count]
            .iter()
            .zip(&context.referenced_image_indices[..context.reference_count])
            .zip(targets)
            .all(|((swapchain, image_index), target)| {
                *swapchain == target.swapchain && *image_index == target.image_index
            });
    if !recording_matches {
        // SAFETY: the command buffer's prior submission completed at the fence.
        if unsafe { (functions.reset_command_buffer)(command_buffer, 0) } != VK_SUCCESS {
            context.usable = false;
            return None;
        }
        let begin = VkCommandBufferBeginInfo {
            s_type: VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
            p_next: std::ptr::null(),
            // The completed buffer is cached per swapchain image and may be
            // submitted again while its plan revision remains current.
            flags: 0,
            inheritance_info: std::ptr::null(),
        };
        // SAFETY: this primary command buffer is reset and not pending.
        if unsafe { (functions.begin_command_buffer)(command_buffer, &raw const begin) }
            != VK_SUCCESS
        {
            context.usable = false;
            return None;
        }
        for target in targets {
            // SAFETY: target resources are compatible live objects from one
            // swapchain and clear rectangles lie within its verified minimum size.
            unsafe { record_target(functions, command_buffer, *target, plans) };
        }
        // SAFETY: recording is active and every render pass was ended.
        if unsafe { (functions.end_command_buffer)(command_buffer) } != VK_SUCCESS {
            context.usable = false;
            return None;
        }
        context.recorded_plan_revision = plan_revision;
    }

    let wait_stages = [VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT; MAX_PRESENT_WAIT_SEMAPHORES];
    let submit = VkSubmitInfo {
        s_type: VK_STRUCTURE_TYPE_SUBMIT_INFO,
        p_next: std::ptr::null(),
        wait_semaphore_count: present.wait_semaphore_count,
        wait_semaphores: present.wait_semaphores,
        wait_dst_stage_mask: if present.wait_semaphore_count == 0 {
            std::ptr::null()
        } else {
            wait_stages.as_ptr()
        },
        command_buffer_count: 1,
        command_buffers: &raw const command_buffer,
        signal_semaphore_count: 1,
        signal_semaphores: &raw const context.semaphore,
    };
    // SAFETY: application wait semaphores remain valid for the duration of
    // this call, and all renderer-owned submit objects belong to this queue.
    if unsafe {
        (functions.queue_submit)(
            queue.queue_address as VkQueue,
            1,
            &raw const submit,
            context.fence,
        )
    } != VK_SUCCESS
    {
        context.usable = false;
        return None;
    }

    context.primary_swapchain = primary.swapchain;
    context.primary_image_index = primary.image_index;
    context.reference_count = targets.len();
    for (slot, target) in context.referenced_swapchains.iter_mut().zip(targets) {
        *slot = target.swapchain;
    }
    for (slot, target) in context.referenced_image_indices.iter_mut().zip(targets) {
        *slot = target.image_index;
    }
    context.assigned = true;
    Some(context.semaphore)
}

#[allow(clippy::too_many_lines)]
unsafe fn record_target(
    functions: DeviceFunctions,
    command_buffer: VkCommandBuffer,
    target: RenderTarget,
    plans: &[&OverlayPlan; 3],
) {
    let begin = VkRenderPassBeginInfo {
        s_type: VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO,
        p_next: std::ptr::null(),
        render_pass: target.render_pass,
        framebuffer: target.framebuffer,
        render_area: VkRect2d {
            offset: VkOffset2d { x: 0, y: 0 },
            extent: target.extent,
        },
        clear_value_count: 0,
        clear_values: std::ptr::null(),
    };
    // SAFETY: the framebuffer/render pass pair was created together.
    unsafe {
        (functions.cmd_begin_render_pass)(
            command_buffer,
            &raw const begin,
            VK_SUBPASS_CONTENTS_INLINE,
        );
    };
    // Draw metrics, saved-notice, and Replay menu surfaces in one render
    // pass, preserving the game's image and avoiding extra attachment
    // transitions for short-lived overlays.
    for plan in plans.iter().filter(|plan| !plan.is_empty()) {
        unsafe { record_plan(functions, command_buffer, target, plan) };
    }
    // SAFETY: a render pass is active in this command buffer.
    unsafe { (functions.cmd_end_render_pass)(command_buffer) };
}

/// The bounded palette tables were stored as squared reference RGB channels.
/// An sRGB attachment encodes linear fragment/clear values on write, while an
/// UNORM attachment stores the supplied values directly. Encode colors for
/// UNORM here so both formats show the same intended reference roles.
fn color_for_target(target: RenderTarget, color: [f32; 4]) -> [f32; 4] {
    if target.srgb_attachment {
        color
    } else {
        [color[0].sqrt(), color[1].sqrt(), color[2].sqrt(), color[3]]
    }
}

#[allow(clippy::too_many_lines)]
unsafe fn record_plan(
    functions: DeviceFunctions,
    command_buffer: VkCommandBuffer,
    target: RenderTarget,
    plan: &OverlayPlan,
) {
    let scale_percent = effective_scale_percent(target.extent, plan);
    let (offset_x, offset_y) = overlay_origin(target.extent, plan, scale_percent);
    let colors = plan.colors;
    if plan.dim_percent > 0 && target.panel_pipeline != 0 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "Vulkan viewport dimensions are f32; supported swapchain extents are exactly representable"
        )]
        let viewport = VkViewport {
            x: 0.0,
            y: 0.0,
            width: target.extent.width as f32,
            height: target.extent.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let scissor = VkRect2d {
            offset: VkOffset2d { x: 0, y: 0 },
            extent: target.extent,
        };
        let opacity = f32::from(plan.dim_percent) / 100.0;
        let blend_constants = [opacity; 4];
        let push_constants =
            PanelPushConstants::from_rect(scissor, 0, if target.srgb_attachment { 9 } else { 0 });
        // SAFETY: the descriptor-free panel pipeline is compatible with the
        // active render pass and the viewport is the current swapchain.
        unsafe {
            (functions.cmd_bind_pipeline)(
                command_buffer,
                VK_PIPELINE_BIND_POINT_GRAPHICS,
                target.panel_pipeline,
            );
            (functions.cmd_set_viewport)(command_buffer, 0, 1, &raw const viewport);
            (functions.cmd_set_scissor)(command_buffer, 0, 1, &raw const scissor);
            (functions.cmd_set_blend_constants)(command_buffer, blend_constants.as_ptr());
            (functions.cmd_push_constants)(
                command_buffer,
                target.panel_pipeline_layout,
                VK_SHADER_STAGE_FRAGMENT_BIT,
                0,
                u32::try_from(mem::size_of_val(&push_constants)).unwrap_or(0),
                std::ptr::from_ref(&push_constants).cast(),
            );
            (functions.cmd_draw)(command_buffer, 3, 1, 0, 0);
        }
    }
    if let Some(panel) = plan.panel {
        let panel = scaled_translated_rect(panel, scale_percent, offset_x, offset_y);
        if target.panel_pipeline != 0 && (plan.opacity_percent < 100 || plan.panel_radius > 0) {
            #[expect(
                clippy::cast_precision_loss,
                reason = "Vulkan viewports are f32 while swapchain coordinates are integers; practical image sizes are exactly representable"
            )]
            let viewport = VkViewport {
                x: panel.rect.offset.x as f32,
                y: panel.rect.offset.y as f32,
                width: panel.rect.extent.width as f32,
                height: panel.rect.extent.height as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            };
            let opacity = f32::from(plan.opacity_percent) / 100.0;
            let blend_constants = [opacity; 4];
            let radius =
                u32::from(plan.panel_radius).saturating_mul(u32::from(scale_percent)) / 100;
            let shader_palette = plan.panel_palette + u32::from(target.srgb_attachment) * 9;
            let push_constants = PanelPushConstants::from_rect(panel.rect, radius, shader_palette);
            // SAFETY: the optional pipeline is compatible with this render
            // pass. Dynamic viewport/scissor exactly cover the bounded panel,
            // and the small push payload clips only the menu's outer corners.
            unsafe {
                (functions.cmd_bind_pipeline)(
                    command_buffer,
                    VK_PIPELINE_BIND_POINT_GRAPHICS,
                    target.panel_pipeline,
                );
                (functions.cmd_set_viewport)(command_buffer, 0, 1, &raw const viewport);
                (functions.cmd_set_scissor)(command_buffer, 0, 1, &raw const panel.rect);
                (functions.cmd_set_blend_constants)(command_buffer, blend_constants.as_ptr());
                (functions.cmd_push_constants)(
                    command_buffer,
                    target.panel_pipeline_layout,
                    VK_SHADER_STAGE_FRAGMENT_BIT,
                    0,
                    u32::try_from(mem::size_of_val(&push_constants)).unwrap_or(0),
                    std::ptr::from_ref(&push_constants).cast(),
                );
                (functions.cmd_draw)(command_buffer, 3, 1, 0, 0);
            }
        } else {
            // Pipeline creation is optional. A fully opaque clear is the safe
            // fallback when the rounded-panel pipeline is unavailable.
            let background = clear_attachment(color_for_target(target, plan.palette.panel_color()));
            // SAFETY: one color attachment and one in-bounds clear rectangle.
            unsafe {
                (functions.cmd_clear_attachments)(
                    command_buffer,
                    1,
                    &raw const background,
                    1,
                    &raw const panel,
                );
            };
        }
    }
    let logo_base = plan
        .logo_base
        .scaled_translated(scale_percent, offset_x, offset_y);
    clear_batch(
        functions,
        command_buffer,
        clear_attachment(color_for_target(
            target,
            if plan.placement == OverlayPlacement::SavedNotice {
                [0.007, 0.007, 0.01, 1.0]
            } else {
                [0.025, 0.03, 0.04, 1.0]
            },
        )),
        &logo_base,
    );
    let logo_dark_red = plan
        .logo_dark_red
        .scaled_translated(scale_percent, offset_x, offset_y);
    clear_batch(
        functions,
        command_buffer,
        clear_attachment(color_for_target(
            target,
            if plan.placement == OverlayPlacement::ReplayMenu {
                MENU_SELECTED_FILL
            } else {
                // The Moment saved pill's receipt arrow keeps the reference
                // notice red (#ec7a81).
                [0.839, 0.195, 0.22, 1.0]
            },
        )),
        &logo_dark_red,
    );
    let dividers = plan
        .dividers
        .scaled_translated(scale_percent, offset_x, offset_y);
    clear_batch(
        functions,
        command_buffer,
        clear_attachment(color_for_target(target, colors.divider)),
        &dividers,
    );
    // Selection and hover outlines share edges with the neutral control grid.
    // Draw them last so the grid cannot erase the right or lower half of a
    // selected duration cell.
    let accent = plan
        .accent
        .scaled_translated(scale_percent, offset_x, offset_y);
    clear_batch(
        functions,
        command_buffer,
        clear_attachment(color_for_target(target, colors.accent)),
        &accent,
    );
    draw_glyph_batch(
        functions,
        command_buffer,
        target,
        &plan.accent_glyphs,
        colors.accent,
        scale_percent,
        offset_x,
        offset_y,
    );
    draw_glyph_batch(
        functions,
        command_buffer,
        target,
        &plan.muted_glyphs,
        colors.muted,
        scale_percent,
        offset_x,
        offset_y,
    );
    draw_glyph_batch(
        functions,
        command_buffer,
        target,
        &plan.text_glyphs,
        colors.text,
        scale_percent,
        offset_x,
        offset_y,
    );
    draw_glyph_batch(
        functions,
        command_buffer,
        target,
        &plan.highlight_glyphs,
        colors.highlight,
        scale_percent,
        offset_x,
        offset_y,
    );
    let cursor_shadow = plan
        .cursor_shadow
        .scaled_translated(scale_percent, offset_x, offset_y);
    clear_batch(
        functions,
        command_buffer,
        clear_attachment(color_for_target(target, [0.02, 0.02, 0.02, 1.0])),
        &cursor_shadow,
    );
    let cursor = plan
        .cursor
        .scaled_translated(scale_percent, offset_x, offset_y);
    clear_batch(
        functions,
        command_buffer,
        clear_attachment(color_for_target(
            target,
            if plan.placement == OverlayPlacement::SavedNotice {
                [0.63, 0.76, 0.70, 1.0]
            } else {
                [0.94, 0.95, 0.97, 1.0]
            },
        )),
        &cursor,
    );
    if let Some(pointer_shadow) = plan.pointer_shadow.as_ref() {
        draw_glyph_instances(
            functions,
            command_buffer,
            target,
            std::slice::from_ref(pointer_shadow),
            [0.015, 0.015, 0.018, 1.0],
            scale_percent,
            offset_x,
            offset_y,
        );
    }
    if let Some(pointer) = plan.pointer.as_ref() {
        draw_glyph_instances(
            functions,
            command_buffer,
            target,
            std::slice::from_ref(pointer),
            [0.94, 0.95, 0.97, 1.0],
            scale_percent,
            offset_x,
            offset_y,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_glyph_batch(
    functions: DeviceFunctions,
    command_buffer: VkCommandBuffer,
    target: RenderTarget,
    batch: &GlyphBatch,
    color: [f32; 4],
    scale_percent: u8,
    offset_x: i32,
    offset_y: i32,
) {
    draw_glyph_instances(
        functions,
        command_buffer,
        target,
        batch.as_slice(),
        color,
        scale_percent,
        offset_x,
        offset_y,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_glyph_instances(
    functions: DeviceFunctions,
    command_buffer: VkCommandBuffer,
    target: RenderTarget,
    instances: &[GlyphInstance],
    color: [f32; 4],
    scale_percent: u8,
    offset_x: i32,
    offset_y: i32,
) {
    if instances.is_empty() || target.glyph_pipeline == 0 || target.glyph_pipeline_layout == 0 {
        return;
    }
    let color = color_for_target(target, color);
    // SAFETY: the glyph pipeline and layout were created together for this
    // render pass. It has no buffers or descriptors and accepts exactly one
    // fixed 108-byte coverage-raster push constant.
    unsafe {
        (functions.cmd_bind_pipeline)(
            command_buffer,
            VK_PIPELINE_BIND_POINT_GRAPHICS,
            target.glyph_pipeline,
        );
        (functions.cmd_set_blend_constants)(command_buffer, color.as_ptr());
    }
    for instance in instances {
        let rect = scaled_translated_rect(
            clear_rect(
                instance.x,
                instance.y,
                overlay_font::GLYPH_WIDTH.saturating_mul(u32::from(instance.scale)),
                overlay_font::GLYPH_HEIGHT.saturating_mul(u32::from(instance.scale)),
            ),
            scale_percent,
            offset_x,
            offset_y,
        );
        if rect.rect.extent.width == 0 || rect.rect.extent.height == 0 {
            continue;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "bounded swapchain glyph coordinates are exactly representable as Vulkan viewport floats"
        )]
        let viewport = VkViewport {
            x: rect.rect.offset.x as f32,
            y: rect.rect.offset.y as f32,
            width: rect.rect.extent.width as f32,
            height: rect.rect.extent.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let raster = match instance.byte {
            u8::MAX => overlay_font::cursor_coverage_raster(),
            value if value == u8::MAX - 1 => overlay_font::cursor_outline_coverage_raster(),
            value if instance.ui_font => overlay_font::ui_coverage_raster(value),
            value => overlay_font::coverage_raster(value),
        };
        let push_constants = GlyphPushConstants::from_raster(raster);
        // SAFETY: the viewport and scissor are bounded to the fitted overlay;
        // the complete scalar push payload remains live for the draw call.
        unsafe {
            (functions.cmd_set_viewport)(command_buffer, 0, 1, &raw const viewport);
            (functions.cmd_set_scissor)(command_buffer, 0, 1, &raw const rect.rect);
            (functions.cmd_push_constants)(
                command_buffer,
                target.glyph_pipeline_layout,
                VK_SHADER_STAGE_FRAGMENT_BIT,
                0,
                u32::try_from(mem::size_of_val(&push_constants)).unwrap_or(0),
                std::ptr::from_ref(&push_constants).cast(),
            );
            (functions.cmd_draw)(command_buffer, 3, 1, 0, 0);
        }
    }
}

fn effective_scale_percent(extent: VkExtent2d, plan: &OverlayPlan) -> u8 {
    let available_width = extent
        .width
        .saturating_sub(u32::try_from(OVERLAY_MARGIN * 2).unwrap_or(0));
    let available_height = extent
        .height
        .saturating_sub(u32::try_from(OVERLAY_MARGIN * 2).unwrap_or(0));
    let width_limit = available_width.saturating_mul(100) / plan.width.max(1);
    let height_limit = available_height.saturating_mul(100) / plan.height.max(1);
    // The approved 520×286 menu occupies about 89% of its reference scene.
    // Preserve that breathing room on smaller presentations; at 1080p the
    // requested 160% scale remains the limit.
    let reference_limit = if plan.placement == OverlayPlacement::ReplayMenu {
        let width_limit = extent.width.saturating_mul(89) / plan.width.max(1);
        let height_limit = extent.height.saturating_mul(89) / plan.height.max(1);
        width_limit.min(height_limit)
    } else {
        u32::MAX
    };
    u8::try_from(
        u32::from(plan.scale_percent)
            .min(width_limit)
            .min(height_limit)
            .min(reference_limit)
            .max(1),
    )
    .unwrap_or(plan.scale_percent)
}

fn overlay_origin(extent: VkExtent2d, plan: &OverlayPlan, scale_percent: u8) -> (i32, i32) {
    let width = i32::try_from(extent.width).unwrap_or(i32::MAX);
    let height = i32::try_from(extent.height).unwrap_or(i32::MAX);
    let plan_width = i32::try_from(plan.width.saturating_mul(u32::from(scale_percent)) / 100)
        .unwrap_or(i32::MAX);
    let plan_height = i32::try_from(plan.height.saturating_mul(u32::from(scale_percent)) / 100)
        .unwrap_or(i32::MAX);
    let right = width
        .saturating_sub(plan_width)
        .saturating_sub(OVERLAY_MARGIN);
    let bottom = height
        .saturating_sub(plan_height)
        .saturating_sub(OVERLAY_MARGIN);
    if plan.placement == OverlayPlacement::ReplayMenu {
        let center_x = width.saturating_sub(plan_width) / 2;
        let center_y = height.saturating_sub(plan_height) / 2;
        let progress = u64::from(plan.animation_progress);
        let maximum = u64::from(u16::MAX);
        let eased = progress
            .saturating_mul(progress)
            .saturating_mul(3 * maximum - 2 * progress)
            / maximum.saturating_pow(2);
        let entry_y = OVERLAY_MARGIN.saturating_sub(center_y);
        let offset = i64::from(entry_y)
            .saturating_mul(i64::try_from(maximum.saturating_sub(eased)).unwrap_or(0))
            / i64::from(u16::MAX);
        return (
            center_x,
            center_y.saturating_add(i32::try_from(offset).unwrap_or(0)),
        );
    }
    match plan.corner {
        OverlayCorner::TopLeft => (OVERLAY_MARGIN, OVERLAY_MARGIN),
        OverlayCorner::TopRight => (right, OVERLAY_MARGIN),
        OverlayCorner::BottomLeft => (OVERLAY_MARGIN, bottom),
        OverlayCorner::BottomRight => (right, bottom),
    }
}

fn scaled_translated_rect(mut rect: VkClearRect, percent: u8, x: i32, y: i32) -> VkClearRect {
    let scale = u32::from(percent);
    rect.rect.offset.x =
        x.saturating_add(rect.rect.offset.x.saturating_mul(i32::from(percent)) / 100);
    rect.rect.offset.y =
        y.saturating_add(rect.rect.offset.y.saturating_mul(i32::from(percent)) / 100);
    rect.rect.extent.width = rect.rect.extent.width.saturating_mul(scale) / 100;
    rect.rect.extent.height = rect.rect.extent.height.saturating_mul(scale) / 100;
    rect
}

fn clear_batch(
    functions: DeviceFunctions,
    command_buffer: VkCommandBuffer,
    attachment: VkClearAttachment,
    batch: &RectBatch,
) {
    let Ok(count) = u32::try_from(batch.length) else {
        return;
    };
    if count == 0 {
        return;
    }
    // SAFETY: the caller records inside a compatible render pass and this
    // fixed slice stays alive for the complete command-recording call.
    unsafe {
        (functions.cmd_clear_attachments)(
            command_buffer,
            1,
            &raw const attachment,
            count,
            batch.as_slice().as_ptr(),
        );
    };
}

const fn clear_attachment(color: [f32; 4]) -> VkClearAttachment {
    VkClearAttachment {
        aspect_mask: VK_IMAGE_ASPECT_COLOR_BIT,
        color_attachment: 0,
        clear_value: VkClearValue {
            color: VkClearColorValue { float32: color },
        },
    }
}

fn lock_renderer() -> std::sync::MutexGuard<'static, RendererState> {
    RENDERER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

const _: () = assert!(MAX_QUEUE_FAMILIES <= 64);
const _: () = assert!(MAX_QUEUE_CONTEXTS >= MAX_SWAPCHAIN_IMAGES);

#[cfg(test)]
mod tests {
    use super::*;

    /// Render one linear-light renderer color as an sRGB hex triplet for the
    /// reference SVG dumps. The square root is the exact inverse of the
    /// sRGB-to-linear conversion used when the palette tables were derived.
    #[expect(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "the channel is clamped to 0..=1 before scaling to a byte"
    )]
    fn hex(color: [f32; 4]) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(6);
        for channel in &color[..3] {
            let byte = (channel.sqrt().clamp(0.0, 1.0) * 255.0).round() as u8;
            write!(out, "{byte:02x}").expect("hex string grows");
        }
        out
    }

    #[test]
    fn fps_values_use_natural_width_without_leading_zeroes() {
        let mut two_digits = FixedText::<8>::default();
        two_digits.push_metric_unpadded(Some(75), 3);
        assert_eq!(two_digits.as_bytes(), b"75");

        let mut three_digits = FixedText::<8>::default();
        three_digits.push_metric_unpadded(Some(143), 3);
        assert_eq!(three_digits.as_bytes(), b"143");

        let mut missing = FixedText::<8>::default();
        missing.push_metric_unpadded(None, 3);
        assert_eq!(missing.as_bytes(), b"---");
    }

    #[test]
    fn replay_menu_layout_and_hit_regions_share_exact_grid_geometry() {
        let plan = OverlayPlan::replay_menu(ReplayMenuView {
            animation_progress: u16::MAX,
            capture_fps: 120,
            quality: 2,
            format: 1,
            selected_duration: 1,
            save_enabled: true,
            cursor_x: 5_000,
            cursor_y: 5_000,
            ..ReplayMenuView::default()
        });
        assert_eq!(plan.width, REPLAY_MENU_WIDTH);
        assert_eq!(plan.height, REPLAY_MENU_HEIGHT);
        assert_eq!(plan.panel_radius, REPLAY_MENU_CORNER_RADIUS);
        for (index, (x, y)) in [
            (83, 145),
            (201, 145),
            (319, 145),
            (437, 145),
            (83, 187),
            (201, 187),
            (319, 187),
            (437, 187),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                replay_menu_hit_target(x, y),
                u8::try_from(index + 1).unwrap()
            );
        }
        assert_eq!(replay_menu_hit_target(358, 245), 9);
        assert_eq!(replay_menu_hit_target(69, 245), 10);
        assert_eq!(replay_menu_hit_target(159, 245), 11);
        assert_eq!(replay_menu_hit_target(510, 276), 0);
        assert!(plan.accent.length <= MAX_ACCENT_RECTS);
        assert!(plan.dividers.length <= MAX_ACCENT_RECTS);
        assert_eq!(plan.logo_dark_red.length, 3);
        assert!(plan.text_glyphs.length <= MAX_TEXT_GLYPHS);
        assert!(plan.muted_glyphs.length <= MAX_TEXT_GLYPHS);
    }

    #[test]
    fn replay_menu_has_no_logo_bar_and_uses_a_separate_pointer() {
        let plan = OverlayPlan::replay_menu(ReplayMenuView {
            animation_progress: u16::MAX,
            cursor_x: 5_000,
            cursor_y: 5_000,
            ..ReplayMenuView::default()
        });
        assert_eq!(plan.logo_base.length, 0);
        assert_eq!(plan.logo_dark_red.length, 2);
        assert_eq!(plan.cursor.length, 0);
        assert!(plan.accent.length >= 2);
        assert!(plan.pointer.is_some());
        assert!(plan.pointer_shadow.is_some());
        assert!(
            !plan
                .text_glyphs
                .as_slice()
                .iter()
                .any(|glyph| glyph.y == 14)
        );
    }

    #[test]
    fn replay_menu_is_centered_and_animation_enters_from_the_top() {
        let mut plan = OverlayPlan::replay_menu(ReplayMenuView {
            animation_progress: u16::MAX,
            ..ReplayMenuView::default()
        });
        let extent = VkExtent2d {
            width: 1_440,
            height: 900,
        };
        assert_eq!(overlay_origin(extent, &plan, 100), (460, 307));
        plan.animation_progress = 0;
        assert_eq!(overlay_origin(extent, &plan, 100), (460, OVERLAY_MARGIN));
    }

    #[test]
    fn replay_menu_uses_a_large_default_scale_but_still_fits_small_swapchains() {
        let plan = OverlayPlan::replay_menu(ReplayMenuView {
            animation_progress: u16::MAX,
            ..ReplayMenuView::default()
        });
        assert_eq!(
            effective_scale_percent(
                VkExtent2d {
                    width: 2_560,
                    height: 1_440,
                },
                &plan,
            ),
            160
        );
        assert!(
            effective_scale_percent(
                VkExtent2d {
                    width: 800,
                    height: 600,
                },
                &plan,
            ) < 160
        );
    }

    #[test]
    fn replay_menu_animation_opens_closes_and_reverses_in_place() {
        let start = 1_000_000;
        let mut animation = ReplayMenuAnimation::default();
        animation.set_visible(true, start);
        assert_eq!(animation.progress, 0);
        assert!(animation.advance(start + REPLAY_MENU_ANIMATION_NS / 2));
        let halfway = animation.progress;
        assert!(halfway > 0 && halfway < u16::MAX);

        animation.set_visible(false, start + REPLAY_MENU_ANIMATION_NS / 2);
        assert_eq!(animation.progress, halfway);
        animation.set_visible(true, start + REPLAY_MENU_ANIMATION_NS / 2);
        assert!(animation.advance(start + REPLAY_MENU_ANIMATION_NS * 2));
        assert_eq!(animation.progress, u16::MAX);

        animation.set_visible(false, start + REPLAY_MENU_ANIMATION_NS * 2);
        assert!(animation.advance(start + REPLAY_MENU_ANIMATION_NS * 3));
        assert_eq!(animation.progress, 0);
    }

    fn menu_telemetry(revision: u64, visible: bool) -> ReplayMenuTelemetry {
        ReplayMenuTelemetry {
            revision,
            visible,
            pointer_pressed: false,
            cursor_x: 5_000,
            cursor_y: 5_000,
            hover_target: 0,
            pressed_target: 0,
            selected_duration_index: 1,
            status: ReplayMenuStatus::Buffering,
            available_seconds: 42,
            click_revision: 0,
            frame_rate: 120,
            quality: 2,
            output_format: 1,
            save_enabled: true,
            overlay_shortcut: ReplayShortcutLabel::from_shortcut("Ctrl+Shift+R"),
            save_shortcut: ReplayShortcutLabel::from_shortcut("Ctrl+F9"),
        }
    }

    #[test]
    fn menu_view_maps_the_daemon_snapshot_without_reinterpreting_codes() {
        let view = ReplayMenuView::from_telemetry(menu_telemetry(2, true), u16::MAX);
        assert_eq!(view.status, 2);
        assert_eq!(view.capture_fps, 120);
        assert_eq!(view.quality, 2);
        assert_eq!(view.format, 1);
        assert_eq!(view.selected_duration, 1);
        assert_eq!(view.available_seconds, 42);
        assert!(view.save_enabled);
        assert_eq!(view.animation_progress, u16::MAX);
    }

    #[test]
    fn menu_telemetry_changes_render_the_panel_even_with_metrics_hidden() {
        let mut state = RendererState {
            metrics_visible: false,
            ..RendererState::default()
        };
        state.update_presentation(None, Some(menu_telemetry(2, true)), 1_000_000);
        assert!(!state.menu.is_empty(), "visible menu renders at once");
        assert!(state.plan.is_empty(), "metrics stay hidden");
        // One full animation window later the panel is fully open, and
        // unchanged revisions must not bump the cached plan revision.
        let revision = state.plan_revision;
        state.update_presentation(None, None, 1_000_000 + REPLAY_MENU_ANIMATION_NS / 2);
        state.update_presentation(None, None, 1_000_000 + REPLAY_MENU_ANIMATION_NS * 2);
        assert_eq!(state.menu_animation.progress, u16::MAX);
        assert!(
            state.plan_revision > revision,
            "animation republishes frames"
        );
        // Let the frame-summary interval settle so the next call measures
        // only the menu's rebuild behavior.
        state.update_presentation(None, None, 1_000_000 + SUMMARY_INTERVAL_NS * 4);
        let revision = state.plan_revision;
        state.update_presentation(None, None, 1_000_000 + SUMMARY_INTERVAL_NS * 4 + 10_000_000);
        assert_eq!(
            state.plan_revision, revision,
            "a settled menu never rebuilds the plan"
        );

        state.update_presentation(None, Some(menu_telemetry(4, false)), 2_000_000_000);
        assert!(!state.menu.is_empty(), "closing menu animates out first");
        state.update_presentation(None, None, 2_000_000_000 + REPLAY_MENU_ANIMATION_NS * 2);
        assert!(
            state.menu.is_empty(),
            "the fully closed menu releases render work"
        );
    }

    #[test]
    fn telemetry_reader_decodes_the_menu_block_after_the_hardware_block() {
        let directory =
            std::env::temp_dir().join(format!("redunar-overlay-menu-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("telemetry.bin");
        let mut bytes = vec![0_u8; OVERLAY_HARDWARE_TELEMETRY_BYTES + REPLAY_MENU_TELEMETRY_BYTES];
        let mut hardware = [0_u8; OVERLAY_HARDWARE_TELEMETRY_BYTES];
        hardware[..8].copy_from_slice(b"RDOVL001");
        bytes[..OVERLAY_HARDWARE_TELEMETRY_BYTES].copy_from_slice(&hardware);
        let mut menu = [0_u8; REPLAY_MENU_TELEMETRY_BYTES];
        redunar_capture::encode_replay_menu_telemetry(menu_telemetry(2, true), &mut menu).unwrap();
        bytes[OVERLAY_HARDWARE_TELEMETRY_BYTES..].copy_from_slice(&menu);
        std::fs::write(&path, &bytes).unwrap();

        let mut reader = TelemetryReader {
            file: Some(std::fs::File::open(&path).unwrap()),
            last_read_ns: 0,
            last_revision: 0,
            last_menu_read_ns: 0,
            last_menu_revision: 0,
        };
        let (hardware, menu) = reader.refresh(1_000, true);
        assert!(
            hardware.is_none(),
            "the hardware block is not decodable here"
        );
        let menu = menu.expect("the menu block decodes at file offset 48");
        assert!(menu.visible);
        assert_eq!(menu.revision, 2);
        // The same revision never reports again, even under eager polling.
        let (_, repeated) = reader.refresh(2_000, true);
        assert!(repeated.is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn hidden_plan_has_no_render_work() {
        assert!(OverlayPlan::hidden().is_empty());
    }

    #[test]
    fn replay_menu_font_contains_every_non_space_character_it_draws() {
        for byte in b"^~-.%*+/0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz" {
            assert!(
                overlay_font::coverage_raster(*byte)
                    .into_iter()
                    .any(|word| word != 0),
                "missing Replay menu glyph for byte {byte}"
            );
        }
    }

    #[test]
    fn replay_menu_buffer_fact_uses_the_authoritative_runtime_state() {
        for (status, expected) in [
            (0, b"N/A".as_slice()),
            (1, b"OFF".as_slice()),
            (2, b"0SEC".as_slice()),
            (3, b"SAVING".as_slice()),
            (4, b"ERROR".as_slice()),
        ] {
            let plan = OverlayPlan::replay_menu(ReplayMenuView {
                status,
                animation_progress: u16::MAX,
                ..ReplayMenuView::default()
            });
            let rendered: Vec<u8> = plan
                .text_glyphs
                .as_slice()
                .iter()
                .map(|glyph| glyph.byte)
                .collect();
            assert!(
                rendered
                    .windows(expected.len())
                    .any(|window| window == expected),
                "missing status label {}",
                String::from_utf8_lossy(expected)
            );
        }
    }

    #[test]
    fn summary_reports_average_frame_time_and_lows_without_allocation() {
        let mut intervals = [10_000_000; 100];
        intervals[0] = 20_000_000;
        let summary = summarize(&intervals);
        assert_eq!(summary.fps, Some(99));
        assert_eq!(summary.frame_time_tenths_ms, Some(100));
        assert_eq!(summary.one_percent_low_fps, Some(50));
        assert_eq!(summary.point_one_percent_low_fps, Some(50));
    }

    #[test]
    fn frame_history_is_fixed_capacity_and_refreshes_at_four_hertz() {
        let mut history = FrameHistory::default();
        assert!(!history.record(1_000_000_000));
        for index in 1..=1_000_u64 {
            let _ = history.record(1_000_000_000 + index * 10_000_000);
        }
        assert_eq!(history.length, FRAME_HISTORY_CAPACITY);
        assert_eq!(history.snapshot.fps, Some(100));
    }

    #[test]
    fn plan_is_bounded_and_has_optional_external_telemetry_row() {
        let base = OverlayPlan::new(
            OverlaySnapshot {
                fps: Some(144),
                frame_time_tenths_ms: Some(69),
                one_percent_low_fps: Some(121),
                point_one_percent_low_fps: Some(110),
                ..OverlaySnapshot::default()
            },
            OverlayConfig::default(),
        );
        let external = OverlayPlan::new(
            OverlaySnapshot {
                cpu_percent: Some(42),
                cpu_temperature_c: Some(61),
                gpu_percent: Some(87),
                gpu_temperature_c: Some(68),
                ..OverlaySnapshot::default()
            },
            OverlayConfig::default(),
        );
        assert!(base.accent.length <= MAX_ACCENT_RECTS);
        assert!(
            base.text_glyphs.length + base.muted_glyphs.length
                < external.text_glyphs.length + external.muted_glyphs.length
        );
        assert!(external.text_glyphs.length <= MAX_TEXT_GLYPHS);
        assert!(external.muted_glyphs.length <= MAX_TEXT_GLYPHS);
    }

    #[test]
    fn presets_are_compact_bounded_and_keep_fps_only_background_free() {
        let metrics = OverlaySnapshot {
            fps: Some(144),
            frame_time_tenths_ms: Some(69),
            one_percent_low_fps: Some(121),
            point_one_percent_low_fps: Some(110),
            cpu_percent: Some(42),
            gpu_percent: Some(87),
            gpu_temperature_c: Some(68),
            ..OverlaySnapshot::default()
        };
        let compact = OverlayPlan::new(metrics, OverlayConfig::default());
        let fps_only = OverlayPlan::new(
            metrics,
            OverlayConfig {
                preset: OverlayPreset::FpsOnly,
                ..OverlayConfig::default()
            },
        );
        let detailed = OverlayPlan::new(
            metrics,
            OverlayConfig {
                preset: OverlayPreset::Detailed,
                ..OverlayConfig::default()
            },
        );
        assert_eq!(
            compact.height,
            PANEL_BASE_HEIGHT + PRIMARY_ROW_HEIGHT + SECONDARY_ROW_HEIGHT
        );
        assert!(fps_only.panel.is_none());
        assert_eq!(fps_only.text_glyphs.length, 0);
        assert_eq!(fps_only.width, FPS_ONLY_WIDTH);
        assert_eq!(fps_only.height, overlay_font::GLYPH_HEIGHT * 2);
        assert!(fps_only.width < compact.width);
        assert!(detailed.muted_glyphs.length > compact.muted_glyphs.length);
        assert_eq!(detailed.height, DETAILED_PANEL_HEIGHT);
    }

    #[test]
    fn metric_layouts_keep_the_same_selected_metrics_in_distinct_bounded_geometry() {
        let metrics = OverlaySnapshot {
            fps: Some(144),
            frame_time_tenths_ms: Some(69),
            one_percent_low_fps: Some(121),
            point_one_percent_low_fps: Some(110),
            cpu_percent: Some(42),
            cpu_temperature_c: Some(61),
            gpu_percent: Some(87),
            gpu_temperature_c: Some(68),
        };
        let grid = OverlayPlan::new(metrics, OverlayConfig::default());
        let ribbon = OverlayPlan::new(
            metrics,
            OverlayConfig {
                layout: OverlayLayout::Ribbon,
                ..OverlayConfig::default()
            },
        );
        let telemetry = OverlayPlan::new(
            metrics,
            OverlayConfig {
                layout: OverlayLayout::Telemetry,
                ..OverlayConfig::default()
            },
        );

        assert_eq!(grid.width, PANEL_WIDTH);
        assert_eq!(ribbon.width, RIBBON_BRAND_WIDTH + 6 * RIBBON_METRIC_WIDTH);
        assert_eq!(ribbon.height, RIBBON_HEIGHT);
        assert_eq!(telemetry.width, TELEMETRY_WIDTH);
        assert!(telemetry.height > ribbon.height);
        let heading_right = telemetry
            .muted_glyphs
            .as_slice()
            .iter()
            .filter(|glyph| glyph.y == 6)
            .map(|glyph| glyph.x + i32::try_from(overlay_font::GLYPH_WIDTH).unwrap_or(0))
            .max()
            .expect("telemetry heading glyphs");
        let first_value_right = telemetry
            .text_glyphs
            .as_slice()
            .iter()
            .filter(|glyph| glyph.y == 31)
            .map(|glyph| glyph.x + i32::try_from(overlay_font::GLYPH_WIDTH).unwrap_or(0))
            .max()
            .expect("first telemetry value glyphs");
        assert_eq!(heading_right, first_value_right);
        for plan in [&grid, &ribbon, &telemetry] {
            assert!(plan.panel.is_some());
            assert_eq!(plan.panel_radius, METRIC_PANEL_RADIUS);
            assert!(plan.text_glyphs.length > 0);
            assert!(plan.muted_glyphs.length > 0);
            assert!(plan.text_glyphs.length <= MAX_TEXT_GLYPHS);
            assert!(plan.muted_glyphs.length <= MAX_TEXT_GLYPHS);
        }

        let short_ribbon = OverlayPlan::new(
            metrics,
            OverlayConfig {
                preset: OverlayPreset::Custom,
                layout: OverlayLayout::Ribbon,
                metrics: METRIC_FPS | METRIC_FRAME_TIME,
                ..OverlayConfig::default()
            },
        );
        let detailed_ribbon = OverlayPlan::new(
            metrics,
            OverlayConfig {
                preset: OverlayPreset::Detailed,
                layout: OverlayLayout::Ribbon,
                ..OverlayConfig::default()
            },
        );
        assert_eq!(
            short_ribbon.width,
            RIBBON_BRAND_WIDTH + 2 * RIBBON_METRIC_WIDTH
        );
        assert_eq!(
            detailed_ribbon.width,
            RIBBON_BRAND_WIDTH + 8 * RIBBON_METRIC_WIDTH
        );
        assert!(short_ribbon.width < ribbon.width);
        assert!(ribbon.width < detailed_ribbon.width);
    }

    #[test]
    fn branding_can_be_hidden_without_removing_metrics() {
        let snapshot = OverlaySnapshot {
            fps: Some(144),
            frame_time_tenths_ms: Some(69),
            ..OverlaySnapshot::default()
        };
        let branded = OverlayPlan::new(snapshot, OverlayConfig::default());
        let unbranded = OverlayPlan::new(
            snapshot,
            OverlayConfig {
                branding_visible: false,
                ..OverlayConfig::default()
            },
        );
        assert!(branded.accent_glyphs.length > unbranded.accent_glyphs.length);
        assert!(unbranded.text_glyphs.length > 0);

        let ribbon = OverlayPlan::new(
            snapshot,
            OverlayConfig {
                layout: OverlayLayout::Ribbon,
                branding_visible: false,
                ..OverlayConfig::default()
            },
        );
        assert_eq!(ribbon.width, 2 * RIBBON_METRIC_WIDTH);
    }

    #[test]
    fn larger_bitmap_glyphs_and_row_spacing_remain_inside_the_plan() {
        let mut glyph_batch = GlyphBatch::default();
        push_text(&mut glyph_batch, b"8", 0, 0);
        let eight = glyph_batch.as_slice().first().expect("8 has a glyph");
        let glyph_right = eight.x + i32::try_from(overlay_font::GLYPH_WIDTH).unwrap();
        let glyph_bottom = eight.y + i32::try_from(overlay_font::GLYPH_HEIGHT).unwrap();
        assert_eq!(glyph_right, 9);
        assert_eq!(glyph_bottom, 12);
        assert_eq!(glyph_bottom, i32::try_from(BITMAP_GLYPH_HEIGHT).unwrap());
        assert_eq!(PRIMARY_ROW_HEIGHT - overlay_font::GLYPH_HEIGHT * 2, 14);
        assert_eq!(SECONDARY_ROW_HEIGHT - overlay_font::GLYPH_HEIGHT, 14);
        assert_eq!(
            FIRST_METRIC_ROW_Y - i32::try_from(PANEL_BASE_HEIGHT).unwrap(),
            6
        );
        assert_eq!(ROW_DIVIDER_OFFSET, 7);

        let detailed = OverlayPlan::new(
            OverlaySnapshot {
                fps: Some(92),
                frame_time_tenths_ms: Some(96),
                one_percent_low_fps: Some(70),
                point_one_percent_low_fps: Some(64),
                cpu_percent: Some(47),
                cpu_temperature_c: Some(68),
                gpu_percent: Some(93),
                gpu_temperature_c: Some(63),
            },
            OverlayConfig {
                preset: OverlayPreset::Detailed,
                ..OverlayConfig::default()
            },
        );
        let divider_rows = detailed
            .dividers
            .as_slice()
            .iter()
            .filter(|divider| divider.rect.extent.height == 1)
            .map(|divider| divider.rect.offset.y)
            .collect::<Vec<_>>();
        assert!(divider_rows.contains(&62));
        assert!(divider_rows.contains(&88));
        assert_eq!(detailed.height, 115);

        let mut text = GlyphBatch::default();
        push_text(&mut text, b"FPS 144", 0, 0);
        assert_eq!(text.length, 6);

        let mut separated = FixedText::<8>::default();
        separated.push(b'A');
        separate_metric(&mut separated);
        separated.push(b'C');
        assert_eq!(separated.as_bytes(), b"A  C");

        let detailed = OverlayPlan::new(
            OverlaySnapshot {
                fps: Some(9_999),
                frame_time_tenths_ms: Some(u16::MAX),
                one_percent_low_fps: Some(9_999),
                point_one_percent_low_fps: Some(9_999),
                cpu_percent: Some(100),
                cpu_temperature_c: Some(100),
                gpu_percent: Some(100),
                gpu_temperature_c: Some(100),
            },
            OverlayConfig {
                preset: OverlayPreset::Detailed,
                ..OverlayConfig::default()
            },
        );
        let plan_width = i32::try_from(detailed.width).expect("plan width fits i32");
        let plan_height = i32::try_from(detailed.height).expect("plan height fits i32");
        for rect in detailed.accent.as_slice() {
            let right = rect.rect.offset.x
                + i32::try_from(rect.rect.extent.width).expect("plan width fits i32");
            let bottom = rect.rect.offset.y
                + i32::try_from(rect.rect.extent.height).expect("plan height fits i32");
            assert!(rect.rect.offset.x >= 0 && right <= plan_width);
            assert!(rect.rect.offset.y >= 0 && bottom <= plan_height);
        }
        for glyph in detailed
            .accent_glyphs
            .as_slice()
            .iter()
            .chain(detailed.muted_glyphs.as_slice())
            .chain(detailed.text_glyphs.as_slice())
        {
            let right = glyph.x
                + i32::try_from(overlay_font::GLYPH_WIDTH * u32::from(glyph.scale)).unwrap();
            let bottom = glyph.y
                + i32::try_from(overlay_font::GLYPH_HEIGHT * u32::from(glyph.scale)).unwrap();
            assert!(glyph.x >= 0 && right <= plan_width);
            assert!(glyph.y >= 0 && bottom <= plan_height);
        }
    }

    #[test]
    fn secondary_metrics_anchor_their_right_hand_values_to_the_panel_edge() {
        let mut point_one = FixedText::<16>::default();
        point_one.push_bytes(b"0.1% LOW ");
        point_one.push_metric_unpadded(Some(87), 3);
        let x = right_aligned_x(&point_one, 8);
        let width = i32::try_from(point_one.length).unwrap() * overlay_font::GLYPH_ADVANCE;
        assert_eq!(x + width, i32::try_from(PANEL_WIDTH).unwrap() - 8);

        let mut gpu = FixedText::<16>::default();
        push_device_metrics(&mut gpu, b"GPU", true, true, Some(99), Some(71));
        let x = right_aligned_x(&gpu, 8);
        let width = i32::try_from(gpu.length).unwrap() * overlay_font::GLYPH_ADVANCE;
        assert_eq!(x + width, i32::try_from(PANEL_WIDTH).unwrap() - 8);
    }

    #[test]
    fn corner_origins_keep_the_overlay_inside_the_swapchain_margin() {
        let extent = VkExtent2d {
            width: 640,
            height: 360,
        };
        let mut plan = OverlayPlan::new(OverlaySnapshot::default(), OverlayConfig::default());
        assert_eq!(overlay_origin(extent, &plan, plan.scale_percent), (12, 12));
        plan.corner = OverlayCorner::TopRight;
        assert_eq!(overlay_origin(extent, &plan, plan.scale_percent), (334, 12));
        plan.corner = OverlayCorner::BottomLeft;
        let bottom = i32::try_from(extent.height - plan.height - 12).unwrap();
        assert_eq!(
            overlay_origin(extent, &plan, plan.scale_percent),
            (12, bottom)
        );
        plan.corner = OverlayCorner::BottomRight;
        assert_eq!(
            overlay_origin(extent, &plan, plan.scale_percent),
            (334, bottom)
        );
    }

    #[test]
    fn malformed_customization_tokens_use_safe_defaults() {
        assert_eq!(
            parse_overlay_preset(Some("unknown")),
            OverlayPreset::Compact
        );
        assert_eq!(
            parse_overlay_preset(Some("fps-only")),
            OverlayPreset::FpsOnly
        );
        assert_eq!(
            parse_overlay_corner(Some("unknown")),
            OverlayCorner::TopLeft
        );
        assert_eq!(
            parse_overlay_corner(Some("bottom-right")),
            OverlayCorner::BottomRight
        );
        assert_eq!(parse_overlay_layout(Some("unknown")), OverlayLayout::Grid);
        assert_eq!(parse_overlay_layout(Some("ribbon")), OverlayLayout::Ribbon);
        assert_eq!(
            parse_overlay_layout(Some("telemetry")),
            OverlayLayout::Telemetry
        );
        for (token, palette) in [
            ("glacier", OverlayPalette::Glacier),
            ("ember", OverlayPalette::Ember),
            ("mint", OverlayPalette::Mint),
            ("mono", OverlayPalette::Mono),
            ("amethyst", OverlayPalette::Amethyst),
            ("solar", OverlayPalette::Solar),
            ("rose", OverlayPalette::Rose),
        ] {
            assert_eq!(parse_overlay_palette(Some(token)), palette);
        }
        assert_eq!(
            parse_overlay_palette(Some("unknown")),
            OverlayPalette::Redunar
        );
        assert_eq!(parse_overlay_metrics(None), METRICS_COMPACT);
        assert_eq!(parse_overlay_metrics(Some("0")), METRICS_COMPACT);
        assert_eq!(parse_overlay_metrics(Some("65535")), METRICS_COMPACT);
        assert_eq!(
            parse_overlay_metrics(Some(&(METRIC_FPS | METRIC_GPU_LOAD).to_string())),
            METRIC_FPS | METRIC_GPU_LOAD
        );
        assert_eq!(parse_overlay_opacity(None), 50);
        assert_eq!(parse_overlay_opacity(Some("0")), 0);
        assert_eq!(parse_overlay_opacity(Some("65")), 65);
        assert_eq!(parse_overlay_opacity(Some("100")), 100);
        assert_eq!(parse_overlay_opacity(Some("101")), 50);
        assert_eq!(parse_overlay_opacity(Some("invalid")), 50);
    }

    #[test]
    fn custom_preset_uses_only_selected_metric_rows() {
        let metrics = OverlaySnapshot {
            fps: Some(120),
            gpu_percent: Some(80),
            ..OverlaySnapshot::default()
        };
        let fps_and_gpu = OverlayPlan::new(
            metrics,
            OverlayConfig {
                preset: OverlayPreset::Custom,
                metrics: METRIC_FPS | METRIC_GPU_LOAD,
                ..OverlayConfig::default()
            },
        );
        let detailed = OverlayPlan::new(
            metrics,
            OverlayConfig {
                preset: OverlayPreset::Detailed,
                ..OverlayConfig::default()
            },
        );
        assert_eq!(
            fps_and_gpu.height,
            PANEL_BASE_HEIGHT + PRIMARY_ROW_HEIGHT + SECONDARY_ROW_HEIGHT
        );
        assert!(fps_and_gpu.text_glyphs.length < detailed.text_glyphs.length);
        assert!(fps_and_gpu.panel.is_some());
    }

    #[test]
    fn hardware_values_round_from_tenths_and_keep_missing_values_distinct() {
        let snapshot = hardware_snapshot(OverlayHardwareTelemetry {
            revision: 2,
            corner: 0,
            preset: 0,
            layout: 0,
            palette: 0,
            metrics: METRICS_COMPACT,
            opacity_percent: 50,
            scale_percent: 100,
            replay_saved_revision: 0,
            metrics_visible: None,
            branding_visible: true,
            cpu_utilization_tenths: Some(994),
            cpu_temperature_tenths_celsius: Some(615),
            gpu_utilization_tenths: None,
            gpu_temperature_tenths_celsius: Some(689),
        });
        assert_eq!(snapshot.cpu_percent, Some(99));
        assert_eq!(snapshot.cpu_temperature_c, Some(62));
        assert_eq!(snapshot.gpu_percent, None);
        assert_eq!(snapshot.gpu_temperature_c, Some(69));

        let no_hardware = OverlayPlan::new(
            OverlaySnapshot {
                fps: Some(120),
                frame_time_tenths_ms: Some(83),
                ..OverlaySnapshot::default()
            },
            OverlayConfig::default(),
        );
        let available_hardware = OverlayPlan::new(snapshot, OverlayConfig::default());
        assert_eq!(no_hardware.height, PANEL_BASE_HEIGHT + PRIMARY_ROW_HEIGHT);
        assert_eq!(
            available_hardware.height,
            PANEL_BASE_HEIGHT + PRIMARY_ROW_HEIGHT + SECONDARY_ROW_HEIGHT
        );
        assert!(available_hardware.text_glyphs.length > 0);
    }

    #[test]
    fn unknown_glyphs_do_not_add_rectangles() {
        let mut batch = GlyphBatch::default();
        push_text(&mut batch, b" ", 0, 0);
        assert_eq!(batch.length, 0);
    }

    #[test]
    fn shared_font_uses_the_fixed_antialiased_raster_contract() {
        assert_eq!(overlay_font::GLYPH_COLUMNS, 9);
        assert_eq!(overlay_font::GLYPH_ROWS, 12);
        assert_eq!(overlay_font::PIXEL_WIDTH, 1);
        assert_eq!(overlay_font::GLYPH_WIDTH, 9);
        assert_eq!(overlay_font::GLYPH_HEIGHT, 12);
        let f = overlay_font::glyph(b'F');
        assert_eq!(f[0], 0);
        assert_eq!(f[11], 0);
        assert!(f[1..11].iter().all(|row| *row != 0));
        assert_eq!(overlay_font::COVERAGE_WIDTH, 18);
        assert_eq!(overlay_font::COVERAGE_HEIGHT, 24);
        assert_eq!(mem::size_of::<GlyphPushConstants>(), 108);
        let pushed = GlyphPushConstants::from_raster(overlay_font::coverage_raster(b'F'));
        assert_eq!(pushed.raster(), overlay_font::coverage_raster(b'F'));
    }

    #[test]
    fn glyph_shader_keeps_driver_safe_scalar_push_members() {
        let source = include_str!("shaders/glyph.frag");
        assert!(!source.contains("uint words["));
        assert!(source.contains("vec2(18.0, 24.0)"));
        assert!(source.contains("mix("));
        for index in 0..overlay_font::COVERAGE_WORDS {
            assert!(source.contains(&format!("uint word{index};")));
        }
    }

    #[test]
    fn panel_shader_clips_only_the_requested_rounded_bounds() {
        assert_eq!(mem::size_of::<PanelPushConstants>(), 24);
        let source = include_str!("shaders/panel.frag");
        assert!(source.contains("layout(push_constant)"));
        assert!(source.contains("rounded_distance"));
        assert!(source.contains("discard"));
        let pushed = PanelPushConstants::from_rect(
            VkRect2d {
                offset: VkOffset2d { x: 10, y: 20 },
                extent: VkExtent2d {
                    width: 690,
                    height: 440,
                },
            },
            12,
            OverlayPalette::Glacier.code(),
        );
        assert_eq!(
            pushed.bounds.map(f32::to_bits),
            [10.0_f32, 20.0, 690.0, 440.0].map(f32::to_bits)
        );
        assert_eq!(pushed.radius.to_bits(), 12.0_f32.to_bits());
        assert_eq!(pushed.palette, 1);
    }

    #[test]
    fn render_replay_menu_reference() {
        use std::fmt::Write as _;
        let Ok(path) = std::env::var("REDUNAR_REPLAY_MENU_REFERENCE") else {
            return;
        };
        let plan = OverlayPlan::replay_menu(ReplayMenuView {
            animation_progress: u16::MAX,
            status: 2,
            available_seconds: 90,
            capture_fps: 120,
            quality: 1,
            format: 1,
            selected_duration: 1,
            cursor_x: 8_200,
            cursor_y: 7_800,
            save_enabled: true,
            overlay_shortcut: ReplayShortcutLabel::from_shortcut("Ctrl+Shift+R"),
            save_shortcut: ReplayShortcutLabel::from_shortcut("Ctrl+F9"),
            ..ReplayMenuView::default()
        });
        let mut svg = String::from(&format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='832' height='458' viewBox='0 0 520 286'><rect width='520' height='286' fill='#151922'/><rect x='0.5' y='0.5' width='519' height='285' rx='11.5' fill='#{}' stroke='#{}'/>",
            hex(MENU_PANEL_COLOR),
            hex(plan.colors.divider)
        ));
        for (batch, color) in [
            (
                &plan.logo_base,
                format!("#{}", hex([0.007, 0.007, 0.01, 1.0])),
            ),
            (&plan.logo_dark_red, format!("#{}", hex(MENU_SELECTED_FILL))),
            (&plan.accent, format!("#{}", hex(plan.colors.accent))),
            (&plan.dividers, format!("#{}", hex(plan.colors.divider))),
            (&plan.cursor_shadow, "#050505".to_string()),
            (&plan.cursor, "#f0f2f7".to_string()),
        ] {
            for rect in batch.as_slice() {
                let rect = rect.rect;
                write!(
                    svg,
                    "<rect x='{}' y='{}' width='{}' height='{}' fill='{color}'/>",
                    rect.offset.x, rect.offset.y, rect.extent.width, rect.extent.height
                )
                .unwrap();
            }
        }
        for (batch, color) in [
            (&plan.accent_glyphs, format!("#{}", hex(plan.colors.accent))),
            (&plan.muted_glyphs, format!("#{}", hex(plan.colors.muted))),
            (&plan.text_glyphs, format!("#{}", hex(plan.colors.text))),
            (
                &plan.highlight_glyphs,
                format!("#{}", hex(plan.colors.highlight)),
            ),
        ] {
            for glyph in batch.as_slice() {
                let raster = if glyph.ui_font {
                    overlay_font::ui_coverage_raster(glyph.byte)
                } else {
                    overlay_font::coverage_raster(glyph.byte)
                };
                for y in 0..24_u32 {
                    for x in 0..18_u32 {
                        let coverage = overlay_font::coverage_pixel(raster, x, y);
                        if coverage == 0 {
                            continue;
                        }
                        let step = f64::from(glyph.scale) / 2.0;
                        write!(svg,"<rect x='{}' y='{}' width='{step}' height='{step}' fill='{color}' opacity='{}'/>",f64::from(glyph.x)+f64::from(x)*step,f64::from(glyph.y)+f64::from(y)*step,f64::from(coverage)/3.0).unwrap();
                    }
                }
            }
        }
        svg.push_str("</svg>");
        std::fs::write(path, svg).unwrap();
    }

    #[test]
    fn render_metrics_reference() {
        use std::fmt::Write as _;
        let Ok(path) = std::env::var("REDUNAR_METRICS_REFERENCE") else {
            return;
        };
        let layout = std::env::var("REDUNAR_METRICS_REFERENCE_LAYOUT").unwrap_or_default();
        let snapshot = OverlaySnapshot {
            fps: Some(144),
            frame_time_tenths_ms: Some(69),
            one_percent_low_fps: Some(118),
            point_one_percent_low_fps: Some(96),
            cpu_percent: Some(38),
            cpu_temperature_c: Some(62),
            gpu_percent: Some(91),
            gpu_temperature_c: Some(68),
        };
        let config = OverlayConfig {
            preset: OverlayPreset::Detailed,
            layout: match layout.as_str() {
                "ribbon" => OverlayLayout::Ribbon,
                "telemetry" => OverlayLayout::Telemetry,
                _ => OverlayLayout::Grid,
            },
            ..OverlayConfig::default()
        };
        let plan = OverlayPlan::new(snapshot, config);
        let colors = plan.colors;
        let mut svg = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='{}' height='{}' viewBox='0 0 {} {}'><rect width='{}' height='{}' fill='#{}' rx='4'/>",
            plan.width * 2,
            plan.height * 2,
            plan.width,
            plan.height,
            plan.width,
            plan.height,
            hex(plan.palette.panel_color())
        );
        for (batch, color) in [
            (&plan.accent, hex(colors.accent)),
            (&plan.dividers, hex(colors.divider)),
        ] {
            for rect in batch.as_slice() {
                let rect = rect.rect;
                write!(
                    svg,
                    "<rect x='{}' y='{}' width='{}' height='{}' fill='#{color}'/>",
                    rect.offset.x, rect.offset.y, rect.extent.width, rect.extent.height
                )
                .unwrap();
            }
        }
        for (batch, color) in [
            (&plan.accent_glyphs, hex(colors.accent)),
            (&plan.muted_glyphs, hex(colors.muted)),
            (&plan.text_glyphs, hex(colors.text)),
        ] {
            for glyph in batch.as_slice() {
                let raster = overlay_font::coverage_raster(glyph.byte);
                for y in 0..24_u32 {
                    for x in 0..18_u32 {
                        let coverage = overlay_font::coverage_pixel(raster, x, y);
                        if coverage == 0 {
                            continue;
                        }
                        let step = f64::from(glyph.scale) / 2.0;
                        write!(
                            svg,
                            "<rect x='{}' y='{}' width='{step}' height='{step}' fill='#{color}' opacity='{}'/>",
                            f64::from(glyph.x) + f64::from(x) * step,
                            f64::from(glyph.y) + f64::from(y) * step,
                            f64::from(coverage) / 3.0
                        )
                        .unwrap();
                    }
                }
            }
        }
        svg.push_str("</svg>");
        std::fs::write(path, svg).unwrap();
    }

    #[test]
    fn malformed_visibility_values_are_hidden() {
        assert_ne!(Some("1"), Some("true"));
        assert_ne!(Some("1"), Some("01"));
        assert_eq!(Some("1"), Some("1"));
    }

    /// Host-dependent validation for the embedded SPIR-V and exact blend
    /// pipeline state. It creates no surface or window and is safe to run with
    /// Mesa's `lvp` software ICD while a desktop session is active.
    #[test]
    #[ignore = "requires a local Vulkan loader and ICD; run explicitly with the headless lvp ICD"]
    #[expect(
        clippy::too_many_lines,
        reason = "the host-dependent probe keeps one linear Vulkan ownership and teardown sequence for auditability"
    )]
    fn blend_pipeline_is_accepted_by_headless_vulkan() {
        #[link(name = "dl")]
        unsafe extern "C" {
            fn dlopen(name: *const c_char, flags: i32) -> *mut std::ffi::c_void;
            fn dlsym(handle: *mut std::ffi::c_void, name: *const c_char) -> *mut std::ffi::c_void;
            fn dlclose(handle: *mut std::ffi::c_void) -> i32;
        }
        type CreateInstance = unsafe extern "system" fn(
            *const VkInstanceCreateInfo,
            *const VkAllocationCallbacks,
            *mut VkInstance,
        ) -> VkResult;
        type DestroyInstance = unsafe extern "system" fn(VkInstance, *const VkAllocationCallbacks);
        type EnumeratePhysicalDevices =
            unsafe extern "system" fn(VkInstance, *mut u32, *mut VkPhysicalDevice) -> VkResult;
        type GetQueueFamilies =
            unsafe extern "system" fn(VkPhysicalDevice, *mut u32, *mut VkQueueFamilyProperties);
        type CreateDevice = unsafe extern "system" fn(
            VkPhysicalDevice,
            *const VkDeviceCreateInfo,
            *const VkAllocationCallbacks,
            *mut VkDevice,
        ) -> VkResult;
        type DestroyDevice = unsafe extern "system" fn(VkDevice, *const VkAllocationCallbacks);

        // Loading explicitly avoids resolving these names to Redunar's own
        // exported layer intercepts inside the test binary.
        let library_name = b"libvulkan.so.1\0";
        // SAFETY: the loader name is a terminated static string.
        let library = unsafe { dlopen(library_name.as_ptr().cast::<c_char>(), 2) };
        assert!(!library.is_null(), "system Vulkan loader is available");
        macro_rules! load {
            ($name:literal, $ty:ty) => {{
                let name = concat!($name, "\0");
                // SAFETY: the live loader handle accepts terminated symbol names.
                let raw = unsafe { dlsym(library, name.as_ptr().cast::<c_char>()) };
                assert!(!raw.is_null(), "Vulkan loader exports {}", $name);
                // SAFETY: each requested symbol name fixes the exact ABI type.
                unsafe { mem::transmute::<*mut std::ffi::c_void, $ty>(raw) }
            }};
        }
        let create_instance = load!("vkCreateInstance", CreateInstance);
        let destroy_instance = load!("vkDestroyInstance", DestroyInstance);
        let enumerate_physical_devices =
            load!("vkEnumeratePhysicalDevices", EnumeratePhysicalDevices);
        let get_queue_families =
            load!("vkGetPhysicalDeviceQueueFamilyProperties", GetQueueFamilies);
        let create_device = load!("vkCreateDevice", CreateDevice);
        let destroy_device = load!("vkDestroyDevice", DestroyDevice);
        let get_device_proc_addr = load!("vkGetDeviceProcAddr", PfnGetDeviceProcAddr);

        let instance_info = VkInstanceCreateInfo {
            s_type: 1,
            p_next: std::ptr::null(),
            flags: 0,
            p_application_info: std::ptr::null(),
            enabled_layer_count: 0,
            enabled_layer_names: std::ptr::null(),
            enabled_extension_count: 0,
            enabled_extension_names: std::ptr::null(),
        };
        let mut instance = std::ptr::null_mut();
        // SAFETY: the create info is complete and output storage is valid.
        assert_eq!(
            unsafe {
                create_instance(
                    &raw const instance_info,
                    std::ptr::null(),
                    &raw mut instance,
                )
            },
            VK_SUCCESS
        );

        let mut physical_count = 0;
        // SAFETY: count-only enumeration for a live instance.
        assert_eq!(
            unsafe {
                enumerate_physical_devices(instance, &raw mut physical_count, std::ptr::null_mut())
            },
            VK_SUCCESS
        );
        assert!(physical_count > 0);
        let mut physical_devices = vec![std::ptr::null_mut(); physical_count as usize];
        // SAFETY: the vector has the enumerated number of output slots.
        assert_eq!(
            unsafe {
                enumerate_physical_devices(
                    instance,
                    &raw mut physical_count,
                    physical_devices.as_mut_ptr(),
                )
            },
            VK_SUCCESS
        );
        let physical_device = physical_devices[0];

        let mut family_count = 0;
        // SAFETY: count-only queue-family query for a live physical device.
        unsafe {
            get_queue_families(physical_device, &raw mut family_count, std::ptr::null_mut());
        }
        let mut families = vec![
            VkQueueFamilyProperties {
                queue_flags: 0,
                queue_count: 0,
                timestamp_valid_bits: 0,
                min_image_transfer_granularity: VkExtent3d {
                    width: 0,
                    height: 0,
                    depth: 0,
                },
            };
            family_count as usize
        ];
        // SAFETY: the vector has the queried number of output slots.
        unsafe {
            get_queue_families(
                physical_device,
                &raw mut family_count,
                families.as_mut_ptr(),
            );
        }
        let family_index = families
            .iter()
            .position(|family| family.queue_flags & VK_QUEUE_GRAPHICS_BIT != 0)
            .and_then(|index| u32::try_from(index).ok())
            .expect("headless Vulkan device has a graphics queue");

        let priority = 1.0_f32;
        let queue_info = VkDeviceQueueCreateInfo {
            s_type: 2,
            p_next: std::ptr::null(),
            flags: 0,
            queue_family_index: family_index,
            queue_count: 1,
            queue_priorities: &raw const priority,
        };
        let extension = b"VK_KHR_swapchain\0";
        let extension_name = extension.as_ptr().cast::<c_char>();
        let device_info = VkDeviceCreateInfo {
            s_type: 3,
            p_next: std::ptr::null(),
            flags: 0,
            queue_create_info_count: 1,
            queue_create_infos: &raw const queue_info,
            enabled_layer_count: 0,
            enabled_layer_names: std::ptr::null(),
            enabled_extension_count: 1,
            enabled_extension_names: &raw const extension_name,
            enabled_features: std::ptr::null(),
        };
        let mut device = std::ptr::null_mut();
        // SAFETY: queue and extension inputs remain live through device creation.
        assert_eq!(
            unsafe {
                create_device(
                    physical_device,
                    &raw const device_info,
                    std::ptr::null(),
                    &raw mut device,
                )
            },
            VK_SUCCESS
        );
        // SAFETY: this is the loader's exact generic device lookup function.
        let functions = unsafe { DeviceFunctions::load(get_device_proc_addr, device) }
            .expect("required Vulkan 1.0 and swapchain entry points");

        let attachment = VkAttachmentDescription {
            flags: 0,
            format: VK_FORMAT_B8G8R8A8_UNORM,
            samples: VK_SAMPLE_COUNT_1_BIT,
            load_op: VK_ATTACHMENT_LOAD_OP_LOAD,
            store_op: VK_ATTACHMENT_STORE_OP_STORE,
            stencil_load_op: VK_ATTACHMENT_LOAD_OP_DONT_CARE,
            stencil_store_op: VK_ATTACHMENT_STORE_OP_DONT_CARE,
            initial_layout: VK_IMAGE_LAYOUT_PRESENT_SRC_KHR,
            final_layout: VK_IMAGE_LAYOUT_PRESENT_SRC_KHR,
        };
        let reference = VkAttachmentReference {
            attachment: 0,
            layout: VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
        };
        let subpass = VkSubpassDescription {
            flags: 0,
            pipeline_bind_point: VK_PIPELINE_BIND_POINT_GRAPHICS,
            input_attachment_count: 0,
            input_attachments: std::ptr::null(),
            color_attachment_count: 1,
            color_attachments: &raw const reference,
            resolve_attachments: std::ptr::null(),
            depth_stencil_attachment: std::ptr::null(),
            preserve_attachment_count: 0,
            preserve_attachments: std::ptr::null(),
        };
        let render_pass_info = VkRenderPassCreateInfo {
            s_type: VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            attachment_count: 1,
            attachments: &raw const attachment,
            subpass_count: 1,
            subpasses: &raw const subpass,
            dependency_count: 0,
            dependencies: std::ptr::null(),
        };
        let mut render_pass = 0;
        // SAFETY: render-pass inputs are complete and use a standard color format.
        assert_eq!(
            unsafe {
                (functions.create_render_pass)(
                    device,
                    &raw const render_pass_info,
                    std::ptr::null(),
                    &raw mut render_pass,
                )
            },
            VK_SUCCESS
        );
        // SAFETY: the render pass and device are live and compatible.
        let (layout, pipeline) =
            unsafe { create_overlay_pipeline(device, functions, render_pass, false) }
                .expect("embedded alpha-blend pipeline must be accepted");
        let (glyph_layout, glyph_pipeline) =
            unsafe { create_overlay_pipeline(device, functions, render_pass, true) }
                .expect("embedded binary-glyph pipeline must be accepted");

        // SAFETY: teardown reverses ownership after no queue work was submitted.
        unsafe {
            (functions.destroy_pipeline)(device, pipeline, std::ptr::null());
            (functions.destroy_pipeline_layout)(device, layout, std::ptr::null());
            (functions.destroy_pipeline)(device, glyph_pipeline, std::ptr::null());
            (functions.destroy_pipeline_layout)(device, glyph_layout, std::ptr::null());
            (functions.destroy_render_pass)(device, render_pass, std::ptr::null());
            destroy_device(device, std::ptr::null());
            destroy_instance(instance, std::ptr::null());
            assert_eq!(dlclose(library), 0);
        }
    }
}
