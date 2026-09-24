//! Private one-shot environment handoff for Steam Launch Options.
//!
//! Steam starts this package's small wrapper as the game command, so the
//! already-running client never needs to inherit Redunar state. The daemon
//! publishes one short-lived activation on a mode-0600 Unix socket. The
//! wrapper claims it once, merges only this module's fixed fields into its
//! inherited environment, and directly `exec`s Steam's original argv.

use crate::launch::{
    REDUNAR_CAPTURE_REPLY_SOCKET_ENV, REDUNAR_OVERLAY_BRANDING_ENV, REDUNAR_OVERLAY_CORNER_ENV,
    REDUNAR_OVERLAY_LAYOUT_ENV, REDUNAR_OVERLAY_METRICS_ENV, REDUNAR_OVERLAY_OPACITY_ENV,
    REDUNAR_OVERLAY_PALETTE_ENV, REDUNAR_OVERLAY_PRESET_ENV, REDUNAR_OVERLAY_TELEMETRY_ENV,
    REDUNAR_OVERLAY_VISIBLE_ENV, REDUNAR_REPLAY_FRAME_RATE_ENV, REDUNAR_REPLAY_PRODUCTION_ENV,
    REDUNAR_REPLAY_TRANSFER_ENV, ReplayTransferLaunchConfig, VULKAN_CAPTURE_LAYER_NAME,
};
use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
use redunar_capture::{CaptureSessionId, PROTOCOL_VERSION};
use redunar_core::{
    OverlayCorner, OverlayLayout, OverlayMetricSet, OverlayOpacity, OverlayPalette, OverlayPreset,
    ReplayFrameRate,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const STEAM_BROKER_SOCKET_FILE: &str = "steam-launch-v1.sock";
const STEAM_WIRE_MAGIC: [u8; 8] = *b"RDSTML01";
const STEAM_WIRE_VERSION: u16 = 5;
const REQUEST_BYTES: usize = 18;
const MAX_PATH_BYTES: usize = 4_096;
const MAX_RESPONSE_BYTES: usize = 5 * MAX_PATH_BYTES + 64;
const RESPONSE_DENIED: u8 = 0;
const RESPONSE_GRANTED: u8 = 1;
const STREAM_TIMEOUT: Duration = Duration::from_millis(500);
const BROKER_POLL_INTERVAL: Duration = Duration::from_millis(250);
// Keep a pending activation long enough for Steam shader preparation while
// retaining a strict one-shot, same-user expiry window.
const MAX_STEAM_ACTIVATION_TTL: Duration = Duration::from_mins(5);
const PRESSURE_VESSEL_FILESYSTEMS_RW_ENV: &str = "PRESSURE_VESSEL_FILESYSTEMS_RW";

pub const DEFAULT_STEAM_ACTIVATION_TTL: Duration = Duration::from_mins(5);

const MANAGED_ENVIRONMENT_NAMES: [&str; 18] = [
    "REDUNAR_CAPTURE_SOCKET",
    REDUNAR_CAPTURE_REPLY_SOCKET_ENV,
    "REDUNAR_CAPTURE_SESSION",
    "REDUNAR_CAPTURE_PROTOCOL",
    REDUNAR_OVERLAY_VISIBLE_ENV,
    REDUNAR_OVERLAY_TELEMETRY_ENV,
    REDUNAR_OVERLAY_PRESET_ENV,
    REDUNAR_OVERLAY_LAYOUT_ENV,
    REDUNAR_OVERLAY_PALETTE_ENV,
    REDUNAR_OVERLAY_BRANDING_ENV,
    REDUNAR_OVERLAY_CORNER_ENV,
    REDUNAR_OVERLAY_OPACITY_ENV,
    REDUNAR_OVERLAY_METRICS_ENV,
    REDUNAR_REPLAY_TRANSFER_ENV,
    REDUNAR_REPLAY_PRODUCTION_ENV,
    REDUNAR_REPLAY_FRAME_RATE_ENV,
    "REDUNAR_GAMESCOPE",
    "REDUNAR_GAMEMODE",
];

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SteamAppId(u32);

impl SteamAppId {
    #[must_use]
    pub const fn new(value: u32) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Closed capture state accepted by the Steam bridge.
///
/// This is deliberately not an environment map. Its wire encoder has a fixed
/// field order, and the wrapper derives the allowlisted variables locally.
#[derive(Clone, Debug, Eq, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the fixed Steam wire mirrors bounded product toggles without arbitrary environment fields"
)]
pub struct SteamCaptureEnvironment {
    layer_manifest_directory: PathBuf,
    opengl_library: Option<PathBuf>,
    capture_socket: PathBuf,
    capture_reply_socket: PathBuf,
    session_id: CaptureSessionId,
    overlay_visible: bool,
    overlay_preset: OverlayPreset,
    overlay_layout: OverlayLayout,
    overlay_palette: OverlayPalette,
    overlay_branding: bool,
    overlay_corner: OverlayCorner,
    overlay_opacity: OverlayOpacity,
    overlay_metrics: OverlayMetricSet,
    gamescope: bool,
    gamemode: bool,
    overlay_telemetry_path: Option<PathBuf>,
    replay: ReplayTransferLaunchConfig,
}

impl SteamCaptureEnvironment {
    /// Construct the fixed private capture environment for one session.
    ///
    /// # Errors
    ///
    /// Returns [`SteamActivationError`] for relative, empty, oversized, or
    /// NUL-containing paths.
    pub fn new(
        layer_manifest_directory: impl Into<PathBuf>,
        capture_socket: impl Into<PathBuf>,
        capture_reply_socket: impl Into<PathBuf>,
        session_id: CaptureSessionId,
    ) -> Result<Self, SteamActivationError> {
        let layer_manifest_directory = layer_manifest_directory.into();
        let capture_socket = capture_socket.into();
        let capture_reply_socket = capture_reply_socket.into();
        validate_wire_path("layer manifest directory", &layer_manifest_directory)?;
        validate_wire_path("capture socket", &capture_socket)?;
        validate_wire_path("capture reply socket", &capture_reply_socket)?;
        Ok(Self {
            layer_manifest_directory,
            opengl_library: None,
            capture_socket,
            capture_reply_socket,
            session_id,
            overlay_visible: false,
            overlay_preset: OverlayPreset::Compact,
            overlay_layout: OverlayLayout::default(),
            overlay_palette: OverlayPalette::default(),
            overlay_branding: true,
            overlay_corner: OverlayCorner::TopLeft,
            overlay_opacity: OverlayOpacity::default(),
            overlay_metrics: OverlayMetricSet::default(),
            gamescope: false,
            gamemode: false,
            overlay_telemetry_path: None,
            replay: ReplayTransferLaunchConfig::default(),
        })
    }

    /// Add the session-private GLX/EGL interposer for native OpenGL games.
    ///
    /// # Errors
    ///
    /// Returns [`SteamActivationError`] when the path cannot be represented by
    /// the bounded bridge protocol.
    pub fn with_opengl_library(
        mut self,
        path: impl Into<PathBuf>,
    ) -> Result<Self, SteamActivationError> {
        let path = path.into();
        validate_wire_path("OpenGL capture library", &path)?;
        self.opengl_library = Some(path);
        Ok(self)
    }

    #[must_use]
    pub const fn with_overlay(
        mut self,
        visible: bool,
        preset: OverlayPreset,
        corner: OverlayCorner,
        opacity: OverlayOpacity,
        metrics: OverlayMetricSet,
    ) -> Self {
        self.overlay_visible = visible;
        self.overlay_preset = preset;
        self.overlay_corner = corner;
        self.overlay_opacity = opacity;
        self.overlay_metrics = metrics;
        self
    }

    #[must_use]
    pub const fn with_gamescope(mut self, enabled: bool) -> Self {
        self.gamescope = enabled;
        self
    }

    #[must_use]
    pub const fn with_overlay_style(
        mut self,
        layout: OverlayLayout,
        palette: OverlayPalette,
    ) -> Self {
        self.overlay_layout = layout;
        self.overlay_palette = palette;
        self
    }

    #[must_use]
    pub const fn with_overlay_branding(mut self, visible: bool) -> Self {
        self.overlay_branding = visible;
        self
    }

    #[must_use]
    pub const fn with_gamemode(mut self, enabled: bool) -> Self {
        self.gamemode = enabled;
        self
    }

    /// Add the daemon-created, read-only overlay telemetry path.
    ///
    /// # Errors
    ///
    /// Returns [`SteamActivationError`] if the path cannot be represented by
    /// the bounded bridge protocol.
    pub fn with_overlay_telemetry_path(
        mut self,
        path: impl Into<PathBuf>,
    ) -> Result<Self, SteamActivationError> {
        let path = path.into();
        validate_wire_path("overlay telemetry file", &path)?;
        self.overlay_telemetry_path = Some(path);
        Ok(self)
    }

    #[must_use]
    pub const fn with_replay_frame_rate(mut self, frame_rate: ReplayFrameRate) -> Self {
        self.replay = ReplayTransferLaunchConfig::new(self.replay.is_requested(), frame_rate);
        self
    }

    #[must_use]
    pub const fn with_replay_requested(mut self, requested: bool) -> Self {
        self.replay = ReplayTransferLaunchConfig::new(requested, self.replay.frame_rate());
        self
    }

    /// Build only Redunar's environment updates against the wrapper's actual
    /// inherited Steam/Proton environment.
    ///
    /// Existing search paths and layers are preserved byte-for-byte, including
    /// non-UTF8 data. Ambiguous overrides fail closed for activation; the
    /// wrapper then runs the original command unchanged.
    #[expect(
        clippy::too_many_lines,
        reason = "the fixed environment handoff remains easier to audit in one ordered block"
    )]
    fn environment_updates(
        &self,
        inherited: &BTreeMap<OsString, OsString>,
    ) -> Result<BTreeMap<OsString, OsString>, SteamActivationError> {
        if inherited.contains_key(OsStr::new("VK_LAYER_PATH")) {
            return Err(SteamActivationError::new(
                "VK_LAYER_PATH conflicts with the private Steam activation",
            ));
        }
        for name in MANAGED_ENVIRONMENT_NAMES {
            if inherited.contains_key(OsStr::new(name)) {
                return Err(SteamActivationError::owned(format!(
                    "inherited {name} conflicts with the private Steam activation"
                )));
            }
        }
        if inherited.contains_key(OsStr::new(REDUNAR_REPLAY_FRAME_RATE_ENV)) {
            return Err(SteamActivationError::owned(format!(
                "inherited {REDUNAR_REPLAY_FRAME_RATE_ENV} conflicts with the private Steam activation"
            )));
        }

        let inherited_layers = inherited
            .get(OsStr::new("VK_INSTANCE_LAYERS"))
            .map_or(&[][..], |value| value.as_bytes());
        if inherited_layers
            .split(|byte| *byte == b':')
            .any(|layer| layer == VULKAN_CAPTURE_LAYER_NAME.as_bytes())
        {
            return Err(SteamActivationError::new(
                "another Redunar Vulkan layer is already active",
            ));
        }

        let mut layer_paths = vec![self.layer_manifest_directory.clone()];
        if let Some(existing) = inherited.get(OsStr::new("VK_ADD_LAYER_PATH")) {
            layer_paths.extend(std::env::split_paths(existing));
        }
        let layer_paths = std::env::join_paths(layer_paths).map_err(|_| {
            SteamActivationError::new("VK_ADD_LAYER_PATH contains an invalid path list")
        })?;
        let shared_write_paths = pressure_vessel_write_paths(&self.capture_socket, inherited)?;

        let enabled_layers = enabled_vulkan_layers(inherited_layers);

        let mut updates = BTreeMap::new();
        updates.insert(OsString::from("VK_ADD_LAYER_PATH"), layer_paths);
        updates.insert(
            OsString::from(PRESSURE_VESSEL_FILESYSTEMS_RW_ENV),
            shared_write_paths,
        );
        updates.insert(
            OsString::from("VK_INSTANCE_LAYERS"),
            OsString::from_vec(enabled_layers),
        );
        if let Some(library) = &self.opengl_library {
            updates.insert(
                OsString::from("LD_PRELOAD"),
                prepend_library_path(library, inherited.get(OsStr::new("LD_PRELOAD")))?,
            );
            // The wrapper only adds the GL interposer. SDL and FAudio must keep
            // the provider chosen by the game/Steam runtime for playback.
        }
        updates.insert(
            OsString::from("REDUNAR_CAPTURE_SOCKET"),
            self.capture_socket.as_os_str().to_owned(),
        );
        updates.insert(
            OsString::from(REDUNAR_CAPTURE_REPLY_SOCKET_ENV),
            self.capture_reply_socket.as_os_str().to_owned(),
        );
        updates.insert(
            OsString::from("REDUNAR_CAPTURE_SESSION"),
            OsString::from(self.session_id.to_hex()),
        );
        updates.insert(
            OsString::from("REDUNAR_CAPTURE_PROTOCOL"),
            OsString::from(PROTOCOL_VERSION.to_string()),
        );
        updates.insert(
            OsString::from(REDUNAR_OVERLAY_VISIBLE_ENV),
            OsString::from(if self.overlay_visible { "1" } else { "0" }),
        );
        updates.insert(
            OsString::from(REDUNAR_OVERLAY_PRESET_ENV),
            OsString::from(overlay_preset_name(self.overlay_preset)),
        );
        updates.insert(
            OsString::from(REDUNAR_OVERLAY_LAYOUT_ENV),
            OsString::from(overlay_layout_name(self.overlay_layout)),
        );
        updates.insert(
            OsString::from(REDUNAR_OVERLAY_PALETTE_ENV),
            OsString::from(overlay_palette_name(self.overlay_palette)),
        );
        updates.insert(
            OsString::from(REDUNAR_OVERLAY_BRANDING_ENV),
            OsString::from(if self.overlay_branding { "1" } else { "0" }),
        );
        updates.insert(
            OsString::from(REDUNAR_OVERLAY_CORNER_ENV),
            OsString::from(overlay_corner_name(self.overlay_corner)),
        );
        updates.insert(
            OsString::from(REDUNAR_OVERLAY_OPACITY_ENV),
            OsString::from(self.overlay_opacity.percent().to_string()),
        );
        updates.insert(
            OsString::from(REDUNAR_OVERLAY_METRICS_ENV),
            OsString::from(self.overlay_metrics.bits().to_string()),
        );
        updates.insert(
            OsString::from("REDUNAR_GAMESCOPE"),
            OsString::from(if self.gamescope { "1" } else { "0" }),
        );
        updates.insert(
            OsString::from("REDUNAR_GAMEMODE"),
            OsString::from(if self.gamemode { "1" } else { "0" }),
        );
        if let Some(path) = &self.overlay_telemetry_path {
            updates.insert(
                OsString::from(REDUNAR_OVERLAY_TELEMETRY_ENV),
                path.as_os_str().to_owned(),
            );
        }
        updates.insert(
            OsString::from(REDUNAR_REPLAY_TRANSFER_ENV),
            OsString::from(if self.replay.is_requested() { "1" } else { "0" }),
        );
        updates.insert(
            OsString::from(REDUNAR_REPLAY_PRODUCTION_ENV),
            OsString::from(if self.replay.is_requested() { "1" } else { "0" }),
        );
        updates.insert(
            OsString::from(REDUNAR_REPLAY_FRAME_RATE_ENV),
            OsString::from(match self.replay.frame_rate() {
                ReplayFrameRate::Fps30 => "30",
                ReplayFrameRate::Fps60 => "60",
                ReplayFrameRate::Fps120 => "120",
            }),
        );
        Ok(updates)
    }
}

fn enabled_vulkan_layers(inherited_layers: &[u8]) -> Vec<u8> {
    let mut enabled = inherited_layers.to_vec();
    if !enabled.is_empty() && !enabled.ends_with(b":") {
        enabled.push(b':');
    }
    enabled.extend_from_slice(VULKAN_CAPTURE_LAYER_NAME.as_bytes());
    enabled
}

fn prepend_library_path(
    library: &Path,
    inherited: Option<&OsString>,
) -> Result<OsString, SteamActivationError> {
    let mut paths = vec![library.to_path_buf()];
    if let Some(inherited) = inherited {
        let inherited_paths = std::env::split_paths(inherited).collect::<Vec<_>>();
        if inherited_paths.iter().any(|path| path == library) {
            return Err(SteamActivationError::new(
                "another Redunar OpenGL interposer is already active",
            ));
        }
        paths.extend(inherited_paths);
    }
    std::env::join_paths(paths)
        .map_err(|_| SteamActivationError::new("LD_PRELOAD contains an invalid path list"))
}

fn pressure_vessel_write_paths(
    capture_socket: &Path,
    inherited: &BTreeMap<OsString, OsString>,
) -> Result<OsString, SteamActivationError> {
    let capture_directory = capture_socket.parent().ok_or_else(|| {
        SteamActivationError::new("capture socket has no private parent directory")
    })?;
    let mut paths = vec![capture_directory.to_path_buf()];
    if let Some(existing) = inherited.get(OsStr::new(PRESSURE_VESSEL_FILESYSTEMS_RW_ENV)) {
        paths.extend(std::env::split_paths(existing));
    }
    std::env::join_paths(paths).map_err(|_| {
        SteamActivationError::new("PRESSURE_VESSEL_FILESYSTEMS_RW contains an invalid path list")
    })
}

#[derive(Clone, Debug)]
struct PendingSteamActivation {
    app_id: SteamAppId,
    expected_uid: u32,
    expires_at: Instant,
    environment: SteamCaptureEnvironment,
}

impl PendingSteamActivation {
    fn new(
        app_id: SteamAppId,
        expected_uid: u32,
        environment: SteamCaptureEnvironment,
        created_at: Instant,
        ttl: Duration,
    ) -> Result<Self, SteamActivationError> {
        if ttl.is_zero() || ttl > MAX_STEAM_ACTIVATION_TTL {
            return Err(SteamActivationError::new(
                "Steam activation TTL must be between 1 nanosecond and 60 seconds",
            ));
        }
        let expires_at = created_at
            .checked_add(ttl)
            .ok_or_else(|| SteamActivationError::new("Steam activation expiry is out of range"))?;
        Ok(Self {
            app_id,
            expected_uid,
            expires_at,
            environment,
        })
    }
}

#[derive(Debug)]
struct PendingActivationStore {
    pending: Mutex<Option<PendingSteamActivation>>,
    claimed_at: Mutex<Option<Instant>>,
}

impl PendingActivationStore {
    fn new(pending: PendingSteamActivation) -> Self {
        Self {
            pending: Mutex::new(Some(pending)),
            claimed_at: Mutex::new(None),
        }
    }

    fn claim(
        &self,
        app_id: SteamAppId,
        peer_uid: u32,
        now: Instant,
    ) -> Option<SteamCaptureEnvironment> {
        let mut pending = lock_unpoisoned(&self.pending);
        if pending
            .as_ref()
            .is_some_and(|activation| now >= activation.expires_at)
        {
            pending.take();
            return None;
        }
        if !pending.as_ref().is_some_and(|activation| {
            activation.app_id == app_id && activation.expected_uid == peer_uid
        }) {
            return None;
        }
        let activation = pending.take()?;
        *lock_unpoisoned(&self.claimed_at) = Some(now);
        Some(activation.environment)
    }

    fn state(&self, now: Instant) -> SteamActivationState {
        if let Some(claimed_at) = *lock_unpoisoned(&self.claimed_at) {
            return SteamActivationState::Claimed {
                elapsed: now.saturating_duration_since(claimed_at),
            };
        }
        let pending = lock_unpoisoned(&self.pending);
        if pending
            .as_ref()
            .is_some_and(|activation| now < activation.expires_at)
        {
            SteamActivationState::Pending
        } else {
            SteamActivationState::Expired
        }
    }
}

/// Observable state of the one-time activation. The claim-relative elapsed
/// time is the capture producer's startup clock; the Steam helper's launch or
/// exit time is not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SteamActivationState {
    Pending,
    Claimed { elapsed: Duration },
    Expired,
}

/// Daemon-owned listener for one pending Steam activation.
pub struct SteamActivationBroker {
    socket_path: PathBuf,
    socket_identity: SocketIdentity,
    store: Arc<PendingActivationStore>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl fmt::Debug for SteamActivationBroker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SteamActivationBroker")
            .field("socket_path", &self.socket_path)
            .field("claimed", &self.claimed())
            .finish_non_exhaustive()
    }
}

impl SteamActivationBroker {
    /// Bind inside Redunar's already-created per-user runtime directory.
    ///
    /// # Errors
    ///
    /// Returns [`SteamActivationError`] under the same conditions as
    /// [`Self::bind`].
    pub fn bind_in_runtime(
        redunar_runtime_directory: &Path,
        app_id: SteamAppId,
        environment: SteamCaptureEnvironment,
        ttl: Duration,
    ) -> Result<Self, SteamActivationError> {
        Self::bind(
            redunar_runtime_directory.join(STEAM_BROKER_SOCKET_FILE),
            app_id,
            environment,
            ttl,
        )
    }

    /// Bind a private socket and publish exactly one short-lived activation.
    ///
    /// # Errors
    ///
    /// Returns [`SteamActivationError`] if the private runtime directory is
    /// unsafe, the socket already exists, peer credentials cannot be secured,
    /// or the bounded worker cannot start.
    pub fn bind(
        socket_path: impl Into<PathBuf>,
        app_id: SteamAppId,
        environment: SteamCaptureEnvironment,
        ttl: Duration,
    ) -> Result<Self, SteamActivationError> {
        let socket_path = socket_path.into();
        validate_socket_parent(&socket_path)?;
        if let Ok(metadata) = fs::symlink_metadata(&socket_path) {
            if !metadata.file_type().is_socket() {
                return Err(SteamActivationError::new(
                    "Steam activation socket path is not a socket",
                ));
            }
            let identity = SocketIdentity::from_metadata(&metadata);
            // A crashed UI can leave its private socket inode behind. Keep a
            // live broker untouched, but reclaim an owner-matching dead socket
            // so the next launch can publish a fresh activation.
            if UnixStream::connect(&socket_path).is_ok() {
                return Err(SteamActivationError::new(
                    "Steam activation socket already exists",
                ));
            }
            remove_socket_if_same(&socket_path, identity);
            if fs::symlink_metadata(&socket_path).is_ok() {
                return Err(SteamActivationError::new(
                    "Steam activation socket already exists",
                ));
            }
        }

        let listener = UnixListener::bind(&socket_path).map_err(|error| {
            SteamActivationError::owned(format!(
                "could not bind private Steam activation socket: {error}"
            ))
        })?;
        let setup = (|| {
            listener.set_nonblocking(true)?;
            fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
            let metadata = fs::symlink_metadata(&socket_path)?;
            let owner_uid = metadata.uid();
            let current_uid = fs::metadata("/proc/self")?.uid();
            if owner_uid != current_uid {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Steam activation socket has an unexpected owner",
                ));
            }
            Ok((SocketIdentity::from_metadata(&metadata), owner_uid))
        })();
        let (socket_identity, owner_uid) = match setup {
            Ok(value) => value,
            Err(error) => {
                drop(listener);
                let _ = fs::remove_file(&socket_path);
                return Err(SteamActivationError::owned(format!(
                    "could not secure private Steam activation socket: {error}"
                )));
            }
        };

        let pending = match PendingSteamActivation::new(
            app_id,
            owner_uid,
            environment,
            Instant::now(),
            ttl,
        ) {
            Ok(pending) => pending,
            Err(error) => {
                drop(listener);
                remove_socket_if_same(&socket_path, socket_identity);
                return Err(error);
            }
        };
        let expires_at = pending.expires_at;
        let store = Arc::new(PendingActivationStore::new(pending));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_store = Arc::clone(&store);
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("redunar-steam-activation".into())
            .spawn(move || broker_loop(&listener, &worker_store, &worker_stop, expires_at))
            .map_err(|error| {
                remove_socket_if_same(&socket_path, socket_identity);
                SteamActivationError::owned(format!(
                    "could not start Steam activation broker: {error}"
                ))
            })?;

        Ok(Self {
            socket_path,
            socket_identity,
            store,
            stop,
            worker: Some(worker),
        })
    }

    #[must_use]
    pub fn claimed(&self) -> bool {
        matches!(self.state(), SteamActivationState::Claimed { .. })
    }

    #[must_use]
    pub fn state(&self) -> SteamActivationState {
        self.store.state(Instant::now())
    }

    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn shutdown(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        self.stop.store(true, Ordering::Release);
        worker.thread().unpark();
        let _ = worker.join();
        remove_socket_if_same(&self.socket_path, self.socket_identity);
    }
}

impl Drop for SteamActivationBroker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Resolve the stable broker location from `XDG_RUNTIME_DIR`.
#[must_use]
pub fn steam_broker_socket_path(xdg_runtime_directory: &Path) -> PathBuf {
    xdg_runtime_directory
        .join("redunar")
        .join(STEAM_BROKER_SOCKET_FILE)
}

/// Claim a matching activation. Protocol, permission, timeout, and absence
/// errors are returned for the wrapper to treat as fail-open.
///
/// # Errors
///
/// Returns [`SteamActivationError`] if the broker cannot be reached, bounded
/// I/O fails, or the response does not match the fixed bridge protocol.
pub fn claim_steam_activation(
    socket_path: &Path,
    app_id: SteamAppId,
) -> Result<Option<SteamCaptureEnvironment>, SteamActivationError> {
    let mut stream = UnixStream::connect(socket_path).map_err(|error| {
        SteamActivationError::owned(format!("Steam activation broker is unavailable: {error}"))
    })?;
    stream
        .set_read_timeout(Some(STREAM_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(STREAM_TIMEOUT)))
        .map_err(|error| {
            SteamActivationError::owned(format!("could not bound broker I/O: {error}"))
        })?;
    request_activation(&mut stream, app_id)
}

fn request_activation(
    stream: &mut UnixStream,
    app_id: SteamAppId,
) -> Result<Option<SteamCaptureEnvironment>, SteamActivationError> {
    stream
        .write_all(&encode_request(app_id, std::process::id()))
        .and_then(|()| stream.shutdown(std::net::Shutdown::Write))
        .map_err(|error| {
            SteamActivationError::owned(format!("could not request activation: {error}"))
        })?;
    let response = read_bounded(stream, MAX_RESPONSE_BYTES).map_err(|error| {
        SteamActivationError::owned(format!("could not read activation response: {error}"))
    })?;
    decode_response(&response)
}

/// Return only environment updates that passed every broker and conflict gate.
/// An empty map means the caller must exec its original command unchanged.
#[must_use]
pub fn resolve_steam_wrapper_environment(
    app_id: SteamAppId,
    socket_path: &Path,
    inherited: &BTreeMap<OsString, OsString>,
) -> BTreeMap<OsString, OsString> {
    let result = claim_steam_activation(socket_path, app_id).and_then(|activation| {
        activation.map_or_else(
            || Ok(BTreeMap::new()),
            |value| value.environment_updates(inherited),
        )
    });
    match result {
        Ok(updates) if updates.is_empty() => {
            eprintln!("redunar-steam-launch: activation denied or expired");
            updates
        }
        Ok(updates) => {
            eprintln!("redunar-steam-launch: activation claimed");
            updates
        }
        Err(error) => {
            eprintln!("redunar-steam-launch: activation unavailable: {error}");
            BTreeMap::new()
        }
    }
}

fn broker_loop(
    listener: &UnixListener,
    store: &PendingActivationStore,
    stop: &AtomicBool,
    expires_at: Instant,
) {
    loop {
        if stop.load(Ordering::Acquire) || Instant::now() >= expires_at {
            return;
        }
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::park_timeout(BROKER_POLL_INTERVAL);
                continue;
            }
            Err(_) => break,
        };
        if stop.load(Ordering::Acquire) {
            return;
        }
        let _ = stream.set_read_timeout(Some(STREAM_TIMEOUT));
        let _ = stream.set_write_timeout(Some(STREAM_TIMEOUT));
        let response = handle_request(&mut stream, store).unwrap_or_else(|_| encode_denied());
        let _ = stream.write_all(&response);
        if matches!(
            store.state(Instant::now()),
            SteamActivationState::Claimed { .. }
        ) {
            return;
        }
    }
}

fn handle_request(
    stream: &mut UnixStream,
    store: &PendingActivationStore,
) -> Result<Vec<u8>, SteamActivationError> {
    let credentials = getsockopt(&*stream, PeerCredentials).map_err(|error| {
        SteamActivationError::owned(format!(
            "could not verify Steam wrapper credentials: {error}"
        ))
    })?;
    let request = read_bounded(stream, REQUEST_BYTES)
        .map_err(|error| SteamActivationError::owned(format!("invalid broker request: {error}")))?;
    handle_request_bytes(
        &request,
        store,
        credentials.pid(),
        credentials.uid(),
        Instant::now(),
    )
}

fn handle_request_bytes(
    request: &[u8],
    store: &PendingActivationStore,
    peer_process_id: i32,
    peer_uid: u32,
    now: Instant,
) -> Result<Vec<u8>, SteamActivationError> {
    let (app_id, claimed_process_id) = decode_request(request)?;
    if peer_process_id <= 0 || u32::try_from(peer_process_id).ok() != Some(claimed_process_id) {
        return Err(SteamActivationError::new(
            "Steam wrapper process identity does not match SO_PEERCRED",
        ));
    }
    let environment = store.claim(app_id, peer_uid, now);
    environment.map_or_else(|| Ok(encode_denied()), |value| encode_granted(&value))
}

fn encode_request(app_id: SteamAppId, process_id: u32) -> [u8; REQUEST_BYTES] {
    let mut bytes = [0; REQUEST_BYTES];
    bytes[..8].copy_from_slice(&STEAM_WIRE_MAGIC);
    bytes[8..10].copy_from_slice(&STEAM_WIRE_VERSION.to_le_bytes());
    bytes[10..14].copy_from_slice(&app_id.get().to_le_bytes());
    bytes[14..18].copy_from_slice(&process_id.to_le_bytes());
    bytes
}

fn decode_request(bytes: &[u8]) -> Result<(SteamAppId, u32), SteamActivationError> {
    if bytes.len() != REQUEST_BYTES
        || bytes.get(..8) != Some(&STEAM_WIRE_MAGIC)
        || read_u16(bytes, 8)? != STEAM_WIRE_VERSION
    {
        return Err(SteamActivationError::new(
            "unsupported Steam activation request",
        ));
    }
    let app_id = SteamAppId::new(read_u32(bytes, 10)?)
        .ok_or_else(|| SteamActivationError::new("Steam app ID cannot be zero"))?;
    Ok((app_id, read_u32(bytes, 14)?))
}

fn encode_denied() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(11);
    bytes.extend_from_slice(&STEAM_WIRE_MAGIC);
    bytes.extend_from_slice(&STEAM_WIRE_VERSION.to_le_bytes());
    bytes.push(RESPONSE_DENIED);
    bytes
}

fn encode_granted(environment: &SteamCaptureEnvironment) -> Result<Vec<u8>, SteamActivationError> {
    let mut bytes = Vec::with_capacity(128);
    bytes.extend_from_slice(&STEAM_WIRE_MAGIC);
    bytes.extend_from_slice(&STEAM_WIRE_VERSION.to_le_bytes());
    bytes.push(RESPONSE_GRANTED);
    push_path(&mut bytes, &environment.layer_manifest_directory)?;
    push_optional_path(&mut bytes, environment.opengl_library.as_deref())?;
    push_path(&mut bytes, &environment.capture_socket)?;
    push_path(&mut bytes, &environment.capture_reply_socket)?;
    bytes.extend_from_slice(&environment.session_id.as_bytes());
    bytes.push(u8::from(environment.overlay_visible));
    bytes.push(overlay_preset_wire(environment.overlay_preset));
    bytes.push(overlay_layout_wire(environment.overlay_layout));
    bytes.push(overlay_palette_wire(environment.overlay_palette));
    bytes.push(u8::from(environment.overlay_branding));
    bytes.push(overlay_corner_wire(environment.overlay_corner));
    bytes.push(environment.overlay_opacity.percent());
    bytes.extend_from_slice(&environment.overlay_metrics.bits().to_le_bytes());
    match &environment.overlay_telemetry_path {
        Some(path) => push_path(&mut bytes, path)?,
        None => bytes.extend_from_slice(&0_u16.to_le_bytes()),
    }
    bytes.push(match environment.replay.frame_rate() {
        ReplayFrameRate::Fps30 => 30,
        ReplayFrameRate::Fps60 => 60,
        ReplayFrameRate::Fps120 => 120,
    });
    bytes.push(u8::from(environment.replay.is_requested()));
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(SteamActivationError::new(
            "Steam activation response exceeds its bound",
        ));
    }
    Ok(bytes)
}

fn decode_response(bytes: &[u8]) -> Result<Option<SteamCaptureEnvironment>, SteamActivationError> {
    if bytes.len() < 11
        || bytes.get(..8) != Some(&STEAM_WIRE_MAGIC)
        || read_u16(bytes, 8)? != STEAM_WIRE_VERSION
    {
        return Err(SteamActivationError::new(
            "unsupported Steam activation response",
        ));
    }
    match bytes[10] {
        RESPONSE_DENIED if bytes.len() == 11 => return Ok(None),
        RESPONSE_GRANTED => {}
        _ => {
            return Err(SteamActivationError::new(
                "invalid activation response status",
            ));
        }
    }

    let mut cursor = 11;
    let manifest = take_path(bytes, &mut cursor, false)?
        .ok_or_else(|| SteamActivationError::new("missing layer manifest directory"))?;
    let opengl_library = take_path(bytes, &mut cursor, true)?;
    let capture_socket = take_path(bytes, &mut cursor, false)?
        .ok_or_else(|| SteamActivationError::new("missing capture socket"))?;
    let capture_reply_socket = take_path(bytes, &mut cursor, false)?
        .ok_or_else(|| SteamActivationError::new("missing capture reply socket"))?;
    let session_bytes = take_array::<16>(bytes, &mut cursor)?;
    let session_id = CaptureSessionId::new(session_bytes)
        .map_err(|_| SteamActivationError::new("invalid capture session ID"))?;
    let overlay_visible = take_bool(bytes, &mut cursor)?;
    let overlay_preset = decode_overlay_preset(take_u8(bytes, &mut cursor)?)?;
    let overlay_layout = decode_overlay_layout(take_u8(bytes, &mut cursor)?)?;
    let overlay_palette = decode_overlay_palette(take_u8(bytes, &mut cursor)?)?;
    let overlay_branding = take_bool(bytes, &mut cursor)?;
    let overlay_corner = decode_overlay_corner(take_u8(bytes, &mut cursor)?)?;
    let overlay_opacity = OverlayOpacity::new(take_u8(bytes, &mut cursor)?)
        .map_err(|_| SteamActivationError::new("invalid overlay opacity"))?;
    let overlay_metrics = OverlayMetricSet::from_bits(take_u16(bytes, &mut cursor)?)
        .map_err(|_| SteamActivationError::new("invalid overlay metrics"))?;
    let overlay_telemetry_path = take_path(bytes, &mut cursor, true)?;
    let replay_frame_rate = match take_u8(bytes, &mut cursor)? {
        30 => ReplayFrameRate::Fps30,
        60 => ReplayFrameRate::Fps60,
        120 => ReplayFrameRate::Fps120,
        _ => return Err(SteamActivationError::new("invalid replay frame rate")),
    };
    let replay_requested = take_bool(bytes, &mut cursor)?;
    if cursor != bytes.len() {
        return Err(SteamActivationError::new(
            "Steam activation response has trailing data",
        ));
    }

    let mut environment =
        SteamCaptureEnvironment::new(manifest, capture_socket, capture_reply_socket, session_id)?
            .with_overlay(
                overlay_visible,
                overlay_preset,
                overlay_corner,
                overlay_opacity,
                overlay_metrics,
            )
            .with_overlay_style(overlay_layout, overlay_palette)
            .with_overlay_branding(overlay_branding)
            .with_replay_frame_rate(replay_frame_rate)
            .with_replay_requested(replay_requested);
    if let Some(path) = opengl_library {
        environment = environment.with_opengl_library(path)?;
    }
    if let Some(path) = overlay_telemetry_path {
        environment = environment.with_overlay_telemetry_path(path)?;
    }
    Ok(Some(environment))
}

fn validate_socket_parent(socket_path: &Path) -> Result<(), SteamActivationError> {
    if !socket_path.is_absolute() {
        return Err(SteamActivationError::new(
            "Steam activation socket must be absolute",
        ));
    }
    let parent = socket_path.parent().ok_or_else(|| {
        SteamActivationError::new("Steam activation socket has no parent directory")
    })?;
    let metadata = fs::symlink_metadata(parent).map_err(|error| {
        SteamActivationError::owned(format!(
            "could not inspect Steam activation directory: {error}"
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(SteamActivationError::new(
            "Steam activation directory is not a real directory",
        ));
    }
    let current_uid = fs::metadata("/proc/self")
        .map_err(|error| {
            SteamActivationError::owned(format!("could not determine daemon owner: {error}"))
        })?
        .uid();
    if metadata.uid() != current_uid {
        return Err(SteamActivationError::new(
            "Steam activation directory belongs to another user",
        ));
    }
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|error| {
        SteamActivationError::owned(format!(
            "could not secure Steam activation directory: {error}"
        ))
    })
}

fn validate_wire_path(label: &str, path: &Path) -> Result<(), SteamActivationError> {
    let bytes = path.as_os_str().as_bytes();
    if !path.is_absolute() || bytes.is_empty() {
        return Err(SteamActivationError::owned(format!(
            "{label} must be absolute"
        )));
    }
    if bytes.contains(&0) {
        return Err(SteamActivationError::owned(format!(
            "{label} contains a NUL byte"
        )));
    }
    if bytes.len() > MAX_PATH_BYTES || bytes.len() > usize::from(u16::MAX) {
        return Err(SteamActivationError::owned(format!(
            "{label} exceeds the Steam bridge path bound"
        )));
    }
    Ok(())
}

fn read_bounded(reader: &mut impl Read, maximum: usize) -> io::Result<Vec<u8>> {
    let limit = u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::with_capacity(maximum);
    reader.take(limit).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Steam activation message exceeds its bound",
        ));
    }
    Ok(bytes)
}

fn push_path(bytes: &mut Vec<u8>, path: &Path) -> Result<(), SteamActivationError> {
    validate_wire_path("Steam activation path", path)?;
    let path = path.as_os_str().as_bytes();
    let length = u16::try_from(path.len())
        .map_err(|_| SteamActivationError::new("Steam activation path is too long"))?;
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(path);
    Ok(())
}

fn push_optional_path(
    bytes: &mut Vec<u8>,
    path: Option<&Path>,
) -> Result<(), SteamActivationError> {
    if let Some(path) = path {
        push_path(bytes, path)
    } else {
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        Ok(())
    }
}

fn take_path(
    bytes: &[u8],
    cursor: &mut usize,
    optional: bool,
) -> Result<Option<PathBuf>, SteamActivationError> {
    let length = usize::from(take_u16(bytes, cursor)?);
    if length == 0 && optional {
        return Ok(None);
    }
    if length == 0 || length > MAX_PATH_BYTES {
        return Err(SteamActivationError::new(
            "invalid Steam activation path length",
        ));
    }
    let end = cursor
        .checked_add(length)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| SteamActivationError::new("truncated Steam activation path"))?;
    let path = PathBuf::from(OsString::from_vec(bytes[*cursor..end].to_vec()));
    *cursor = end;
    validate_wire_path("Steam activation path", &path)?;
    Ok(Some(path))
}

fn take_array<const N: usize>(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<[u8; N], SteamActivationError> {
    let end = cursor
        .checked_add(N)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| SteamActivationError::new("truncated Steam activation response"))?;
    let value = bytes[*cursor..end]
        .try_into()
        .map_err(|_| SteamActivationError::new("invalid Steam activation field"))?;
    *cursor = end;
    Ok(value)
}

fn take_u8(bytes: &[u8], cursor: &mut usize) -> Result<u8, SteamActivationError> {
    let value = *bytes
        .get(*cursor)
        .ok_or_else(|| SteamActivationError::new("truncated Steam activation response"))?;
    *cursor = cursor.saturating_add(1);
    Ok(value)
}

fn take_bool(bytes: &[u8], cursor: &mut usize) -> Result<bool, SteamActivationError> {
    match take_u8(bytes, cursor)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(SteamActivationError::new(
            "invalid Steam activation boolean",
        )),
    }
}

fn take_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16, SteamActivationError> {
    let value = read_u16(bytes, *cursor)?;
    *cursor = cursor.saturating_add(2);
    Ok(value)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, SteamActivationError> {
    let value = bytes
        .get(offset..offset.saturating_add(2))
        .and_then(|value| value.try_into().ok())
        .ok_or_else(|| SteamActivationError::new("truncated Steam activation integer"))?;
    Ok(u16::from_le_bytes(value))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, SteamActivationError> {
    let value = bytes
        .get(offset..offset.saturating_add(4))
        .and_then(|value| value.try_into().ok())
        .ok_or_else(|| SteamActivationError::new("truncated Steam activation integer"))?;
    Ok(u32::from_le_bytes(value))
}

const fn overlay_preset_wire(preset: OverlayPreset) -> u8 {
    match preset {
        OverlayPreset::Compact => 1,
        OverlayPreset::FpsOnly => 2,
        OverlayPreset::Detailed => 3,
        OverlayPreset::Custom => 4,
    }
}

fn decode_overlay_preset(value: u8) -> Result<OverlayPreset, SteamActivationError> {
    match value {
        1 => Ok(OverlayPreset::Compact),
        2 => Ok(OverlayPreset::FpsOnly),
        3 => Ok(OverlayPreset::Detailed),
        4 => Ok(OverlayPreset::Custom),
        _ => Err(SteamActivationError::new("invalid overlay preset")),
    }
}

const fn overlay_preset_name(preset: OverlayPreset) -> &'static str {
    match preset {
        OverlayPreset::Compact => "compact",
        OverlayPreset::FpsOnly => "fps-only",
        OverlayPreset::Detailed => "detailed",
        OverlayPreset::Custom => "custom",
    }
}

const fn overlay_layout_wire(layout: OverlayLayout) -> u8 {
    match layout {
        OverlayLayout::Grid => 1,
        OverlayLayout::Ribbon => 2,
        OverlayLayout::Telemetry => 3,
    }
}

fn decode_overlay_layout(value: u8) -> Result<OverlayLayout, SteamActivationError> {
    match value {
        1 => Ok(OverlayLayout::Grid),
        2 => Ok(OverlayLayout::Ribbon),
        3 => Ok(OverlayLayout::Telemetry),
        _ => Err(SteamActivationError::new("invalid overlay layout")),
    }
}

const fn overlay_layout_name(layout: OverlayLayout) -> &'static str {
    match layout {
        OverlayLayout::Grid => "grid",
        OverlayLayout::Ribbon => "ribbon",
        OverlayLayout::Telemetry => "telemetry",
    }
}

const fn overlay_palette_wire(palette: OverlayPalette) -> u8 {
    match palette {
        OverlayPalette::Redunar => 1,
        OverlayPalette::Glacier => 2,
        OverlayPalette::Ember => 3,
        OverlayPalette::Mint => 4,
        OverlayPalette::Mono => 5,
        OverlayPalette::Amethyst => 6,
        OverlayPalette::Solar => 7,
        OverlayPalette::Rose => 8,
    }
}

fn decode_overlay_palette(value: u8) -> Result<OverlayPalette, SteamActivationError> {
    match value {
        1 => Ok(OverlayPalette::Redunar),
        2 => Ok(OverlayPalette::Glacier),
        3 => Ok(OverlayPalette::Ember),
        4 => Ok(OverlayPalette::Mint),
        5 => Ok(OverlayPalette::Mono),
        6 => Ok(OverlayPalette::Amethyst),
        7 => Ok(OverlayPalette::Solar),
        8 => Ok(OverlayPalette::Rose),
        _ => Err(SteamActivationError::new("invalid overlay palette")),
    }
}

const fn overlay_palette_name(palette: OverlayPalette) -> &'static str {
    match palette {
        OverlayPalette::Redunar => "redunar",
        OverlayPalette::Glacier => "glacier",
        OverlayPalette::Ember => "ember",
        OverlayPalette::Mint => "mint",
        OverlayPalette::Mono => "mono",
        OverlayPalette::Amethyst => "amethyst",
        OverlayPalette::Solar => "solar",
        OverlayPalette::Rose => "rose",
    }
}

const fn overlay_corner_wire(corner: OverlayCorner) -> u8 {
    match corner {
        OverlayCorner::TopLeft => 1,
        OverlayCorner::TopRight => 2,
        OverlayCorner::BottomLeft => 3,
        OverlayCorner::BottomRight => 4,
    }
}

fn decode_overlay_corner(value: u8) -> Result<OverlayCorner, SteamActivationError> {
    match value {
        1 => Ok(OverlayCorner::TopLeft),
        2 => Ok(OverlayCorner::TopRight),
        3 => Ok(OverlayCorner::BottomLeft),
        4 => Ok(OverlayCorner::BottomRight),
        _ => Err(SteamActivationError::new("invalid overlay corner")),
    }
}

const fn overlay_corner_name(corner: OverlayCorner) -> &'static str {
    match corner {
        OverlayCorner::TopLeft => "top-left",
        OverlayCorner::TopRight => "top-right",
        OverlayCorner::BottomLeft => "bottom-left",
        OverlayCorner::BottomRight => "bottom-right",
    }
}

#[derive(Clone, Copy)]
struct SocketIdentity {
    device: u64,
    inode: u64,
}

impl SocketIdentity {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
}

fn remove_socket_if_same(path: &Path, identity: SocketIdentity) {
    if fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.dev() == identity.device && metadata.ino() == identity.inode)
    {
        let _ = fs::remove_file(path);
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SteamActivationError {
    message: String,
}

impl SteamActivationError {
    fn new(message: &str) -> Self {
        Self {
            message: message.to_owned(),
        }
    }

    fn owned(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for SteamActivationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for SteamActivationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir()
                .join(format!("redunar-steam-bridge-{}-{id}", std::process::id()));
            fs::create_dir_all(&root).expect("create fixture");
            Self { root }
        }

        fn socket_path(&self) -> PathBuf {
            self.root.join(STEAM_BROKER_SOCKET_FILE)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).expect("remove fixture");
        }
    }

    fn app_id() -> SteamAppId {
        SteamAppId::new(1_808_500).expect("non-zero app ID")
    }

    fn environment() -> SteamCaptureEnvironment {
        SteamCaptureEnvironment::new(
            "/run/user/1000/redunar/capture/layers",
            "/run/user/1000/redunar/capture/capture.sock",
            "/run/user/1000/redunar/capture/capture-reply.sock",
            CaptureSessionId::new([7; 16]).expect("session ID"),
        )
        .expect("capture environment")
        .with_opengl_library("/run/user/1000/redunar/capture/libredunar_capture_opengl.so")
        .expect("OpenGL library")
        .with_overlay(
            true,
            OverlayPreset::Detailed,
            OverlayCorner::BottomRight,
            OverlayOpacity::new(75).expect("opacity"),
            OverlayMetricSet::DETAILED,
        )
        .with_overlay_style(OverlayLayout::Telemetry, OverlayPalette::Amethyst)
        .with_overlay_branding(false)
        .with_overlay_telemetry_path("/run/user/1000/redunar/capture/overlay.bin")
        .expect("telemetry path")
        .with_replay_requested(true)
        .with_replay_frame_rate(ReplayFrameRate::Fps30)
    }

    fn store_at(now: Instant, ttl: Duration, uid: u32) -> PendingActivationStore {
        PendingActivationStore::new(
            PendingSteamActivation::new(app_id(), uid, environment(), now, ttl)
                .expect("pending activation"),
        )
    }

    #[test]
    fn wrong_app_and_uid_do_not_consume_pending_activation() {
        let now = Instant::now();
        let store = store_at(now, Duration::from_secs(30), 1_000);
        let wrong_app = SteamAppId::new(12).expect("app ID");

        assert!(store.claim(wrong_app, 1_000, now).is_none());
        assert!(store.claim(app_id(), 1_001, now).is_none());
        assert!(store.claim(app_id(), 1_000, now).is_some());
    }

    #[test]
    fn expired_activation_is_rejected_deterministically() {
        let now = Instant::now();
        let store = store_at(now, Duration::from_secs(2), 1_000);

        assert!(
            store
                .claim(app_id(), 1_000, now + Duration::from_secs(2))
                .is_none()
        );
        assert!(store.claim(app_id(), 1_000, now).is_none());
    }

    #[test]
    fn activation_can_be_claimed_only_once() {
        let now = Instant::now();
        let store = store_at(now, Duration::from_secs(30), 1_000);

        assert!(store.claim(app_id(), 1_000, now).is_some());
        assert!(store.claim(app_id(), 1_000, now).is_none());
        assert!(matches!(
            store.state(now),
            SteamActivationState::Claimed {
                elapsed: Duration::ZERO
            }
        ));
    }

    #[test]
    fn fixed_wire_round_trip_preserves_non_utf8_paths() {
        let mut value = environment();
        value.layer_manifest_directory =
            PathBuf::from(OsString::from_vec(b"/layers/\xff".to_vec()));
        let encoded = encode_granted(&value).expect("encode activation");
        let decoded = decode_response(&encoded)
            .expect("decode activation")
            .expect("granted activation");

        assert_eq!(decoded, value);
        let updates = decoded
            .environment_updates(&BTreeMap::new())
            .expect("expand child environment");
        assert_eq!(
            updates.get(OsStr::new(REDUNAR_OVERLAY_LAYOUT_ENV)),
            Some(&OsString::from("telemetry"))
        );
        assert_eq!(
            updates.get(OsStr::new(REDUNAR_OVERLAY_PALETTE_ENV)),
            Some(&OsString::from("amethyst"))
        );
        assert_eq!(
            updates.get(OsStr::new(REDUNAR_OVERLAY_BRANDING_ENV)),
            Some(&OsString::from("0"))
        );
    }

    #[test]
    fn every_overlay_corner_survives_steam_wire_and_environment_expansion() {
        for (corner, expected) in [
            (OverlayCorner::TopLeft, "top-left"),
            (OverlayCorner::TopRight, "top-right"),
            (OverlayCorner::BottomLeft, "bottom-left"),
            (OverlayCorner::BottomRight, "bottom-right"),
        ] {
            let value = SteamCaptureEnvironment::new(
                "/run/user/1000/redunar/capture/layers",
                "/run/user/1000/redunar/capture/capture.sock",
                "/run/user/1000/redunar/capture/capture-reply.sock",
                CaptureSessionId::new([7; 16]).expect("session ID"),
            )
            .expect("capture environment")
            .with_overlay(
                true,
                OverlayPreset::Compact,
                corner,
                OverlayOpacity::default(),
                OverlayMetricSet::COMPACT,
            );
            let encoded = encode_granted(&value).expect("encode activation");
            let decoded = decode_response(&encoded)
                .expect("decode activation")
                .expect("granted activation");
            let updates = decoded
                .environment_updates(&BTreeMap::new())
                .expect("expand child environment");

            assert_eq!(
                updates.get(OsStr::new(REDUNAR_OVERLAY_CORNER_ENV)),
                Some(&OsString::from(expected))
            );
        }
    }

    #[test]
    fn environment_merge_preserves_non_utf8_layers_and_rejects_conflicts() {
        let mut inherited = BTreeMap::new();
        inherited.insert(
            OsString::from("VK_INSTANCE_LAYERS"),
            OsString::from_vec(b"VK_LAYER_ONE:\xff".to_vec()),
        );
        inherited.insert(
            OsString::from("VK_ADD_LAYER_PATH"),
            OsString::from_vec(b"/existing/\xfe".to_vec()),
        );
        inherited.insert(
            OsString::from(PRESSURE_VESSEL_FILESYSTEMS_RW_ENV),
            OsString::from("/already-shared"),
        );
        inherited.insert(
            OsString::from("LD_PRELOAD"),
            OsString::from_vec(b"/existing/liboverlay.so:/existing/\xfd.so".to_vec()),
        );
        let updates = environment()
            .environment_updates(&inherited)
            .expect("merge environment");
        assert_eq!(
            updates
                .get(OsStr::new("VK_INSTANCE_LAYERS"))
                .expect("layers")
                .as_bytes(),
            b"VK_LAYER_ONE:\xff:VK_LAYER_REDUNAR_capture"
        );
        assert_eq!(
            updates.get(OsStr::new(REDUNAR_REPLAY_TRANSFER_ENV)),
            Some(&OsString::from("1"))
        );
        assert_eq!(
            updates.get(OsStr::new(REDUNAR_REPLAY_PRODUCTION_ENV)),
            Some(&OsString::from("1"))
        );
        assert_eq!(
            updates.get(OsStr::new(REDUNAR_CAPTURE_REPLY_SOCKET_ENV)),
            Some(&OsString::from(
                "/run/user/1000/redunar/capture/capture-reply.sock"
            ))
        );
        assert_eq!(
            updates
                .get(OsStr::new("LD_PRELOAD"))
                .expect("preload")
                .as_bytes(),
            b"/run/user/1000/redunar/capture/libredunar_capture_opengl.so:/existing/liboverlay.so:/existing/\xfd.so"
        );
        assert!(!updates.contains_key(OsStr::new("SDL_DYNAMIC_API")));
        let shared = updates
            .get(OsStr::new(PRESSURE_VESSEL_FILESYSTEMS_RW_ENV))
            .expect("pressure-vessel paths");
        let shared = std::env::split_paths(shared).collect::<Vec<_>>();
        assert_eq!(shared[0], Path::new("/run/user/1000/redunar/capture"));
        assert_eq!(shared[1], Path::new("/already-shared"));

        inherited.insert(OsString::from("VK_LAYER_PATH"), OsString::from("/override"));
        assert!(environment().environment_updates(&inherited).is_err());
        inherited.remove(OsStr::new("VK_LAYER_PATH"));
        inherited.insert(
            OsString::from("REDUNAR_CAPTURE_SESSION"),
            OsString::from("foreign"),
        );
        assert!(environment().environment_updates(&inherited).is_err());
        inherited.remove(OsStr::new("REDUNAR_CAPTURE_SESSION"));
        inherited.insert(
            OsString::from("LD_PRELOAD"),
            OsString::from("/run/user/1000/redunar/capture/libredunar_capture_opengl.so"),
        );
        assert!(environment().environment_updates(&inherited).is_err());
    }

    #[test]
    fn absent_broker_is_fail_open_with_no_environment_updates() {
        let fixture = Fixture::new();
        let inherited = BTreeMap::from([(OsString::from("EXISTING"), OsString::from("preserved"))]);

        assert!(
            resolve_steam_wrapper_environment(app_id(), &fixture.socket_path(), &inherited)
                .is_empty()
        );
        assert_eq!(
            inherited.get(OsStr::new("EXISTING")),
            Some(&OsString::from("preserved"))
        );
    }

    #[test]
    fn handler_checks_injected_peer_credentials_and_consumes_matching_app_once() {
        fn request(
            store: &PendingActivationStore,
            requested_app: SteamAppId,
            process_id: i32,
            uid: u32,
            now: Instant,
        ) -> Result<Option<SteamCaptureEnvironment>, SteamActivationError> {
            let process_id_wire = u32::try_from(process_id).expect("positive process ID");
            let response = handle_request_bytes(
                &encode_request(requested_app, process_id_wire),
                store,
                process_id,
                uid,
                now,
            )?;
            decode_response(&response)
        }

        let now = Instant::now();
        let uid = fs::metadata("/proc/self").expect("process metadata").uid();
        let store = store_at(now, Duration::from_secs(30), uid);
        let process_id = i32::try_from(std::process::id()).expect("process ID fits i32");
        let mismatched_process = handle_request_bytes(
            &encode_request(app_id(), std::process::id()),
            &store,
            process_id.saturating_add(1),
            uid,
            now,
        );
        assert!(mismatched_process.is_err());
        let wrong = request(
            &store,
            SteamAppId::new(12).expect("app ID"),
            process_id,
            uid,
            now,
        )
        .expect("wrong app response");
        assert!(wrong.is_none());
        assert!(
            request(&store, app_id(), process_id, uid, now)
                .expect("matching response")
                .is_some()
        );
        assert!(matches!(
            store.state(now),
            SteamActivationState::Claimed { .. }
        ));
        assert!(
            request(&store, app_id(), process_id, uid, now)
                .expect("replay response")
                .is_none()
        );
    }

    #[test]
    fn default_activation_ttl_matches_the_shader_startup_budget() {
        assert_eq!(DEFAULT_STEAM_ACTIVATION_TTL, Duration::from_mins(5));
        assert!(DEFAULT_STEAM_ACTIVATION_TTL <= MAX_STEAM_ACTIVATION_TTL);
    }

    #[test]
    fn activation_ttl_is_bounded() {
        let now = Instant::now();
        assert!(
            PendingSteamActivation::new(app_id(), 1_000, environment(), now, Duration::ZERO)
                .is_err()
        );
        assert!(
            PendingSteamActivation::new(
                app_id(),
                1_000,
                environment(),
                now,
                MAX_STEAM_ACTIVATION_TTL + Duration::from_nanos(1),
            )
            .is_err()
        );
    }
}
