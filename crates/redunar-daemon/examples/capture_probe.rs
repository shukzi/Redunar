use redunar_capture::OverlayRuntimeStatus;
use redunar_capture_vulkan::replay_video::{VulkanVideoH264Device, VulkanVideoH264Request};
use redunar_core::{
    GameId, GlobalGameProfile, Inheritable, OverlayCorner, OverlayLayout, OverlayMetricSet,
    OverlayOpacity, OverlayPalette, OverlayPreset, OverlayScale, PerGameProfile, ReplayDuration,
    ReplayFrameRate, ReplaySettings,
};
use redunar_daemon::{
    AddGameRequest, CapturePhase, CaptureSessionConfig, CaptureSessionHandle, CaptureSnapshot,
    DmaBufReplayFrame, GameSessionFeatureRequest, GameSessionRequest,
    ProductionGameSessionCoordinator, RedunarService, ReplayBudget, ReplayClipStore,
    ReplayHardwarePipeline, ReplayOutputFormat, ReplayPhase, ReplayRuntimeStatus, StoredReplayClip,
    VulkanVideoH264Backend,
};
use redunar_platform::ReplayTransferLaunchConfig;
use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_FRAME_COUNT: &str = "360";
const CHILD_TIMEOUT: Duration = Duration::from_secs(20);
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(2);

#[expect(
    clippy::too_many_lines,
    reason = "the diagnostic keeps one linear launch, validation, and teardown sequence"
)]
fn main() -> Result<(), Box<dyn Error>> {
    let executable = env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("/usr/bin/vkcube"), PathBuf::from);
    if !executable.is_absolute() || !executable.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "capture target must be an absolute executable file: {}",
                executable.display()
            ),
        )
        .into());
    }

    let runtime_root = runtime_root()?;
    let probe_state = ProbeState::new(&runtime_root)?;
    let production_replay_requested = env::var("REDUNAR_CAPTURE_PROBE_PRODUCTION_REPLAY")
        .ok()
        .as_deref()
        == Some("1");
    let service = if production_replay_requested {
        RedunarService::for_isolated_replay_probe(&probe_state.directory)
    } else {
        RedunarService::with_state_directory(&probe_state.directory)
    };
    let arguments = default_arguments(&executable)?;
    let steam_bridge = env::var("REDUNAR_CAPTURE_PROBE_STEAM_BRIDGE")
        .ok()
        .as_deref()
        == Some("1");
    let display_name = executable
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Capture probe")
        .to_owned();
    let request = if steam_bridge {
        let mut request = AddGameRequest::new(display_name, "/usr/bin/steam");
        request.launch.arguments = vec![OsString::from("-applaunch"), OsString::from("413150")];
        request
    } else {
        let mut request = AddGameRequest::new(display_name, &executable);
        request.launch.arguments.clone_from(&arguments);
        request
    };
    let added = service.add_game(request)?;
    let overlay_enabled = env::var("REDUNAR_CAPTURE_PROBE_OVERLAY").ok().as_deref() == Some("1");
    let replay_encode_requested = env::var("REDUNAR_CAPTURE_PROBE_REPLAY_ENCODE")
        .ok()
        .as_deref()
        == Some("1");
    let hold_replay_releases = env::var("REDUNAR_CAPTURE_PROBE_HOLD_REPLAY_RELEASES")
        .ok()
        .as_deref()
        == Some("1");
    let replay_candidate_requested = replay_encode_requested
        || production_replay_requested
        || env::var("REDUNAR_CAPTURE_PROBE_REPLAY_CANDIDATE")
            .ok()
            .as_deref()
            == Some("1");
    configure_overlay(&service, added.id, overlay_enabled)?;
    if production_replay_requested {
        let mut global = service.load_game_catalog()?.global_profile;
        global.instant_replay = true;
        global.replay.frame_rate = replay_probe_frame_rate()?;
        service.update_global_game_profile(global)?;
    }
    let catalog = service.load_game_catalog()?;
    let game = catalog
        .games
        .iter()
        .find(|game| game.id == added.id)
        .ok_or_else(|| io::Error::other("persisted probe game is missing"))?;

    let mut config = CaptureSessionConfig::new(&runtime_root, capture_library()?);
    if let Some(opengl_library) = opengl_capture_library()? {
        config = config.with_opengl_library(opengl_library);
    }
    let mut session = service.start_capture_session(&config)?;
    let environment = env::vars_os().collect::<BTreeMap<_, _>>();
    if overlay_enabled {
        session.update_overlay_hardware(Some(&service.snapshot()?))?;
        session.update_overlay_config(&game.profile.resolve(catalog.global_profile))?;
    }
    let plan = if steam_bridge {
        None
    } else if overlay_enabled || production_replay_requested {
        Some(session.game_launch_plan_for_profile(game, catalog.global_profile, &environment)?)
    } else {
        Some(session.game_launch_plan(&game.launch, &environment)?)
    };
    // Diagnostic-only override. Production profile planning always keeps this
    // request off until every Replay readiness gate has passed.
    let plan = if replay_candidate_requested && !production_replay_requested && !steam_bridge {
        Some(
            plan.expect("direct probe has a launch plan")
                .with_replay_transfer_config(ReplayTransferLaunchConfig::new(
                    true,
                    replay_probe_frame_rate()?,
                )),
        )
    } else {
        plan
    };
    let mut command = if steam_bridge {
        if replay_candidate_requested {
            session.publish_steam_activation_for_replay_diagnostic(
                game,
                catalog.global_profile,
                Duration::from_secs(30),
                ReplayTransferLaunchConfig::new(true, replay_probe_frame_rate()?),
            )?;
        } else {
            session.publish_steam_activation_for_profile(
                game,
                catalog.global_profile,
                Duration::from_secs(30),
            )?;
        }
        let mut command = Command::new(steam_wrapper()?);
        command
            .arg("--app-id")
            .arg("413150")
            .arg("--")
            .arg(&executable)
            .args(&arguments);
        let isolated_xdg = runtime_root
            .parent()
            .ok_or_else(|| io::Error::other("Steam probe runtime has no XDG parent directory"))?;
        command.env("XDG_RUNTIME_DIR", isolated_xdg);
        command
    } else {
        plan.expect("direct probe has a launch plan").command()
    };
    configure_diagnostic_environment(&mut command);
    command.stdout(Stdio::null()).stderr(Stdio::inherit());

    if production_replay_requested {
        if steam_bridge {
            return Err(io::Error::other(
                "production Replay probe currently requires a direct launch",
            )
            .into());
        }
        return run_production_replay_probe(
            &service,
            game.id,
            game.display_name.as_str(),
            session,
            command,
            overlay_enabled,
            &probe_state.directory,
        );
    }

    println!("receiver_pid={}", std::process::id());
    println!("catalog_game_id={}", game.id.get());
    println!("catalog_arguments={}", game.launch.arguments.len());
    println!("overlay_requested={overlay_enabled}");
    println!("replay_candidate_requested={replay_candidate_requested}");
    println!("replay_encode_requested={replay_encode_requested}");
    println!("hold_replay_releases={hold_replay_releases}");
    println!("steam_bridge_requested={steam_bridge}");
    let start_process = process_sample()?;
    let started = Instant::now();
    let mut child = command.spawn()?;
    let (status, replay_clip) = if hold_replay_releases {
        let status = run_replay_backpressure_validation(&mut child, &session, child_timeout()?)?;
        (status, None)
    } else if replay_encode_requested {
        let settings = ReplaySettings {
            frame_rate: replay_probe_frame_rate()?,
            ..ReplaySettings::default()
        };
        let (status, clip) = run_replay_encode_validation(
            &mut child,
            &session,
            &probe_state.directory,
            settings,
            child_timeout()?,
        )?;
        (status, Some(clip))
    } else {
        (wait_for_child(&mut child, child_timeout()?)?, None)
    };
    let snapshot = wait_for_completion(&session, COMPLETION_TIMEOUT);
    let elapsed = started.elapsed();
    let end_process = process_sample()?;

    print_snapshot(&snapshot, elapsed, start_process, end_process);
    if let Some(clip) = replay_clip {
        println!("replay_encoded_clip={}", clip.path.display());
        println!("replay_encoded_bytes={}", clip.bytes);
    }
    session.shutdown();
    service.remove_game(game.id)?;

    if !status.success() {
        return Err(io::Error::other(format!("capture target exited with {status}")).into());
    }
    if snapshot.phase != CapturePhase::Completed || snapshot.received_frame_count == 0 {
        return Err(io::Error::other("capture did not complete with frame telemetry").into());
    }
    if overlay_enabled && snapshot.overlay_status != Some(OverlayRuntimeStatus::Active) {
        return Err(io::Error::other(format!(
            "overlay probe did not submit rendered output (status={:?})",
            snapshot.overlay_status
        ))
        .into());
    }
    if replay_candidate_requested
        && env::var("REDUNAR_CAPTURE_PROBE_REQUIRE_REPLAY_CANDIDATE")
            .ok()
            .as_deref()
            == Some("1")
        && snapshot.replay_source_candidate.is_none()
    {
        return Err(io::Error::other("Vulkan target exposed no eligible replay source").into());
    }
    Ok(())
}

fn run_replay_backpressure_validation(
    child: &mut Child,
    session: &CaptureSessionHandle,
    timeout: Duration,
) -> Result<std::process::ExitStatus, Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    let mut held = Vec::new();
    let status = loop {
        while let Some((sequence, _frame)) = session.take_replay_validation_export()? {
            // Deliberately retain the daemon's ownership token. The game
            // must keep presenting and drop Replay work at the six-slot cap.
            held.push(sequence);
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(
                io::Error::new(io::ErrorKind::TimedOut, "backpressure probe timed out").into(),
            );
        }
        thread::sleep(Duration::from_millis(1));
    };
    println!("held_replay_exports={}", held.len());
    for sequence in held {
        session.release_replay_validation_export(sequence)?;
    }
    Ok(status)
}

#[expect(
    clippy::too_many_lines,
    reason = "the hardware probe keeps launch, native state polling, save, and teardown in one auditable sequence"
)]
fn run_production_replay_probe(
    service: &RedunarService,
    game_id: GameId,
    game_name: &str,
    session: CaptureSessionHandle,
    mut command: Command,
    overlay_enabled: bool,
    state_directory: &Path,
) -> Result<(), Box<dyn Error>> {
    let coordinator = service.game_session_coordinator();
    let (fds_before, threads_before) = process_resources()?;
    let catalog = service.load_game_catalog()?;
    let settings = catalog.global_profile.replay;
    let mut effective_profile = catalog
        .games
        .iter()
        .find(|game| game.id == game_id)
        .ok_or_else(|| io::Error::other("production probe game is missing"))?
        .profile
        .resolve(catalog.global_profile);
    if env::var("REDUNAR_CAPTURE_PROBE_REPLAY_FORMAT")
        .ok()
        .as_deref()
        == Some("mp4")
    {
        service.set_replay_output_format(ReplayOutputFormat::Mp4)?;
    }
    coordinator.begin(
        GameSessionRequest {
            game_id: Some(game_id),
            game_name: game_name.to_owned(),
            metrics: GameSessionFeatureRequest::Enabled,
            overlay: overlay_enabled.into(),
            replay: GameSessionFeatureRequest::Enabled,
        },
        Some(session),
        ReplayRuntimeStatus::validation_candidate(settings),
    )?;
    let mut cleanup = ProductionProbeCleanup {
        coordinator: coordinator.clone(),
        child: None,
    };
    service.start_replay_validation_candidate()?;
    let start_process = process_sample()?;
    let started = Instant::now();
    cleanup.child = Some(command.spawn()?);
    coordinator.mark_running();
    let deadline = started + child_timeout()?;
    let mut requested_save = false;
    let mut saved_notice_published = false;
    let mut menu_checked = false;
    let mut visibility_checked = false;
    let mut saw_buffering = false;
    let mut saw_saving = false;
    let preferred_format = if env::var("REDUNAR_CAPTURE_PROBE_REPLAY_FORMAT")
        .ok()
        .as_deref()
        == Some("mp4")
    {
        ReplayOutputFormat::Mp4
    } else {
        ReplayOutputFormat::Matroska
    };
    let status = loop {
        let replay = coordinator.replay_runtime().status();
        if requested_save && !saved_notice_published && replay.completed_save_revision > 0 {
            service.publish_replay_saved_notice()?;
            saved_notice_published = true;
        }
        saw_buffering |= replay.phase == ReplayPhase::Buffering && replay.encoded_packet_count > 0;
        saw_saving |= replay.phase == ReplayPhase::Saving;
        if !menu_checked && replay.buffered_duration_ns >= 1_000_000_000 {
            check_replay_menu_control(state_directory, preferred_format)?;
            if service.replay_preferences()?.output_format != preferred_format {
                return Err(io::Error::other("menu format choice did not persist").into());
            }
            menu_checked = true;
        }
        if !visibility_checked && replay.buffered_duration_ns >= 1_500_000_000 {
            effective_profile.overlay_visible = !overlay_enabled;
            coordinator.update_overlay_config(&effective_profile)?;
            effective_profile.overlay_visible = overlay_enabled;
            coordinator.update_overlay_config(&effective_profile)?;
            if coordinator.replay_runtime().status().phase != ReplayPhase::Buffering {
                return Err(io::Error::other("overlay visibility interrupted Replay").into());
            }
            visibility_checked = true;
        }
        if !requested_save && replay.buffered_duration_ns >= 2_000_000_000 {
            if env::var("REDUNAR_CAPTURE_PROBE_SAVE_VIA_MENU")
                .ok()
                .as_deref()
                == Some("1")
            {
                expect_replay_control(state_directory, "MENU TOGGLE\n", "OK GRAB\n")?;
                expect_replay_control(state_directory, "MENU MOVE 4000 3500\n", "OK\n")?;
                expect_replay_control(state_directory, "MENU BUTTON 1\n", "OK\n")?;
                expect_replay_control(state_directory, "MENU BUTTON 0\n", "OK RELEASE\n")?;
            } else if env::var("REDUNAR_CAPTURE_PROBE_SAVE_VIA_CONTROL")
                .ok()
                .as_deref()
                == Some("1")
            {
                expect_replay_control(state_directory, "SAVE 15\n", "OK\n")?;
            } else {
                service.save_replay(ReplayDuration::Seconds15)?;
            }
            requested_save = true;
            // A successful request enters Saving synchronously; the small
            // synthetic clip can finish before the next polling interval.
            saw_saving = true;
        }
        if let Some(status) = cleanup
            .child
            .as_mut()
            .expect("probe child spawned")
            .try_wait()?
        {
            break status;
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "production Replay probe timed out",
            )
            .into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    let save_deadline = Instant::now() + Duration::from_secs(10);
    while requested_save
        && coordinator
            .replay_runtime()
            .status()
            .completed_save_revision
            == 0
    {
        let replay = coordinator.replay_runtime().status();
        saw_saving |= replay.phase == ReplayPhase::Saving;
        if replay.phase == ReplayPhase::Failed || Instant::now() >= save_deadline {
            return Err(
                io::Error::other(format!("production Replay save failed: {replay:?}")).into(),
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
    let replay = coordinator.replay_runtime().status();
    let snapshot = coordinator
        .capture_snapshot()
        .ok_or_else(|| io::Error::other("capture missing"))?;
    print_snapshot(
        &snapshot,
        started.elapsed(),
        start_process,
        process_sample()?,
    );
    println!("production_replay_buffering={saw_buffering}");
    println!("production_replay_saving={saw_saving}");
    println!("production_replay_menu_control={menu_checked}");
    println!("production_replay_visibility_toggle={visibility_checked}");
    println!("production_replay_saved_notice={saved_notice_published}");
    println!(
        "production_replay_completed_save_revision={}",
        replay.completed_save_revision
    );
    println!(
        "production_replay_encoded_packets={}",
        replay.encoded_packet_count
    );
    println!(
        "production_replay_buffered_ns={}",
        replay.buffered_duration_ns
    );
    let save_directory = service.replay_save_directory()?;
    let clip = std::fs::read_dir(&save_directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("mkv" | "mp4")
            )
        })
        .ok_or_else(|| io::Error::other("production Replay committed no clip"))?;
    println!("replay_encoded_clip={}", clip.display());
    println!("replay_encoded_bytes={}", clip.metadata()?.len());
    coordinator.end()?;
    let (fds_after, threads_after) = process_resources()?;
    println!("production_replay_fds_before={fds_before}");
    println!("production_replay_fds_after={fds_after}");
    println!("production_replay_threads_before={threads_before}");
    println!("production_replay_threads_after={threads_after}");
    service.remove_game(game_id)?;
    if !status.success()
        || snapshot.phase != CapturePhase::Completed
        || !saw_buffering
        || !saw_saving
        || !menu_checked
        || !visibility_checked
        || !saved_notice_published
        || !requested_save
        || replay.completed_save_revision == 0
        || fds_after > fds_before.saturating_add(4)
        || threads_after > threads_before.saturating_add(1)
    {
        return Err(io::Error::other("production Replay lifecycle did not complete").into());
    }
    if !state_directory.exists() {
        return Err(io::Error::other("probe state unexpectedly disappeared").into());
    }
    Ok(())
}

fn process_resources() -> io::Result<(usize, usize)> {
    Ok((
        std::fs::read_dir("/proc/self/fd")?.count(),
        std::fs::read_dir("/proc/self/task")?.count(),
    ))
}

fn replay_control_command(state_directory: &Path, command: &str) -> io::Result<String> {
    let mut stream = UnixStream::connect(state_directory.join("replay-control-v1.sock"))?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.write_all(command.as_bytes())?;
    let mut response = String::new();
    io::BufReader::new(stream).read_line(&mut response)?;
    Ok(response)
}

fn expect_replay_control(state_directory: &Path, command: &str, expected: &str) -> io::Result<()> {
    let response = replay_control_command(state_directory, command)?;
    if response != expected {
        return Err(io::Error::other(format!("{command:?}: {response:?}")));
    }
    Ok(())
}

fn check_replay_menu_control(state_directory: &Path, format: ReplayOutputFormat) -> io::Result<()> {
    for (command, expected) in [
        ("MENU TOGGLE\n", "OK GRAB\n"),
        ("MENU MOVE -2000 3500\n", "OK\n"),
        ("MENU BUTTON 1\n", "OK\n"),
        ("MENU BUTTON 0\n", "OK\n"),
    ] {
        expect_replay_control(state_directory, command, expected)?;
    }
    if format == ReplayOutputFormat::Matroska {
        expect_replay_control(state_directory, "MENU MOVE -2000 0\n", "OK\n")?;
        expect_replay_control(state_directory, "MENU BUTTON 1\n", "OK\n")?;
        expect_replay_control(state_directory, "MENU BUTTON 0\n", "OK\n")?;
    }
    let move_to_duration = if format == ReplayOutputFormat::Mp4 {
        "MENU MOVE -2000 -3500\n"
    } else {
        "MENU MOVE 0 -3500\n"
    };
    for (command, expected) in [
        (move_to_duration, "OK\n"),
        ("MENU BUTTON 1\n", "OK\n"),
        ("MENU BUTTON 0\n", "OK\n"),
        ("MENU PING\n", "OK\n"),
        ("MENU ESCAPE\n", "OK RELEASE\n"),
        ("MENU PING\n", "OK RELEASE\n"),
    ] {
        expect_replay_control(state_directory, command, expected)?;
    }
    Ok(())
}

struct ProductionProbeCleanup {
    coordinator: ProductionGameSessionCoordinator,
    child: Option<Child>,
}

impl Drop for ProductionProbeCleanup {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut()
            && matches!(child.try_wait(), Ok(None))
        {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = self.coordinator.end();
    }
}

fn run_replay_encode_validation(
    child: &mut Child,
    session: &CaptureSessionHandle,
    state_directory: &Path,
    settings: ReplaySettings,
    timeout: Duration,
) -> Result<(std::process::ExitStatus, StoredReplayClip), Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    let mut pipeline = None;
    let mut submit_count = 0_u64;
    let mut submit_total = Duration::ZERO;
    let mut submit_max = Duration::ZERO;
    let status = loop {
        while let Some((sequence, frame)) = session.take_replay_validation_export()? {
            if pipeline.is_none() {
                pipeline = Some(validation_pipeline(state_directory, settings, &frame)?);
            }
            let recorder = pipeline
                .as_mut()
                .expect("validation pipeline initialized from this frame");
            if recorder.stream().width() != frame.width
                || recorder.stream().height() != frame.height
            {
                let replacement = validation_backend(settings, &frame)?;
                recorder.reset(Box::new(replacement))?;
                release_completed_validation_exports(session, recorder)?;
            }
            let submit_started = Instant::now();
            let submit_result = recorder.submit_frame(sequence, frame);
            let submit_elapsed = submit_started.elapsed();
            submit_count = submit_count.saturating_add(1);
            submit_total = submit_total.saturating_add(submit_elapsed);
            submit_max = submit_max.max(submit_elapsed);
            if let Err(error) = submit_result {
                let _ = recorder.shutdown();
                release_completed_validation_exports(session, recorder)?;
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
            release_completed_validation_exports(session, recorder)?;
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            if let Some(recorder) = pipeline.as_mut() {
                let _ = recorder.shutdown();
                release_completed_validation_exports(session, recorder)?;
            }
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Replay encode validation target exceeded the diagnostic timeout",
            )
            .into());
        }
        thread::sleep(Duration::from_millis(1));
    };
    let mut pipeline = pipeline.ok_or_else(|| {
        let snapshot = session.snapshot();
        io::Error::other(format!(
            "Replay validation received no completed DMA-BUF exports (candidate={:?}, rejection={:?}, copied_frames={}, exported_frames={}, capture_failure={:?})",
            snapshot.replay_source_candidate,
            snapshot.replay_source_rejection,
            snapshot.replay_copied_frame_count,
            snapshot.replay_exported_frame_count,
            snapshot.failure,
        ))
    })?;
    let clip = pipeline.save_replay()?;
    release_completed_validation_exports(session, &mut pipeline)?;
    println!("replay_submit_count={submit_count}");
    println!("replay_submit_total_us={}", submit_total.as_micros());
    println!("replay_submit_max_us={}", submit_max.as_micros());
    pipeline.shutdown()?;
    release_completed_validation_exports(session, &mut pipeline)?;
    Ok((status, clip))
}

fn validation_pipeline(
    state_directory: &Path,
    settings: ReplaySettings,
    frame: &DmaBufReplayFrame,
) -> Result<ReplayHardwarePipeline, Box<dyn Error>> {
    let backend = validation_backend(settings, frame)?;
    let store = ReplayClipStore::open(
        state_directory.join("encoded-replay"),
        ReplayBudget::from_settings(settings),
    )?;
    Ok(ReplayHardwarePipeline::new(
        settings,
        store,
        Box::new(backend),
    )?)
}

fn validation_backend(
    settings: ReplaySettings,
    frame: &DmaBufReplayFrame,
) -> Result<VulkanVideoH264Backend, Box<dyn Error>> {
    let variable_rate = settings.frame_rate == ReplayFrameRate::Variable;
    let request = VulkanVideoH264Request {
        width: frame.width,
        height: frame.height,
        frames_per_second: if variable_rate {
            redunar_capture_vulkan::replay_video::maximum_variable_frame_rate(
                frame.width,
                frame.height,
            )
        } else {
            u8::try_from(settings.frame_rate.frames_per_second())
                .expect("fixed Replay frame rates are bounded")
        },
        variable_rate,
        target_megabits_per_second: settings.quality.target_megabits_per_second(),
    };
    let encoder = VulkanVideoH264Device::open(request)?
        .create_session()?
        .create_parameters()?
        .create_production_encoder()?;
    Ok(VulkanVideoH264Backend::new(encoder)?)
}

fn release_completed_validation_exports(
    session: &CaptureSessionHandle,
    pipeline: &mut ReplayHardwarePipeline,
) -> Result<(), Box<dyn Error>> {
    for sequence in pipeline.take_completed_inputs() {
        session.release_replay_validation_export(sequence)?;
    }
    Ok(())
}

fn configure_diagnostic_environment(command: &mut std::process::Command) {
    if env::var("REDUNAR_CAPTURE_PROBE_OVERLAY").ok().as_deref() == Some("1") {
        command.env("REDUNAR_OVERLAY_DIAGNOSTIC", "1");
    }
    if env::var("REDUNAR_CAPTURE_PROBE_REPLAY_ENCODE")
        .ok()
        .as_deref()
        == Some("1")
        || env::var("REDUNAR_CAPTURE_PROBE_LOG_REPLAY_POOL")
            .ok()
            .as_deref()
            == Some("1")
    {
        command.env("REDUNAR_REPLAY_DIAGNOSTIC_EXPORT_DMABUF", "1");
    }
    if env::var("REDUNAR_CAPTURE_PROBE_REPLAY_CANDIDATE")
        .ok()
        .as_deref()
        == Some("1")
    {
        command.env("REDUNAR_REPLAY_DIAGNOSTIC_OPENGL_READBACK", "1");
    }
    if env::var("REDUNAR_CAPTURE_PROBE_ADD_TRANSFER_SRC")
        .ok()
        .as_deref()
        == Some("1")
    {
        command.env("REDUNAR_REPLAY_DIAGNOSTIC_ADD_TRANSFER_SRC", "1");
    }
    if env::var("REDUNAR_CAPTURE_PROBE_FAIL_COPY_SETUP")
        .ok()
        .as_deref()
        == Some("1")
    {
        command.env("REDUNAR_REPLAY_DIAGNOSTIC_FAIL_COPY_SETUP", "1");
    }
    if let Some(stage) = env::var_os("REDUNAR_CAPTURE_PROBE_GL_FAIL") {
        command.env("REDUNAR_REPLAY_GL_FAIL", stage);
    }
}

fn configure_overlay(
    service: &RedunarService,
    game_id: GameId,
    enabled: bool,
) -> Result<(), Box<dyn Error>> {
    if !enabled {
        return Ok(());
    }
    service.update_global_game_profile(GlobalGameProfile {
        overlay_visible: true,
        overlay_preset: match env::var("REDUNAR_CAPTURE_PROBE_PRESET").ok().as_deref() {
            Some("fps-only") => OverlayPreset::FpsOnly,
            Some("detailed") => OverlayPreset::Detailed,
            Some("custom") => OverlayPreset::Custom,
            _ => OverlayPreset::Compact,
        },
        overlay_layout: match env::var("REDUNAR_CAPTURE_PROBE_LAYOUT").ok().as_deref() {
            Some("ribbon") => OverlayLayout::Ribbon,
            Some("telemetry") => OverlayLayout::Telemetry,
            _ => OverlayLayout::Grid,
        },
        overlay_palette: match env::var("REDUNAR_CAPTURE_PROBE_PALETTE").ok().as_deref() {
            Some("glacier") => OverlayPalette::Glacier,
            Some("ember") => OverlayPalette::Ember,
            Some("mint") => OverlayPalette::Mint,
            Some("mono") => OverlayPalette::Mono,
            Some("amethyst") => OverlayPalette::Amethyst,
            Some("solar") => OverlayPalette::Solar,
            Some("rose") => OverlayPalette::Rose,
            _ => OverlayPalette::Redunar,
        },
        overlay_branding: env::var("REDUNAR_CAPTURE_PROBE_BRANDING").ok().as_deref() != Some("0"),
        overlay_metrics: env::var("REDUNAR_CAPTURE_PROBE_METRICS")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .and_then(|bits| OverlayMetricSet::from_bits(bits).ok())
            .unwrap_or_default(),
        overlay_corner: match env::var("REDUNAR_CAPTURE_PROBE_CORNER").ok().as_deref() {
            Some("top-right") => OverlayCorner::TopRight,
            Some("bottom-left") => OverlayCorner::BottomLeft,
            Some("bottom-right") => OverlayCorner::BottomRight,
            _ => OverlayCorner::TopLeft,
        },
        overlay_opacity: env::var("REDUNAR_CAPTURE_PROBE_OPACITY")
            .ok()
            .and_then(|value| value.parse::<u8>().ok())
            .and_then(|percent| OverlayOpacity::new(percent).ok())
            .unwrap_or_default(),
        overlay_scale: env::var("REDUNAR_CAPTURE_PROBE_SCALE")
            .ok()
            .and_then(|value| value.parse::<u8>().ok())
            .and_then(OverlayScale::new)
            .unwrap_or_default(),
        instant_replay: env::var("REDUNAR_CAPTURE_PROBE_REPLAY_CANDIDATE")
            .ok()
            .as_deref()
            == Some("1")
            || env::var("REDUNAR_CAPTURE_PROBE_REPLAY_ENCODE")
                .ok()
                .as_deref()
                == Some("1"),
        ..GlobalGameProfile::default()
    })?;
    service.update_game_profile(
        game_id,
        PerGameProfile {
            overlay_visible: Inheritable::Custom(true),
            ..PerGameProfile::default()
        },
    )?;
    Ok(())
}

fn replay_probe_frame_rate() -> Result<ReplayFrameRate, Box<dyn Error>> {
    match env::var("REDUNAR_CAPTURE_PROBE_REPLAY_FPS").ok().as_deref() {
        None | Some("60") => Ok(ReplayFrameRate::Fps60),
        Some("30") => Ok(ReplayFrameRate::Fps30),
        Some("120") => Ok(ReplayFrameRate::Fps120),
        Some("variable") => Ok(ReplayFrameRate::Variable),
        Some(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "REDUNAR_CAPTURE_PROBE_REPLAY_FPS must be 30, 60, 120, or variable",
        )
        .into()),
    }
}

struct ProbeState {
    directory: PathBuf,
    runtime_root: PathBuf,
}

impl ProbeState {
    fn new(runtime_root: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(runtime_root)?;
        let directory = runtime_root.join(format!("game-catalog-{}", std::process::id()));
        std::fs::create_dir(&directory)?;
        Ok(Self {
            directory,
            runtime_root: runtime_root.to_path_buf(),
        })
    }
}

impl Drop for ProbeState {
    fn drop(&mut self) {
        if env::var("REDUNAR_CAPTURE_PROBE_KEEP_STATE").ok().as_deref() == Some("1") {
            eprintln!(
                "Redunar capture probe retained state at {}",
                self.directory.display()
            );
            return;
        }
        // The probe may create an encoded-replay directory below its isolated
        // state root. Remove the whole private tree so repeated diagnostics do
        // not accumulate clips under XDG_RUNTIME_DIR.
        let _ = std::fs::remove_dir_all(&self.directory);
        let _ = std::fs::remove_dir(&self.runtime_root);
        if self
            .runtime_root
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name.to_string_lossy().starts_with("redunar-steam-probe-"))
        {
            let _ = std::fs::remove_dir(self.runtime_root.parent().expect("checked parent"));
        }
    }
}

fn runtime_root() -> Result<PathBuf, Box<dyn Error>> {
    let root = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "XDG_RUNTIME_DIR is unavailable or not absolute",
            )
        })?;
    if env::var("REDUNAR_CAPTURE_PROBE_STEAM_BRIDGE")
        .ok()
        .as_deref()
        == Some("1")
    {
        Ok(root
            .join(format!("redunar-steam-probe-{}", std::process::id()))
            .join("redunar"))
    } else {
        Ok(root.join("redunar-capture-probe"))
    }
}

fn steam_wrapper() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(wrapper) = env::var_os("REDUNAR_CAPTURE_PROBE_STEAM_WRAPPER").map(PathBuf::from) {
        if wrapper.is_absolute() && wrapper.is_file() {
            return Ok(wrapper);
        }
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "REDUNAR_CAPTURE_PROBE_STEAM_WRAPPER must be an absolute executable file",
        )
        .into());
    }
    let executable = env::current_exe()?;
    let profile_directory = executable
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::other("capture probe has no build profile directory"))?;
    let wrapper = profile_directory.join("redunar-steam-launch");
    if wrapper.is_file() {
        Ok(wrapper)
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "build redunar-steam-launch first; no wrapper at {}",
                wrapper.display()
            ),
        )
        .into())
    }
}

fn capture_library() -> Result<PathBuf, Box<dyn Error>> {
    let executable = env::current_exe()?;
    let executable_directory = executable
        .parent()
        .ok_or_else(|| io::Error::other("capture probe has no executable directory"))?;
    let direct = executable_directory.join("libredunar_capture_vulkan.so");
    if direct.is_file() {
        return Ok(direct);
    }
    let profile_directory = executable_directory
        .parent()
        .ok_or_else(|| io::Error::other("capture probe has no build profile directory"))?;
    let profile_library = profile_directory.join("libredunar_capture_vulkan.so");
    if profile_library.is_file() {
        return Ok(profile_library);
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "build redunar-capture-vulkan first; no layer library at {} or {}",
            direct.display(),
            profile_library.display()
        ),
    )
    .into())
}

fn opengl_capture_library() -> Result<Option<PathBuf>, Box<dyn Error>> {
    let executable = env::current_exe()?;
    let executable_directory = executable
        .parent()
        .ok_or_else(|| io::Error::other("capture probe has no executable directory"))?;
    let direct = executable_directory.join("libredunar_capture_opengl.so");
    if direct.is_file() {
        return Ok(Some(direct));
    }
    let profile_directory = executable_directory
        .parent()
        .ok_or_else(|| io::Error::other("capture probe has no build profile directory"))?;
    let profile_library = profile_directory.join("libredunar_capture_opengl.so");
    Ok(profile_library.is_file().then_some(profile_library))
}

fn default_arguments(executable: &Path) -> Result<Vec<OsString>, Box<dyn Error>> {
    if executable
        .file_name()
        .is_some_and(|name| name == "vkcube" || name == "vkcubepp")
    {
        let frame_count = env::var("REDUNAR_CAPTURE_PROBE_FRAMES")
            .unwrap_or_else(|_| DEFAULT_FRAME_COUNT.to_owned());
        let parsed = frame_count.parse::<u32>().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "REDUNAR_CAPTURE_PROBE_FRAMES must be an integer",
            )
        })?;
        if !(2..=1_000_000).contains(&parsed) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "REDUNAR_CAPTURE_PROBE_FRAMES must be between 2 and 1000000",
            )
            .into());
        }
        // The default must exceed the renderer's bounded panel plus its two
        // safety margins. A smaller probe can validate capture but cannot
        // exercise any overlay pixels.
        let width = probe_dimension("REDUNAR_CAPTURE_PROBE_WIDTH", 640, 320, 3_840)?;
        let height = probe_dimension("REDUNAR_CAPTURE_PROBE_HEIGHT", 240, 180, 2_160)?;
        Ok([
            OsString::from("--c"),
            OsString::from(frame_count),
            OsString::from("--wsi"),
            probe_wsi()?,
            OsString::from("--width"),
            OsString::from(width.to_string()),
            OsString::from("--height"),
            OsString::from(height.to_string()),
        ]
        .into_iter()
        .collect::<Vec<_>>())
    } else {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::ProbeState;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn probe_state_removes_nested_runtime_outputs() {
        let root = std::env::temp_dir().join(format!(
            "redunar-capture-probe-cleanup-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let nested = root.join("game-catalog/encoded-replay");
        fs::create_dir_all(&nested).expect("nested probe output");
        fs::write(nested.join("clip.mkv"), b"fixture").expect("probe clip");

        drop(ProbeState {
            directory: root.clone(),
            runtime_root: root.clone(),
        });

        assert!(!root.exists());
    }
}

fn probe_wsi() -> io::Result<OsString> {
    match env::var_os("REDUNAR_CAPTURE_PROBE_WSI") {
        None => Ok(OsString::from("xcb")),
        Some(value) if value == "xcb" || value == "wayland" => Ok(value),
        Some(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "REDUNAR_CAPTURE_PROBE_WSI must be xcb or wayland",
        )),
    }
}

fn probe_dimension(name: &str, default: u32, minimum: u32, maximum: u32) -> io::Result<u32> {
    let value = match env::var(name) {
        Ok(value) => value.parse::<u32>().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{name} must be an integer"),
            )
        })?,
        Err(env::VarError::NotPresent) => default,
        Err(env::VarError::NotUnicode(_)) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{name} must be valid UTF-8"),
            ));
        }
    };
    if !(minimum..=maximum).contains(&value) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} must be between {minimum} and {maximum}"),
        ));
    }
    Ok(value)
}

fn child_timeout() -> Result<Duration, Box<dyn Error>> {
    let Some(value) = env::var_os("REDUNAR_CAPTURE_PROBE_TIMEOUT_SECONDS") else {
        return Ok(CHILD_TIMEOUT);
    };
    let seconds = value
        .to_str()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (1..=600).contains(value))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "REDUNAR_CAPTURE_PROBE_TIMEOUT_SECONDS must be between 1 and 600",
            )
        })?;
    Ok(Duration::from_secs(seconds))
}

fn wait_for_child(child: &mut Child, timeout: Duration) -> io::Result<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "capture target exceeded the diagnostic timeout",
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn wait_for_completion(
    session: &CaptureSessionHandle,
    timeout: Duration,
) -> std::sync::Arc<CaptureSnapshot> {
    let deadline = Instant::now() + timeout;
    loop {
        let snapshot = session.snapshot();
        if matches!(
            snapshot.phase,
            CapturePhase::Completed | CapturePhase::Failed
        ) || Instant::now() >= deadline
        {
            return snapshot;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[derive(Clone, Copy)]
struct ProcessSample {
    cpu_ticks: u64,
    resident_kib: u64,
}

fn process_sample() -> io::Result<ProcessSample> {
    let stat = std::fs::read_to_string("/proc/self/stat")?;
    let fields = stat
        .rsplit_once(')')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "malformed /proc/self/stat"))?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    let user_ticks = fields
        .get(11)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing process user ticks"))?;
    let system_ticks = fields
        .get(12)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "missing process system ticks")
        })?;
    let status = std::fs::read_to_string("/proc/self/status")?;
    let resident_kib = status
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing process RSS"))?;
    Ok(ProcessSample {
        cpu_ticks: user_ticks.saturating_add(system_ticks),
        resident_kib,
    })
}

fn print_snapshot(
    snapshot: &CaptureSnapshot,
    elapsed: Duration,
    start_process: ProcessSample,
    end_process: ProcessSample,
) {
    println!("phase={:?}", snapshot.phase);
    println!("producer_pid={:?}", snapshot.producer_process_id);
    println!("capture_api={:?}", snapshot.capture_api);
    println!("frames={}", snapshot.received_frame_count);
    println!("drops={}", snapshot.dropped_frame_count);
    println!("rejects={}", snapshot.rejected_message_count);
    println!("overlay_status={:?}", snapshot.overlay_status);
    println!("elapsed_ms={}", elapsed.as_millis());
    println!(
        "receiver_cpu_ticks={}",
        end_process
            .cpu_ticks
            .saturating_sub(start_process.cpu_ticks)
    );
    println!("receiver_rss_start_kib={}", start_process.resident_kib);
    println!("receiver_rss_end_kib={}", end_process.resident_kib);
    if let Some(metrics) = &snapshot.metrics {
        println!("average_fps={:.2}", metrics.average_fps);
        println!("one_percent_low_fps={:.2}", metrics.one_percent_low_fps);
        println!(
            "point_one_percent_low_fps={:.2}",
            metrics.point_one_percent_low_fps
        );
        println!("newest_frame_time_ms={:.3}", metrics.newest_frame_time_ms);
    }
    if let Some(candidate) = snapshot.replay_source_candidate {
        println!("replay_candidate_width={}", candidate.width);
        println!("replay_candidate_height={}", candidate.height);
        println!("replay_candidate_format={:?}", candidate.pixel_format);
        println!(
            "replay_candidate_target_fps={}",
            candidate.target_frames_per_second
        );
    } else {
        println!("replay_candidate=none");
    }
    if let Some(reason) = snapshot.replay_source_rejection {
        println!("replay_candidate_rejection={reason:?}");
    }
    println!(
        "replay_copied_frames={}",
        snapshot.replay_copied_frame_count
    );
    println!(
        "replay_exported_frames={}",
        snapshot.replay_exported_frame_count
    );
    if let Some(source) = snapshot.replay_latest_copy_source {
        println!("replay_latest_copy_width={}", source.width);
        println!("replay_latest_copy_height={}", source.height);
        println!("replay_latest_copy_format={:?}", source.pixel_format);
    }
    if let Some(bytes) = snapshot.replay_latest_copied_bytes {
        println!("replay_latest_copied_bytes={bytes}");
    }
    if let Some(checksum) = snapshot.replay_latest_sample_checksum {
        println!("replay_latest_sample_checksum={checksum:016x}");
    }
    if let Some(failure) = &snapshot.failure {
        println!("failure={failure}");
    }
}
