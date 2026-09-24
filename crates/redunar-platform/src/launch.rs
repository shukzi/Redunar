use redunar_capture::{CaptureSessionId, PROTOCOL_VERSION};
use redunar_core::{
    OverlayCorner, OverlayLayout, OverlayMetricSet, OverlayOpacity, OverlayPalette, OverlayPreset,
    ReplayFrameRate,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

pub const VULKAN_CAPTURE_LAYER_NAME: &str = "VK_LAYER_REDUNAR_capture";
pub const VULKAN_CAPTURE_MANIFEST_FILE: &str = "VkLayer_REDUNAR_capture.json";
pub const REDUNAR_OVERLAY_VISIBLE_ENV: &str = "REDUNAR_OVERLAY_VISIBLE";
pub const REDUNAR_OVERLAY_TELEMETRY_ENV: &str = "REDUNAR_OVERLAY_TELEMETRY";
pub const REDUNAR_OVERLAY_PRESET_ENV: &str = "REDUNAR_OVERLAY_PRESET";
pub const REDUNAR_OVERLAY_LAYOUT_ENV: &str = "REDUNAR_OVERLAY_LAYOUT";
pub const REDUNAR_OVERLAY_PALETTE_ENV: &str = "REDUNAR_OVERLAY_PALETTE";
pub const REDUNAR_OVERLAY_BRANDING_ENV: &str = "REDUNAR_OVERLAY_BRANDING";
pub const REDUNAR_OVERLAY_CORNER_ENV: &str = "REDUNAR_OVERLAY_CORNER";
pub const REDUNAR_OVERLAY_OPACITY_ENV: &str = "REDUNAR_OVERLAY_OPACITY_PERCENT";
pub const REDUNAR_OVERLAY_METRICS_ENV: &str = "REDUNAR_OVERLAY_METRICS";
pub const REDUNAR_REPLAY_TRANSFER_ENV: &str = "REDUNAR_REPLAY_TRANSFER";
pub const REDUNAR_REPLAY_FRAME_RATE_ENV: &str = "REDUNAR_REPLAY_FRAME_RATE";
pub const REDUNAR_REPLAY_PRODUCTION_ENV: &str = "REDUNAR_REPLAY_PRODUCTION";
pub const REDUNAR_CAPTURE_REPLY_SOCKET_ENV: &str = "REDUNAR_CAPTURE_REPLY_SOCKET";
static NEXT_MANIFEST_TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// Whether a saved command can pass Redunar's private capture environment to
/// the game it starts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureLaunchSupport {
    Supported,
    Unavailable(CaptureLaunchUnavailableReason),
}

impl CaptureLaunchSupport {
    #[must_use]
    pub const fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }

    #[must_use]
    pub const fn reason(self) -> Option<CaptureLaunchUnavailableReason> {
        match self {
            Self::Supported => None,
            Self::Unavailable(reason) => Some(reason),
        }
    }
}

/// A stable capability reason suitable for matching in the UI, with a calm
/// plain-language [`Display`](fmt::Display) explanation for users.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureLaunchUnavailableReason {
    FlatpakSandbox,
    SteamForwarding,
}

impl fmt::Display for CaptureLaunchUnavailableReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::FlatpakSandbox => {
                "Flatpak games cannot access Redunar's private host capture files yet"
            }
            Self::SteamForwarding => {
                "Steam forwarding cannot pass Redunar's private capture environment to the game when the Steam client is already running"
            }
        })
    }
}

/// Which process a caller can safely treat as the lifetime of a launch.
/// Forwarding clients acknowledge a request and exit independently of the
/// game, so their child PID is not game-lifetime evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameLaunchProcessOwnership {
    DirectChild,
    ForwardedSteam { app_id: Option<u32> },
}

/// Bounded presentation configuration passed only to a launched child.
///
/// Overlay v0 intentionally has one boolean setting. Keeping this typed avoids
/// forwarding an arbitrary environment fragment or configuration string into
/// the game process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayLaunchConfig {
    visible: bool,
    preset: OverlayPreset,
    layout: OverlayLayout,
    palette: OverlayPalette,
    branding_visible: bool,
    corner: OverlayCorner,
    opacity: OverlayOpacity,
    metrics: OverlayMetricSet,
}

impl OverlayLaunchConfig {
    #[must_use]
    pub fn new(visible: bool) -> Self {
        Self {
            visible,
            preset: OverlayPreset::Compact,
            layout: OverlayLayout::default(),
            palette: OverlayPalette::default(),
            branding_visible: true,
            corner: OverlayCorner::TopLeft,
            opacity: OverlayOpacity::default(),
            metrics: OverlayMetricSet::default(),
        }
    }

    #[must_use]
    pub const fn with_customization(
        mut self,
        preset: OverlayPreset,
        corner: OverlayCorner,
        opacity: OverlayOpacity,
    ) -> Self {
        self.preset = preset;
        self.corner = corner;
        self.opacity = opacity;
        self
    }

    #[must_use]
    pub const fn is_visible(self) -> bool {
        self.visible
    }

    #[must_use]
    pub const fn with_metrics(mut self, metrics: OverlayMetricSet) -> Self {
        self.metrics = metrics;
        self
    }

    #[must_use]
    pub const fn with_style(mut self, layout: OverlayLayout, palette: OverlayPalette) -> Self {
        self.layout = layout;
        self.palette = palette;
        self
    }

    #[must_use]
    pub const fn with_branding(mut self, visible: bool) -> Self {
        self.branding_visible = visible;
        self
    }
}

impl Default for OverlayLaunchConfig {
    fn default() -> Self {
        Self::new(false)
    }
}

/// Typed, child-only replay frame-transfer request.
///
/// The daemon passes `requested = true` only for a launch whose effective
/// Replay preference and runtime capability gates have both passed. Persisted
/// user preference alone must never activate GPU work in the game process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplayTransferLaunchConfig {
    requested: bool,
    frame_rate: ReplayFrameRate,
}

impl ReplayTransferLaunchConfig {
    #[must_use]
    pub const fn new(requested: bool, frame_rate: ReplayFrameRate) -> Self {
        Self {
            requested,
            frame_rate,
        }
    }

    #[must_use]
    pub const fn is_requested(self) -> bool {
        self.requested
    }

    #[must_use]
    pub const fn frame_rate(self) -> ReplayFrameRate {
        self.frame_rate
    }
}

impl Default for ReplayTransferLaunchConfig {
    fn default() -> Self {
        Self::new(false, ReplayFrameRate::Fps60)
    }
}

/// Create a private, session-scoped explicit-layer manifest pointing at the
/// built Redunar layer. Nothing is installed into a system or user Vulkan
/// directory, so discovery remains limited to the child launch environment.
///
/// # Errors
///
/// Returns [`CaptureLaunchError`] for invalid paths, a missing/non-regular
/// layer library, or an atomic manifest write failure.
pub fn prepare_vulkan_layer_directory(
    session_directory: impl AsRef<Path>,
    layer_library: impl AsRef<Path>,
) -> Result<PathBuf, CaptureLaunchError> {
    let session_directory = session_directory.as_ref();
    let layer_library = layer_library.as_ref();
    validate_absolute("capture session directory", session_directory)?;
    validate_absolute("Vulkan capture layer library", layer_library)?;
    let metadata = fs::metadata(layer_library).map_err(|error| {
        CaptureLaunchError::owned(format!(
            "could not inspect Vulkan capture layer {}: {error}",
            layer_library.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(CaptureLaunchError::new(
            "Vulkan capture layer path is not a regular file",
        ));
    }
    let library_path = layer_library
        .to_str()
        .ok_or_else(|| CaptureLaunchError::new("Vulkan capture layer path is not valid UTF-8"))?;

    fs::create_dir_all(session_directory).map_err(|error| {
        CaptureLaunchError::owned(format!(
            "could not create capture session directory {}: {error}",
            session_directory.display()
        ))
    })?;
    fs::set_permissions(session_directory, fs::Permissions::from_mode(0o700)).map_err(|error| {
        CaptureLaunchError::owned(format!(
            "could not secure capture session directory {}: {error}",
            session_directory.display()
        ))
    })?;

    let mut manifest = String::from(concat!(
        "{\n",
        "  \"file_format_version\": \"1.2.0\",\n",
        "  \"layer\": {\n",
        "    \"name\": \"VK_LAYER_REDUNAR_capture\",\n",
        "    \"type\": \"GLOBAL\",\n",
        "    \"library_path\": \""
    ));
    manifest.push_str(&json_escape(library_path));
    manifest.push_str(concat!(
        "\",\n",
        "    \"api_version\": \"1.0.0\",\n",
        "    \"implementation_version\": \"1\",\n",
        "    \"description\": \"Redunar bounded frame presentation telemetry\",\n",
        "    \"functions\": {\n",
        "      \"vkNegotiateLoaderLayerInterfaceVersion\": \"vkNegotiateLoaderLayerInterfaceVersion\"\n",
        "    }\n",
        "  }\n",
        "}\n"
    ));
    let destination = session_directory.join(VULKAN_CAPTURE_MANIFEST_FILE);
    atomic_write_private(&destination, manifest.as_bytes()).map_err(|error| {
        CaptureLaunchError::owned(format!(
            "could not write Vulkan capture manifest {}: {error}",
            destination.display()
        ))
    })?;
    Ok(session_directory.to_path_buf())
}

/// A shell-free launch description that enables Redunar's explicit Vulkan
/// layer only for one child process. Constructing a plan does not spawn it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureLaunchPlan {
    executable: PathBuf,
    arguments: Vec<OsString>,
    working_directory: Option<PathBuf>,
    environment: BTreeMap<OsString, OsString>,
    existing_vulkan_layers: Vec<OsString>,
}

impl CaptureLaunchPlan {
    /// Build a child-only Vulkan capture environment.
    ///
    /// The caller supplies an environment snapshot so conflict decisions are
    /// deterministic and tests never mutate process-global environment state.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureLaunchError`] for relative paths, NUL bytes, malformed
    /// inherited path lists, a sandbox launcher that cannot see Redunar's
    /// private host paths, or an overriding `VK_LAYER_PATH` that would make
    /// explicit-layer discovery ambiguous.
    pub fn new(
        executable: impl Into<PathBuf>,
        arguments: impl IntoIterator<Item = OsString>,
        layer_manifest_directory: impl Into<PathBuf>,
        socket_path: impl Into<PathBuf>,
        session_id: CaptureSessionId,
        inherited_environment: &BTreeMap<OsString, OsString>,
    ) -> Result<Self, CaptureLaunchError> {
        let executable = executable.into();
        let layer_manifest_directory = layer_manifest_directory.into();
        let socket_path = socket_path.into();
        validate_absolute("game executable", &executable)?;
        validate_absolute("layer manifest directory", &layer_manifest_directory)?;
        validate_absolute("capture socket", &socket_path)?;
        validate_no_nul("game executable", executable.as_os_str())?;
        validate_no_nul(
            "layer manifest directory",
            layer_manifest_directory.as_os_str(),
        )?;
        validate_no_nul("capture socket", socket_path.as_os_str())?;
        let arguments = arguments.into_iter().collect::<Vec<_>>();
        for argument in &arguments {
            validate_no_nul("game argument", argument)?;
        }

        if let CaptureLaunchSupport::Unavailable(reason) =
            capture_launch_support(&executable, &arguments)
        {
            return Err(CaptureLaunchError::owned(format!(
                "{reason}; launch without metrics or overlay"
            )));
        }

        if inherited_environment.contains_key(OsStr::new("VK_LAYER_PATH")) {
            return Err(CaptureLaunchError::new(
                "VK_LAYER_PATH is already set; Redunar will not silently override it",
            ));
        }

        let mut environment = BTreeMap::new();
        let add_layer_path = prepend_search_path(
            &layer_manifest_directory,
            inherited_environment.get(OsStr::new("VK_ADD_LAYER_PATH")),
        )?;
        environment.insert(OsString::from("VK_ADD_LAYER_PATH"), add_layer_path);

        let inherited_layers = inherited_environment
            .get(OsStr::new("VK_INSTANCE_LAYERS"))
            .map_or_else(Vec::new, |value| split_colon_list(value));
        let mut enabled_layers = inherited_layers.clone();
        if !enabled_layers
            .iter()
            .any(|layer| layer == OsStr::new(VULKAN_CAPTURE_LAYER_NAME))
        {
            enabled_layers.push(OsString::from(VULKAN_CAPTURE_LAYER_NAME));
        }
        environment.insert(
            OsString::from("VK_INSTANCE_LAYERS"),
            join_colon_list(&enabled_layers),
        );
        environment.insert(
            OsString::from("REDUNAR_CAPTURE_SOCKET"),
            socket_path.into_os_string(),
        );
        environment.insert(
            OsString::from("REDUNAR_CAPTURE_SESSION"),
            OsString::from(session_id.to_hex()),
        );
        environment.insert(
            OsString::from("REDUNAR_CAPTURE_PROTOCOL"),
            OsString::from(PROTOCOL_VERSION.to_string()),
        );
        environment.insert(
            OsString::from(REDUNAR_OVERLAY_VISIBLE_ENV),
            OsString::from("0"),
        );
        environment.insert(
            OsString::from(REDUNAR_REPLAY_TRANSFER_ENV),
            OsString::from("0"),
        );
        environment.insert(
            OsString::from(REDUNAR_REPLAY_PRODUCTION_ENV),
            OsString::from("0"),
        );
        environment.insert(
            OsString::from(REDUNAR_REPLAY_FRAME_RATE_ENV),
            OsString::from("60"),
        );

        Ok(Self {
            executable,
            arguments,
            working_directory: None,
            environment,
            existing_vulkan_layers: inherited_layers,
        })
    }

    /// Set an optional direct child working directory without shell parsing.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureLaunchError`] for a relative path or NUL byte.
    pub fn with_working_directory(
        mut self,
        working_directory: Option<PathBuf>,
    ) -> Result<Self, CaptureLaunchError> {
        if let Some(directory) = &working_directory {
            validate_absolute("game working directory", directory)?;
            validate_no_nul("game working directory", directory.as_os_str())?;
        }
        self.working_directory = working_directory;
        Ok(self)
    }

    /// Preload Redunar's GLX/EGL presentation observer into this direct child.
    /// Existing preload entries are preserved in their original order after
    /// Redunar's library. The library is launch-scoped and never installed as
    /// a global OpenGL provider.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureLaunchError`] when the library path is relative,
    /// contains a NUL byte, or cannot be combined with the inherited preload
    /// list.
    pub fn with_opengl_capture_library(
        mut self,
        library: impl Into<PathBuf>,
        inherited_environment: &BTreeMap<OsString, OsString>,
    ) -> Result<Self, CaptureLaunchError> {
        let library = library.into();
        validate_absolute("OpenGL capture library", &library)?;
        validate_no_nul("OpenGL capture library", library.as_os_str())?;
        let preload = prepend_search_path(
            &library,
            inherited_environment.get(OsStr::new("LD_PRELOAD")),
        )?;
        self.environment
            .insert(OsString::from("LD_PRELOAD"), preload);
        // Leave SDL's provider selection to the game. A forced dynapi path can
        // change the SDL instance used by its audio backend (including FAudio).
        Ok(self)
    }

    /// Add Redunar's bounded overlay configuration to the child environment.
    /// The parent process environment is never mutated.
    #[must_use]
    pub fn with_overlay_config(mut self, config: OverlayLaunchConfig) -> Self {
        self.environment.insert(
            OsString::from(REDUNAR_OVERLAY_VISIBLE_ENV),
            OsString::from(if config.is_visible() { "1" } else { "0" }),
        );
        self.environment.insert(
            OsString::from(REDUNAR_OVERLAY_PRESET_ENV),
            OsString::from(match config.preset {
                OverlayPreset::Compact => "compact",
                OverlayPreset::FpsOnly => "fps-only",
                OverlayPreset::Detailed => "detailed",
                OverlayPreset::Custom => "custom",
            }),
        );
        self.environment.insert(
            OsString::from(REDUNAR_OVERLAY_LAYOUT_ENV),
            OsString::from(match config.layout {
                OverlayLayout::Grid => "grid",
                OverlayLayout::Ribbon => "ribbon",
                OverlayLayout::Telemetry => "telemetry",
            }),
        );
        self.environment.insert(
            OsString::from(REDUNAR_OVERLAY_PALETTE_ENV),
            OsString::from(match config.palette {
                OverlayPalette::Redunar => "redunar",
                OverlayPalette::Glacier => "glacier",
                OverlayPalette::Ember => "ember",
                OverlayPalette::Mint => "mint",
                OverlayPalette::Mono => "mono",
                OverlayPalette::Amethyst => "amethyst",
                OverlayPalette::Solar => "solar",
                OverlayPalette::Rose => "rose",
            }),
        );
        self.environment.insert(
            OsString::from(REDUNAR_OVERLAY_BRANDING_ENV),
            OsString::from(if config.branding_visible { "1" } else { "0" }),
        );
        self.environment.insert(
            OsString::from(REDUNAR_OVERLAY_CORNER_ENV),
            OsString::from(match config.corner {
                OverlayCorner::TopLeft => "top-left",
                OverlayCorner::TopRight => "top-right",
                OverlayCorner::BottomLeft => "bottom-left",
                OverlayCorner::BottomRight => "bottom-right",
            }),
        );
        self.environment.insert(
            OsString::from(REDUNAR_OVERLAY_OPACITY_ENV),
            OsString::from(config.opacity.percent().to_string()),
        );
        self.environment.insert(
            OsString::from(REDUNAR_OVERLAY_METRICS_ENV),
            OsString::from(config.metrics.bits().to_string()),
        );
        self
    }

    /// Add the bounded replay-transfer request to this child only.
    #[must_use]
    pub fn with_replay_transfer_config(mut self, config: ReplayTransferLaunchConfig) -> Self {
        self.environment.insert(
            OsString::from(REDUNAR_REPLAY_TRANSFER_ENV),
            OsString::from(if config.requested { "1" } else { "0" }),
        );
        self.environment.insert(
            OsString::from(REDUNAR_REPLAY_PRODUCTION_ENV),
            OsString::from(if config.requested { "1" } else { "0" }),
        );
        self.environment.insert(
            OsString::from(REDUNAR_REPLAY_FRAME_RATE_ENV),
            OsString::from(match config.frame_rate {
                ReplayFrameRate::Fps30 => "30",
                ReplayFrameRate::Fps60 => "60",
                ReplayFrameRate::Fps120 => "120",
            }),
        );
        self
    }

    /// Pass the daemon-owned, fixed-size read-only telemetry file to the child.
    /// The path is private to this capture session and is never added to the
    /// parent environment.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureLaunchError`] when the path is relative or contains a
    /// NUL byte.
    pub fn with_overlay_telemetry_path(
        mut self,
        telemetry_path: impl Into<PathBuf>,
    ) -> Result<Self, CaptureLaunchError> {
        let telemetry_path = telemetry_path.into();
        validate_absolute("overlay telemetry file", &telemetry_path)?;
        validate_no_nul("overlay telemetry file", telemetry_path.as_os_str())?;
        self.environment.insert(
            OsString::from(REDUNAR_OVERLAY_TELEMETRY_ENV),
            telemetry_path.into_os_string(),
        );
        Ok(self)
    }

    /// Set the private daemon-to-producer reply socket used for replay slot
    /// release acknowledgements.
    #[must_use]
    pub fn with_capture_reply_socket(mut self, socket_path: impl Into<PathBuf>) -> Self {
        self.environment.insert(
            OsString::from(REDUNAR_CAPTURE_REPLY_SOCKET_ENV),
            socket_path.into().into_os_string(),
        );
        self
    }

    #[must_use]
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        command.args(&self.arguments);
        if let Some(directory) = &self.working_directory {
            command.current_dir(directory);
        }
        command.envs(&self.environment);
        command
    }

    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    #[must_use]
    pub fn arguments(&self) -> &[OsString] {
        &self.arguments
    }

    #[must_use]
    pub fn working_directory(&self) -> Option<&Path> {
        self.working_directory.as_deref()
    }

    #[must_use]
    pub fn environment(&self) -> &BTreeMap<OsString, OsString> {
        &self.environment
    }

    #[must_use]
    pub fn existing_vulkan_layers(&self) -> &[OsString] {
        &self.existing_vulkan_layers
    }
}

/// Whether a direct child launch can access Redunar's private host-side
/// explicit-layer manifest, socket, and library paths.
///
/// Flatpak intentionally creates a separate filesystem and environment
/// boundary. Passing host paths to the `flatpak` launcher would therefore look
/// configured while the sandboxed game cannot load them. A future Flatpak
/// bridge must opt in through a separately verified launch path.
#[must_use]
pub fn supports_host_explicit_layer_launch(executable: &Path) -> bool {
    !executable
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.eq_ignore_ascii_case("flatpak"))
}

/// Whether an exact command can inherit Redunar's private capture environment.
///
/// A second native Steam invocation forwards only its arguments to an already
/// running client and exits. The game is then created by that existing client,
/// so treating `steam -applaunch` as capture-capable would arm a session whose
/// manifest, socket, and session identity can never reach the game. Keep the
/// typed launch available, but require the explicit no-metrics path until a
/// separately verified Steam integration exists.
#[must_use]
pub fn supports_host_capture_launch(executable: &Path, arguments: &[OsString]) -> bool {
    capture_launch_support(executable, arguments).is_supported()
}

#[must_use]
pub fn capture_launch_support(executable: &Path, arguments: &[OsString]) -> CaptureLaunchSupport {
    if !supports_host_explicit_layer_launch(executable) {
        return CaptureLaunchSupport::Unavailable(CaptureLaunchUnavailableReason::FlatpakSandbox);
    }
    if matches!(
        game_launch_process_ownership(executable, arguments),
        GameLaunchProcessOwnership::ForwardedSteam { .. }
    ) {
        return CaptureLaunchSupport::Unavailable(CaptureLaunchUnavailableReason::SteamForwarding);
    }
    CaptureLaunchSupport::Supported
}

#[must_use]
pub fn game_launch_process_ownership(
    executable: &Path,
    arguments: &[OsString],
) -> GameLaunchProcessOwnership {
    let is_steam_client = executable
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| {
            name.eq_ignore_ascii_case("steam")
                || name.eq_ignore_ascii_case("steam.sh")
                || name.eq_ignore_ascii_case("bin_steam.sh")
        });
    if !is_steam_client {
        return GameLaunchProcessOwnership::DirectChild;
    }
    if let Some(index) = arguments
        .iter()
        .position(|argument| argument == OsStr::new("-applaunch"))
    {
        let app_id = arguments
            .get(index.saturating_add(1))
            .and_then(|argument| argument.to_str())
            .and_then(|argument| argument.parse::<u32>().ok())
            .filter(|app_id| *app_id != 0);
        return GameLaunchProcessOwnership::ForwardedSteam { app_id };
    }
    if arguments.iter().any(|argument| {
        argument
            .to_str()
            .is_some_and(|value| value.starts_with("steam://"))
    }) {
        return GameLaunchProcessOwnership::ForwardedSteam { app_id: None };
    }
    GameLaunchProcessOwnership::DirectChild
}

fn validate_absolute(label: &str, path: &Path) -> Result<(), CaptureLaunchError> {
    if !path.is_absolute() {
        return Err(CaptureLaunchError::owned(format!(
            "{label} must be an absolute path"
        )));
    }
    Ok(())
}

fn validate_no_nul(label: &str, value: &OsStr) -> Result<(), CaptureLaunchError> {
    if value.as_bytes().contains(&0) {
        return Err(CaptureLaunchError::owned(format!(
            "{label} contains a NUL byte"
        )));
    }
    Ok(())
}

fn prepend_search_path(
    layer_directory: &Path,
    inherited: Option<&OsString>,
) -> Result<OsString, CaptureLaunchError> {
    let mut paths = vec![layer_directory.to_path_buf()];
    if let Some(inherited) = inherited {
        paths.extend(std::env::split_paths(inherited));
    }
    std::env::join_paths(paths)
        .map_err(|_| CaptureLaunchError::new("VK_ADD_LAYER_PATH contains an invalid path list"))
}

fn split_colon_list(value: &OsStr) -> Vec<OsString> {
    value
        .as_bytes()
        .split(|byte| *byte == b':')
        .filter(|part| !part.is_empty())
        .map(|part| OsString::from(String::from_utf8_lossy(part).into_owned()))
        .collect()
}

fn join_colon_list(values: &[OsString]) -> OsString {
    let mut joined = Vec::new();
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            joined.push(b':');
        }
        joined.extend_from_slice(value.as_bytes());
    }
    OsString::from(String::from_utf8_lossy(&joined).into_owned())
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            value if value.is_control() => {
                use std::fmt::Write as _;
                write!(escaped, "\\u{:04x}", u32::from(value)).expect("write to string");
            }
            value => escaped.push(value),
        }
    }
    escaped
}

fn atomic_write_private(destination: &Path, contents: &[u8]) -> io::Result<()> {
    let directory = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "manifest has no parent directory",
        )
    })?;
    for _ in 0..32 {
        let id = NEXT_MANIFEST_TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let temporary = directory.join(format!(
            ".{VULKAN_CAPTURE_MANIFEST_FILE}.{}.{id}.tmp",
            std::process::id()
        ));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let result = (|| {
            file.write_all(contents)?;
            file.sync_all()?;
            fs::rename(&temporary, destination)?;
            File::open(directory)?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        return result;
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a temporary Vulkan manifest",
    ))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureLaunchError {
    message: String,
}

impl CaptureLaunchError {
    fn new(message: &str) -> Self {
        Self {
            message: message.to_owned(),
        }
    }

    fn owned(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for CaptureLaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CaptureLaunchError {}

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
            let root = std::env::temp_dir().join(format!(
                "redunar-launch-fixture-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("create fixture");
            Self { root }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).expect("remove fixture");
        }
    }

    fn session_id() -> CaptureSessionId {
        CaptureSessionId::new([9; 16]).expect("session id")
    }

    #[test]
    fn launch_plan_preserves_literal_arguments_and_scopes_capture_to_child() {
        let mut inherited = BTreeMap::new();
        inherited.insert(
            OsString::from("VK_INSTANCE_LAYERS"),
            OsString::from("VK_LAYER_EXISTING_overlay"),
        );
        inherited.insert(
            OsString::from("VK_ADD_LAYER_PATH"),
            OsString::from("/opt/existing/layers"),
        );
        inherited.insert(
            OsString::from(REDUNAR_OVERLAY_VISIBLE_ENV),
            OsString::from("untrusted-inherited-value"),
        );
        let argument = OsString::from("$(touch /tmp/redunar-must-not-execute)");
        let plan = CaptureLaunchPlan::new(
            "/usr/bin/game",
            [argument.clone()],
            "/opt/redunar/layers",
            "/run/user/1000/redunar/capture.sock",
            session_id(),
            &inherited,
        )
        .expect("launch plan")
        .with_working_directory(Some(PathBuf::from("/games/example")))
        .expect("working directory")
        .with_overlay_config(OverlayLaunchConfig::new(true).with_customization(
            OverlayPreset::FpsOnly,
            OverlayCorner::BottomRight,
            OverlayOpacity::new(50).expect("opacity"),
        ))
        .with_overlay_telemetry_path("/run/user/1000/redunar/overlay-telemetry-v1.bin")
        .expect("overlay telemetry path");

        assert_eq!(plan.arguments(), &[argument]);
        assert_eq!(plan.working_directory(), Some(Path::new("/games/example")));
        assert_eq!(
            plan.environment().get(OsStr::new("VK_INSTANCE_LAYERS")),
            Some(&OsString::from(
                "VK_LAYER_EXISTING_overlay:VK_LAYER_REDUNAR_capture"
            ))
        );
        assert_eq!(
            plan.environment().get(OsStr::new("VK_ADD_LAYER_PATH")),
            Some(&OsString::from("/opt/redunar/layers:/opt/existing/layers"))
        );
        assert_eq!(
            plan.existing_vulkan_layers(),
            &[OsString::from("VK_LAYER_EXISTING_overlay")]
        );
        assert_eq!(
            plan.environment()
                .get(OsStr::new(REDUNAR_OVERLAY_VISIBLE_ENV)),
            Some(&OsString::from("1"))
        );
        assert_eq!(
            plan.environment()
                .get(OsStr::new(REDUNAR_OVERLAY_PRESET_ENV)),
            Some(&OsString::from("fps-only"))
        );
        assert_eq!(
            plan.environment()
                .get(OsStr::new(REDUNAR_OVERLAY_CORNER_ENV)),
            Some(&OsString::from("bottom-right"))
        );
        assert_eq!(
            plan.environment()
                .get(OsStr::new(REDUNAR_OVERLAY_OPACITY_ENV)),
            Some(&OsString::from("50"))
        );
        assert_eq!(
            plan.environment()
                .get(OsStr::new(REDUNAR_OVERLAY_METRICS_ENV)),
            Some(&OsString::from(
                OverlayMetricSet::COMPACT.bits().to_string()
            ))
        );
        assert_eq!(
            plan.environment()
                .get(OsStr::new(REDUNAR_OVERLAY_TELEMETRY_ENV)),
            Some(&OsString::from(
                "/run/user/1000/redunar/overlay-telemetry-v1.bin"
            ))
        );
        assert_eq!(
            inherited.get(OsStr::new(REDUNAR_OVERLAY_VISIBLE_ENV)),
            Some(&OsString::from("untrusted-inherited-value")),
            "constructing a child plan must not mutate the inherited snapshot"
        );
    }

    #[test]
    fn overlay_style_has_exact_child_environment_tokens() {
        let plan = CaptureLaunchPlan::new(
            "/usr/bin/game",
            [],
            "/opt/redunar/layers",
            "/run/user/1000/redunar/capture.sock",
            session_id(),
            &BTreeMap::new(),
        )
        .expect("launch plan")
        .with_overlay_config(
            OverlayLaunchConfig::new(true)
                .with_style(OverlayLayout::Ribbon, OverlayPalette::Mint)
                .with_branding(false),
        );
        assert_eq!(
            plan.environment()
                .get(OsStr::new(REDUNAR_OVERLAY_LAYOUT_ENV)),
            Some(&OsString::from("ribbon"))
        );
        assert_eq!(
            plan.environment()
                .get(OsStr::new(REDUNAR_OVERLAY_PALETTE_ENV)),
            Some(&OsString::from("mint"))
        );
        assert_eq!(
            plan.environment()
                .get(OsStr::new(REDUNAR_OVERLAY_BRANDING_ENV)),
            Some(&OsString::from("0"))
        );
    }

    #[test]
    fn every_overlay_corner_has_an_exact_child_environment_token() {
        for (corner, expected) in [
            (OverlayCorner::TopLeft, "top-left"),
            (OverlayCorner::TopRight, "top-right"),
            (OverlayCorner::BottomLeft, "bottom-left"),
            (OverlayCorner::BottomRight, "bottom-right"),
        ] {
            let plan = CaptureLaunchPlan::new(
                "/usr/bin/game",
                [],
                "/opt/redunar/layers",
                "/run/user/1000/redunar/capture.sock",
                session_id(),
                &BTreeMap::new(),
            )
            .expect("launch plan")
            .with_overlay_config(OverlayLaunchConfig::new(true).with_customization(
                OverlayPreset::Compact,
                corner,
                OverlayOpacity::default(),
            ));

            assert_eq!(
                plan.environment()
                    .get(OsStr::new(REDUNAR_OVERLAY_CORNER_ENV)),
                Some(&OsString::from(expected))
            );
        }
    }

    #[test]
    fn overlay_launch_configuration_is_bounded_and_explicit_when_hidden() {
        let plan = CaptureLaunchPlan::new(
            "/usr/bin/game",
            [],
            "/opt/redunar/layers",
            "/run/user/1000/redunar/capture.sock",
            session_id(),
            &BTreeMap::new(),
        )
        .expect("launch plan")
        .with_overlay_config(OverlayLaunchConfig::default());

        assert_eq!(
            plan.environment()
                .get(OsStr::new(REDUNAR_OVERLAY_VISIBLE_ENV)),
            Some(&OsString::from("0"))
        );
    }

    #[test]
    fn replay_transfer_configuration_is_child_only_and_closed() {
        let mut inherited = BTreeMap::new();
        inherited.insert(
            OsString::from(REDUNAR_REPLAY_TRANSFER_ENV),
            OsString::from("1"),
        );
        inherited.insert(
            OsString::from(REDUNAR_REPLAY_FRAME_RATE_ENV),
            OsString::from("999"),
        );
        let plan = CaptureLaunchPlan::new(
            "/usr/bin/game",
            [],
            "/opt/redunar/layers",
            "/run/user/1000/redunar/capture.sock",
            session_id(),
            &inherited,
        )
        .expect("launch plan");
        assert_eq!(
            plan.environment()
                .get(OsStr::new(REDUNAR_REPLAY_TRANSFER_ENV)),
            Some(&OsString::from("0"))
        );
        assert_eq!(
            plan.environment()
                .get(OsStr::new(REDUNAR_REPLAY_PRODUCTION_ENV)),
            Some(&OsString::from("0"))
        );
        assert_eq!(
            plan.environment()
                .get(OsStr::new(REDUNAR_REPLAY_FRAME_RATE_ENV)),
            Some(&OsString::from("60"))
        );

        let requested = plan.with_replay_transfer_config(ReplayTransferLaunchConfig::new(
            true,
            ReplayFrameRate::Fps30,
        ));
        assert_eq!(
            requested
                .environment()
                .get(OsStr::new(REDUNAR_REPLAY_TRANSFER_ENV)),
            Some(&OsString::from("1"))
        );
        assert_eq!(
            requested
                .environment()
                .get(OsStr::new(REDUNAR_REPLAY_PRODUCTION_ENV)),
            Some(&OsString::from("1"))
        );
        assert_eq!(
            requested
                .environment()
                .get(OsStr::new(REDUNAR_REPLAY_FRAME_RATE_ENV)),
            Some(&OsString::from("30"))
        );
        assert_eq!(
            inherited.get(OsStr::new(REDUNAR_REPLAY_TRANSFER_ENV)),
            Some(&OsString::from("1"))
        );
        assert_eq!(
            inherited.get(OsStr::new(REDUNAR_REPLAY_FRAME_RATE_ENV)),
            Some(&OsString::from("999"))
        );
    }

    #[test]
    fn opengl_capture_preload_is_child_only_and_preserves_existing_entries() {
        let mut inherited = BTreeMap::new();
        inherited.insert(
            OsString::from("LD_PRELOAD"),
            OsString::from("/opt/other/first.so:/opt/other/second.so"),
        );
        let plan = CaptureLaunchPlan::new(
            "/usr/bin/game",
            [],
            "/opt/redunar/layers",
            "/run/user/1000/redunar/capture.sock",
            session_id(),
            &inherited,
        )
        .expect("launch plan")
        .with_opengl_capture_library("/opt/redunar/libredunar_capture_opengl.so", &inherited)
        .expect("OpenGL preload");

        assert_eq!(
            plan.environment().get(OsStr::new("LD_PRELOAD")),
            Some(&OsString::from(
                "/opt/redunar/libredunar_capture_opengl.so:/opt/other/first.so:/opt/other/second.so"
            ))
        );
        assert!(
            !plan
                .environment()
                .contains_key(OsStr::new("SDL_DYNAMIC_API"))
        );
        assert_eq!(
            inherited.get(OsStr::new("LD_PRELOAD")),
            Some(&OsString::from("/opt/other/first.so:/opt/other/second.so"))
        );
    }

    #[test]
    fn overlay_telemetry_path_must_be_absolute() {
        let plan = CaptureLaunchPlan::new(
            "/usr/bin/game",
            [],
            "/opt/redunar/layers",
            "/run/user/1000/redunar/capture.sock",
            session_id(),
            &BTreeMap::new(),
        )
        .expect("launch plan");
        assert!(
            plan.with_overlay_telemetry_path("relative/telemetry.bin")
                .is_err()
        );
    }

    #[test]
    fn launch_plan_rejects_ambiguous_or_relative_configuration() {
        let empty = BTreeMap::new();
        assert!(
            CaptureLaunchPlan::new(
                "relative-game",
                [],
                "/opt/redunar/layers",
                "/run/user/1000/redunar/capture.sock",
                session_id(),
                &empty,
            )
            .is_err()
        );
        assert!(
            CaptureLaunchPlan::new(
                "/usr/bin/game",
                [],
                "/opt/redunar/layers",
                "/run/user/1000/redunar/capture.sock",
                session_id(),
                &empty,
            )
            .expect("base launch plan")
            .with_working_directory(Some(PathBuf::from("relative-directory")))
            .is_err()
        );

        let mut conflict = BTreeMap::new();
        conflict.insert(
            OsString::from("VK_LAYER_PATH"),
            OsString::from("/custom/layers"),
        );
        assert!(
            CaptureLaunchPlan::new(
                "/usr/bin/game",
                [],
                "/opt/redunar/layers",
                "/run/user/1000/redunar/capture.sock",
                session_id(),
                &conflict,
            )
            .expect_err("layer path conflict")
            .to_string()
            .contains("will not silently override")
        );
    }

    #[test]
    fn flatpak_launches_cannot_claim_host_layer_capture_support() {
        assert!(!supports_host_explicit_layer_launch(Path::new(
            "/usr/bin/flatpak"
        )));
        assert!(!supports_host_explicit_layer_launch(Path::new(
            "/usr/bin/FLATPAK"
        )));
        assert!(supports_host_explicit_layer_launch(Path::new(
            "/games/native-game"
        )));

        let error = CaptureLaunchPlan::new(
            "/usr/bin/flatpak",
            [OsString::from("run"), OsString::from("example.Game")],
            "/opt/redunar/layers",
            "/run/user/1000/redunar/capture.sock",
            session_id(),
            &BTreeMap::new(),
        )
        .expect_err("host capture paths must not be forwarded into Flatpak");

        assert!(
            error
                .to_string()
                .contains("launch without metrics or overlay")
        );
    }

    #[test]
    fn forwarded_steam_launches_report_a_typed_capture_boundary() {
        let executable = Path::new("/usr/lib/steam/bin_steam.sh");
        let arguments = [OsString::from("-applaunch"), OsString::from("1808500")];

        assert_eq!(
            game_launch_process_ownership(executable, &arguments),
            GameLaunchProcessOwnership::ForwardedSteam {
                app_id: Some(1_808_500)
            }
        );
        assert_eq!(
            capture_launch_support(executable, &arguments),
            CaptureLaunchSupport::Unavailable(CaptureLaunchUnavailableReason::SteamForwarding)
        );
        assert!(
            CaptureLaunchUnavailableReason::SteamForwarding
                .to_string()
                .contains("already running")
        );
        assert!(!supports_host_capture_launch(executable, &arguments));

        let error = CaptureLaunchPlan::new(
            executable,
            arguments,
            "/opt/redunar/layers",
            "/run/user/1000/redunar/capture.sock",
            session_id(),
            &BTreeMap::new(),
        )
        .expect_err("a forwarded command cannot carry the private environment");
        assert!(error.to_string().contains("Steam forwarding"));
        assert!(error.to_string().contains("launch without metrics"));
    }

    #[test]
    fn direct_executables_remain_capture_capable_and_child_owned() {
        let executable = Path::new("/games/native-game");
        let arguments = [OsString::from("--safe-mode")];

        assert_eq!(
            capture_launch_support(executable, &arguments),
            CaptureLaunchSupport::Supported
        );
        assert_eq!(
            game_launch_process_ownership(executable, &arguments),
            GameLaunchProcessOwnership::DirectChild
        );
    }

    #[test]
    fn manifest_is_private_session_scoped_and_points_to_exact_library() {
        let fixture = Fixture::new();
        let library = fixture.root.join("libredunar_capture_vulkan.so");
        fs::write(&library, b"fixture").expect("write fixture library");
        let session = fixture.root.join("session");
        let directory =
            prepare_vulkan_layer_directory(&session, &library).expect("prepare manifest");
        let manifest = directory.join(VULKAN_CAPTURE_MANIFEST_FILE);
        let contents = fs::read_to_string(&manifest).expect("read manifest");

        assert!(contents.contains(library.to_str().expect("utf8 fixture path")));
        assert!(contents.contains(VULKAN_CAPTURE_LAYER_NAME));
        assert!(contents.contains("\"api_version\": \"1.0.0\""));
        assert_eq!(
            fs::metadata(&directory)
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(manifest)
                .expect("manifest metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::read_dir(directory)
                .expect("read session directory")
                .count(),
            1,
            "atomic creation must not leave temporary files"
        );
    }
}
