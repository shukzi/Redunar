//! Linux platform adapters. Hardware discovery is read-only; launch adapters
//! create only private, per-user runtime state and child-process environments.

mod background;
mod desktop_discovery;
mod game_process;
mod launch;
mod linux;
mod process_control;
mod steam_discovery;
mod steam_launch_bridge;
mod steam_launch_options;

pub use background::{BackgroundCoordinator, BackgroundLoad, observe_background_load};
pub use desktop_discovery::DesktopGameDiscovery;
pub use game_process::{GameDetectionError, LinuxGameProcessDetector};
pub use launch::{
    CaptureLaunchError, CaptureLaunchPlan, CaptureLaunchSupport, CaptureLaunchUnavailableReason,
    GameLaunchProcessOwnership, OverlayLaunchConfig, REDUNAR_OVERLAY_CORNER_ENV,
    REDUNAR_OVERLAY_LAYOUT_ENV, REDUNAR_OVERLAY_METRICS_ENV, REDUNAR_OVERLAY_OPACITY_ENV,
    REDUNAR_OVERLAY_PALETTE_ENV, REDUNAR_OVERLAY_PRESET_ENV, REDUNAR_OVERLAY_TELEMETRY_ENV,
    REDUNAR_OVERLAY_VISIBLE_ENV, REDUNAR_REPLAY_FRAME_RATE_ENV, REDUNAR_REPLAY_PRODUCTION_ENV,
    REDUNAR_REPLAY_TRANSFER_ENV, ReplayTransferLaunchConfig, VULKAN_CAPTURE_LAYER_NAME,
    VULKAN_CAPTURE_MANIFEST_FILE, capture_launch_support, game_launch_process_ownership,
    prepare_vulkan_layer_directory, supports_host_capture_launch,
    supports_host_explicit_layer_launch,
};
pub use linux::{LinuxCpuUtilizationSampler, LinuxHardwareProbe, LinuxTelemetrySampler};
pub use process_control::{
    AffinityMask, apply_affinity, apply_priority, read_affinity, read_priority,
};
pub use steam_discovery::{
    DiscoveredGame, GameDiscoveryError, GameDiscoverySource, SteamGameDiscovery,
    candidate_from_process,
};
pub use steam_launch_bridge::{
    DEFAULT_STEAM_ACTIVATION_TTL, SteamActivationBroker, SteamActivationError,
    SteamActivationState, SteamAppId, SteamCaptureEnvironment, claim_steam_activation,
    resolve_steam_wrapper_environment, steam_broker_socket_path,
};
pub use steam_launch_options::{
    SteamLaunchOptionsDetector, SteamLaunchOptionsReason, SteamLaunchOptionsStatus,
};

mod memory;
