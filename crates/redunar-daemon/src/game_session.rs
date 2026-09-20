use crate::capture_session::ReplayExportEndpoint;
use crate::replay_menu::ReplayMenuState;
use crate::{
    CaptureLaunchDisposition, CaptureLaunchProcessState, CapturePhase, CaptureSessionError,
    CaptureSessionHandle, DmaBufReplayFrame, HardwareEncoderBackend, OverlayRuntimeStatus,
    ProductionReplayRuntime, ReplayBackendReadiness, ReplayFailure, ReplayHardwarePipeline,
    ReplayPhase, ReplayRuntimeError, ReplayRuntimeStatus, SteamActivationState,
};
use redunar_core::{GameId, ReplayDuration, ReplaySettings, SystemSnapshot};
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

mod saved_notice;

const REPLAY_EXPORT_WAIT: Duration = Duration::from_millis(250);
/// First wait before the pump re-arms a replacement recorder after a
/// recoverable failure. Each consecutive failed re-arm doubles the wait up
/// to the maximum so a persistently broken encoder retries slowly instead of
/// spinning or ending the game session's replay for good.
const REPLAY_REARM_BACKOFF: Duration = Duration::from_millis(500);
const REPLAY_REARM_BACKOFF_MAX: Duration = Duration::from_secs(10);
/// Consecutive frame-source failures tolerated before the running recorder
/// is failed over so the reported health reflects the missing source.
const REPLAY_SOURCE_ERROR_LIMIT: u32 = 10;

/// One bounded, daemon-owned view of all work attached to a running game.
///
/// Optional capture and overlay failures are deliberately component-local. A
/// failed optional component never changes the game lifetime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameSessionComponentState {
    Disabled,
    Unavailable,
    Pending,
    Active,
    Stopped,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameSessionPhase {
    Idle,
    Launching,
    Running,
    Ending,
    Ended,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GameSessionFeatureRequest {
    Disabled,
    Enabled,
}

impl GameSessionFeatureRequest {
    const fn is_enabled(&self) -> bool {
        matches!(self, Self::Enabled)
    }
}

impl From<bool> for GameSessionFeatureRequest {
    fn from(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameSessionRequest {
    pub game_id: Option<GameId>,
    pub game_name: String,
    pub metrics: GameSessionFeatureRequest,
    pub overlay: GameSessionFeatureRequest,
    pub replay: GameSessionFeatureRequest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameSessionStatus {
    pub revision: u64,
    pub phase: GameSessionPhase,
    pub game_id: Option<GameId>,
    pub game_name: Option<String>,
    pub metrics: GameSessionComponentState,
    pub overlay: GameSessionComponentState,
    pub replay: GameSessionComponentState,
    pub failure: Option<String>,
}

impl Default for GameSessionStatus {
    fn default() -> Self {
        Self {
            revision: 0,
            phase: GameSessionPhase::Idle,
            game_id: None,
            game_name: None,
            metrics: GameSessionComponentState::Disabled,
            overlay: GameSessionComponentState::Disabled,
            replay: GameSessionComponentState::Disabled,
            failure: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameSessionError {
    message: String,
}

impl GameSessionError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for GameSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for GameSessionError {}

/// Shared production coordinator cached by [`crate::RedunarService`].
/// Native windows share this owner so view lifetimes cannot discard capture or replay state.
#[derive(Clone)]
pub struct ProductionGameSessionCoordinator {
    inner: Arc<CoordinatorInner>,
}

struct CoordinatorInner {
    state: Mutex<CoordinatorState>,
    replay: ProductionReplayRuntime,
    replay_menu: Mutex<ReplayMenuState>,
    replay_menu_revision: std::sync::atomic::AtomicU64,
    /// Private state directory holding the clip-to-game attribution ledger.
    clip_games_state: PathBuf,
}

#[derive(Default)]
struct CoordinatorState {
    status: GameSessionStatus,
    request: Option<GameSessionRequest>,
    capture: Option<CaptureSessionHandle>,
    noticed_save_revision: u64,
    /// Completed-save revision last attributed in the clip-games ledger.
    recorded_save_revision: u64,
    pending_replay_releases: VecDeque<u64>,
    replay_pump: Option<ReplayExportPump>,
}

struct ReplayExportPump {
    stop: Arc<AtomicBool>,
    source: Arc<dyn ReplayFrameSource>,
    worker: Option<JoinHandle<Result<(), ReplayRuntimeError>>>,
}

pub(crate) trait ReplayFrameSource: Send + Sync {
    fn wait_next(
        &self,
        timeout: Duration,
    ) -> Result<Option<(u64, DmaBufReplayFrame)>, ReplayRuntimeError>;
    fn release(&self, sequence: u64) -> Result<(), ReplayRuntimeError>;
    fn drain_and_release(&self);
    fn wake(&self);
}

impl ReplayFrameSource for ReplayExportEndpoint {
    fn wait_next(
        &self,
        timeout: Duration,
    ) -> Result<Option<(u64, DmaBufReplayFrame)>, ReplayRuntimeError> {
        let Some(export) = ReplayExportEndpoint::wait_next(self, timeout) else {
            return Ok(None);
        };
        let sequence = export.sequence;
        match ReplayExportEndpoint::import(self, export) {
            Ok(frame) => Ok(Some(frame)),
            Err(error) => {
                // The queue already removed this export, so the pump cannot
                // release it. Return producer ownership here; otherwise a
                // rejected export would strand its staging context forever
                // and silently stall capture after enough rejects.
                let _ = ReplayExportEndpoint::release(self, sequence);
                Err(ReplayRuntimeError::new(error.to_string()))
            }
        }
    }

    fn release(&self, sequence: u64) -> Result<(), ReplayRuntimeError> {
        ReplayExportEndpoint::release(self, sequence)
            .map_err(|error| ReplayRuntimeError::new(error.to_string()))
    }

    fn drain_and_release(&self) {
        ReplayExportEndpoint::drain_and_release(self);
    }

    fn wake(&self) {
        ReplayExportEndpoint::wake(self);
    }
}

type ReplayPipelineFactory =
    Arc<dyn Fn(&DmaBufReplayFrame) -> Result<ReplayHardwarePipeline, String> + Send + Sync>;
type ReplayBackendFactory = Arc<
    dyn Fn(&DmaBufReplayFrame) -> Result<Box<dyn HardwareEncoderBackend>, String> + Send + Sync,
>;

impl ReplayExportPump {
    fn stop_and_join(&mut self) -> Result<(), ReplayRuntimeError> {
        self.stop.store(true, Ordering::Release);
        self.source.wake();
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        worker
            .join()
            .map_err(|_| ReplayRuntimeError::new("Replay export worker panicked"))?
    }
}

impl Drop for ReplayExportPump {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

impl ProductionGameSessionCoordinator {
    #[must_use]
    pub(crate) fn new(state_directory: PathBuf) -> Self {
        Self {
            inner: Arc::new(CoordinatorInner {
                state: Mutex::new(CoordinatorState::default()),
                replay: ProductionReplayRuntime::unavailable(ReplaySettings::default()),
                replay_menu: Mutex::new(ReplayMenuState::default()),
                replay_menu_revision: std::sync::atomic::AtomicU64::new(2),
                clip_games_state: state_directory,
            }),
        }
    }

    #[must_use]
    pub fn status(&self) -> GameSessionStatus {
        lock_unpoisoned(&self.inner.state).status.clone()
    }

    #[must_use]
    pub fn replay_runtime(&self) -> ProductionReplayRuntime {
        self.inner.replay.clone()
    }

    /// Hand a duration-selected save to the daemon-owned Replay runtime.
    ///
    /// # Errors
    ///
    /// Returns an error when Replay is unavailable, inactive, failed, or
    /// already assembling another clip.
    pub fn save_replay(&self, duration: ReplayDuration) -> Result<(), ReplayRuntimeError> {
        self.inner.replay.save(duration)
    }

    pub(crate) fn toggle_replay_menu(&self, now: Instant) -> Result<bool, ReplayRuntimeError> {
        let can_open = {
            let state = lock_unpoisoned(&self.inner.state);
            replay_menu_has_render_target(&state)
        };
        let currently_visible = lock_unpoisoned(&self.inner.replay_menu).is_visible();
        if !currently_visible && !can_open {
            return Err(ReplayRuntimeError::new(
                "Replay menu requires a connected game capture",
            ));
        }
        let visible = lock_unpoisoned(&self.inner.replay_menu).toggle(now);
        self.publish_replay_menu();
        Ok(visible)
    }

    /// Close the Replay menu and publish the corresponding daemon state.
    ///
    /// Native shells call this when their own menu surface is hidden so the
    /// backend cannot retain a stale visible/pressed state between toggles.
    pub fn close_replay_menu(&self) {
        lock_unpoisoned(&self.inner.replay_menu).close();
        self.publish_replay_menu();
    }

    pub(crate) fn move_replay_menu_cursor(&self, dx: i32, dy: i32, now: Instant) {
        lock_unpoisoned(&self.inner.replay_menu).move_cursor(dx, dy, now);
        self.publish_replay_menu();
    }

    pub(crate) fn replay_menu_button(
        &self,
        pressed: bool,
        now: Instant,
    ) -> Result<bool, ReplayRuntimeError> {
        let runtime = self.inner.replay.status();
        let ready = runtime.phase == ReplayPhase::Buffering
            && runtime.buffered_duration_ns >= 1_000_000_000;
        let (save, closed) = {
            let mut menu = lock_unpoisoned(&self.inner.replay_menu);
            let save = menu.button(pressed, ready, now);
            (save, !menu.is_visible())
        };
        if let Some(duration) = save {
            self.save_replay(duration)?;
        }
        self.publish_replay_menu();
        Ok(closed)
    }

    pub(crate) fn replay_menu_heartbeat(&self, now: Instant) {
        lock_unpoisoned(&self.inner.replay_menu).heartbeat(now);
        self.publish_replay_menu();
    }

    pub(crate) fn replay_menu_watchdog(&self, now: Instant) -> bool {
        let closed = lock_unpoisoned(&self.inner.replay_menu).close_if_stale(now);
        if closed {
            self.publish_replay_menu();
        }
        closed
    }

    #[must_use]
    pub(crate) fn replay_menu_telemetry(
        &self,
        revision: u64,
    ) -> redunar_capture::ReplayMenuTelemetry {
        lock_unpoisoned(&self.inner.replay_menu).telemetry(
            revision,
            self.inner.replay.status(),
            self.inner.replay.output_format(),
        )
    }

    fn publish_replay_menu(&self) {
        let revision = self
            .inner
            .replay_menu_revision
            .fetch_add(2, Ordering::Relaxed)
            .saturating_add(2);
        let telemetry = self.replay_menu_telemetry(revision);
        if let Some(capture) = lock_unpoisoned(&self.inner.state).capture.as_ref() {
            let _ = capture.update_replay_menu(telemetry);
        }
    }

    /// Attribute clips committed since the last poll to the live game.
    ///
    /// The save worker queues each committed clip name under the same mutex
    /// as the completed-save revision bump, so draining names here cannot
    /// race the counter. Attribution is labeling data: any ledger write
    /// failure is swallowed after one warning and never affects the save or
    /// the game session. If two saves complete between polls, both names were
    /// queued, so each clip still gets its own line.
    fn record_completed_clip_games(&self) {
        let completed = self.inner.replay.status().completed_save_revision;
        let game_name = {
            let mut state = lock_unpoisoned(&self.inner.state);
            if state.recorded_save_revision >= completed {
                return;
            }
            state.recorded_save_revision = completed;
            state.status.game_name.clone()
        };
        let Some(game_name) = game_name else {
            // A save outside a named Redunar session stays unattributed in
            // the ledger; the inventory's session-window fallback still gets
            // a chance when the session history later records a game.
            let _ = self.inner.replay.take_committed_clip_names();
            return;
        };
        for name in self.inner.replay.take_committed_clip_names() {
            if let Err(error) =
                crate::replay_clip_games::record(&self.inner.clip_games_state, &name, &game_name)
            {
                eprintln!("Redunar could not attribute a saved Replay clip: {error}");
            }
        }
    }

    /// Forward a completed-save presentation edge to the active capture
    /// overlay. Replay storage remains authoritative; absence of an overlay
    /// never changes the save result.
    ///
    /// # Errors
    ///
    /// Returns an error if no capture runtime is active or its telemetry file
    /// cannot be updated.
    pub fn publish_replay_saved_notice(&self) -> Result<(), ReplayRuntimeError> {
        let completed = self.inner.replay.status().completed_save_revision;
        let mut state = lock_unpoisoned(&self.inner.state);
        let CoordinatorState {
            capture,
            noticed_save_revision,
            ..
        } = &mut *state;
        saved_notice::publish_completed(completed, noticed_save_revision, || {
            capture
                .as_ref()
                .ok_or_else(|| ReplayRuntimeError::new("the game session has no capture runtime"))?
                .publish_replay_saved_notice()
                .map_err(|error| ReplayRuntimeError::new(error.to_string()))
        })
    }

    /// Start the daemon-owned export consumer independently from desktop UI
    /// polling. The factories are injected by the service so this coordinator
    /// does not own storage paths or Vulkan device selection.
    ///
    /// The pipeline is created lazily from the first exported frame because a
    /// game's actual swapchain dimensions are not known at launch time.
    ///
    /// # Errors
    ///
    /// Returns an error when validation does not authorize activation, the
    /// session no longer requests Replay, capture is absent, another pump is
    /// active, or the worker thread cannot be created safely.
    pub fn start_replay_export_pump<P, B>(
        &self,
        settings: ReplaySettings,
        readiness: ReplayBackendReadiness,
        pipeline_factory: P,
        backend_factory: B,
    ) -> Result<(), ReplayRuntimeError>
    where
        P: Fn(&DmaBufReplayFrame) -> Result<ReplayHardwarePipeline, String> + Send + Sync + 'static,
        B: Fn(&DmaBufReplayFrame) -> Result<Box<dyn HardwareEncoderBackend>, String>
            + Send
            + Sync
            + 'static,
    {
        crate::replay_runtime::validate_production_readiness(readiness)?;
        let endpoint = {
            let state = lock_unpoisoned(&self.inner.state);
            validate_replay_session_state(&state)?;
            if state.replay_pump.is_some() {
                return Err(ReplayRuntimeError::new(
                    "an Instant Replay export worker is already active",
                ));
            }
            state
                .capture
                .as_ref()
                .ok_or_else(|| {
                    ReplayRuntimeError::new(
                        "Instant Replay requires the game frame-transfer runtime",
                    )
                })?
                .replay_export_endpoint()
        };
        self.start_replay_frame_pump(
            settings,
            readiness,
            Arc::new(endpoint),
            pipeline_factory,
            backend_factory,
        )
    }

    pub(crate) fn start_replay_frame_pump<P, B>(
        &self,
        settings: ReplaySettings,
        readiness: ReplayBackendReadiness,
        source: Arc<dyn ReplayFrameSource>,
        pipeline_factory: P,
        backend_factory: B,
    ) -> Result<(), ReplayRuntimeError>
    where
        P: Fn(&DmaBufReplayFrame) -> Result<ReplayHardwarePipeline, String> + Send + Sync + 'static,
        B: Fn(&DmaBufReplayFrame) -> Result<Box<dyn HardwareEncoderBackend>, String>
            + Send
            + Sync
            + 'static,
    {
        crate::replay_runtime::validate_production_readiness(readiness)?;
        {
            let state = lock_unpoisoned(&self.inner.state);
            validate_replay_session_state(&state)?;
            if state.replay_pump.is_some() {
                return Err(ReplayRuntimeError::new(
                    "an Instant Replay export worker is already active",
                ));
            }
        }
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_source = Arc::clone(&source);
        let weak_inner = Arc::downgrade(&self.inner);
        let replay = self.inner.replay.clone();
        let pipeline_factory = Arc::new(pipeline_factory);
        let backend_factory = Arc::new(backend_factory);
        let worker = thread::Builder::new()
            .name("redunar-replay-export".to_owned())
            .spawn(move || {
                run_replay_export_pump(
                    worker_stop,
                    worker_source,
                    replay,
                    weak_inner,
                    settings,
                    readiness,
                    pipeline_factory,
                    backend_factory,
                )
            })
            .map_err(|error| {
                ReplayRuntimeError::new(format!("could not start Replay export worker: {error}"))
            })?;
        let mut state = lock_unpoisoned(&self.inner.state);
        // The session may have ended while the OS created the thread. Refuse
        // attachment and synchronously stop the worker in that rare race.
        if validate_replay_session_state(&state).is_err() || state.replay_pump.is_some() {
            stop.store(true, Ordering::Release);
            source.wake();
            drop(state);
            let _ = worker.join();
            return Err(ReplayRuntimeError::new(
                "the game session ended before Replay could start",
            ));
        }
        state.replay_pump = Some(ReplayExportPump {
            stop,
            source,
            worker: Some(worker),
        });
        Ok(())
    }

    /// Import one validated exported Vulkan frame and submit it to the
    /// daemon-owned hardware pipeline.
    ///
    /// # Errors
    ///
    /// Returns an error when no capture export matches `sequence`, the
    /// producer FD cannot be duplicated, or the encoder rejects the frame.
    pub fn submit_replay_export(&self, sequence: u64) -> Result<(), ReplayRuntimeError> {
        let frame = lock_unpoisoned(&self.inner.state)
            .capture
            .as_ref()
            .ok_or_else(|| ReplayRuntimeError::new("the game session has no capture runtime"))?
            .import_replay_export(sequence)
            .map_err(|error| ReplayRuntimeError::new(error.to_string()))?;
        let result = self.inner.replay.submit_frame(sequence, frame);
        let mut state = lock_unpoisoned(&self.inner.state);
        collect_and_release_replay_exports(&mut state, &self.inner.replay);
        result
    }

    /// Publish a launch only after the shell-free child spawn succeeded.
    /// Replacing a live session is refused so two UI windows cannot acquire
    /// unrelated feature lifecycles for different games.
    ///
    /// # Errors
    ///
    /// Returns [`GameSessionError`] when another game session is live.
    pub fn begin(
        &self,
        request: GameSessionRequest,
        capture: Option<CaptureSessionHandle>,
        replay: ReplayRuntimeStatus,
    ) -> Result<(), GameSessionError> {
        // Flush any clip that committed after the end-of-session attribution
        // pass (a save finishing during shutdown) while the previous game
        // name is still published, so it cannot be re-attributed to the
        // session that is about to start.
        self.record_completed_clip_games();
        if replay.backend_readiness.validation_allowed() {
            self.inner
                .replay
                .configure_validation_candidate(replay.settings);
        } else {
            self.inner.replay.configure_unavailable(replay.settings);
        }
        let replay_status = self.inner.replay.status();
        let mut state = lock_unpoisoned(&self.inner.state);
        if !matches!(
            state.status.phase,
            GameSessionPhase::Idle | GameSessionPhase::Ended
        ) {
            return Err(GameSessionError::new(
                "another daemon-owned game session is already active",
            ));
        }
        let capture_attached = capture.is_some();
        state.request = Some(request.clone());
        state.capture = capture;
        state.noticed_save_revision = replay_status.completed_save_revision;
        state.recorded_save_revision = replay_status.completed_save_revision;
        state.pending_replay_releases.clear();
        state.status = GameSessionStatus {
            revision: state.status.revision.saturating_add(1),
            phase: GameSessionPhase::Launching,
            game_id: request.game_id,
            game_name: Some(request.game_name),
            metrics: requested_state(request.metrics.is_enabled(), capture_attached),
            overlay: requested_state(request.overlay.is_enabled(), capture_attached),
            replay: replay_component(replay_status, request.replay.is_enabled()),
            failure: None,
        };
        Ok(())
    }

    /// Mark a shell-free launch as running even when no optional capture
    /// producer was requested.
    pub fn mark_running(&self) {
        let mut state = lock_unpoisoned(&self.inner.state);
        if state.status.phase == GameSessionPhase::Launching {
            state.status.phase = GameSessionPhase::Running;
            bump(&mut state.status);
        }
    }

    #[must_use]
    pub fn capture_snapshot(&self) -> Option<Arc<crate::CaptureSnapshot>> {
        let mut state = lock_unpoisoned(&self.inner.state);
        let snapshot = state.capture.as_ref().map(CaptureSessionHandle::snapshot)?;
        apply_capture_snapshot(&mut state, &snapshot);
        Some(snapshot)
    }

    #[must_use]
    pub fn steam_activation_state(&self) -> Option<SteamActivationState> {
        lock_unpoisoned(&self.inner.state)
            .capture
            .as_ref()
            .and_then(CaptureSessionHandle::steam_activation_state)
    }

    /// # Errors
    ///
    /// Returns [`CaptureSessionError`] when no capture is attached or the
    /// bounded overlay telemetry update fails.
    pub fn update_overlay_hardware(
        &self,
        snapshot: Option<&SystemSnapshot>,
    ) -> Result<(), CaptureSessionError> {
        lock_unpoisoned(&self.inner.state)
            .capture
            .as_ref()
            .ok_or_else(|| {
                CaptureSessionError::owned("the game session has no capture runtime".to_owned())
            })?
            .update_overlay_hardware(snapshot)
    }

    /// Update the active overlay appearance without restarting the game.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureSessionError`] when no capture is attached or the update fails.
    pub fn update_overlay_config(
        &self,
        profile: &redunar_core::EffectiveGameProfile,
    ) -> Result<(), CaptureSessionError> {
        let mut state = lock_unpoisoned(&self.inner.state);
        if !matches!(
            state.status.phase,
            GameSessionPhase::Launching | GameSessionPhase::Running
        ) {
            return Err(CaptureSessionError::owned(
                "the game session has ended".to_owned(),
            ));
        }
        let capture = state.capture.as_ref().ok_or_else(|| {
            CaptureSessionError::owned("the game session has no capture runtime".to_owned())
        })?;
        if matches!(
            capture.snapshot().phase,
            CapturePhase::Completed | CapturePhase::Failed
        ) {
            return Err(CaptureSessionError::owned(
                "the game's overlay runtime is no longer available".to_owned(),
            ));
        }
        capture.update_overlay_config(profile)?;
        let snapshot = capture.snapshot();
        if let Some(request) = state.request.as_mut() {
            request.overlay = profile.overlay_visible.into();
        }
        apply_capture_snapshot(&mut state, &snapshot);
        Ok(())
    }

    #[must_use]
    pub fn observe_launch_process(
        &self,
        process: &CaptureLaunchProcessState,
    ) -> Option<CaptureLaunchDisposition> {
        // The launch supervisor runs even while the desktop window is hidden.
        // Observe only committed saves; accepted requests and failures do not
        // emit success, and a telemetry write failure is retried next poll.
        let _ = self.publish_replay_saved_notice();
        self.record_completed_clip_games();
        let mut state = lock_unpoisoned(&self.inner.state);
        let disposition = state
            .capture
            .as_mut()
            .map(|capture| capture.observe_launch_process(process));
        if let Some(capture) = state.capture.as_ref() {
            let snapshot = capture.snapshot();
            apply_capture_snapshot(&mut state, &snapshot);
        }
        disposition
    }

    /// Release only the fail-open capture/overlay transport while the game
    /// itself remains active (for example, a forwarded Steam launch whose
    /// capture activation expired).
    pub fn stop_capture(&self) {
        let mut replay_pump = lock_unpoisoned(&self.inner.state).replay_pump.take();
        if let Some(pump) = replay_pump.as_mut() {
            let _ = pump.stop_and_join();
        } else {
            let _ = self.inner.replay.shutdown();
        }
        let capture = {
            let mut state = lock_unpoisoned(&self.inner.state);
            collect_and_release_replay_exports(&mut state, &self.inner.replay);
            state.status.metrics = stop_optional(state.status.metrics);
            state.status.overlay = stop_optional(state.status.overlay);
            state.status.replay = stop_optional(state.status.replay);
            bump(&mut state.status);
            state.capture.take()
        };
        drop(capture);
    }

    /// Stop all optional session resources and publish a terminal state.
    ///
    /// # Errors
    ///
    /// Returns [`GameSessionError`] when Replay cleanup cannot complete.
    pub fn end(&self) -> Result<(), GameSessionError> {
        // One final attribution pass before the session stops so a save that
        // completed since the last supervisor poll is still labeled with this
        // game. Later drains fall back to session-history window matching.
        self.record_completed_clip_games();
        let mut replay_pump = {
            let mut state = lock_unpoisoned(&self.inner.state);
            if matches!(
                state.status.phase,
                GameSessionPhase::Idle | GameSessionPhase::Ended
            ) {
                return Ok(());
            }
            state.status.phase = GameSessionPhase::Ending;
            state.status.metrics = stop_optional(state.status.metrics);
            state.status.overlay = stop_optional(state.status.overlay);
            state.status.replay = stop_optional(state.status.replay);
            bump(&mut state.status);
            state.replay_pump.take()
        };

        let replay_cleanup = replay_pump.as_mut().map_or_else(
            || self.inner.replay.shutdown(),
            ReplayExportPump::stop_and_join,
        );
        let capture = {
            let mut state = lock_unpoisoned(&self.inner.state);
            collect_and_release_replay_exports(&mut state, &self.inner.replay);
            state.capture.take()
        };
        drop(capture);

        let mut state = lock_unpoisoned(&self.inner.state);
        state.status.phase = GameSessionPhase::Ended;
        match replay_cleanup {
            Ok(()) => {
                state.status.failure = None;
                bump(&mut state.status);
                Ok(())
            }
            Err(error) => {
                state.status.failure =
                    Some(format!("Game-session cleanup did not complete: {error}"));
                bump(&mut state.status);
                Err(GameSessionError::new(error.to_string()))
            }
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the worker keeps each injected ownership boundary explicit"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "the worker thread owns these handles for its complete lifetime"
)]
#[allow(
    clippy::too_many_lines,
    reason = "the source-independent worker keeps video, optional audio, completion, and shutdown ordering explicit"
)]
fn run_replay_export_pump(
    stop: Arc<AtomicBool>,
    source: Arc<dyn ReplayFrameSource>,
    replay: ProductionReplayRuntime,
    coordinator: Weak<CoordinatorInner>,
    settings: ReplaySettings,
    readiness: ReplayBackendReadiness,
    pipeline_factory: ReplayPipelineFactory,
    backend_factory: ReplayBackendFactory,
) -> Result<(), ReplayRuntimeError> {
    let mut dimensions = None;
    // One recording failure is recoverable: the pump reports the failure,
    // releases producer ownership of every in-flight export, and then re-arms
    // a fresh validated pipeline after a bounded backoff. Failures therefore
    // cost a short window of disk history instead of the whole game session.
    let mut rearm_deadline: Option<Instant> = None;
    let mut rearm_backoff = REPLAY_REARM_BACKOFF;
    let mut consecutive_source_errors = 0_u32;
    let mut audio_worker = None;
    while !stop.load(Ordering::Acquire) {
        let frame = match source.wait_next(REPLAY_EXPORT_WAIT) {
            Ok(Some(frame)) => frame,
            Ok(None) => continue,
            Err(error) => {
                consecutive_source_errors = consecutive_source_errors.saturating_add(1);
                eprintln!(
                    "Redunar Replay: frame source failed ({consecutive_source_errors} consecutive): {error}"
                );
                // A single stale or malformed export must not end recording;
                // a running recorder keeps its last frames and waits for the
                // next good one. Only a sustained source outage fails the
                // recorder over so the reported health reflects reality.
                let limit = if dimensions.is_some() {
                    REPLAY_SOURCE_ERROR_LIMIT
                } else {
                    1
                };
                if rearm_deadline.is_none() && consecutive_source_errors >= limit {
                    enter_replay_recovery(
                        &replay,
                        &coordinator,
                        ReplayFailure::FrameSourceLost,
                        &mut dimensions,
                        &mut rearm_deadline,
                        &mut rearm_backoff,
                    );
                }
                continue;
            }
        };
        consecutive_source_errors = 0;
        let (sequence, frame) = frame;
        if let Some(deadline) = rearm_deadline {
            if Instant::now() < deadline {
                let _ = source.release(sequence);
                continue;
            }
            // The backoff elapsed, so this frame attempts the re-arm.
            rearm_deadline = None;
            // Retire the failed pipeline's finished GPU fences before the
            // replacement records, so the producer's staging contexts return.
            let _ = release_runtime_completions(source.as_ref(), &replay);
        }
        let frame_dimensions = (frame.width, frame.height);
        if dimensions.is_none() {
            let Ok(pipeline) = pipeline_factory(&frame) else {
                eprintln!("Redunar Replay: hardware encoder pipeline creation failed");
                let _ = source.release(sequence);
                enter_replay_recovery(
                    &replay,
                    &coordinator,
                    ReplayFailure::EncoderFailed,
                    &mut dimensions,
                    &mut rearm_deadline,
                    &mut rearm_backoff,
                );
                continue;
            };
            if let Err(error) = replay.start_validated_pipeline(settings, pipeline, readiness) {
                eprintln!("Redunar Replay: recorder activation failed: {error}");
                let _ = source.release(sequence);
                enter_replay_recovery(
                    &replay,
                    &coordinator,
                    ReplayFailure::EncoderFailed,
                    &mut dimensions,
                    &mut rearm_deadline,
                    &mut rearm_backoff,
                );
                continue;
            }
            dimensions = Some(frame_dimensions);
            rearm_backoff = REPLAY_REARM_BACKOFF;
            eprintln!(
                "Redunar Replay: rolling recorder active at {}x{}",
                frame_dimensions.0, frame_dimensions.1
            );
            publish_replay_component(&coordinator, GameSessionComponentState::Active);
            // One audio worker serves the whole game session. It survives
            // recorder re-arms, so re-arming never spawns a second PipeWire
            // consumer for the same game node.
            if audio_worker.is_none() {
                audio_worker = start_game_audio_worker(
                    Arc::clone(&stop),
                    replay.clone(),
                    coordinator.clone(),
                    frame.timestamp_ns,
                );
            }
        } else if dimensions != Some(frame_dimensions) {
            let Ok(replacement) = backend_factory(&frame) else {
                eprintln!(
                    "Redunar Replay: replacement encoder creation failed after a source resize"
                );
                let _ = source.release(sequence);
                let _ = release_runtime_completions(source.as_ref(), &replay);
                enter_replay_recovery(
                    &replay,
                    &coordinator,
                    ReplayFailure::EncoderFailed,
                    &mut dimensions,
                    &mut rearm_deadline,
                    &mut rearm_backoff,
                );
                continue;
            };
            if let Err(error) = replay.reset_backend(replacement) {
                eprintln!("Redunar Replay: recorder reset failed after a source resize: {error}");
                let _ = source.release(sequence);
                let _ = release_runtime_completions(source.as_ref(), &replay);
                enter_replay_recovery(
                    &replay,
                    &coordinator,
                    ReplayFailure::EncoderFailed,
                    &mut dimensions,
                    &mut rearm_deadline,
                    &mut rearm_backoff,
                );
                continue;
            }
            dimensions = Some(frame_dimensions);
            rearm_backoff = REPLAY_REARM_BACKOFF;
            eprintln!(
                "Redunar Replay: rolling recorder reset for {}x{}",
                frame_dimensions.0, frame_dimensions.1
            );
            let _ = release_runtime_completions(source.as_ref(), &replay);
        }
        if let Err(error) = replay.submit_frame(sequence, frame) {
            eprintln!("Redunar Replay: hardware frame submission failed: {error}");
            let completed = replay.take_completed_exports();
            if !completed.contains(&sequence) {
                let _ = source.release(sequence);
            }
            for completed_sequence in completed {
                let _ = source.release(completed_sequence);
            }
            enter_replay_recovery(
                &replay,
                &coordinator,
                ReplayFailure::EncoderFailed,
                &mut dimensions,
                &mut rearm_deadline,
                &mut rearm_backoff,
            );
            continue;
        }
        if release_runtime_completions(source.as_ref(), &replay).is_err() {
            enter_replay_recovery(
                &replay,
                &coordinator,
                ReplayFailure::EncoderFailed,
                &mut dimensions,
                &mut rearm_deadline,
                &mut rearm_backoff,
            );
        }
    }

    if let Some(worker) = audio_worker {
        let _ = worker.join();
    }
    let shutdown = replay.shutdown();
    let releases = release_runtime_completions(source.as_ref(), &replay);
    source.drain_and_release();
    shutdown.and(releases)
}

/// Publish one recoverable recorder failure and schedule a re-arm attempt.
/// The exponential backoff bounds retry churn while a broken encoder or
/// storage path keeps rejecting new pipelines.
fn enter_replay_recovery(
    replay: &ProductionReplayRuntime,
    coordinator: &Weak<CoordinatorInner>,
    failure: ReplayFailure,
    dimensions: &mut Option<(u32, u32)>,
    rearm_deadline: &mut Option<Instant>,
    rearm_backoff: &mut Duration,
) {
    replay.fail(failure);
    publish_replay_component(coordinator, GameSessionComponentState::Failed);
    *dimensions = None;
    *rearm_deadline = Some(Instant::now() + *rearm_backoff);
    *rearm_backoff = rearm_backoff.mul_f32(2.0).min(REPLAY_REARM_BACKOFF_MAX);
}

fn start_game_audio_worker(
    stop: Arc<AtomicBool>,
    replay: ProductionReplayRuntime,
    coordinator: Weak<CoordinatorInner>,
    timeline_timestamp_ns: u64,
) -> Option<JoinHandle<()>> {
    let timeline_started = Instant::now();
    thread::Builder::new()
        .name("redunar-replay-audio".to_owned())
        .spawn(move || {
            run_game_audio_worker(
                &stop,
                &replay,
                &coordinator,
                timeline_timestamp_ns,
                timeline_started,
            );
        })
        .map_err(|error| eprintln!("Redunar Replay: could not start game audio worker: {error}"))
        .ok()
}

fn run_game_audio_worker(
    stop: &AtomicBool,
    replay: &ProductionReplayRuntime,
    coordinator: &Weak<CoordinatorInner>,
    timeline_timestamp_ns: u64,
    timeline_started: Instant,
) {
    // Audio initialization can legitimately lag video (many games create
    // their PipeWire stream only after the first rendered frames). Keep the
    // worker alive and reconnect after transient PipeWire/device failures so
    // audio does not silently disappear for the rest of the session.
    while !stop.load(Ordering::Acquire) {
        let Some(root_pid) = replay_game_process_id(coordinator) else {
            thread::sleep(Duration::from_millis(250));
            continue;
        };
        let process_ids = redunar_capture_audio::process_tree(root_pid);
        let node = match redunar_capture_audio::discover_pipewire_game_node(&process_ids) {
            Ok(Some(found)) => found,
            Ok(None) => {
                thread::sleep(Duration::from_secs(1));
                continue;
            }
            Err(error) => {
                eprintln!("Redunar Replay: game audio discovery failed; retrying: {error}");
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        let mut capture = match redunar_capture_audio::PipeWireGameAudioCapture::start(
            &node,
            timeline_timestamp_ns,
            timeline_started,
        ) {
            Ok(capture) => capture,
            Err(error) => {
                eprintln!("Redunar Replay: game audio capture unavailable; retrying: {error}");
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        eprintln!("Redunar Replay: game-owned audio capture active");
        let mut reconnect = false;
        let mut recorder_pause_logged = false;
        while !stop.load(Ordering::Acquire) {
            match capture.next_packet(Duration::from_millis(100)) {
                Ok(Some(packet)) => {
                    if let Err(error) = replay.submit_audio(packet) {
                        if matches!(
                            replay.status().phase,
                            ReplayPhase::Buffering | ReplayPhase::Saving
                        ) {
                            eprintln!(
                                "Redunar Replay: game audio buffering stopped; retrying: {error}"
                            );
                            reconnect = true;
                            break;
                        }
                        // The recorder itself is stopped (failed, re-arming,
                        // or inactive), which is not a broken audio
                        // transport. Keep this PipeWire consumer attached and
                        // drop packets until the pump re-arms a pipeline;
                        // reconnecting here would respawn the capture helper
                        // about once per second for the rest of the session.
                        if !recorder_pause_logged {
                            eprintln!(
                                "Redunar Replay: game audio paused with the recorder: {error}"
                            );
                            recorder_pause_logged = true;
                        }
                    } else {
                        recorder_pause_logged = false;
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!("Redunar Replay: game audio capture stopped; retrying: {error}");
                    reconnect = true;
                    break;
                }
            }
        }
        if reconnect {
            thread::sleep(Duration::from_secs(1));
        }
    }
}

fn replay_game_process_id(coordinator: &Weak<CoordinatorInner>) -> Option<u32> {
    let coordinator = coordinator.upgrade()?;
    lock_unpoisoned(&coordinator.state)
        .capture
        .as_ref()
        .and_then(|capture| capture.snapshot().producer_process_id)
}

fn release_runtime_completions(
    source: &dyn ReplayFrameSource,
    replay: &ProductionReplayRuntime,
) -> Result<(), ReplayRuntimeError> {
    for sequence in replay.take_completed_exports() {
        source.release(sequence)?;
    }
    Ok(())
}

fn publish_replay_component(
    coordinator: &Weak<CoordinatorInner>,
    component: GameSessionComponentState,
) {
    let Some(coordinator) = coordinator.upgrade() else {
        return;
    };
    let mut state = lock_unpoisoned(&coordinator.state);
    if matches!(
        state.status.phase,
        GameSessionPhase::Launching | GameSessionPhase::Running
    ) && state.status.replay != component
    {
        state.status.replay = component;
        bump(&mut state.status);
    }
}

fn validate_replay_session_state(state: &CoordinatorState) -> Result<(), ReplayRuntimeError> {
    if !matches!(
        state.status.phase,
        GameSessionPhase::Launching | GameSessionPhase::Running
    ) {
        return Err(ReplayRuntimeError::new(
            "Instant Replay requires an active game session",
        ));
    }
    if !state
        .request
        .as_ref()
        .is_some_and(|request| request.replay.is_enabled())
    {
        return Err(ReplayRuntimeError::new(
            "Instant Replay was not requested for this game session",
        ));
    }
    if state.capture.is_none() {
        return Err(ReplayRuntimeError::new(
            "Instant Replay requires the game frame-transfer runtime",
        ));
    }
    Ok(())
}

fn collect_and_release_replay_exports(
    state: &mut CoordinatorState,
    replay: &ProductionReplayRuntime,
) {
    for sequence in replay.take_completed_exports() {
        if !state.pending_replay_releases.contains(&sequence) {
            state.pending_replay_releases.push_back(sequence);
        }
    }
    let Some(capture) = state.capture.as_ref() else {
        return;
    };
    while let Some(sequence) = state.pending_replay_releases.front().copied() {
        if capture.release_replay_export(sequence).is_err() {
            break;
        }
        state.pending_replay_releases.pop_front();
    }
}

fn requested_state(requested: bool, available: bool) -> GameSessionComponentState {
    match (requested, available) {
        (false, _) => GameSessionComponentState::Disabled,
        (true, true) => GameSessionComponentState::Pending,
        (true, false) => GameSessionComponentState::Unavailable,
    }
}

fn replay_component(status: ReplayRuntimeStatus, requested: bool) -> GameSessionComponentState {
    if !requested {
        return GameSessionComponentState::Disabled;
    }
    match status.phase {
        ReplayPhase::Unavailable => GameSessionComponentState::Unavailable,
        ReplayPhase::Inactive => GameSessionComponentState::Pending,
        ReplayPhase::Buffering | ReplayPhase::Saving => GameSessionComponentState::Active,
        ReplayPhase::Failed => GameSessionComponentState::Failed,
    }
}

fn replay_menu_has_render_target(state: &CoordinatorState) -> bool {
    state.status.phase == GameSessionPhase::Running
        && state
            .capture
            .as_ref()
            .is_some_and(|capture| capture.snapshot().phase == CapturePhase::Capturing)
}

fn stop_optional(state: GameSessionComponentState) -> GameSessionComponentState {
    match state {
        GameSessionComponentState::Pending | GameSessionComponentState::Active => {
            GameSessionComponentState::Stopped
        }
        other => other,
    }
}

fn apply_capture_snapshot(state: &mut CoordinatorState, snapshot: &crate::CaptureSnapshot) {
    let request = state.request.as_ref();
    let metrics_requested = request.is_some_and(|request| request.metrics.is_enabled());
    let overlay_requested = request.is_some_and(|request| request.overlay.is_enabled());
    let metrics = if metrics_requested {
        match snapshot.phase {
            CapturePhase::Armed => GameSessionComponentState::Pending,
            CapturePhase::Capturing => GameSessionComponentState::Active,
            CapturePhase::Completed => GameSessionComponentState::Stopped,
            CapturePhase::Failed => GameSessionComponentState::Failed,
        }
    } else {
        GameSessionComponentState::Disabled
    };
    let overlay = if overlay_requested {
        match snapshot.overlay_status {
            None | Some(OverlayRuntimeStatus::Requested) => GameSessionComponentState::Pending,
            Some(OverlayRuntimeStatus::Active) => GameSessionComponentState::Active,
            Some(OverlayRuntimeStatus::Error(_)) => GameSessionComponentState::Failed,
        }
    } else {
        GameSessionComponentState::Disabled
    };
    let phase = if snapshot.phase == CapturePhase::Armed {
        GameSessionPhase::Launching
    } else {
        GameSessionPhase::Running
    };
    if state.status.metrics != metrics
        || state.status.overlay != overlay
        || state.status.phase != phase
    {
        state.status.metrics = metrics;
        state.status.overlay = overlay;
        state.status.phase = phase;
        bump(&mut state.status);
    }
}

fn bump(status: &mut GameSessionStatus) {
    status.revision = status.revision.saturating_add(1);
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use redunar_core::ReplaySettings;
    use std::time::Instant;

    fn request() -> GameSessionRequest {
        GameSessionRequest {
            game_id: GameId::new(9).ok(),
            game_name: "Fixture Game".to_owned(),
            metrics: GameSessionFeatureRequest::Enabled,
            overlay: GameSessionFeatureRequest::Enabled,
            replay: GameSessionFeatureRequest::Enabled,
        }
    }

    #[test]
    fn requested_components_share_one_launch_state() {
        let mut state = CoordinatorState::default();
        let request = request();
        state.request = Some(request.clone());
        state.status = GameSessionStatus {
            phase: GameSessionPhase::Launching,
            game_id: request.game_id,
            game_name: Some(request.game_name),
            metrics: requested_state(request.metrics.is_enabled(), true),
            overlay: requested_state(request.overlay.is_enabled(), true),
            replay: replay_component(
                ReplayRuntimeStatus::unavailable(ReplaySettings::default()),
                request.replay.is_enabled(),
            ),
            ..GameSessionStatus::default()
        };
        assert_eq!(state.status.metrics, GameSessionComponentState::Pending);
        assert_eq!(state.status.overlay, GameSessionComponentState::Pending);
        assert_eq!(state.status.replay, GameSessionComponentState::Unavailable);
    }

    #[test]
    fn replay_start_requires_active_requested_session_and_capture() {
        let idle = CoordinatorState::default();
        assert!(
            validate_replay_session_state(&idle)
                .expect_err("idle session")
                .to_string()
                .contains("active game session")
        );

        let without_capture = CoordinatorState {
            request: Some(request()),
            status: GameSessionStatus {
                phase: GameSessionPhase::Running,
                ..GameSessionStatus::default()
            },
            ..CoordinatorState::default()
        };
        assert!(
            validate_replay_session_state(&without_capture)
                .expect_err("missing capture")
                .to_string()
                .contains("frame-transfer")
        );

        let mut disabled = without_capture;
        disabled.request.as_mut().expect("request").replay = GameSessionFeatureRequest::Disabled;
        assert!(
            validate_replay_session_state(&disabled)
                .expect_err("Replay disabled")
                .to_string()
                .contains("not requested")
        );
    }

    #[test]
    fn replay_menu_refuses_mouse_capture_without_a_live_game_render_target() {
        assert!(!replay_menu_has_render_target(&CoordinatorState::default()));
    }

    #[test]
    fn optional_cleanup_is_explicit_and_idempotent() {
        for state in [
            GameSessionComponentState::Pending,
            GameSessionComponentState::Active,
        ] {
            assert_eq!(stop_optional(state), GameSessionComponentState::Stopped);
            assert_eq!(
                stop_optional(stop_optional(state)),
                GameSessionComponentState::Stopped
            );
        }
        assert_eq!(
            stop_optional(GameSessionComponentState::Failed),
            GameSessionComponentState::Failed
        );
    }

    #[test]
    fn replay_pump_startup_failure_is_component_local_and_stop_is_idempotent() {
        let (endpoint, acknowledgements) = ReplayExportEndpoint::new_for_test();
        let stop = Arc::new(AtomicBool::new(false));
        let runtime = ProductionReplayRuntime::unavailable(ReplaySettings::default());
        let worker = {
            let worker_stop = Arc::clone(&stop);
            let worker_endpoint = endpoint.clone();
            let worker_runtime = runtime.clone();
            thread::spawn(move || {
                run_replay_export_pump(
                    worker_stop,
                    Arc::new(worker_endpoint),
                    worker_runtime,
                    Weak::<CoordinatorInner>::new(),
                    ReplaySettings::default(),
                    ReplayBackendReadiness::fully_verified(),
                    Arc::new(|_| Err("fixture backend initialization failed".to_owned())),
                    Arc::new(|_| Err("replacement must not be requested".to_owned())),
                )
            })
        };
        let mut pump = ReplayExportPump {
            stop,
            source: Arc::new(endpoint.clone()),
            worker: Some(worker),
        };
        endpoint.enqueue_for_test(crate::capture_session::ReplayExportMetadata {
            sequence: 1,
            fd: std::fs::File::open("/dev/null")
                .expect("open fixture fd")
                .into(),
            source: redunar_capture::ReplaySourceCandidate {
                width: 320,
                height: 240,
                pixel_format: redunar_capture::ReplayPixelFormat::Bgra8Unorm,
                target_frames_per_second: 60,
            },
            offset: 0,
            stride: 1_280,
            modifier: 0,
            timestamp_ns: 1,
            duration_ns: 16_666_667,
        });

        let deadline = Instant::now() + Duration::from_secs(1);
        while runtime.status().phase != ReplayPhase::Failed && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(runtime.status().phase, ReplayPhase::Failed);
        pump.stop_and_join().expect("stop failed pump");
        pump.stop_and_join().expect("second stop is idempotent");
        assert_eq!(*lock_unpoisoned(&acknowledgements), vec![1]);
    }
}
