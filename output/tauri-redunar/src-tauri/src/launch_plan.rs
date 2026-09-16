//! Prepare a saved launch using production capability and profile rules.
//! No process starts until the session owner accepts this complete plan.
use redunar_core::{EffectiveGameProfile, GameId, GameRecord, Inheritable};
use redunar_daemon::{
    CaptureSessionConfig, CaptureSessionHandle, GameLaunchProcessOwnership, GameSessionRequest,
    RedunarService, ReplayRuntimeStatus, SteamAppId, SteamBridgeSetupStatus,
    DEFAULT_STEAM_ACTIVATION_TTL,
};
use std::{
    collections::BTreeMap,
    process::{Command, Stdio},
};

pub struct PreparedLaunch {
    pub command: Command,
    pub capture: Option<CaptureSessionHandle>,
    pub request: GameSessionRequest,
    pub profile: EffectiveGameProfile,
    pub ownership: GameLaunchProcessOwnership,
    pub replay: ReplayRuntimeStatus,
}

pub fn prepare(service: &RedunarService, id: GameId) -> Result<PreparedLaunch, String> {
    let catalog = service.load_game_catalog().map_err(|e| e.to_string())?;
    let mut game = catalog
        .games
        .into_iter()
        .find(|game| game.id == id)
        .ok_or("The saved game no longer exists. Reload the library.")?;
    // Recheck before creating capture state or starting any launcher.
    crate::installation::for_game(&game).require_installed()?;
    let requested = game.profile.resolve(catalog.global_profile);
    let modules = service.module_statuses();
    for (name, enabled, status) in [
        (
            "Frame metrics",
            requested.capture_metrics,
            &modules.frame_metrics,
        ),
        (
            "In-game metrics",
            requested.overlay_visible,
            &modules.in_game_overlay,
        ),
    ] {
        if enabled && !status.allows_runtime() {
            return Err(format!(
                "{name} is requested but its runtime is unavailable: {status:?}. Complete the local runtime setup before launching."
            ));
        }
    }
    let mut profile = service
        .effective_game_profile(id)
        .map_err(|e| e.to_string())?;
    let ownership = service.process_ownership_for_launch(&game.launch);
    if matches!(
        ownership,
        GameLaunchProcessOwnership::ForwardedSteam { app_id: None }
    ) {
        return Err("This Steam launch has no identifiable game ID. Review its saved launch settings before launching here.".into());
    }
    // Visibility is not runtime installation. Prepare the supported telemetry
    // path even with all displays/recording initially off, so it can be shown
    // later without injecting anything into an already running process.
    let supported_launch = match ownership {
            GameLaunchProcessOwnership::DirectChild => {
                service.capture_support_for_launch(&game.launch).is_supported()
            }
            GameLaunchProcessOwnership::ForwardedSteam { app_id } => app_id
                .and_then(SteamAppId::new)
                .is_some_and(|id| matches!(service.steam_bridge_setup_status(id),
                    SteamBridgeSetupStatus::Available(setup) if setup.configuration_status().is_configured())),
        };
    let overlay_available = modules.in_game_overlay.allows_runtime() && supported_launch;
    // The compatibility value stored by earlier catalog formats cannot disable
    // replay on capable launches. Unsupported launchers still avoid capture.
    profile.instant_replay = modules.instant_replay.allows_runtime() && supported_launch;
    game.profile.capture_metrics = Inheritable::Custom(profile.capture_metrics);
    game.profile.overlay_visible = Inheritable::Custom(profile.overlay_visible);
    game.profile.instant_replay = Inheritable::Custom(profile.instant_replay);
    let capture_requested = needs_capture_runtime(&profile, overlay_available);
    let replay = if profile.instant_replay {
        service.replay_runtime_status().map_err(|e| e.to_string())?
    } else {
        ReplayRuntimeStatus::unavailable(profile.replay)
    };
    if profile.instant_replay && !replay.backend_readiness.validation_allowed() {
        return Err("Instant Replay is unavailable for this runtime. Finish the recorder setup before launching.".into());
    }
    let (mut command, capture) = if capture_requested {
        if let GameLaunchProcessOwnership::ForwardedSteam {
            app_id: Some(app_id),
        } = ownership
        {
            let app_id = SteamAppId::new(app_id).ok_or("Invalid Steam game identifier")?;
            match service.steam_bridge_setup_status(app_id) {
                SteamBridgeSetupStatus::Available(setup)
                    if setup.configuration_status().is_configured() => {}
                SteamBridgeSetupStatus::Available(setup) => {
                    return Err(format!(
                        "Steam launch options are missing or unconfirmed. Set this game's Steam Launch Options to: {}",
                        setup.launch_options()
                    ));
                }
                SteamBridgeSetupStatus::Unavailable(reason) => {
                    return Err(format!("Steam capture setup is unavailable: {reason}"));
                }
            }
        } else if let Some(reason) = service.capture_support_for_launch(&game.launch).reason() {
            return Err(format!(
                "{reason}. Disable the requested capture features or use a supported launch."
            ));
        }
        let config = CaptureSessionConfig::for_current_build().map_err(|e| e.to_string())?;
        let mut capture = service
            .start_capture_session(&config)
            .map_err(|e| e.to_string())?;
        let command = match ownership {
            GameLaunchProcessOwnership::DirectChild => {
                let inherited = std::env::vars_os().collect::<BTreeMap<_, _>>();
                capture
                    .game_launch_plan_for_profile(&game, catalog.global_profile, &inherited)
                    .map_err(|e| e.to_string())?
                    .command()
            }
            GameLaunchProcessOwnership::ForwardedSteam { .. } => {
                capture
                    .publish_steam_activation_for_profile(
                        &game,
                        catalog.global_profile,
                        DEFAULT_STEAM_ACTIVATION_TTL,
                    )
                    .map_err(|e| e.to_string())?;
                direct_command(&game)
            }
        };
        (command, Some(capture))
    } else {
        (direct_command(&game), None)
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(PreparedLaunch {
        command,
        capture,
        request: GameSessionRequest {
            game_id: Some(id),
            game_name: game.display_name,
            metrics: profile.capture_metrics.into(),
            overlay: profile.overlay_visible.into(),
            replay: profile.instant_replay.into(),
        },
        profile,
        ownership,
        replay,
    })
}

fn needs_capture_runtime(profile: &EffectiveGameProfile, overlay_available: bool) -> bool {
    profile.capture_metrics
        || profile.overlay_visible
        || profile.instant_replay
        || overlay_available
}

fn direct_command(game: &GameRecord) -> Command {
    let mut command = Command::new(&game.launch.executable);
    command.args(&game.launch.arguments);
    if let Some(directory) = &game.launch.working_directory {
        command.current_dir(directory);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn hidden_overlay_still_prepares_runtime_without_enabling_recording() {
        let profile =
            redunar_core::PerGameProfile::default().resolve(redunar_core::GlobalGameProfile {
                capture_metrics: false,
                overlay_visible: false,
                instant_replay: false,
                ..Default::default()
            });
        assert!(needs_capture_runtime(&profile, true));
        assert!(!profile.capture_metrics && !profile.instant_replay && !profile.overlay_visible);
        assert!(
            !needs_capture_runtime(&profile, false),
            "unsupported all-off launches remain possible"
        );
        let mut shown = profile;
        shown.overlay_visible = true;
        assert!(needs_capture_runtime(&shown, true));
        shown.overlay_visible = false;
        assert!(
            needs_capture_runtime(&shown, true),
            "hiding never removes runtime preparation"
        );
    }

    #[test]
    fn uninstalled_game_is_rejected_before_capture_preparation_and_keeps_profile() {
        let root =
            std::env::temp_dir().join(format!("redunar-uninstalled-launch-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = root.join("game");
        std::fs::write(&executable, b"fixture executable").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let service = RedunarService::with_state_directory(root.join("state"));
        let game = service
            .add_game(redunar_daemon::AddGameRequest::new(
                "Saved game",
                &executable,
            ))
            .unwrap();
        let saved = service.load_game_catalog().unwrap();
        std::fs::remove_file(executable).unwrap();
        let error = match prepare(&service, game.id) {
            Ok(_) => panic!("missing game must not prepare a launch"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            "This game is not installed. Reinstall it before launching."
        );
        assert_eq!(service.load_game_catalog().unwrap(), saved);
        drop(service);
        std::fs::remove_dir_all(root).unwrap();
    }
}
