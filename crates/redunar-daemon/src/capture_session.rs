use crate::capture_lifecycle::{CaptureLaunchLifecycle, LifecycleAction};
use crate::{
    CaptureLaunchDisposition, CaptureLaunchProcessState, CaptureSessionModel, CaptureSnapshot,
};
use redunar_capture::{
    CaptureSessionId, MAX_MESSAGE_BYTES, OVERLAY_HARDWARE_TELEMETRY_BYTES,
    OverlayHardwareTelemetry, REPLAY_MENU_TELEMETRY_BYTES, ReplayMenuTelemetry, ReplayPixelFormat,
    decode_message, encode_overlay_hardware_telemetry, encode_replay_menu_telemetry,
};
use redunar_core::{
    EffectiveGameProfile, GameLaunchConfig, GameRecord, GlobalGameProfile, SystemSnapshot,
};
use redunar_platform::{
    CaptureLaunchPlan, GameLaunchProcessOwnership, OverlayLaunchConfig, ReplayTransferLaunchConfig,
    SteamActivationBroker, SteamActivationState, SteamAppId, SteamCaptureEnvironment,
    SteamLaunchOptionsDetector, SteamLaunchOptionsReason, SteamLaunchOptionsStatus,
    game_launch_process_ownership, prepare_vulkan_layer_directory,
};
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::OwnedFd;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const CAPTURE_SOCKET_FILE: &str = "capture.sock";
const REPLAY_EXPORT_QUEUE_CAPACITY: usize = 8;
// Exportable copy routes use a small fixed slot pool. Confirmation must fit
// inside that pool because provisional frames have not reached the encoder's
// normal completion/release cycle yet.
const REPLAY_PRODUCER_CONFIRMATION_EXPORTS: u16 = 3;
const REPLAY_PRODUCER_CANDIDATE_CAPACITY: usize = 8;
const REPLAY_PRODUCER_STALE_NS: u64 = 1_000_000_000;
const CAPTURE_LIBRARY_FILE: &str = "libredunar_capture_vulkan.so";
const OPENGL_CAPTURE_LIBRARY_FILE: &str = "libredunar_capture_opengl.so";
const STEAM_LAUNCH_WRAPPER_FILE: &str = "redunar-steam-launch";
const OVERLAY_TELEMETRY_FILE: &str = "overlay-telemetry-v1.bin";
// Steam may compile or refresh shaders before the first Vulkan frame reaches the
// capture layer. Keep the activation bounded, but allow a normal slow startup.
const STEAM_PRODUCER_STARTUP_GRACE: Duration = Duration::from_mins(5);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaptureRuntimeStatus {
    Available,
    Unavailable(String),
}

/// Whether the packaged Steam wrapper needed for a specific app ID is ready.
/// `Available` describes package readiness; callers must separately inspect
/// [`SteamBridgeSetup::configuration_status`] before arming capture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SteamBridgeSetupStatus {
    Available(SteamBridgeSetup),
    Unavailable(String),
}

impl SteamBridgeSetupStatus {
    /// True only when the package is ready and native Steam's persisted exact
    /// app-specific Launch Options value has been verified.
    #[must_use]
    pub fn is_configured(&self) -> bool {
        matches!(self, Self::Available(setup) if setup.configuration_status().is_configured())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SteamBridgeSetup {
    app_id: SteamAppId,
    launch_options: String,
    configuration_status: SteamLaunchOptionsStatus,
}

impl SteamBridgeSetup {
    #[must_use]
    pub const fn app_id(&self) -> SteamAppId {
        self.app_id
    }

    #[must_use]
    pub fn launch_options(&self) -> &str {
        &self.launch_options
    }

    /// Read-only status of the exact value persisted by native Steam.
    #[must_use]
    pub const fn configuration_status(&self) -> &SteamLaunchOptionsStatus {
        &self.configuration_status
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureSessionConfig {
    runtime_root: PathBuf,
    layer_library: PathBuf,
    opengl_library: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CaptureModuleGates {
    pub frame_metrics: bool,
    pub in_game_overlay: bool,
    pub instant_replay: bool,
}

impl CaptureModuleGates {
    pub(crate) const fn capture_requested(self) -> bool {
        self.frame_metrics || self.in_game_overlay || self.instant_replay
    }

    fn resolve_profile(
        self,
        profile: redunar_core::PerGameProfile,
        mut global: GlobalGameProfile,
    ) -> EffectiveGameProfile {
        global.capture_metrics = self.frame_metrics;
        global.overlay_visible = self.in_game_overlay;
        global.instant_replay = self.instant_replay;
        let mut effective = profile.resolve(global);
        effective.capture_metrics &= self.frame_metrics;
        effective.overlay_visible &= self.in_game_overlay;
        effective.instant_replay &= self.instant_replay;
        effective
    }
}

impl Default for CaptureModuleGates {
    fn default() -> Self {
        Self {
            frame_metrics: true,
            in_game_overlay: true,
            instant_replay: true,
        }
    }
}

impl CaptureSessionConfig {
    #[must_use]
    pub fn new(runtime_root: impl Into<PathBuf>, layer_library: impl Into<PathBuf>) -> Self {
        Self {
            runtime_root: runtime_root.into(),
            layer_library: layer_library.into(),
            opengl_library: None,
        }
    }

    /// Add the launch-scoped GLX/EGL observer used by direct and native Steam games.
    #[must_use]
    pub fn with_opengl_library(mut self, library: impl Into<PathBuf>) -> Self {
        self.opengl_library = Some(library.into());
        self
    }

    /// Locate development/packaged capture components without installing a
    /// graphics layer or provider globally. The Vulkan layer is required for
    /// the established runtime; the OpenGL observer is enabled when its
    /// adjacent sidecar is present.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureSessionError`] when `XDG_RUNTIME_DIR` is missing or
    /// relative, the current executable cannot be resolved, or the adjacent
    /// Vulkan layer library is unavailable.
    pub fn for_current_build() -> Result<Self, CaptureSessionError> {
        let runtime_root = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or_else(|| {
                CaptureSessionError::new("XDG_RUNTIME_DIR is unavailable or not absolute")
            })?
            .join("redunar");
        let executable = env::current_exe().map_err(|error| {
            CaptureSessionError::owned(format!("could not locate Redunar executable: {error}"))
        })?;
        let executable_directory = executable.parent().ok_or_else(|| {
            CaptureSessionError::new("Redunar executable has no parent directory")
        })?;
        let layer_library = find_layer_library(&executable);
        let Some(layer_library) = layer_library else {
            return Err(CaptureSessionError::owned(format!(
                "Vulkan capture layer is unavailable at {}",
                executable_directory.join(CAPTURE_LIBRARY_FILE).display()
            )));
        };
        let mut config = Self::new(runtime_root, layer_library);
        if let Some(opengl_library) =
            find_adjacent_library(&executable, OPENGL_CAPTURE_LIBRARY_FILE)
        {
            config = config.with_opengl_library(opengl_library);
        }
        Ok(config)
    }

    #[must_use]
    pub fn runtime_root(&self) -> &Path {
        &self.runtime_root
    }

    /// Sweep capture-session directories left behind by a previous Redunar
    /// instance that crashed or was killed before its cleanup ran. Callers
    /// must confirm that no other live instance owns the backend session
    /// first; every directory matching our exact private naming is then
    /// stale by definition. Foreign names, non-directories, and entries
    /// owned by another uid are never touched, and unlinking a socket still
    /// held open by a surviving game only removes the path. Returns the
    /// number of directories removed; never fails a launch over leftovers.
    #[must_use]
    pub fn sweep_stale_session_directories(&self) -> usize {
        let current_uid = process_uid();
        // The runtime root's owner is the session user; refuse to sweep a
        // directory we do not own.
        let root_owner = fs::metadata(&self.runtime_root)
            .ok()
            .map(|metadata| metadata.uid());
        if root_owner != Some(current_uid) {
            return 0;
        }
        let Ok(entries) = fs::read_dir(&self.runtime_root) else {
            return 0;
        };
        let mut removed = 0;
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(hex) = file_name
                .to_str()
                .and_then(|name| name.strip_prefix("capture-"))
            else {
                continue;
            };
            if hex.len() != 32 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                continue;
            }
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !metadata.is_dir() || metadata.uid() != current_uid {
                continue;
            }
            cleanup_session_directory(&path);
            if !path.exists() {
                removed += 1;
            }
        }
        removed
    }

    #[must_use]
    pub fn layer_library(&self) -> &Path {
        &self.layer_library
    }

    #[must_use]
    pub fn opengl_library(&self) -> Option<&Path> {
        self.opengl_library.as_deref()
    }

    /// Verify the exact sibling wrapper and build the bounded app-ID-specific
    /// Steam Launch Options snippet. This reads package metadata only.
    #[must_use]
    pub fn steam_bridge_setup_status(&self, app_id: SteamAppId) -> SteamBridgeSetupStatus {
        if !self.layer_library.is_file() {
            return SteamBridgeSetupStatus::Unavailable(
                "Redunar's Vulkan capture layer is unavailable".to_owned(),
            );
        }
        let Some(directory) = self.layer_library.parent() else {
            return SteamBridgeSetupStatus::Unavailable(
                "Redunar's capture runtime has no executable directory".to_owned(),
            );
        };
        let wrapper = directory.join(STEAM_LAUNCH_WRAPPER_FILE);
        let metadata = match fs::symlink_metadata(&wrapper) {
            Ok(metadata)
                if metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && metadata.permissions().mode() & 0o111 != 0 =>
            {
                metadata
            }
            Ok(_) => {
                return SteamBridgeSetupStatus::Unavailable(
                    "The Redunar Steam launch wrapper is not a regular executable file".to_owned(),
                );
            }
            Err(error) => {
                return SteamBridgeSetupStatus::Unavailable(format!(
                    "The Redunar Steam launch wrapper is unavailable: {error}"
                ));
            }
        };
        debug_assert!(metadata.is_file());
        let launch_options = match steam_launch_options(&wrapper, app_id) {
            Ok(launch_options) => launch_options,
            Err(reason) => return SteamBridgeSetupStatus::Unavailable(reason.to_owned()),
        };
        let configuration_status = env::var_os("HOME").map_or_else(
            || {
                SteamLaunchOptionsStatus::Unavailable(
                    SteamLaunchOptionsReason::HomeDirectoryUnavailable,
                )
            },
            |home| SteamLaunchOptionsDetector::new(home).status(app_id, &launch_options),
        );
        SteamBridgeSetupStatus::Available(SteamBridgeSetup {
            app_id,
            launch_options,
            configuration_status,
        })
    }
}

fn find_layer_library(executable: &Path) -> Option<PathBuf> {
    find_adjacent_library(executable, CAPTURE_LIBRARY_FILE)
}

fn find_adjacent_library(executable: &Path, file_name: &str) -> Option<PathBuf> {
    let executable_directory = executable.parent()?;
    let cargo_example_profile = (executable_directory.file_name() == Some(OsStr::new("examples")))
        .then(|| {
            executable_directory
                .parent()
                .map(|parent| parent.join(file_name))
        })
        .flatten();
    [
        executable_directory.join(file_name),
        // `cargo tauri dev` leaves shared libraries in the debug `deps`
        // directory beside the development executable. Keep this fallback
        // local so development uses the same private launch contract.
        executable_directory.join("deps").join(file_name),
    ]
    .into_iter()
    .chain(cargo_example_profile)
    .find(|candidate| candidate.is_file())
}

fn steam_launch_options(wrapper: &Path, app_id: SteamAppId) -> Result<String, &'static str> {
    if !wrapper.is_absolute() {
        return Err("The Redunar Steam launch wrapper path is not absolute");
    }
    let wrapper = wrapper
        .to_str()
        .ok_or("The Redunar Steam launch wrapper path is not valid UTF-8")?;
    if !wrapper.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b'+')
    }) {
        return Err(
            "The Redunar Steam launch wrapper path contains characters that are unsafe in Steam Launch Options",
        );
    }
    Ok(format!("{wrapper} --app-id {} -- %command%", app_id.get()))
}

pub struct CaptureSessionHandle {
    shared: Arc<CaptureShared>,
    worker: Option<JoinHandle<()>>,
    session_id: CaptureSessionId,
    session_directory: PathBuf,
    layer_manifest_directory: PathBuf,
    opengl_library: Option<PathBuf>,
    socket_path: PathBuf,
    reply_socket_path: PathBuf,
    overlay_telemetry_path: PathBuf,
    overlay_telemetry_file: File,
    overlay_telemetry_revision: AtomicU64,
    overlay_config: AtomicU64,
    overlay_replay_saved_revision: AtomicU16,
    launch_started_at: Instant,
    lifecycle: CaptureLaunchLifecycle,
    steam_activation_broker: Option<SteamActivationBroker>,
    module_gates: CaptureModuleGates,
    last_imported_replay_export: Arc<AtomicU64>,
}

impl fmt::Debug for CaptureSessionHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CaptureSessionHandle")
            .field("snapshot", &self.snapshot())
            .field("session_directory", &self.session_directory)
            .finish_non_exhaustive()
    }
}

impl CaptureSessionHandle {
    pub(crate) fn start(
        config: &CaptureSessionConfig,
        module_gates: CaptureModuleGates,
    ) -> Result<Self, CaptureSessionError> {
        if !config.runtime_root.is_absolute()
            || !config.layer_library.is_absolute()
            || config
                .opengl_library
                .as_ref()
                .is_some_and(|library| !library.is_absolute())
        {
            return Err(CaptureSessionError::new(
                "capture runtime and library paths must be absolute",
            ));
        }
        let session_id = random_session_id()?;
        let session_directory = config
            .runtime_root
            .join(format!("capture-{}", session_id.to_hex()));
        let layer_manifest_directory =
            prepare_vulkan_layer_directory(&session_directory, &config.layer_library)
                .map_err(|error| CaptureSessionError::owned(error.to_string()))?;
        let opengl_library = config
            .opengl_library
            .as_deref()
            .map(|library| prepare_opengl_capture_library(&session_directory, library))
            .transpose()
            .inspect_err(|_| {
                cleanup_session_directory(&session_directory);
            })?;
        let (overlay_telemetry_path, overlay_telemetry_file) =
            create_overlay_telemetry_file(&session_directory).map_err(|error| {
                cleanup_session_directory(&session_directory);
                CaptureSessionError::owned(format!(
                    "could not prepare private overlay telemetry: {error}"
                ))
            })?;
        let socket_path = session_directory.join(CAPTURE_SOCKET_FILE);
        let socket = match UnixDatagram::bind(&socket_path) {
            Ok(socket) => socket,
            Err(error) => {
                cleanup_session_directory(&session_directory);
                return Err(CaptureSessionError::owned(format!(
                    "could not bind capture socket: {error}"
                )));
            }
        };
        if let Err(error) = fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600)) {
            drop(socket);
            cleanup_session_directory(&session_directory);
            return Err(CaptureSessionError::owned(format!(
                "could not secure capture socket: {error}"
            )));
        }
        let release_socket = socket.try_clone().map_err(|error| {
            cleanup_session_directory(&session_directory);
            CaptureSessionError::owned(format!("could not clone capture reply socket: {error}"))
        })?;
        let reply_socket_path = session_directory.join("capture-reply.sock");

        let initial = CaptureSessionModel::new(session_id).snapshot();
        let shared = Arc::new(CaptureShared {
            latest: Mutex::new(Arc::new(initial)),
            stop: AtomicBool::new(false),
            replay_exports: Mutex::new(VecDeque::with_capacity(REPLAY_EXPORT_QUEUE_CAPACITY)),
            replay_export_ready: Condvar::new(),
            replay_release: ReplayReleaseTransport {
                session_id,
                reply_socket_path: reply_socket_path.clone(),
                socket: Some(Arc::new(release_socket)),
                targets: Arc::new(Mutex::new(BTreeMap::new())),
                #[cfg(test)]
                acknowledgements: None,
            },
        });
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("redunar-capture".into())
            .spawn(move || receiver_loop(&socket, &worker_shared, session_id))
            .map_err(|error| {
                cleanup_session_directory(&session_directory);
                CaptureSessionError::owned(format!("could not start capture worker: {error}"))
            })?;

        Ok(Self {
            shared,
            worker: Some(worker),
            session_id,
            session_directory,
            layer_manifest_directory,
            opengl_library,
            socket_path,
            reply_socket_path,
            overlay_telemetry_path,
            overlay_telemetry_file,
            overlay_telemetry_revision: AtomicU64::new(2),
            overlay_config: AtomicU64::new(pack_overlay_config(0, 0, 1, 50, 100, 0, 0, true)),
            overlay_replay_saved_revision: AtomicU16::new(0),
            launch_started_at: Instant::now(),
            lifecycle: CaptureLaunchLifecycle::new(),
            steam_activation_broker: None,
            module_gates,
            last_imported_replay_export: Arc::new(AtomicU64::new(u64::MAX)),
        })
    }

    #[must_use]
    pub fn snapshot(&self) -> Arc<CaptureSnapshot> {
        Arc::clone(&lock_unpoisoned(&self.shared.latest))
    }

    /// Create the narrow, cloneable Replay transport used by the daemon's
    /// encoder worker. It cannot stop capture or mutate overlay/metrics state.
    pub(crate) fn replay_export_endpoint(&self) -> ReplayExportEndpoint {
        ReplayExportEndpoint {
            shared: Arc::clone(&self.shared),
            last_imported_replay_export: Arc::clone(&self.last_imported_replay_export),
        }
    }

    /// Import the most recently transferred producer FD after validating its
    /// exact sequence and metadata. This is the only daemon-side bridge from
    /// the metadata protocol to a [`crate::DmaBufReplayFrame`]; pixels never enter the
    /// capture socket.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureSessionError`] when the producer identity, requested
    /// sequence, exported FD, or frame metadata is unavailable or invalid.
    pub fn import_replay_export(
        &self,
        sequence: u64,
    ) -> Result<crate::DmaBufReplayFrame, CaptureSessionError> {
        let snapshot = self.snapshot();
        if snapshot.replay_latest_export_sequence != Some(sequence) {
            return Err(CaptureSessionError::new(
                "replay export sequence is stale or unavailable",
            ));
        }
        if snapshot.producer_process_id.is_none() {
            return Err(CaptureSessionError::new(
                "replay export producer identity is unavailable",
            ));
        }
        if snapshot.replay_latest_export_fd.is_none() {
            return Err(CaptureSessionError::new("replay export FD is unavailable"));
        }
        let mut exports = lock_unpoisoned(&self.shared.replay_exports);
        let Some(index) = exports
            .iter()
            .position(|export| export.sequence == sequence)
        else {
            return Err(CaptureSessionError::new(
                "replay export descriptor is stale or unavailable",
            ));
        };
        let export = exports.remove(index).ok_or_else(|| {
            CaptureSessionError::new("replay export descriptor became unavailable")
        })?;
        drop(exports);
        self.import_replay_export_metadata(export)
    }

    pub(crate) fn take_next_replay_export(&self) -> Option<ReplayExportMetadata> {
        self.replay_export_endpoint().take_next()
    }

    /// Take and duplicate one completed Replay export for an explicit
    /// end-to-end hardware validation run.
    ///
    /// This does not acknowledge the producer buffer. The caller must retain
    /// `sequence` and call [`Self::release_replay_validation_export`] only
    /// after the hardware encoder completion fence has signaled or its private
    /// device has reached a safe idle shutdown.
    ///
    /// # Errors
    ///
    /// Returns an error when producer identity or exported DMA-BUF metadata is
    /// unavailable, unsafe, or cannot be duplicated.
    pub fn take_replay_validation_export(
        &self,
    ) -> Result<Option<(u64, crate::DmaBufReplayFrame)>, CaptureSessionError> {
        let Some(export) = self.take_next_replay_export() else {
            return Ok(None);
        };
        if self.snapshot().producer_process_id.is_none() {
            return Err(CaptureSessionError::new(
                "replay producer identity is unavailable",
            ));
        }
        let sequence = export.sequence;
        self.import_replay_export_metadata(export)
            .map(|frame| Some((sequence, frame)))
    }

    /// Acknowledge one validation export whose encoder ownership has ended.
    ///
    /// # Errors
    ///
    /// Returns an error when the bounded reply message cannot be delivered to
    /// the capture producer.
    pub fn release_replay_validation_export(
        &self,
        sequence: u64,
    ) -> Result<(), CaptureSessionError> {
        self.release_replay_export(sequence)
    }

    pub(crate) fn release_replay_export(&self, sequence: u64) -> Result<(), CaptureSessionError> {
        self.shared.replay_release.release(sequence)
    }

    pub(crate) fn import_replay_export_metadata(
        &self,
        export: ReplayExportMetadata,
    ) -> Result<crate::DmaBufReplayFrame, CaptureSessionError> {
        import_replay_export_metadata(&self.last_imported_replay_export, export)
    }

    #[must_use]
    pub const fn session_id(&self) -> CaptureSessionId {
        self.session_id
    }

    /// Build a shell-free command plan for one game executable. Existing
    /// Vulkan layer settings are preserved or reported as conflicts.
    ///
    /// # Errors
    ///
    /// Returns the platform launch validation error when paths or inherited
    /// layer settings are unsafe or ambiguous.
    pub fn launch_plan(
        &self,
        executable: impl Into<PathBuf>,
        arguments: impl IntoIterator<Item = OsString>,
        inherited_environment: &BTreeMap<OsString, OsString>,
    ) -> Result<CaptureLaunchPlan, redunar_platform::CaptureLaunchError> {
        let plan = CaptureLaunchPlan::new(
            executable,
            arguments,
            &self.layer_manifest_directory,
            &self.socket_path,
            self.session_id,
            inherited_environment,
        )?
        .with_capture_reply_socket(&self.reply_socket_path);
        self.with_opengl_capture(plan, inherited_environment)
    }

    /// Build a shell-free capture plan from a persisted local game record.
    ///
    /// # Errors
    ///
    /// Returns the platform launch validation error when persisted launch
    /// paths or inherited Vulkan settings are unsafe or ambiguous.
    pub fn game_launch_plan(
        &self,
        launch: &GameLaunchConfig,
        inherited_environment: &BTreeMap<OsString, OsString>,
    ) -> Result<CaptureLaunchPlan, redunar_platform::CaptureLaunchError> {
        let plan = CaptureLaunchPlan::new(
            launch.executable.clone(),
            launch.arguments.clone(),
            &self.layer_manifest_directory,
            &self.socket_path,
            self.session_id,
            inherited_environment,
        )?
        .with_capture_reply_socket(&self.reply_socket_path)
        .with_working_directory(launch.working_directory.clone())?;
        self.with_opengl_capture(plan, inherited_environment)
    }

    fn with_opengl_capture(
        &self,
        plan: CaptureLaunchPlan,
        inherited_environment: &BTreeMap<OsString, OsString>,
    ) -> Result<CaptureLaunchPlan, redunar_platform::CaptureLaunchError> {
        let Some(library) = self.configured_opengl_library() else {
            return Ok(plan);
        };
        plan.with_opengl_capture_library(library, inherited_environment)
    }

    fn configured_opengl_library(&self) -> Option<&Path> {
        self.opengl_library.as_deref()
    }

    /// Build a shell-free child plan from one persisted game and its resolved
    /// global/per-game profile. Overlay visibility is carried independently
    /// from frame collection and is scoped to this child process.
    ///
    /// # Errors
    ///
    /// Returns the platform launch validation error when persisted launch
    /// paths or inherited Vulkan settings are unsafe or ambiguous.
    pub fn game_launch_plan_for_profile(
        &self,
        game: &GameRecord,
        global_profile: GlobalGameProfile,
        inherited_environment: &BTreeMap<OsString, OsString>,
    ) -> Result<CaptureLaunchPlan, redunar_platform::CaptureLaunchError> {
        let effective = self.resolve_profile(game.profile, global_profile);
        let plan = self
            .game_launch_plan(&game.launch, inherited_environment)?
            .with_overlay_config(
                OverlayLaunchConfig::new(effective.overlay_visible)
                    .with_customization(
                        effective.overlay_preset,
                        effective.overlay_corner,
                        effective.overlay_opacity,
                    )
                    .with_style(effective.overlay_layout, effective.overlay_palette)
                    .with_branding(effective.overlay_branding)
                    .with_metrics(effective.overlay_metrics),
            )
            .with_replay_transfer_config(replay_transfer_launch_config(effective, true));
        plan.with_overlay_telemetry_path(self.overlay_telemetry_path.clone())
    }

    /// Publish one short-lived, one-time capture activation for the exact
    /// native Steam app ID in a saved forwarding launch.
    ///
    /// This only arms the private broker; it never edits Steam configuration,
    /// injects a process, or launches a command. Production capability remains
    /// unavailable until the UI/session lifecycle invokes and supervises this
    /// complete handshake.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureSessionError`] when the launch is not an unambiguous
    /// native Steam `-applaunch`, an activation was already published, or the
    /// private broker cannot be secured.
    pub fn publish_steam_activation_for_profile(
        &mut self,
        game: &GameRecord,
        global_profile: GlobalGameProfile,
        ttl: Duration,
    ) -> Result<(), CaptureSessionError> {
        self.publish_steam_activation(game, global_profile, ttl, None)
    }

    /// Publish a native-Steam activation with an explicit Replay transfer
    /// request for a bounded capture diagnostic. Production callers must use
    /// [`Self::publish_steam_activation_for_profile`] so module readiness and
    /// the effective profile remain authoritative.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureSessionError`] under the same activation, path, and
    /// ownership failures as the production profile method.
    pub fn publish_steam_activation_for_replay_diagnostic(
        &mut self,
        game: &GameRecord,
        global_profile: GlobalGameProfile,
        ttl: Duration,
        replay: ReplayTransferLaunchConfig,
    ) -> Result<(), CaptureSessionError> {
        self.publish_steam_activation(game, global_profile, ttl, Some(replay))
    }

    fn publish_steam_activation(
        &mut self,
        game: &GameRecord,
        global_profile: GlobalGameProfile,
        ttl: Duration,
        replay_override: Option<ReplayTransferLaunchConfig>,
    ) -> Result<(), CaptureSessionError> {
        if self.steam_activation_broker.is_some() {
            return Err(CaptureSessionError::new(
                "this capture session already published a Steam activation",
            ));
        }
        let app_id =
            match game_launch_process_ownership(&game.launch.executable, &game.launch.arguments) {
                GameLaunchProcessOwnership::ForwardedSteam {
                    app_id: Some(app_id),
                } => SteamAppId::new(app_id).ok_or_else(|| {
                    CaptureSessionError::new("Steam activation app ID cannot be zero")
                })?,
                _ => {
                    return Err(CaptureSessionError::new(
                        "Steam activation requires an unambiguous native steam -applaunch command",
                    ));
                }
            };
        let effective = self.resolve_profile(game.profile, global_profile);
        let replay = replay_override.unwrap_or_else(|| {
            ReplayTransferLaunchConfig::new(effective.instant_replay, effective.replay.frame_rate)
        });
        let mut environment = SteamCaptureEnvironment::new(
            &self.layer_manifest_directory,
            &self.socket_path,
            &self.reply_socket_path,
            self.session_id,
        )
        .map_err(|error| CaptureSessionError::owned(error.to_string()))?
        .with_overlay(
            effective.overlay_visible,
            effective.overlay_preset,
            effective.overlay_corner,
            effective.overlay_opacity,
            effective.overlay_metrics,
        )
        .with_overlay_style(effective.overlay_layout, effective.overlay_palette)
        .with_overlay_branding(effective.overlay_branding)
        .with_replay_requested(replay.is_requested())
        .with_replay_frame_rate(replay.frame_rate());
        if let Some(library) = self.configured_opengl_library() {
            environment = environment
                .with_opengl_library(library)
                .map_err(|error| CaptureSessionError::owned(error.to_string()))?;
        }
        environment = environment
            .with_overlay_telemetry_path(&self.overlay_telemetry_path)
            .map_err(|error| CaptureSessionError::owned(error.to_string()))?;
        let runtime_root = self.session_directory.parent().ok_or_else(|| {
            CaptureSessionError::new("capture session has no private runtime directory")
        })?;
        let broker = SteamActivationBroker::bind_in_runtime(runtime_root, app_id, environment, ttl)
            .map_err(|error| CaptureSessionError::owned(error.to_string()))?;
        self.steam_activation_broker = Some(broker);
        Ok(())
    }

    fn resolve_profile(
        &self,
        profile: redunar_core::PerGameProfile,
        global: GlobalGameProfile,
    ) -> EffectiveGameProfile {
        self.module_gates.resolve_profile(profile, global)
    }

    /// Whether the matching Steam wrapper claimed this session's activation.
    #[must_use]
    pub fn steam_activation_claimed(&self) -> bool {
        self.steam_activation_broker
            .as_ref()
            .is_some_and(SteamActivationBroker::claimed)
    }

    /// Current bridge state for a caller supervising a forwarded Steam launch.
    #[must_use]
    pub fn steam_activation_state(&self) -> Option<SteamActivationState> {
        self.steam_activation_broker
            .as_ref()
            .map(SteamActivationBroker::state)
    }

    /// Replace the private fixed-size hardware snapshot consumed by the
    /// in-process overlay. The write uses an odd/even revision pair so a
    /// concurrent reader rejects a torn update. Failure affects only optional
    /// hardware rows; capture and the game continue unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureSessionError`] when the private telemetry file cannot
    /// be updated.
    pub fn update_overlay_hardware(
        &self,
        hardware: Option<&SystemSnapshot>,
    ) -> Result<(), CaptureSessionError> {
        write_overlay_hardware(
            &self.overlay_telemetry_file,
            &self.overlay_telemetry_revision,
            hardware,
            self.overlay_config.load(Ordering::Relaxed),
            self.overlay_replay_saved_revision.load(Ordering::Relaxed),
        )
        .map_err(|error| {
            CaptureSessionError::owned(format!("could not update overlay telemetry: {error}"))
        })
    }

    pub(crate) fn update_replay_menu(
        &self,
        telemetry: ReplayMenuTelemetry,
    ) -> Result<(), CaptureSessionError> {
        let mut bytes = [0; REPLAY_MENU_TELEMETRY_BYTES];
        encode_replay_menu_telemetry(telemetry, &mut bytes)
            .map_err(|error| CaptureSessionError::owned(error.to_string()))?;
        let odd = telemetry.revision.saturating_sub(1).to_le_bytes();
        self.overlay_telemetry_file
            .write_all_at(&odd, (OVERLAY_HARDWARE_TELEMETRY_BYTES + 16) as u64)
            .and_then(|()| {
                self.overlay_telemetry_file
                    .write_all_at(&odd, (OVERLAY_HARDWARE_TELEMETRY_BYTES + 120) as u64)
            })
            .map_err(|error| {
                CaptureSessionError::owned(format!(
                    "could not begin Replay menu telemetry update: {error}"
                ))
            })?;
        self.overlay_telemetry_file
            .write_all_at(&bytes, OVERLAY_HARDWARE_TELEMETRY_BYTES as u64)
            .map_err(|error| {
                CaptureSessionError::owned(format!(
                    "could not update Replay menu telemetry: {error}"
                ))
            })
    }

    /// Publish one presentation-only edge after the Replay store durably
    /// commits a clip. The overlay may show it briefly; this file is never the
    /// authority for save success or clip inventory.
    ///
    /// # Errors
    ///
    /// Returns an error if the active session telemetry file cannot be
    /// updated.
    pub fn publish_replay_saved_notice(&self) -> Result<(), CaptureSessionError> {
        let previous = self
            .overlay_replay_saved_revision
            .fetch_add(1, Ordering::Relaxed);
        let next = previous.wrapping_add(1).max(1);
        if next == 1 && previous == u16::MAX {
            self.overlay_replay_saved_revision
                .store(next, Ordering::Relaxed);
        }
        write_overlay_replay_notice(
            &self.overlay_telemetry_file,
            &self.overlay_telemetry_revision,
            next,
        )
        .map_err(|error| {
            CaptureSessionError::owned(format!("could not publish Replay save notice: {error}"))
        })
    }

    /// Update corner, preset, metric selection, and opacity for a running overlay.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureSessionError`] when the private telemetry file cannot be updated.
    pub fn update_overlay_config(
        &self,
        profile: &EffectiveGameProfile,
    ) -> Result<(), CaptureSessionError> {
        let packed = pack_overlay_config(
            overlay_corner_code(profile.overlay_corner),
            overlay_preset_code(profile.overlay_preset),
            profile.overlay_metrics.bits(),
            profile.overlay_opacity.percent(),
            profile.overlay_scale.percent(),
            overlay_layout_code(profile.overlay_layout),
            overlay_palette_code(profile.overlay_palette),
            profile.overlay_branding,
        );
        let packed = packed
            | ((if profile.overlay_visible {
                1_u64
            } else {
                2_u64
            }) << 48);
        write_overlay_config(
            &self.overlay_telemetry_file,
            &self.overlay_telemetry_revision,
            packed,
        )
        .map_err(|error| {
            CaptureSessionError::owned(format!("could not update overlay configuration: {error}"))
        })?;
        self.overlay_config.store(packed, Ordering::Relaxed);
        Ok(())
    }

    /// Reconcile the directly launched wrapper with the registered capture
    /// producer. A wrapper exit alone is not terminal: its producer descendant
    /// remains authoritative while its exact `/proc` identity is alive.
    ///
    /// Terminal capture resources are closed before [`CaptureLaunchDisposition::Release`]
    /// is returned, so a late descendant cannot attach to an old session.
    pub fn observe_launch_process(
        &mut self,
        launched_process: &CaptureLaunchProcessState,
    ) -> CaptureLaunchDisposition {
        let snapshot = self.snapshot();
        let activation_state = self.steam_activation_state();
        let startup = startup_clock(self.launch_started_at.elapsed(), activation_state);
        if snapshot.phase != crate::CapturePhase::Failed {
            match startup {
                CaptureStartupClock::WaitingForSteam => {
                    return CaptureLaunchDisposition::KeepActive;
                }
                CaptureStartupClock::SteamExpired => {
                    self.fail_if_active(
                        "Steam did not claim the pending capture activation before it expired.",
                    );
                    return CaptureLaunchDisposition::Release;
                }
                CaptureStartupClock::Elapsed(_) => {}
            }
        }
        if steam_producer_is_starting(
            snapshot.phase,
            snapshot.producer_process_id,
            activation_state,
        ) {
            return CaptureLaunchDisposition::KeepActive;
        }
        if steam_producer_startup_timed_out(
            snapshot.phase,
            snapshot.producer_process_id,
            activation_state,
        ) {
            self.fail_if_active(format!(
                "Steam setup connected, but the frame-metrics producer could not reach Redunar within {} seconds.",
                STEAM_PRODUCER_STARTUP_GRACE.as_secs()
            ));
            return CaptureLaunchDisposition::Release;
        }
        let forwarded_complete = CaptureLaunchProcessState::Exited(
            "the Steam forwarding helper is not the game lifetime authority".to_owned(),
        );
        let lifecycle_process = if activation_state.is_some() {
            &forwarded_complete
        } else {
            launched_process
        };
        let decision = self.lifecycle.evaluate(
            match startup {
                CaptureStartupClock::Elapsed(elapsed) => elapsed,
                CaptureStartupClock::WaitingForSteam | CaptureStartupClock::SteamExpired => {
                    Duration::ZERO
                }
            },
            snapshot.phase,
            snapshot.producer_process_id,
            lifecycle_process,
        );
        match decision.action {
            LifecycleAction::Continue => {}
            LifecycleAction::Finish => self.shutdown(),
            LifecycleAction::Fail(message) => self.fail_if_active(message),
        }
        decision.disposition
    }

    pub fn shutdown(&mut self) {
        if let Some(mut broker) = self.steam_activation_broker.take() {
            broker.shutdown();
        }
        if self.stop_receiver() {
            cleanup_session_directory(&self.session_directory);
        }
    }

    /// End a capture that lost its producer without a protocol goodbye.
    ///
    /// The receiver is joined before the final state is written so a queued
    /// goodbye can still win the race and preserve a successful completion.
    /// This is intended for the daemon's child-process supervisor; producer
    /// crashes must not leave the user interface stuck in `Capturing`.
    pub fn fail_if_active(&mut self, message: impl Into<String>) {
        if matches!(
            self.snapshot().phase,
            crate::CapturePhase::Completed | crate::CapturePhase::Failed
        ) {
            return;
        }
        if let Some(mut broker) = self.steam_activation_broker.take() {
            broker.shutdown();
        }
        if self.stop_receiver() {
            cleanup_session_directory(&self.session_directory);
        }

        let current = self.snapshot();
        if matches!(
            current.phase,
            crate::CapturePhase::Completed | crate::CapturePhase::Failed
        ) {
            return;
        }
        let mut failed = (*current).clone();
        failed.phase = crate::CapturePhase::Failed;
        failed.failure = Some(message.into());
        failed.revision = failed.revision.saturating_add(1);
        publish(&self.shared, failed);
    }

    fn stop_receiver(&mut self) -> bool {
        let Some(worker) = self.worker.take() else {
            return false;
        };
        self.shared.stop.store(true, Ordering::Release);
        if let Ok(waker) = UnixDatagram::unbound() {
            let _ = waker.send_to(&[], &self.socket_path);
        }
        let _ = worker.join();
        true
    }
}

fn write_overlay_config(file: &File, revision: &AtomicU64, packed: u64) -> io::Result<()> {
    let mut bytes = [0_u8; OVERLAY_HARDWARE_TELEMETRY_BYTES];
    file.read_exact_at(&mut bytes, 0)?;
    let next = revision
        .fetch_add(2, Ordering::Relaxed)
        .checked_add(2)
        .ok_or_else(|| io::Error::other("overlay telemetry revision overflowed"))?;
    let odd = next - 1;
    file.write_all_at(&odd.to_le_bytes(), 16)?;
    file.write_all_at(&odd.to_le_bytes(), 40)?;
    bytes[12] = u8::try_from(packed & 0xff).expect("masked corner fits");
    bytes[13] = u8::try_from((packed >> 8) & 0xff).expect("masked preset fits");
    bytes[14..16].copy_from_slice(
        &u16::try_from((packed >> 16) & 0xffff)
            .expect("masked metrics fit")
            .to_le_bytes(),
    );
    bytes[32] = u8::try_from((packed >> 32) & 0xff).expect("masked opacity fits");
    bytes[33] = u8::try_from((packed >> 40) & 0xff).expect("masked scale fits");
    bytes[36] = ((packed >> 48) & 0x03) as u8;
    bytes[37] = ((packed >> 56) & 0x0f) as u8;
    bytes[38] = ((packed >> 60) & 0x0f) as u8;
    bytes[39] = u8::from(((packed >> 48) & 0x04) != 0);
    bytes[16..24].copy_from_slice(&next.to_le_bytes());
    bytes[40..48].copy_from_slice(&next.to_le_bytes());
    file.write_all_at(&bytes, 0)
}

fn write_overlay_replay_notice(
    file: &File,
    revision: &AtomicU64,
    replay_saved_revision: u16,
) -> io::Result<()> {
    let mut bytes = [0_u8; OVERLAY_HARDWARE_TELEMETRY_BYTES];
    file.read_exact_at(&mut bytes, 0)?;
    let next = revision
        .fetch_add(2, Ordering::Relaxed)
        .checked_add(2)
        .ok_or_else(|| io::Error::other("overlay telemetry revision overflowed"))?;
    let odd = next - 1;
    file.write_all_at(&odd.to_le_bytes(), 16)?;
    file.write_all_at(&odd.to_le_bytes(), 40)?;
    bytes[34..36].copy_from_slice(&replay_saved_revision.to_le_bytes());
    bytes[16..24].copy_from_slice(&next.to_le_bytes());
    bytes[40..48].copy_from_slice(&next.to_le_bytes());
    file.write_all_at(&bytes, 0)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CaptureStartupClock {
    Elapsed(Duration),
    WaitingForSteam,
    SteamExpired,
}

const fn startup_clock(
    direct_elapsed: Duration,
    activation_state: Option<SteamActivationState>,
) -> CaptureStartupClock {
    match activation_state {
        None => CaptureStartupClock::Elapsed(direct_elapsed),
        Some(SteamActivationState::Pending) => CaptureStartupClock::WaitingForSteam,
        Some(SteamActivationState::Claimed { elapsed }) => CaptureStartupClock::Elapsed(elapsed),
        Some(SteamActivationState::Expired) => CaptureStartupClock::SteamExpired,
    }
}

fn steam_producer_is_starting(
    phase: crate::CapturePhase,
    producer_process_id: Option<u32>,
    activation_state: Option<SteamActivationState>,
) -> bool {
    matches!(phase, crate::CapturePhase::Armed)
        && producer_process_id.is_none()
        && matches!(
            activation_state,
            Some(SteamActivationState::Claimed { elapsed })
                if elapsed < STEAM_PRODUCER_STARTUP_GRACE
        )
}

fn steam_producer_startup_timed_out(
    phase: crate::CapturePhase,
    producer_process_id: Option<u32>,
    activation_state: Option<SteamActivationState>,
) -> bool {
    matches!(phase, crate::CapturePhase::Armed)
        && producer_process_id.is_none()
        && matches!(
            activation_state,
            Some(SteamActivationState::Claimed { elapsed })
                if elapsed >= STEAM_PRODUCER_STARTUP_GRACE
        )
}

const fn replay_transfer_launch_config(
    effective: EffectiveGameProfile,
    runtime_allowed: bool,
) -> ReplayTransferLaunchConfig {
    // The child receives the transfer request only after the app-wide module
    // gate and backend-readiness checks have allowed Replay for this launch.
    // The bounded Vulkan export is the prompt-free path that was validated in
    // the real-game test; persisted preference alone is not sufficient.
    ReplayTransferLaunchConfig::new(
        effective.instant_replay && runtime_allowed,
        effective.replay.frame_rate,
    )
}

impl Drop for CaptureSessionHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct CaptureShared {
    latest: Mutex<Arc<CaptureSnapshot>>,
    stop: AtomicBool,
    replay_exports: Mutex<VecDeque<ReplayExportMetadata>>,
    replay_export_ready: Condvar,
    replay_release: ReplayReleaseTransport,
}

#[derive(Clone)]
struct ReplayReleaseTransport {
    session_id: CaptureSessionId,
    reply_socket_path: PathBuf,
    socket: Option<Arc<UnixDatagram>>,
    targets: Arc<Mutex<BTreeMap<u64, PathBuf>>>,
    #[cfg(test)]
    acknowledgements: Option<Arc<Mutex<Vec<u64>>>>,
}

impl ReplayReleaseTransport {
    fn register(
        &self,
        sequence: u64,
        sender_path: Option<&Path>,
    ) -> Result<(), CaptureSessionError> {
        let target = sender_path.unwrap_or(&self.reply_socket_path);
        let expected_parent = self
            .reply_socket_path
            .parent()
            .ok_or_else(|| CaptureSessionError::new("replay reply socket has no private parent"))?;
        if !target.is_absolute() || target.parent() != Some(expected_parent) {
            return Err(CaptureSessionError::new(
                "replay producer reply socket is outside the private session",
            ));
        }
        let mut targets = lock_unpoisoned(&self.targets);
        match targets.entry(sequence) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(target.to_path_buf());
            }
            std::collections::btree_map::Entry::Occupied(_) => {
                return Err(CaptureSessionError::new(
                    "replay producer reused an in-flight export sequence",
                ));
            }
        }
        Ok(())
    }

    fn release(&self, sequence: u64) -> Result<(), CaptureSessionError> {
        #[cfg(test)]
        if let Some(acknowledgements) = &self.acknowledgements {
            lock_unpoisoned(acknowledgements).push(sequence);
            return Ok(());
        }
        let target = lock_unpoisoned(&self.targets)
            .get(&sequence)
            .cloned()
            .ok_or_else(|| CaptureSessionError::new("replay export has no reply route"))?;
        let message = redunar_capture::CaptureMessage::ReplayFrameReleased {
            session_id: self.session_id,
            sequence,
        };
        let mut buffer = [0; MAX_MESSAGE_BYTES];
        let length = redunar_capture::encode_message(&message, &mut buffer)
            .map_err(|error| CaptureSessionError::owned(error.to_string()))?;
        let socket = self.socket.as_ref().ok_or_else(|| {
            CaptureSessionError::new("replay release transport has no session socket")
        })?;
        match socket.send_to(&buffer[..length], &target) {
            Ok(_) => {
                lock_unpoisoned(&self.targets).remove(&sequence);
                Ok(())
            }
            // Once the game removes its reply socket, the Vulkan device and
            // exportable buffers are already being destroyed. No buffer can
            // be reused, so a missing recipient is a completed release.
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                ) =>
            {
                lock_unpoisoned(&self.targets).remove(&sequence);
                Ok(())
            }
            Err(error) => Err(CaptureSessionError::owned(format!(
                "could not release replay export: {error}"
            ))),
        }
    }
}

/// Narrow cloneable endpoint for the daemon-owned Replay encoder worker.
pub(crate) struct ReplayExportEndpoint {
    shared: Arc<CaptureShared>,
    last_imported_replay_export: Arc<AtomicU64>,
}

impl Clone for ReplayExportEndpoint {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
            last_imported_replay_export: Arc::clone(&self.last_imported_replay_export),
        }
    }
}

impl ReplayExportEndpoint {
    #[cfg(test)]
    pub(crate) fn new_for_test() -> (Self, Arc<Mutex<Vec<u64>>>) {
        let session_id = CaptureSessionId::new([9; 16]).expect("non-zero test session");
        let acknowledgements = Arc::new(Mutex::new(Vec::new()));
        let shared = Arc::new(CaptureShared {
            latest: Mutex::new(Arc::new(CaptureSessionModel::new(session_id).snapshot())),
            stop: AtomicBool::new(false),
            replay_exports: Mutex::new(VecDeque::new()),
            replay_export_ready: Condvar::new(),
            replay_release: ReplayReleaseTransport {
                session_id,
                reply_socket_path: PathBuf::new(),
                socket: None,
                targets: Arc::new(Mutex::new(BTreeMap::new())),
                acknowledgements: Some(Arc::clone(&acknowledgements)),
            },
        });
        (
            Self {
                shared,
                last_imported_replay_export: Arc::new(AtomicU64::new(u64::MAX)),
            },
            acknowledgements,
        )
    }

    pub(crate) fn take_next(&self) -> Option<ReplayExportMetadata> {
        lock_unpoisoned(&self.shared.replay_exports).pop_front()
    }

    pub(crate) fn wait_next(&self, timeout: Duration) -> Option<ReplayExportMetadata> {
        let exports = lock_unpoisoned(&self.shared.replay_exports);
        let mut exports = if exports.is_empty() {
            self.shared
                .replay_export_ready
                .wait_timeout(exports, timeout)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0
        } else {
            exports
        };
        exports.pop_front()
    }

    pub(crate) fn import(
        &self,
        export: ReplayExportMetadata,
    ) -> Result<(u64, crate::DmaBufReplayFrame), CaptureSessionError> {
        let sequence = export.sequence;
        import_replay_export_metadata(&self.last_imported_replay_export, export)
            .map(|frame| (sequence, frame))
    }

    pub(crate) fn release(&self, sequence: u64) -> Result<(), CaptureSessionError> {
        self.shared.replay_release.release(sequence)
    }

    pub(crate) fn drain_and_release(&self) {
        let pending = {
            let mut exports = lock_unpoisoned(&self.shared.replay_exports);
            exports
                .drain(..)
                .map(|export| export.sequence)
                .collect::<Vec<_>>()
        };
        for sequence in pending {
            let _ = self.release(sequence);
        }
    }

    pub(crate) fn wake(&self) {
        self.shared.replay_export_ready.notify_all();
    }

    #[cfg(test)]
    pub(crate) fn enqueue_for_test(&self, export: ReplayExportMetadata) {
        enqueue_replay_export(&self.shared, export);
    }
}

pub(crate) struct ReplayExportMetadata {
    pub sequence: u64,
    pub fd: OwnedFd,
    pub source: redunar_capture::ReplaySourceCandidate,
    pub offset: u32,
    pub stride: u32,
    pub modifier: u64,
    pub timestamp_ns: u64,
    pub duration_ns: u64,
}

fn enqueue_replay_export(shared: &CaptureShared, exported: ReplayExportMetadata) {
    let mut exports = lock_unpoisoned(&shared.replay_exports);
    let displaced = (exports.len() == REPLAY_EXPORT_QUEUE_CAPACITY)
        .then(|| exports.pop_front())
        .flatten();
    exports.push_back(exported);
    drop(exports);
    if let Some(displaced) = displaced {
        let _ = shared.replay_release.release(displaced.sequence);
    }
    shared.replay_export_ready.notify_one();
}

fn import_replay_export_metadata(
    last_imported: &AtomicU64,
    export: ReplayExportMetadata,
) -> Result<crate::DmaBufReplayFrame, CaptureSessionError> {
    let ReplayExportMetadata {
        sequence,
        fd,
        source,
        offset,
        stride,
        modifier,
        timestamp_ns,
        duration_ns,
    } = export;
    let fourcc = match source.pixel_format {
        ReplayPixelFormat::Rgba8Unorm | ReplayPixelFormat::Rgba8Srgb => 0x3432_4152,
        ReplayPixelFormat::Bgra8Unorm | ReplayPixelFormat::Bgra8Srgb => 0x3432_4142,
        ReplayPixelFormat::A2b10g10r10Unorm => 0x3033_4241,
        ReplayPixelFormat::A2r10g10b10Unorm => 0x3033_5241,
    };
    let frame = crate::DmaBufReplayFrame::new(
        fd,
        source.width,
        source.height,
        fourcc,
        modifier,
        timestamp_ns,
        duration_ns,
        vec![crate::DmaBufPlane { offset, stride }],
    )
    .map_err(|error| CaptureSessionError::owned(format!("invalid replay export frame: {error}")))?;
    if last_imported.load(Ordering::Acquire) == sequence {
        return Err(CaptureSessionError::new(
            "replay export sequence was already imported",
        ));
    }
    last_imported.store(sequence, Ordering::Release);
    Ok(frame)
}

fn receiver_loop(socket: &UnixDatagram, shared: &CaptureShared, session_id: CaptureSessionId) {
    let mut model = CaptureSessionModel::new(session_id);
    let mut replay_producers = ReplayProducerSelector::default();
    let mut buffer = [0; MAX_MESSAGE_BYTES + 1];
    loop {
        match redunar_capture_vulkan::fd_transport::receive_datagram_fd(socket, &mut buffer) {
            Ok(_) if shared.stop.load(Ordering::Acquire) => break,
            Ok(received) if received.length == 0 => model.reject_transport_message(),
            Ok(received) if received.length > MAX_MESSAGE_BYTES => {
                model.reject_transport_message();
            }
            Ok(received) => match decode_message(&buffer[..received.length]) {
                Ok(redunar_capture::CaptureMessage::FrameBatch {
                    session_id,
                    api,
                    first_sequence,
                    frame_intervals_ns,
                }) => {
                    let _ = model.accept_frame_batch(
                        session_id,
                        api,
                        first_sequence,
                        &frame_intervals_ns,
                    );
                }
                Ok(message) => accept_capture_message(
                    &mut model,
                    &mut replay_producers,
                    shared,
                    message,
                    received.descriptor,
                    received.sender_path.as_deref(),
                ),
                Err(_) => model.reject_transport_message(),
            },
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                model.fail(format!("capture socket failed: {error}"));
                publish(shared, model.snapshot());
                break;
            }
        }
        publish(shared, model.snapshot());
    }
}

fn accept_capture_message(
    model: &mut CaptureSessionModel,
    replay_producers: &mut ReplayProducerSelector,
    shared: &CaptureShared,
    message: redunar_capture::CaptureMessage,
    received_fd: Option<OwnedFd>,
    sender_path: Option<&Path>,
) {
    let is_hello = matches!(&message, redunar_capture::CaptureMessage::Hello { .. });
    let exported = replay_export_metadata(&message, received_fd);
    let export_message = matches!(
        &message,
        redunar_capture::CaptureMessage::ReplayFrameExported { .. }
    );
    if export_message && exported.is_none() {
        model.reject_transport_message();
        return;
    }
    if let Some(exported) = exported.as_ref()
        && !replay_producers.accepts(sender_path, exported.source, exported.timestamp_ns)
    {
        if release_provisional_export(shared, exported.sequence, sender_path).is_err() {
            model.reject_transport_message();
        }
        return;
    }
    let accepted = model.accept(message).is_ok();
    if accepted && is_hello {
        let stale = {
            let mut exports = lock_unpoisoned(&shared.replay_exports);
            exports
                .drain(..)
                .map(|export| export.sequence)
                .collect::<Vec<_>>()
        };
        for sequence in stale {
            let _ = shared.replay_release.release(sequence);
        }
    } else if accepted && let Some(exported) = exported {
        if shared
            .replay_release
            .register(exported.sequence, sender_path)
            .is_ok()
        {
            enqueue_replay_export(shared, exported);
        } else {
            model.reject_transport_message();
        }
    }
}

fn replay_export_metadata(
    message: &redunar_capture::CaptureMessage,
    received_fd: Option<OwnedFd>,
) -> Option<ReplayExportMetadata> {
    let redunar_capture::CaptureMessage::ReplayFrameExported {
        sequence,
        source,
        offset,
        stride,
        modifier,
        timestamp_ns,
        duration_ns,
        ..
    } = message
    else {
        return None;
    };
    received_fd.map(|fd| ReplayExportMetadata {
        sequence: *sequence,
        fd,
        source: *source,
        offset: *offset,
        stride: *stride,
        modifier: *modifier,
        timestamp_ns: *timestamp_ns,
        duration_ns: *duration_ns,
    })
}

fn release_provisional_export(
    shared: &CaptureShared,
    sequence: u64,
    sender_path: Option<&Path>,
) -> Result<(), CaptureSessionError> {
    shared.replay_release.register(sequence, sender_path)?;
    shared.replay_release.release(sequence)
}

#[derive(Default)]
struct ReplayProducerSelector {
    selected: Option<PathBuf>,
    selected_last_timestamp_ns: u64,
    candidates: BTreeMap<PathBuf, ReplayProducerCandidate>,
}

#[derive(Clone, Copy)]
struct ReplayProducerCandidate {
    exports: u16,
}

impl ReplayProducerSelector {
    fn accepts(
        &mut self,
        sender_path: Option<&Path>,
        _source: redunar_capture::ReplaySourceCandidate,
        timestamp_ns: u64,
    ) -> bool {
        let Some(sender_path) = sender_path else {
            // Connected socket pairs used by local diagnostics have no
            // filesystem sender. They are already a single explicit producer.
            return true;
        };
        if let Some(selected) = &self.selected {
            if selected == sender_path {
                self.selected_last_timestamp_ns = self.selected_last_timestamp_ns.max(timestamp_ns);
                return true;
            }
            // A startup helper may legitimately export first. Once it has
            // stopped presenting for a full second, let a sustained game
            // process earn ownership instead of pinning Replay forever.
            if timestamp_ns.saturating_sub(self.selected_last_timestamp_ns)
                <= REPLAY_PRODUCER_STALE_NS
            {
                return false;
            }
            crate::log_op!(
                "Redunar Replay: the previous capture producer became idle; validating its replacement"
            );
            self.selected = None;
            self.selected_last_timestamp_ns = 0;
            self.candidates.clear();
        }
        if !self.candidates.contains_key(sender_path)
            && self.candidates.len() == REPLAY_PRODUCER_CANDIDATE_CAPACITY
        {
            return false;
        }
        let candidate = self
            .candidates
            .entry(sender_path.to_path_buf())
            .or_insert(ReplayProducerCandidate { exports: 0 });
        // Producer identity belongs to the process reply socket, not to one
        // swapchain shape. Real games commonly replace startup, menu, and
        // gameplay swapchains on the same process. Resetting confirmation for
        // each size or format change could keep a healthy game provisional
        // forever, leaving Replay visibly enabled but with no rolling buffer.
        candidate.exports = candidate.exports.saturating_add(1);
        if candidate.exports < REPLAY_PRODUCER_CONFIRMATION_EXPORTS {
            return false;
        }
        crate::log_op!(
            "Redunar Replay: capture producer confirmed after {} exports",
            candidate.exports
        );
        self.selected = Some(sender_path.to_path_buf());
        self.selected_last_timestamp_ns = timestamp_ns;
        self.candidates.clear();
        true
    }
}

fn publish(shared: &CaptureShared, snapshot: CaptureSnapshot) {
    *lock_unpoisoned(&shared.latest) = Arc::new(snapshot);
}

fn random_session_id() -> Result<CaptureSessionId, CaptureSessionError> {
    let mut random = File::open("/dev/urandom").map_err(|error| {
        CaptureSessionError::owned(format!("could not open randomness: {error}"))
    })?;
    for _ in 0..4 {
        let mut bytes = [0; 16];
        random.read_exact(&mut bytes).map_err(|error| {
            CaptureSessionError::owned(format!("could not read randomness: {error}"))
        })?;
        if let Ok(session_id) = CaptureSessionId::new(bytes) {
            return Ok(session_id);
        }
    }
    Err(CaptureSessionError::new(
        "could not generate a non-zero capture session id",
    ))
}

fn create_overlay_telemetry_file(directory: &Path) -> io::Result<(PathBuf, File)> {
    let path = directory.join(OVERLAY_TELEMETRY_FILE);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    let telemetry = OverlayHardwareTelemetry {
        revision: 2,
        corner: 0,
        preset: 0,
        layout: 0,
        palette: 0,
        metrics: 1,
        opacity_percent: 50,
        scale_percent: 100,
        replay_saved_revision: 0,
        metrics_visible: None,
        branding_visible: true,
        cpu_utilization_tenths: None,
        cpu_temperature_tenths_celsius: None,
        gpu_utilization_tenths: None,
        gpu_temperature_tenths_celsius: None,
    };
    let mut bytes = [0; OVERLAY_HARDWARE_TELEMETRY_BYTES];
    encode_overlay_hardware_telemetry(telemetry, &mut bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    file.write_all_at(&bytes, 0)?;
    let menu = ReplayMenuTelemetry {
        revision: 2,
        visible: false,
        pointer_pressed: false,
        cursor_x: 5_000,
        cursor_y: 5_000,
        hover_target: 0,
        pressed_target: 0,
        selected_duration_index: 1,
        status: redunar_capture::ReplayMenuStatus::Inactive,
        available_seconds: 0,
        click_revision: 0,
        frame_rate: 60,
        quality: 1,
        output_format: 0,
        save_enabled: false,
        overlay_shortcut: redunar_capture::ReplayShortcutLabel::EMPTY,
        save_shortcut: redunar_capture::ReplayShortcutLabel::EMPTY,
    };
    let mut menu_bytes = [0; REPLAY_MENU_TELEMETRY_BYTES];
    encode_replay_menu_telemetry(menu, &mut menu_bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    file.write_all_at(&menu_bytes, OVERLAY_HARDWARE_TELEMETRY_BYTES as u64)?;
    Ok((path, file))
}

fn write_overlay_hardware(
    file: &File,
    revision: &AtomicU64,
    hardware: Option<&SystemSnapshot>,
    packed_config: u64,
    replay_saved_revision: u16,
) -> io::Result<()> {
    let next_revision = revision
        .fetch_add(2, Ordering::Relaxed)
        .checked_add(2)
        .filter(|revision| *revision > 0 && revision.is_multiple_of(2))
        .ok_or_else(|| io::Error::other("overlay telemetry revision overflowed"))?;
    let odd_revision = next_revision - 1;
    file.write_all_at(&odd_revision.to_le_bytes(), 16)?;
    file.write_all_at(&odd_revision.to_le_bytes(), 40)?;

    let gpu = hardware.and_then(|snapshot| snapshot.gpus.first());
    let telemetry = OverlayHardwareTelemetry {
        revision: next_revision,
        corner: (packed_config & 0xff) as u8,
        preset: ((packed_config >> 8) & 0xff) as u8,
        layout: ((packed_config >> 56) & 0x0f) as u8,
        palette: ((packed_config >> 60) & 0x0f) as u8,
        metrics: ((packed_config >> 16) & 0xffff) as u16,
        opacity_percent: ((packed_config >> 32) & 0xff) as u8,
        scale_percent: ((packed_config >> 40) & 0xff) as u8,
        replay_saved_revision,
        metrics_visible: match (packed_config >> 48) & 0xff {
            value if value & 0x03 == 1 => Some(true),
            value if value & 0x03 == 2 => Some(false),
            _ => None,
        },
        branding_visible: ((packed_config >> 48) & 0x04) != 0,
        cpu_utilization_tenths: hardware
            .and_then(|snapshot| snapshot.cpu.utilization_percent)
            .and_then(|value| metric_tenths(value, 1_000)),
        cpu_temperature_tenths_celsius: hardware
            .and_then(|snapshot| snapshot.cpu.temperature_celsius)
            .and_then(|value| metric_tenths(value, 2_000)),
        gpu_utilization_tenths: gpu
            .and_then(|gpu| gpu.utilization_percent)
            .and_then(|value| metric_tenths(value, 1_000)),
        gpu_temperature_tenths_celsius: gpu
            .and_then(|gpu| gpu.temperature_celsius)
            .and_then(|value| metric_tenths(value, 2_000)),
    };
    let mut bytes = [0; OVERLAY_HARDWARE_TELEMETRY_BYTES];
    encode_overlay_hardware_telemetry(telemetry, &mut bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    file.write_all_at(&bytes, 0)
}

#[expect(
    clippy::too_many_arguments,
    reason = "the fixed wire layout has eight independent fields"
)]
fn pack_overlay_config(
    corner: u8,
    preset: u8,
    metrics: u16,
    opacity: u8,
    scale: u8,
    layout: u8,
    palette: u8,
    branding_visible: bool,
) -> u64 {
    u64::from(corner)
        | (u64::from(preset) << 8)
        | (u64::from(metrics) << 16)
        | (u64::from(opacity) << 32)
        | (u64::from(scale) << 40)
        | (u64::from(branding_visible) << 50)
        | (u64::from(layout & 0x0f) << 56)
        | (u64::from(palette & 0x0f) << 60)
}

const fn overlay_corner_code(value: redunar_core::OverlayCorner) -> u8 {
    match value {
        redunar_core::OverlayCorner::TopLeft => 0,
        redunar_core::OverlayCorner::TopRight => 1,
        redunar_core::OverlayCorner::BottomLeft => 2,
        redunar_core::OverlayCorner::BottomRight => 3,
    }
}
const fn overlay_preset_code(value: redunar_core::OverlayPreset) -> u8 {
    match value {
        redunar_core::OverlayPreset::Compact => 0,
        redunar_core::OverlayPreset::Detailed => 1,
        redunar_core::OverlayPreset::FpsOnly => 2,
        redunar_core::OverlayPreset::Custom => 3,
    }
}

const fn overlay_layout_code(value: redunar_core::OverlayLayout) -> u8 {
    match value {
        redunar_core::OverlayLayout::Grid => 0,
        redunar_core::OverlayLayout::Ribbon => 1,
        redunar_core::OverlayLayout::Telemetry => 2,
    }
}

const fn overlay_palette_code(value: redunar_core::OverlayPalette) -> u8 {
    match value {
        redunar_core::OverlayPalette::Redunar => 0,
        redunar_core::OverlayPalette::Glacier => 1,
        redunar_core::OverlayPalette::Ember => 2,
        redunar_core::OverlayPalette::Mint => 3,
        redunar_core::OverlayPalette::Mono => 4,
        redunar_core::OverlayPalette::Amethyst => 5,
        redunar_core::OverlayPalette::Solar => 6,
        redunar_core::OverlayPalette::Rose => 7,
    }
}

fn metric_tenths(value: f64, maximum: u16) -> Option<u16> {
    if !value.is_finite() || value < 0.0 || value > f64::from(maximum) / 10.0 {
        return None;
    }
    let scaled = (value * 10.0).round();
    // The domain is at most 2,000, so a tiny bounded binary search avoids a
    // lossy float-to-integer cast and makes the accepted range explicit.
    let mut lower = 0_u16;
    let mut upper = maximum;
    while lower < upper {
        let middle = lower + (upper - lower) / 2;
        if f64::from(middle) < scaled {
            lower = middle.saturating_add(1);
        } else {
            upper = middle;
        }
    }
    Some(lower)
}

fn cleanup_session_directory(directory: &Path) {
    // The known files are removed explicitly first, then the private
    // directory is swept: the in-game Vulkan layer binds additional
    // per-process reply sockets (r-<pid>.sock) that this daemon never
    // names, and an unlinked-but-open reply socket stays usable because
    // the game already holds its connected file descriptor.
    for file in [
        CAPTURE_SOCKET_FILE,
        OVERLAY_TELEMETRY_FILE,
        redunar_platform::VULKAN_CAPTURE_MANIFEST_FILE,
    ] {
        let _ = fs::remove_file(directory.join(file));
    }
    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries.flatten() {
            let is_dir = entry.file_type().is_ok_and(|file_type| file_type.is_dir());
            if is_dir {
                let _ = fs::remove_dir(entry.path());
            } else {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let _ = fs::remove_dir(directory);
}

fn prepare_opengl_capture_library(
    session_directory: &Path,
    source: &Path,
) -> Result<PathBuf, CaptureSessionError> {
    if !source.is_absolute() {
        return Err(CaptureSessionError::new(
            "OpenGL capture library path must be absolute",
        ));
    }
    let metadata = fs::symlink_metadata(source).map_err(|error| {
        CaptureSessionError::owned(format!(
            "could not inspect OpenGL capture library {}: {error}",
            source.display()
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CaptureSessionError::new(
            "OpenGL capture library must be a regular file",
        ));
    }
    let destination = session_directory.join(OPENGL_CAPTURE_LIBRARY_FILE);
    let mut input = File::open(source).map_err(|error| {
        CaptureSessionError::owned(format!(
            "could not open OpenGL capture library {}: {error}",
            source.display()
        ))
    })?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o500)
        .open(&destination)
        .map_err(|error| {
            CaptureSessionError::owned(format!(
                "could not prepare private OpenGL capture library: {error}"
            ))
        })?;
    if let Err(error) = io::copy(&mut input, &mut output).and_then(|_| output.sync_all()) {
        drop(output);
        let _ = fs::remove_file(&destination);
        return Err(CaptureSessionError::owned(format!(
            "could not copy private OpenGL capture library: {error}"
        )));
    }
    Ok(destination)
}

/// Resolve the current uid from /proc/self, matching the ownership checks
/// the Steam activation bridge uses for the same runtime directory.
fn process_uid() -> u32 {
    fs::metadata("/proc/self").map_or(u32::MAX, |metadata| metadata.uid())
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureSessionError {
    message: String,
}

impl CaptureSessionError {
    pub(crate) fn new(message: &str) -> Self {
        Self {
            message: message.to_owned(),
        }
    }

    pub(crate) fn owned(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for CaptureSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CaptureSessionError {}

#[cfg(test)]
mod tests {
    use super::*;
    use redunar_capture::{
        CaptureApi, CaptureMessage, MAX_MESSAGE_BYTES, decode_overlay_hardware_telemetry,
        encode_message,
    };
    use redunar_core::{
        CpuSnapshot, GpuSnapshot, Inheritable, PerGameProfile, ReplayFrameRate, ReplaySettings,
    };
    use std::os::unix::ffi::OsStringExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        library: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = env::temp_dir().join(format!(
                "redunar-capture-session-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("create fixture");
            let library = root.join(CAPTURE_LIBRARY_FILE);
            fs::write(&library, b"fixture").expect("write layer fixture");
            Self { root, library }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).expect("remove fixture");
        }
    }

    #[test]
    fn saved_overlay_visibility_survives_hardware_updates_and_failed_writes() {
        let fixture = Fixture::new();
        let service = crate::RedunarService::with_state_directory(fixture.root.join("state"));
        let mut capture = service
            .start_capture_session(&CaptureSessionConfig::new(
                fixture.root.join("runtime"),
                &fixture.library,
            ))
            .unwrap();
        let mut profile = PerGameProfile::default().resolve(GlobalGameProfile::default());
        let read = || {
            decode_overlay_hardware_telemetry(
                &fs::read(&capture.overlay_telemetry_path).unwrap()
                    [..OVERLAY_HARDWARE_TELEMETRY_BYTES],
            )
            .unwrap()
        };
        for visible in [false, true, false] {
            profile.overlay_visible = visible;
            capture.update_overlay_config(&profile).unwrap();
            capture.update_overlay_hardware(None).unwrap();
            assert_eq!(read().metrics_visible, Some(visible));
            assert_eq!(read().opacity_percent, profile.overlay_opacity.percent());
        }
        capture.publish_replay_saved_notice().unwrap();
        assert_eq!(read().metrics_visible, Some(false));
        assert_ne!(read().replay_saved_revision, 0);
        let before = capture.overlay_config.load(Ordering::Relaxed);
        let good = fs::read(&capture.overlay_telemetry_path).unwrap();
        fs::write(&capture.overlay_telemetry_path, []).unwrap();
        profile.overlay_visible = true;
        assert!(capture.update_overlay_config(&profile).is_err());
        assert_eq!(capture.overlay_config.load(Ordering::Relaxed), before);
        fs::write(&capture.overlay_telemetry_path, good).unwrap();
        capture.update_overlay_config(&profile).unwrap();
        assert_eq!(read().metrics_visible, Some(true));
        capture.shutdown();
    }

    #[test]
    fn opengl_library_is_copied_into_the_private_session_directory() {
        let fixture = Fixture::new();
        let source = fixture.root.join(OPENGL_CAPTURE_LIBRARY_FILE);
        fs::write(&source, b"OpenGL interposer fixture").expect("write OpenGL fixture");
        let session = fixture.root.join("private-session");
        fs::create_dir(&session).expect("create private session");

        let prepared =
            prepare_opengl_capture_library(&session, &source).expect("prepare OpenGL library");

        assert_eq!(prepared, session.join(OPENGL_CAPTURE_LIBRARY_FILE));
        assert_eq!(
            fs::read(&prepared).expect("read prepared library"),
            b"OpenGL interposer fixture"
        );
        assert_eq!(
            fs::metadata(&prepared)
                .expect("prepared metadata")
                .permissions()
                .mode()
                & 0o777,
            0o500
        );
    }

    #[test]
    fn development_executable_can_use_capture_layer_from_debug_dependencies() {
        let fixture = Fixture::new();
        fs::remove_file(&fixture.library).expect("remove packaged layer fixture");
        let executable = fixture.root.join("redunar-tauri");
        let deps = fixture.root.join("deps");
        fs::create_dir_all(&deps).expect("create deps fixture");
        let fallback = deps.join(CAPTURE_LIBRARY_FILE);
        fs::write(&fallback, b"fixture").expect("write fallback layer");
        let located = find_layer_library(&executable).expect("locate development capture layer");
        assert_eq!(located, fallback);
    }

    #[test]
    fn cargo_example_can_use_capture_library_from_its_profile_directory() {
        let fixture = Fixture::new();
        fs::remove_file(&fixture.library).expect("remove packaged layer fixture");
        let examples = fixture.root.join("examples");
        fs::create_dir_all(&examples).expect("create examples fixture");
        let executable = examples.join("capture_probe");
        let fallback = fixture.root.join(CAPTURE_LIBRARY_FILE);
        fs::write(&fallback, b"fixture").expect("write profile layer");

        let located = find_layer_library(&executable).expect("locate profile capture layer");

        assert_eq!(located, fallback);
    }

    fn wait_for_revision(shared: &CaptureShared, revision: u64) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while lock_unpoisoned(&shared.latest).revision < revision && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(
            lock_unpoisoned(&shared.latest).revision >= revision,
            "capture did not update"
        );
    }

    fn receiver_pair() -> (
        CaptureSessionId,
        Arc<CaptureShared>,
        UnixDatagram,
        JoinHandle<()>,
    ) {
        let session_id = CaptureSessionId::new([8; 16]).expect("session id");
        let (receiver, sender) = UnixDatagram::pair().expect("socket pair");
        let shared = Arc::new(CaptureShared {
            latest: Mutex::new(Arc::new(CaptureSessionModel::new(session_id).snapshot())),
            stop: AtomicBool::new(false),
            replay_exports: Mutex::new(VecDeque::new()),
            replay_export_ready: Condvar::new(),
            replay_release: ReplayReleaseTransport {
                session_id,
                reply_socket_path: env::temp_dir().join(format!(
                    "redunar-missing-replay-reply-{}",
                    std::process::id()
                )),
                socket: None,
                targets: Arc::new(Mutex::new(BTreeMap::new())),
                acknowledgements: None,
            },
        });
        let worker_shared = Arc::clone(&shared);
        let worker = thread::spawn(move || receiver_loop(&receiver, &worker_shared, session_id));
        (session_id, shared, sender, worker)
    }

    fn stop_receiver(shared: &CaptureShared, sender: &UnixDatagram, worker: JoinHandle<()>) {
        shared.stop.store(true, Ordering::Release);
        sender.send(&[]).expect("wake receiver");
        worker.join().expect("join receiver");
    }

    #[test]
    fn private_overlay_hardware_snapshot_is_bounded_and_tear_detecting() {
        let fixture = Fixture::new();
        let (path, file) = create_overlay_telemetry_file(&fixture.root).expect("telemetry file");
        assert_eq!(
            fs::metadata(&path)
                .expect("telemetry metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let revision = AtomicU64::new(2);
        let hardware = SystemSnapshot {
            memory: None,
            cpu: CpuSnapshot {
                vendor: "AuthenticAMD".into(),
                model: "Fixture CPU".into(),
                logical_cpus: 8,
                temperature_celsius: Some(62.44),
                utilization_percent: Some(48.74),
                scaling_driver: None,
                governor: None,
                energy_performance_preference: None,
            },
            gpus: vec![GpuSnapshot {
                card: "card0".into(),
                vendor_id: "0x1002".into(),
                device_id: None,
                model: "Fixture GPU".into(),
                driver: None,
                temperature_celsius: Some(71.05),
                utilization_percent: Some(99.1),
                clock_mhz: None,
                vram_used_bytes: None,
                vram_total_bytes: None,
                power_watts: None,
                performance_level: None,
            }],
        };
        write_overlay_hardware(
            &file,
            &revision,
            Some(&hardware),
            pack_overlay_config(0, 0, 1, 50, 100, 0, 0, true),
            0,
        )
        .expect("write telemetry");

        let mut bytes = [0; OVERLAY_HARDWARE_TELEMETRY_BYTES];
        file.read_exact_at(&mut bytes, 0).expect("read telemetry");
        let decoded = decode_overlay_hardware_telemetry(&bytes).expect("decode telemetry");
        assert_eq!(decoded.revision, 4);
        assert_eq!(decoded.cpu_utilization_tenths, Some(487));
        assert_eq!(decoded.cpu_temperature_tenths_celsius, Some(624));
        assert_eq!(decoded.gpu_utilization_tenths, Some(991));
        assert_eq!(decoded.gpu_temperature_tenths_celsius, Some(711));

        write_overlay_replay_notice(&file, &revision, 9).expect("publish Replay notice");
        file.read_exact_at(&mut bytes, 0)
            .expect("read Replay notice");
        let noticed = decode_overlay_hardware_telemetry(&bytes).expect("decode Replay notice");
        assert_eq!(noticed.revision, 6);
        assert_eq!(noticed.replay_saved_revision, 9);

        write_overlay_hardware(
            &file,
            &revision,
            None,
            pack_overlay_config(0, 0, 1, 50, 100, 0, 0, true),
            9,
        )
        .expect("clear telemetry");
        file.read_exact_at(&mut bytes, 0)
            .expect("read cleared telemetry");
        let cleared = decode_overlay_hardware_telemetry(&bytes).expect("decode cleared telemetry");
        assert_eq!(cleared.revision, 8);
        assert_eq!(cleared.replay_saved_revision, 9);
        assert_eq!(cleared.cpu_utilization_tenths, None);
        assert_eq!(cleared.gpu_utilization_tenths, None);
    }

    #[test]
    fn replay_transfer_requires_both_effective_preference_and_runtime_gate() {
        let effective = PerGameProfile::default().resolve(GlobalGameProfile {
            instant_replay: true,
            replay: ReplaySettings {
                frame_rate: ReplayFrameRate::Fps30,
                ..ReplaySettings::default()
            },
            ..GlobalGameProfile::default()
        });
        let config = replay_transfer_launch_config(effective, false);
        assert!(!config.is_requested());
        assert_eq!(config.frame_rate(), ReplayFrameRate::Fps30);

        let verified = replay_transfer_launch_config(effective, true);
        assert!(verified.is_requested());
        assert_eq!(verified.frame_rate(), ReplayFrameRate::Fps30);
    }

    #[test]
    fn module_gate_hides_explicit_per_game_overlay_in_launch_plan() {
        let gates = CaptureModuleGates {
            frame_metrics: true,
            in_game_overlay: false,
            instant_replay: false,
        };
        let effective = gates.resolve_profile(
            PerGameProfile {
                overlay_visible: Inheritable::Custom(true),
                ..PerGameProfile::default()
            },
            GlobalGameProfile::default(),
        );

        assert!(!effective.overlay_visible);
        assert!(effective.capture_metrics);
    }

    #[test]
    fn global_overlay_customization_survives_profile_and_module_resolution() {
        for corner in [
            redunar_core::OverlayCorner::TopLeft,
            redunar_core::OverlayCorner::TopRight,
            redunar_core::OverlayCorner::BottomLeft,
            redunar_core::OverlayCorner::BottomRight,
        ] {
            let global = GlobalGameProfile {
                overlay_visible: true,
                overlay_preset: redunar_core::OverlayPreset::Detailed,
                overlay_metrics: redunar_core::OverlayMetricSet::DETAILED,
                overlay_corner: corner,
                overlay_opacity: redunar_core::OverlayOpacity::new(65).expect("opacity"),
                ..GlobalGameProfile::default()
            };
            let effective =
                CaptureModuleGates::default().resolve_profile(PerGameProfile::default(), global);

            assert!(effective.overlay_visible);
            assert_eq!(effective.overlay_preset, global.overlay_preset);
            assert_eq!(effective.overlay_metrics, global.overlay_metrics);
            assert_eq!(effective.overlay_corner, corner);
            assert_eq!(effective.overlay_opacity, global.overlay_opacity);
        }
    }

    #[test]
    fn steam_setup_requires_executable_sibling_and_returns_exact_typed_snippet() {
        let fixture = Fixture::new();
        let config = CaptureSessionConfig::new(&fixture.root, &fixture.library);
        let app_id = SteamAppId::new(1_808_500).expect("Steam app ID");
        assert!(matches!(
            config.steam_bridge_setup_status(app_id),
            SteamBridgeSetupStatus::Unavailable(_)
        ));

        let wrapper = fixture.root.join(STEAM_LAUNCH_WRAPPER_FILE);
        fs::write(&wrapper, b"fixture").expect("write wrapper fixture");
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))
            .expect("make wrapper executable");
        let SteamBridgeSetupStatus::Available(setup) = config.steam_bridge_setup_status(app_id)
        else {
            panic!("executable sibling should make setup available");
        };
        assert_eq!(setup.app_id(), app_id);
        assert_eq!(
            setup.launch_options(),
            format!(
                "{} --app-id 1808500 -- %command%",
                wrapper.to_str().expect("safe fixture path")
            )
        );

        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o644))
            .expect("remove executable bit");
        assert!(matches!(
            config.steam_bridge_setup_status(app_id),
            SteamBridgeSetupStatus::Unavailable(_)
        ));
    }

    #[test]
    fn steam_launch_options_reject_paths_that_need_shell_quoting() {
        let app_id = SteamAppId::new(42).expect("Steam app ID");
        assert_eq!(
            steam_launch_options(Path::new("/usr/bin/redunar-steam-launch"), app_id)
                .expect("packaged path is safe"),
            "/usr/bin/redunar-steam-launch --app-id 42 -- %command%"
        );
        assert!(
            steam_launch_options(Path::new("/tmp/Redunar Build/redunar-steam-launch"), app_id)
                .is_err()
        );
        let non_utf8 = PathBuf::from(OsString::from_vec(
            b"/tmp/redunar-\xff/redunar-steam-launch".to_vec(),
        ));
        assert!(steam_launch_options(&non_utf8, app_id).is_err());
    }

    #[test]
    fn steam_shader_startup_has_a_five_minute_bounded_window() {
        assert_eq!(STEAM_PRODUCER_STARTUP_GRACE, Duration::from_mins(5));
        assert!(steam_producer_is_starting(
            crate::CapturePhase::Armed,
            None,
            Some(SteamActivationState::Claimed {
                elapsed: Duration::from_mins(5).saturating_sub(Duration::from_secs(1)),
            }),
        ));
    }

    #[test]
    fn steam_claim_time_restarts_the_capture_producer_grace() {
        let old_direct_elapsed = crate::ARMED_STARTUP_GRACE + Duration::from_secs(20);
        assert_eq!(
            startup_clock(old_direct_elapsed, Some(SteamActivationState::Pending)),
            CaptureStartupClock::WaitingForSteam
        );
        assert_eq!(
            startup_clock(
                old_direct_elapsed,
                Some(SteamActivationState::Claimed {
                    elapsed: Duration::from_millis(250),
                }),
            ),
            CaptureStartupClock::Elapsed(Duration::from_millis(250))
        );
        assert_eq!(
            startup_clock(old_direct_elapsed, Some(SteamActivationState::Expired)),
            CaptureStartupClock::SteamExpired
        );
        assert!(steam_producer_is_starting(
            crate::CapturePhase::Armed,
            None,
            Some(SteamActivationState::Claimed {
                elapsed: STEAM_PRODUCER_STARTUP_GRACE
                    .checked_sub(Duration::from_millis(1))
                    .expect("Steam grace exceeds one millisecond"),
            }),
        ));
        assert!(!steam_producer_is_starting(
            crate::CapturePhase::Armed,
            None,
            Some(SteamActivationState::Claimed {
                elapsed: STEAM_PRODUCER_STARTUP_GRACE,
            }),
        ));
        assert!(steam_producer_startup_timed_out(
            crate::CapturePhase::Armed,
            None,
            Some(SteamActivationState::Claimed {
                elapsed: STEAM_PRODUCER_STARTUP_GRACE,
            }),
        ));
        assert!(!steam_producer_startup_timed_out(
            crate::CapturePhase::Capturing,
            Some(42),
            Some(SteamActivationState::Claimed {
                elapsed: STEAM_PRODUCER_STARTUP_GRACE,
            }),
        ));
        assert!(!steam_producer_is_starting(
            crate::CapturePhase::Armed,
            Some(42),
            Some(SteamActivationState::Claimed {
                elapsed: Duration::from_secs(1),
            }),
        ));
    }

    #[test]
    fn receiver_accepts_local_protocol_over_bounded_socket_pair() {
        let (session_id, shared, sender, worker) = receiver_pair();
        let hello = CaptureMessage::Hello {
            session_id,
            process_id: 42,
            api: CaptureApi::Vulkan,
            producer_started_monotonic_ns: 1,
        };
        let mut bytes = [0; MAX_MESSAGE_BYTES];
        let length = encode_message(&hello, &mut bytes).expect("encode hello");
        sender.send(&bytes[..length]).expect("send hello");
        wait_for_revision(&shared, 1);

        assert_eq!(
            lock_unpoisoned(&shared.latest).producer_process_id,
            Some(42)
        );
        stop_receiver(&shared, &sender, worker);
    }

    #[test]
    fn saturated_replay_queue_acknowledges_displaced_export() {
        let (endpoint, acknowledgements) = ReplayExportEndpoint::new_for_test();
        for sequence in 1..=u64::try_from(REPLAY_EXPORT_QUEUE_CAPACITY + 1).expect("bounded") {
            endpoint.enqueue_for_test(ReplayExportMetadata {
                sequence,
                fd: File::open("/dev/null").expect("open fd").into(),
                source: redunar_capture::ReplaySourceCandidate {
                    width: 320,
                    height: 240,
                    pixel_format: ReplayPixelFormat::Bgra8Unorm,
                    target_frames_per_second: 60,
                },
                offset: 0,
                stride: 1_280,
                modifier: 0,
                timestamp_ns: sequence,
                duration_ns: 16_666_667,
            });
        }

        assert_eq!(*lock_unpoisoned(&acknowledgements), vec![1]);
        assert_eq!(
            lock_unpoisoned(&endpoint.shared.replay_exports).len(),
            REPLAY_EXPORT_QUEUE_CAPACITY
        );
    }

    #[test]
    #[ignore = "requires filesystem Unix sockets allowed by the host"]
    fn replay_release_returns_to_the_socket_that_exported_the_frame() {
        let fixture = Fixture::new();
        let daemon_path = fixture.root.join("capture.sock");
        let first_path = fixture.root.join("r-101.sock");
        let second_path = fixture.root.join("r-202.sock");
        let daemon = UnixDatagram::bind(&daemon_path).expect("bind daemon capture socket");
        let first = UnixDatagram::bind(&first_path).expect("bind first producer reply socket");
        first.connect(&daemon_path).expect("connect first producer");
        first.set_nonblocking(true).expect("first nonblocking");
        let second = UnixDatagram::bind(&second_path).expect("bind second producer reply socket");
        second
            .connect(&daemon_path)
            .expect("connect second producer");
        second.set_nonblocking(true).expect("second nonblocking");
        let transport = ReplayReleaseTransport {
            session_id: CaptureSessionId::new([7; 16]).expect("session id"),
            reply_socket_path: fixture.root.join("capture-reply.sock"),
            socket: Some(Arc::new(daemon)),
            targets: Arc::new(Mutex::new(BTreeMap::new())),
            acknowledgements: None,
        };

        transport
            .register(9, Some(&second_path))
            .expect("register exporting producer");
        transport.release(9).expect("release export");

        let mut bytes = [0_u8; MAX_MESSAGE_BYTES];
        let length = second.recv(&mut bytes).expect("receive release");
        assert!(matches!(
            decode_message(&bytes[..length]),
            Ok(redunar_capture::CaptureMessage::ReplayFrameReleased { sequence: 9, .. })
        ));
        assert_eq!(
            first
                .recv(&mut bytes)
                .expect_err("other producer stays idle")
                .kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(lock_unpoisoned(&transport.targets).is_empty());
    }

    #[test]
    fn replay_producer_selection_hands_off_from_an_idle_proton_helper() {
        let helper = Path::new("/run/user/1000/redunar/session/r-101.sock");
        let game = Path::new("/run/user/1000/redunar/session/r-202.sock");
        let source = redunar_capture::ReplaySourceCandidate {
            width: 2_560,
            height: 1_440,
            pixel_format: ReplayPixelFormat::Bgra8Unorm,
            target_frames_per_second: 60,
        };
        let mut selector = ReplayProducerSelector::default();

        let frame_ns = 16_666_667;
        for export in 1..=REPLAY_PRODUCER_CONFIRMATION_EXPORTS {
            assert_eq!(
                selector.accepts(Some(helper), source, u64::from(export) * frame_ns),
                export == REPLAY_PRODUCER_CONFIRMATION_EXPORTS
            );
        }
        let helper_last = u64::from(REPLAY_PRODUCER_CONFIRMATION_EXPORTS) * frame_ns;
        assert!(!selector.accepts(Some(game), source, helper_last + REPLAY_PRODUCER_STALE_NS));
        let game_start = helper_last + REPLAY_PRODUCER_STALE_NS + 1;
        for export in 0..REPLAY_PRODUCER_CONFIRMATION_EXPORTS {
            assert_eq!(
                selector.accepts(
                    Some(game),
                    source,
                    game_start + u64::from(export) * frame_ns
                ),
                export + 1 == REPLAY_PRODUCER_CONFIRMATION_EXPORTS
            );
        }
        assert!(!selector.accepts(Some(helper), source, game_start + 3 * frame_ns));
    }

    #[test]
    fn replay_producer_confirmation_survives_game_swapchain_recreation() {
        let game = Path::new("/run/user/1000/redunar/session/r-202.sock");
        let startup = redunar_capture::ReplaySourceCandidate {
            width: 1_280,
            height: 720,
            pixel_format: ReplayPixelFormat::Bgra8Unorm,
            target_frames_per_second: 60,
        };
        let gameplay = redunar_capture::ReplaySourceCandidate {
            width: 2_560,
            height: 1_440,
            pixel_format: ReplayPixelFormat::Bgra8Srgb,
            target_frames_per_second: 60,
        };
        let mut selector = ReplayProducerSelector::default();

        for export in 1..REPLAY_PRODUCER_CONFIRMATION_EXPORTS {
            let source = if export % 2 == 0 { startup } else { gameplay };
            assert!(!selector.accepts(Some(game), source, u64::from(export) * 16_666_667));
        }
        assert!(selector.accepts(
            Some(game),
            gameplay,
            u64::from(REPLAY_PRODUCER_CONFIRMATION_EXPORTS) * 16_666_667
        ));
    }

    #[test]
    fn oversized_datagram_is_rejected_without_stopping_session() {
        let (_, shared, sender, worker) = receiver_pair();
        sender
            .send(&vec![1; MAX_MESSAGE_BYTES + 1])
            .expect("send oversized message");
        wait_for_revision(&shared, 1);
        assert_eq!(lock_unpoisoned(&shared.latest).rejected_message_count, 1);
        stop_receiver(&shared, &sender, worker);
    }

    #[test]
    #[ignore = "requires filesystem Unix sockets allowed by the host"]
    fn session_handle_binds_private_socket_and_cleans_state() {
        let fixture = Fixture::new();
        let config = CaptureSessionConfig::new(&fixture.root, &fixture.library);
        let mut handle = CaptureSessionHandle::start(&config, CaptureModuleGates::default())
            .expect("start capture");
        let socket_path = handle.socket_path.clone();
        let session_directory = handle.session_directory.clone();
        assert_eq!(
            fs::metadata(&socket_path)
                .expect("socket metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        handle.shutdown();
        assert!(!session_directory.exists());
    }

    #[test]
    #[ignore = "requires filesystem Unix sockets allowed by the host"]
    fn active_session_can_be_failed_after_its_producer_exits() {
        let fixture = Fixture::new();
        let config = CaptureSessionConfig::new(&fixture.root, &fixture.library);
        let mut handle = CaptureSessionHandle::start(&config, CaptureModuleGates::default())
            .expect("start capture");
        let session_directory = handle.session_directory.clone();

        handle.fail_if_active("capture process exited without a goodbye");

        let snapshot = handle.snapshot();
        assert_eq!(snapshot.phase, crate::CapturePhase::Failed);
        assert_eq!(
            snapshot.failure.as_deref(),
            Some("capture process exited without a goodbye")
        );
        assert!(!session_directory.exists());
    }

    #[test]
    fn cleanup_removes_reply_sockets_the_daemon_never_named() {
        // The in-game layer binds extra r-<pid>.sock replies in the session
        // directory. A cleanup that only unlinks the three known names lets
        // remove_dir fail and leaks the whole directory on every launch.
        let fixture = Fixture::new();
        let directory = fixture.root.join("capture-leak-check");
        fs::create_dir(&directory).expect("create session dir");
        fs::write(directory.join(CAPTURE_SOCKET_FILE), b"").expect("write capture socket stand-in");
        fs::write(directory.join("r-4242.sock"), b"").expect("write reply socket stand-in");
        fs::create_dir(directory.join("empty-subdir")).expect("create subdir");
        cleanup_session_directory(&directory);
        assert!(
            !directory.exists(),
            "a directory holding unnamed reply sockets must be removed"
        );
    }

    #[test]
    fn stale_sweep_removes_only_our_private_capture_directories() {
        let fixture = Fixture::new();
        let config = CaptureSessionConfig::new(&fixture.root, &fixture.library);
        let stale = fixture
            .root
            .join("capture-0123456789abcdef0123456789abcdef");
        fs::create_dir(&stale).expect("create stale dir");
        fs::write(stale.join("r-77.sock"), b"").expect("write stray socket");
        // Look-alikes that must survive: wrong prefix, wrong name shape, and
        // a matching-name plain file rather than a directory.
        let foreign_prefix = fixture.root.join("replay-0123456789abcdef0123456789abcdef");
        let malformed = fixture.root.join("capture-0123456789abcdef0123456789abcde");
        let not_a_dir = fixture
            .root
            .join("capture-0123456789abcdef0123456789abcdef0");
        fs::create_dir(&foreign_prefix).expect("create foreign prefix dir");
        fs::create_dir(&malformed).expect("create malformed dir");
        fs::write(&not_a_dir, b"").expect("create look-alike file");
        assert_eq!(config.sweep_stale_session_directories(), 1);
        assert!(!stale.exists());
        assert!(foreign_prefix.exists());
        assert!(malformed.exists());
        assert!(not_a_dir.exists());
        // A second sweep is a no-op, and the fixture root itself survives.
        assert_eq!(config.sweep_stale_session_directories(), 0);
        assert!(fixture.root.exists());
    }
}
