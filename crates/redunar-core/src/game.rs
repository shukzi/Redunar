use std::ffi::OsString;
use std::path::PathBuf;

/// A read-only description of a process that carries game-runtime markers.
///
/// Detection is intentionally conservative: being a normal user process is
/// not enough for Redunar to classify an application as a game.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameProcess {
    pub pid: u32,
    pub comm: String,
    pub executable: PathBuf,
    pub steam_app_id: Option<u32>,
    /// Explicit `GameMode` runtime marker used to avoid competing CPU writers.
    pub game_mode_active: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GameId(u64);

impl GameId {
    /// Construct a daemon-assigned local game identifier.
    ///
    /// # Errors
    ///
    /// Returns [`GameIdentityError`] when the identifier is zero.
    pub fn new(value: u64) -> Result<Self, GameIdentityError> {
        if value == 0 {
            return Err(GameIdentityError);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GameMatchRule {
    SteamAppId(u32),
    Executable(PathBuf),
    ExecutableName(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameRecord {
    pub id: GameId,
    pub display_name: String,
    pub match_rules: Vec<GameMatchRule>,
    pub launch: GameLaunchConfig,
    pub profile: PerGameProfile,
}

/// A direct, launcher-agnostic process description. Arguments remain separate
/// operating-system strings so neither the daemon nor UI needs shell parsing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameLaunchConfig {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub working_directory: Option<PathBuf>,
}

/// Bounded built-in overlay layouts. Custom remains limited to Redunar's known
/// metric bitset rather than accepting arbitrary renderer configuration.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OverlayPreset {
    /// FPS, frame time, and available CPU/GPU load and temperatures.
    #[default]
    Compact,
    /// Red FPS text only, with no background panel.
    FpsOnly,
    /// Compact metrics plus 1% and 0.1% lows.
    Detailed,
    /// User-selected metrics from a bounded built-in set.
    Custom,
}

/// Bounded visual structures for the in-game metric overlay. Metric selection
/// remains independent so every layout can honor the same Custom set.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OverlayLayout {
    #[default]
    Grid,
    Ribbon,
    Telemetry,
}

/// Closed color palettes keep the injected renderer deterministic and avoid
/// accepting arbitrary color data through a game process environment.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OverlayPalette {
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

/// Bounded metric-selection bits used only by the Custom overlay preset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayMetricSet(u16);

impl OverlayMetricSet {
    pub const FPS: u16 = 1 << 0;
    pub const FRAME_TIME: u16 = 1 << 1;
    pub const ONE_PERCENT_LOW: u16 = 1 << 2;
    pub const POINT_ONE_PERCENT_LOW: u16 = 1 << 3;
    pub const CPU_LOAD: u16 = 1 << 4;
    pub const CPU_TEMPERATURE: u16 = 1 << 5;
    pub const GPU_LOAD: u16 = 1 << 6;
    pub const GPU_TEMPERATURE: u16 = 1 << 7;
    pub const KNOWN: u16 = Self::FPS
        | Self::FRAME_TIME
        | Self::ONE_PERCENT_LOW
        | Self::POINT_ONE_PERCENT_LOW
        | Self::CPU_LOAD
        | Self::CPU_TEMPERATURE
        | Self::GPU_LOAD
        | Self::GPU_TEMPERATURE;
    pub const COMPACT: Self = Self(
        Self::FPS
            | Self::FRAME_TIME
            | Self::CPU_LOAD
            | Self::CPU_TEMPERATURE
            | Self::GPU_LOAD
            | Self::GPU_TEMPERATURE,
    );
    pub const DETAILED: Self = Self(Self::KNOWN);
    pub const FPS_ONLY: Self = Self(Self::FPS);

    /// Construct a non-empty set containing only known metrics.
    ///
    /// # Errors
    ///
    /// Returns [`OverlayMetricSetError`] for zero or unknown bits.
    pub const fn from_bits(bits: u16) -> Result<Self, OverlayMetricSetError> {
        if bits != 0 && bits & !Self::KNOWN == 0 {
            Ok(Self(bits))
        } else {
            Err(OverlayMetricSetError)
        }
    }

    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    #[must_use]
    pub const fn contains(self, metric: u16) -> bool {
        self.0 & metric != 0
    }
}

impl Default for OverlayMetricSet {
    fn default() -> Self {
        Self::COMPACT
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayMetricSetError;

impl std::fmt::Display for OverlayMetricSetError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("overlay metrics must contain known non-empty selections")
    }
}

impl std::error::Error for OverlayMetricSetError {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OverlayCorner {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Background opacity percentage for panel-based overlay presets.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct OverlayOpacity(u8);

impl OverlayOpacity {
    pub const DEFAULT_PERCENT: u8 = 50;

    /// Construct a bounded percentage.
    ///
    /// # Errors
    ///
    /// Returns [`OverlayOpacityError`] when `percent` exceeds 100.
    pub const fn new(percent: u8) -> Result<Self, OverlayOpacityError> {
        if percent <= 100 {
            Ok(Self(percent))
        } else {
            Err(OverlayOpacityError)
        }
    }

    #[must_use]
    pub const fn percent(self) -> u8 {
        self.0
    }
}

impl Default for OverlayOpacity {
    fn default() -> Self {
        Self(Self::DEFAULT_PERCENT)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayOpacityError;

impl std::fmt::Display for OverlayOpacityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("overlay opacity must be between 0 and 100 percent")
    }
}

impl std::error::Error for OverlayOpacityError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayScale(u8);

impl OverlayScale {
    #[must_use]
    pub const fn new(percent: u8) -> Option<Self> {
        if percent >= 50 && percent <= 200 && percent.is_multiple_of(5) {
            Some(Self(percent))
        } else {
            None
        }
    }
    #[must_use]
    pub const fn percent(self) -> u8 {
        self.0
    }
}

impl Default for OverlayScale {
    fn default() -> Self {
        Self(100)
    }
}

/// Closed replay durations keep both encoded ring memory and save latency
/// predictable before a recording backend is enabled.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReplayDuration {
    Seconds15,
    #[default]
    Seconds30,
    Seconds60,
    Seconds120,
    Seconds180,
    Seconds300,
    Seconds600,
    Seconds900,
}

impl ReplayDuration {
    #[must_use]
    pub const fn seconds(self) -> u16 {
        match self {
            Self::Seconds15 => 15,
            Self::Seconds30 => 30,
            Self::Seconds60 => 60,
            Self::Seconds120 => 120,
            Self::Seconds180 => 180,
            Self::Seconds300 => 300,
            Self::Seconds600 => 600,
            Self::Seconds900 => 900,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReplayFrameRate {
    Fps30,
    #[default]
    Fps60,
    Fps120,
}

impl ReplayFrameRate {
    #[must_use]
    pub const fn frames_per_second(self) -> u16 {
        match self {
            Self::Fps30 => 30,
            Self::Fps60 => 60,
            Self::Fps120 => 120,
        }
    }

    /// Whether these coded dimensions are supported at this capture rate.
    ///
    /// Redunar deliberately limits 120 FPS capture to 1080p-or-lower surfaces
    /// so the high-rate option cannot be selected for 1440p or 4K games, even
    /// when an encoder could technically accept the request.
    #[must_use]
    pub const fn supports_dimensions(self, width: u32, height: u32) -> bool {
        width > 0
            && height > 0
            && (!matches!(self, Self::Fps120)
                || (width <= 1_920 && height <= 1_080)
                || (height <= 1_920 && width <= 1_080))
            && width
                .div_ceil(16)
                .saturating_mul(height.div_ceil(16))
                .saturating_mul(self.frames_per_second() as u32)
                <= 2_073_600
    }
}

/// Initial encoder targets. These are budgets, not promises that a future
/// backend will produce a constant bitrate for every game.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReplayQuality {
    Efficient,
    #[default]
    Balanced,
    High,
}

impl ReplayQuality {
    #[must_use]
    pub const fn target_megabits_per_second(self) -> u16 {
        match self {
            Self::Efficient => 12,
            Self::Balanced => 24,
            Self::High => 40,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReplayStorageLimit {
    GiB5,
    #[default]
    GiB10,
    GiB25,
    GiB50,
    Unlimited,
}

impl ReplayStorageLimit {
    #[must_use]
    pub const fn gibibytes(self) -> u8 {
        match self {
            Self::GiB5 => 5,
            Self::GiB10 => 10,
            Self::GiB25 => 25,
            Self::GiB50 => 50,
            Self::Unlimited => 0,
        }
    }

    #[must_use]
    pub const fn storage_bytes(self, bytes_per_gibibyte: u64) -> u64 {
        match self {
            Self::Unlimited => u64::MAX,
            _ => (self.gibibytes() as u64).saturating_mul(bytes_per_gibibyte),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReplaySettings {
    pub duration: ReplayDuration,
    pub frame_rate: ReplayFrameRate,
    pub quality: ReplayQuality,
    pub storage_limit: ReplayStorageLimit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "profile switches are independent user choices, not one shared state machine"
)]
pub struct GlobalGameProfile {
    pub capture_metrics: bool,
    /// Show Redunar's read-only in-game metrics presentation when its runtime
    /// is available. Frame collection remains an independent setting.
    pub overlay_visible: bool,
    pub overlay_preset: OverlayPreset,
    pub overlay_layout: OverlayLayout,
    pub overlay_palette: OverlayPalette,
    pub overlay_metrics: OverlayMetricSet,
    pub overlay_corner: OverlayCorner,
    pub overlay_opacity: OverlayOpacity,
    pub overlay_scale: OverlayScale,
    pub gamescope_enabled: bool,
    pub gamemode_enabled: bool,
    pub replay: ReplaySettings,
    pub instant_replay: bool,
}

impl Default for GlobalGameProfile {
    fn default() -> Self {
        Self {
            capture_metrics: true,
            overlay_visible: false,
            overlay_preset: OverlayPreset::default(),
            overlay_layout: OverlayLayout::default(),
            overlay_palette: OverlayPalette::default(),
            overlay_metrics: OverlayMetricSet::default(),
            overlay_corner: OverlayCorner::default(),
            overlay_opacity: OverlayOpacity::default(),
            overlay_scale: OverlayScale::default(),
            gamescope_enabled: false,
            gamemode_enabled: false,
            replay: ReplaySettings::default(),
            instant_replay: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PerGameProfile {
    pub capture_metrics: Inheritable<bool>,
    pub overlay_visible: Inheritable<bool>,
    pub instant_replay: Inheritable<bool>,
}

impl Default for PerGameProfile {
    fn default() -> Self {
        Self {
            capture_metrics: Inheritable::InheritGlobal,
            overlay_visible: Inheritable::InheritGlobal,
            instant_replay: Inheritable::InheritGlobal,
        }
    }
}

impl PerGameProfile {
    #[must_use]
    pub const fn resolve(self, global: GlobalGameProfile) -> EffectiveGameProfile {
        EffectiveGameProfile {
            capture_metrics: self.capture_metrics.resolve(global.capture_metrics),
            overlay_visible: self.overlay_visible.resolve(global.overlay_visible),
            overlay_preset: global.overlay_preset,
            overlay_layout: global.overlay_layout,
            overlay_palette: global.overlay_palette,
            overlay_metrics: global.overlay_metrics,
            overlay_corner: global.overlay_corner,
            overlay_opacity: global.overlay_opacity,
            overlay_scale: global.overlay_scale,
            gamescope_enabled: global.gamescope_enabled,
            gamemode_enabled: global.gamemode_enabled,
            replay: global.replay,
            instant_replay: self.instant_replay.resolve(global.instant_replay),
        }
    }

    #[must_use]
    pub const fn inherits_everything(self) -> bool {
        matches!(self.capture_metrics, Inheritable::InheritGlobal)
            && matches!(self.overlay_visible, Inheritable::InheritGlobal)
            && matches!(self.instant_replay, Inheritable::InheritGlobal)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "resolved profile switches remain independent runtime decisions"
)]
pub struct EffectiveGameProfile {
    pub capture_metrics: bool,
    pub overlay_visible: bool,
    pub overlay_preset: OverlayPreset,
    pub overlay_layout: OverlayLayout,
    pub overlay_palette: OverlayPalette,
    pub overlay_metrics: OverlayMetricSet,
    pub overlay_corner: OverlayCorner,
    pub overlay_opacity: OverlayOpacity,
    pub overlay_scale: OverlayScale,
    pub gamescope_enabled: bool,
    pub gamemode_enabled: bool,
    pub replay: ReplaySettings,
    pub instant_replay: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GameCatalog {
    pub global_profile: GlobalGameProfile,
    pub games: Vec<GameRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GameResolution {
    /// A high-confidence identifier or exact executable path matched exactly
    /// one local game and is safe for automatic activation.
    Automatic(GameId),
    /// Only a process/executable name matched. Show it to the user, but do not
    /// activate a session automatically because names are not unique.
    Suggested(GameId),
    /// Several records tied at the best confidence. Redunar must ask the user
    /// to resolve or merge them rather than choosing one.
    Ambiguous(Vec<GameId>),
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Inheritable<T> {
    InheritGlobal,
    Custom(T),
}

impl<T: Copy> Inheritable<T> {
    #[must_use]
    pub const fn resolve(self, global: T) -> T {
        match self {
            Self::InheritGlobal => global,
            Self::Custom(value) => value,
        }
    }
}

/// Resolve one detected process against the local catalog without consulting
/// a launcher or remote database.
#[must_use]
pub fn resolve_game(records: &[GameRecord], process: &GameProcess) -> GameResolution {
    let mut best_score = 0;
    let mut best = Vec::new();
    for record in records {
        let score = record
            .match_rules
            .iter()
            .map(|rule| match_score(rule, process))
            .max()
            .unwrap_or(0);
        if score > best_score {
            best_score = score;
            best.clear();
            best.push(record.id);
        } else if score > 0 && score == best_score {
            best.push(record.id);
        }
    }

    match (best_score, best.as_slice()) {
        (0, _) => GameResolution::Unknown,
        (_, [game]) if best_score >= 80 => GameResolution::Automatic(*game),
        (_, [game]) => GameResolution::Suggested(*game),
        (_, games) => GameResolution::Ambiguous(games.to_vec()),
    }
}

fn match_score(rule: &GameMatchRule, process: &GameProcess) -> u8 {
    match rule {
        GameMatchRule::SteamAppId(app_id) if process.steam_app_id == Some(*app_id) => 100,
        GameMatchRule::Executable(path) if process.executable == *path => 90,
        GameMatchRule::ExecutableName(name)
            if process.comm == *name
                || process
                    .executable
                    .file_name()
                    .is_some_and(|value| value == std::ffi::OsStr::new(name)) =>
        {
            50
        }
        _ => 0,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GameIdentityError;

impl std::fmt::Display for GameIdentityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("game id cannot be zero")
    }
}

impl std::error::Error for GameIdentityError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: u64) -> GameId {
        GameId::new(value).expect("game id")
    }

    fn process(steam_app_id: Option<u32>) -> GameProcess {
        GameProcess {
            pid: 42,
            comm: "game-bin".into(),
            executable: "/games/example/game-bin".into(),
            steam_app_id,
            game_mode_active: false,
        }
    }

    fn record(id_value: u64, display_name: &str, match_rules: Vec<GameMatchRule>) -> GameRecord {
        GameRecord {
            id: id(id_value),
            display_name: display_name.into(),
            match_rules,
            launch: GameLaunchConfig {
                executable: "/games/example/game-bin".into(),
                arguments: Vec::new(),
                working_directory: Some("/games/example".into()),
            },
            profile: PerGameProfile::default(),
        }
    }

    #[test]
    fn exact_identifiers_activate_without_launcher_coupling() {
        let records = [record(
            1,
            "Example Game",
            vec![
                GameMatchRule::SteamAppId(123),
                GameMatchRule::Executable("/games/example/game-bin".into()),
            ],
        )];
        assert_eq!(
            resolve_game(&records, &process(Some(123))),
            GameResolution::Automatic(id(1))
        );
        assert_eq!(
            resolve_game(&records, &process(None)),
            GameResolution::Automatic(id(1))
        );
    }

    #[test]
    fn name_only_matches_are_suggestions_and_ties_are_ambiguous() {
        let records = [
            record(
                1,
                "One",
                vec![GameMatchRule::ExecutableName("game-bin".into())],
            ),
            record(
                2,
                "Two",
                vec![GameMatchRule::ExecutableName("other".into())],
            ),
        ];
        assert_eq!(
            resolve_game(&records, &process(None)),
            GameResolution::Suggested(id(1))
        );

        let tied = [
            records[0].clone(),
            record(
                3,
                "Three",
                vec![GameMatchRule::ExecutableName("game-bin".into())],
            ),
        ];
        assert_eq!(
            resolve_game(&tied, &process(None)),
            GameResolution::Ambiguous(vec![id(1), id(3)])
        );
    }

    #[test]
    fn strongest_identity_wins_and_exact_ties_never_choose_for_the_user() {
        let records = [
            record(
                1,
                "Filename only",
                vec![GameMatchRule::ExecutableName("game-bin".into())],
            ),
            record(
                2,
                "Exact executable",
                vec![GameMatchRule::Executable("/games/example/game-bin".into())],
            ),
            record(3, "Steam identity", vec![GameMatchRule::SteamAppId(123)]),
        ];

        assert_eq!(
            resolve_game(&records, &process(Some(123))),
            GameResolution::Automatic(id(3))
        );
        assert_eq!(
            resolve_game(&records, &process(None)),
            GameResolution::Automatic(id(2))
        );

        let exact_tie = [
            records[1].clone(),
            record(
                4,
                "Duplicate exact evidence",
                vec![GameMatchRule::Executable("/games/example/game-bin".into())],
            ),
        ];
        assert_eq!(
            resolve_game(&exact_tie, &process(None)),
            GameResolution::Ambiguous(vec![id(2), id(4)])
        );
    }

    #[test]
    fn per_game_values_explicitly_inherit_or_override_global_values() {
        assert!(Inheritable::InheritGlobal.resolve(true));
        assert!(!Inheritable::Custom(false).resolve(true));
        assert!(GameId::new(0).is_err());

        let global = GlobalGameProfile {
            capture_metrics: true,
            overlay_visible: true,
            instant_replay: false,
            ..GlobalGameProfile::default()
        };
        let inherited = PerGameProfile::default();
        assert!(inherited.inherits_everything());
        assert_eq!(
            inherited.resolve(global),
            EffectiveGameProfile {
                capture_metrics: true,
                overlay_visible: true,
                overlay_preset: OverlayPreset::Compact,
                overlay_layout: OverlayLayout::Grid,
                overlay_palette: OverlayPalette::Redunar,
                overlay_metrics: OverlayMetricSet::COMPACT,
                overlay_corner: OverlayCorner::TopLeft,
                overlay_opacity: OverlayOpacity::default(),
                overlay_scale: OverlayScale::default(),
                gamescope_enabled: false,
                gamemode_enabled: false,
                replay: ReplaySettings::default(),
                instant_replay: false,
            }
        );

        let custom = PerGameProfile {
            capture_metrics: Inheritable::Custom(false),
            overlay_visible: Inheritable::Custom(false),
            instant_replay: Inheritable::InheritGlobal,
        };
        assert!(!custom.inherits_everything());
        assert_eq!(
            custom.resolve(global),
            EffectiveGameProfile {
                capture_metrics: false,
                overlay_visible: false,
                overlay_preset: OverlayPreset::Compact,
                overlay_layout: OverlayLayout::Grid,
                overlay_palette: OverlayPalette::Redunar,
                overlay_metrics: OverlayMetricSet::COMPACT,
                overlay_corner: OverlayCorner::TopLeft,
                overlay_opacity: OverlayOpacity::default(),
                overlay_scale: OverlayScale::default(),
                gamescope_enabled: false,
                gamemode_enabled: false,
                replay: ReplaySettings::default(),
                instant_replay: false,
            }
        );
    }

    #[test]
    fn replay_120_fps_is_limited_to_1080p() {
        assert!(ReplayFrameRate::Fps120.supports_dimensions(1_920, 1_080));
        assert!(ReplayFrameRate::Fps120.supports_dimensions(1_080, 1_920));
        assert!(!ReplayFrameRate::Fps120.supports_dimensions(2_560, 1_440));
        assert!(!ReplayFrameRate::Fps120.supports_dimensions(2_560, 720));
        assert!(!ReplayFrameRate::Fps120.supports_dimensions(3_840, 2_160));
        assert!(ReplayFrameRate::Fps60.supports_dimensions(3_840, 2_160));
    }

    #[test]
    fn overlay_customization_defaults_are_compact_and_bounded() {
        let profile = GlobalGameProfile::default();
        assert_eq!(profile.overlay_preset, OverlayPreset::Compact);
        assert_eq!(profile.overlay_corner, OverlayCorner::TopLeft);
        assert_eq!(profile.overlay_opacity.percent(), 50);
        assert_eq!(profile.overlay_metrics, OverlayMetricSet::COMPACT);
        assert_eq!(profile.replay, ReplaySettings::default());
        assert_eq!(profile.replay.duration.seconds(), 30);
        assert_eq!(profile.replay.frame_rate.frames_per_second(), 60);
        assert_eq!(profile.replay.quality.target_megabits_per_second(), 24);
        assert_eq!(profile.replay.storage_limit.gibibytes(), 10);
        assert_eq!(OverlayOpacity::new(0).map(OverlayOpacity::percent), Ok(0));
        assert_eq!(
            OverlayOpacity::new(100).map(OverlayOpacity::percent),
            Ok(100)
        );
        assert!(OverlayOpacity::new(101).is_err());
        assert_eq!(OverlayScale::new(50).map(OverlayScale::percent), Some(50));
        assert_eq!(OverlayScale::new(135).map(OverlayScale::percent), Some(135));
        assert_eq!(OverlayScale::new(200).map(OverlayScale::percent), Some(200));
        assert_eq!(OverlayScale::new(49), None);
        assert_eq!(OverlayScale::new(136), None);
        assert_eq!(OverlayScale::new(201), None);
        assert!(OverlayMetricSet::COMPACT.contains(OverlayMetricSet::FPS));
        assert!(!OverlayMetricSet::COMPACT.contains(OverlayMetricSet::ONE_PERCENT_LOW));
        assert_eq!(
            OverlayMetricSet::from_bits(OverlayMetricSet::FPS | OverlayMetricSet::GPU_LOAD)
                .map(OverlayMetricSet::bits),
            Ok(OverlayMetricSet::FPS | OverlayMetricSet::GPU_LOAD)
        );
        assert!(OverlayMetricSet::from_bits(0).is_err());
        assert!(OverlayMetricSet::from_bits(1 << 15).is_err());
    }
}
