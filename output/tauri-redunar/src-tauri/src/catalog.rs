use redunar_core::{GameId, GameMatchRule, Inheritable, PerGameProfile};
use serde::Serialize;
use std::collections::HashMap;

/// Open the platform's native file chooser for a local executable.
///
/// The picker only chooses a path. The existing catalog command remains the
/// authority that validates the file and records it.
#[tauri::command]
pub fn pick_executable() -> Result<Option<String>, String> {
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;

        let dialog = gtk::FileChooserNative::new(
            Some("Choose a game executable"),
            None::<&gtk::Window>,
            gtk::FileChooserAction::Open,
            Some("Choose"),
            Some("Cancel"),
        );
        dialog.set_select_multiple(false);
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Executable files"));
        filter.add_pattern("*");
        dialog.add_filter(filter);

        let selected = if dialog.run() == gtk::ResponseType::Accept {
            dialog
                .filename()
                .filter(|path| path.is_file())
                .map(|path| path.to_string_lossy().into_owned())
        } else {
            None
        };
        dialog.destroy();
        Ok(selected)
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("The native executable picker is currently available on Linux only".into())
    }
}

#[derive(Serialize)]
pub struct CatalogGame {
    id: String,
    name: String,
    executable: String,
    arguments: Vec<String>,
    working_directory: Option<String>,
    steam_app_id: Option<u32>,
    overrides: HashMap<String, bool>,
    revision: String,
}

#[derive(Serialize)]
pub struct DiscoveredGameDto {
    name: String,
    install_directory: String,
    launch_executable: Option<String>,
    source: String,
    source_id: Option<u32>,
    importable: bool,
}

#[derive(Serialize)]
pub struct SteamSetupDto {
    app_id: Option<u32>,
    available: bool,
    configured: bool,
    configuration_state: String,
    status: String,
    launch_options: Option<String>,
}

fn steam_configuration_state(status: &redunar_daemon::SteamLaunchOptionsStatus) -> &'static str {
    match status {
        redunar_daemon::SteamLaunchOptionsStatus::Configured => "configured",
        redunar_daemon::SteamLaunchOptionsStatus::NotConfigured => "not-configured",
        redunar_daemon::SteamLaunchOptionsStatus::Ambiguous(_) => "needs-attention",
        redunar_daemon::SteamLaunchOptionsStatus::Unavailable(_) => "unavailable",
    }
}

fn discovered_game_dto(game: &redunar_daemon::DiscoveredGame) -> DiscoveredGameDto {
    let (source, source_id) = match &game.source {
        redunar_daemon::GameDiscoverySource::Steam { app_id } => ("Steam".into(), Some(*app_id)),
        redunar_daemon::GameDiscoverySource::DesktopEntry { .. } => ("Desktop entry".into(), None),
        redunar_daemon::GameDiscoverySource::RunningProcess { pid } => {
            ("Running process".into(), Some(*pid))
        }
    };
    DiscoveredGameDto {
        name: game.display_name.clone(),
        install_directory: game.install_directory.to_string_lossy().into_owned(),
        launch_executable: game
            .launch
            .as_ref()
            .map(|launch| launch.executable.to_string_lossy().into_owned()),
        source,
        source_id,
        importable: game.launch.is_some(),
    }
}

#[tauri::command]
pub fn discover_games() -> Result<Vec<DiscoveredGameDto>, String> {
    crate::backend::service()
        .discover_installed_games()
        .map(|games| games.iter().map(discovered_game_dto).collect())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn import_discovered_games(indices: Vec<usize>) -> Result<Vec<CatalogGame>, String> {
    crate::backend::ensure_write_access()?;
    if indices.is_empty() {
        return Err("Select at least one discovered game to import".into());
    }
    let candidates = crate::backend::service()
        .discover_installed_games()
        .map_err(|error| error.to_string())?;
    let selected = indices
        .into_iter()
        .map(|index| {
            candidates.get(index).cloned().ok_or_else(|| {
                "A discovery result changed. Scan again before importing.".to_string()
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    crate::backend::service()
        .import_discovered_games(selected)
        .map_err(|error| error.to_string())?;
    catalog_games()
}

fn active_profile_overrides(profile: PerGameProfile) -> HashMap<String, bool> {
    let mut overrides = HashMap::new();
    for (key, value) in [
        ("overlay", profile.overlay_visible),
        ("captureMetrics", profile.capture_metrics),
    ] {
        if let Inheritable::Custom(value) = value {
            overrides.insert(key.into(), value);
        }
    }
    overrides
}

#[tauri::command]
pub fn catalog_games() -> Result<Vec<CatalogGame>, String> {
    let catalog = crate::backend::service()
        .load_game_catalog()
        .map_err(|e| e.to_string())?;
    Ok(catalog
        .games
        .into_iter()
        .map(|game| CatalogGame {
            id: game.id.get().to_string(),
            name: game.display_name,
            executable: game.launch.executable.to_string_lossy().into_owned(),
            arguments: game
                .launch
                .arguments
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            working_directory: game
                .launch
                .working_directory
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            steam_app_id: game.match_rules.iter().find_map(|rule| match rule {
                GameMatchRule::SteamAppId(app_id) => Some(*app_id),
                _ => None,
            }),
            overrides: active_profile_overrides(game.profile),
            revision: format!("{:?}", game.profile),
        })
        .collect())
}

fn profile_patch(
    current: PerGameProfile,
    values: &serde_json::Value,
) -> Result<PerGameProfile, String> {
    let object = values.as_object().ok_or("Expected game overrides")?;
    // Null explicitly restores inheritance; omission preserves an existing setting.
    // Unknown keys must not silently produce a successful but ineffective save.
    for key in object.keys() {
        if !["overlay", "replay", "captureMetrics"].contains(&key.as_str()) {
            return Err(format!("Unsupported game override: {key}"));
        }
    }
    let read = |key: &str, old| -> Result<Inheritable<bool>, String> {
        match object.get(key) {
            None => Ok(old),
            Some(serde_json::Value::Null) => Ok(Inheritable::InheritGlobal),
            Some(serde_json::Value::Bool(value)) => Ok(Inheritable::Custom(*value)),
            _ => Err(format!("Invalid value for {key}")),
        }
    };
    Ok(PerGameProfile {
        capture_metrics: read("captureMetrics", current.capture_metrics)?,
        overlay_visible: read("overlay", current.overlay_visible)?,
        // Launch preparation ignores this retired override. Preserve its stored
        // value so a visibility edit cannot trip the live recording lock.
        instant_replay: current.instant_replay,
    })
}

#[derive(Serialize)]
pub struct GameProfileSave {
    games: Vec<CatalogGame>,
    #[serde(rename = "liveNotice")]
    live_notice: String,
}

#[tauri::command]
pub fn save_game_profile(
    game_id: String,
    values: serde_json::Value,
    expected_revision: String,
    sessions: tauri::State<'_, crate::sessions::Sessions>,
) -> Result<GameProfileSave, String> {
    crate::backend::ensure_write_access()?;
    let service = crate::backend::service();
    let id = GameId::new(game_id.parse().map_err(|_| "Invalid game identifier")?)
        .map_err(|e| e.to_string())?;
    let current = service
        .load_game_catalog()
        .map_err(|e| e.to_string())?
        .games
        .into_iter()
        .find(|g| g.id == id)
        .ok_or("The game no longer exists")?;
    if format!("{:?}", current.profile) != expected_revision {
        return Err("Game settings changed elsewhere. Reload the library before saving.".into());
    }
    service
        .update_game_profile(id, profile_patch(current.profile, &values)?)
        .map_err(|e| e.to_string())?;
    let live_notice = crate::profiles::overlay_save_notice(
        "Game settings",
        sessions.update_game_overlay_config(id),
    );
    Ok(GameProfileSave {
        games: catalog_games()?,
        live_notice,
    })
}

#[tauri::command]
pub fn add_game(name: String, executable: String) -> Result<Vec<CatalogGame>, String> {
    crate::backend::ensure_write_access()?;
    crate::backend::service()
        .add_game(redunar_daemon::AddGameRequest::new(name, executable))
        .map_err(|e| e.to_string())?;
    catalog_games()
}

#[tauri::command]
pub fn update_game_launch(
    game_id: String,
    executable: String,
    arguments: Vec<String>,
    working_directory: Option<String>,
) -> Result<Vec<CatalogGame>, String> {
    crate::backend::ensure_write_access()?;
    let id = GameId::new(game_id.parse().map_err(|_| "Invalid game identifier")?)
        .map_err(|e| e.to_string())?;
    let launch = redunar_core::GameLaunchConfig {
        executable: executable.into(),
        arguments: arguments.into_iter().map(Into::into).collect(),
        working_directory: working_directory.map(Into::into),
    };
    crate::backend::service()
        .update_game_launch(id, launch)
        .map_err(|e| e.to_string())?;
    catalog_games()
}

#[tauri::command]
pub fn remove_game(game_id: String) -> Result<Vec<CatalogGame>, String> {
    crate::backend::ensure_write_access()?;
    let id = GameId::new(game_id.parse().map_err(|_| "Invalid game identifier")?)
        .map_err(|e| e.to_string())?;
    let app_id = crate::backend::service()
        .load_game_catalog()
        .map_err(|e| e.to_string())?
        .games
        .iter()
        .find(|game| game.id == id)
        .and_then(crate::installation::steam_id);
    crate::backend::service()
        .remove_game(id)
        .map_err(|e| e.to_string())?;
    if let Some(app_id) = app_id {
        crate::artwork::remove_unused(app_id);
    }
    catalog_games()
}

#[tauri::command]
pub fn steam_setup_status(game_id: String) -> Result<SteamSetupDto, String> {
    let id = GameId::new(game_id.parse().map_err(|_| "Invalid game identifier")?)
        .map_err(|e| e.to_string())?;
    let game = crate::backend::service()
        .load_game_catalog()
        .map_err(|e| e.to_string())?
        .games
        .into_iter()
        .find(|game| game.id == id)
        .ok_or("The selected game no longer exists")?;
    let Some(app_id) = game.match_rules.iter().find_map(|rule| match rule {
        GameMatchRule::SteamAppId(app_id) => Some(*app_id),
        _ => None,
    }) else {
        return Ok(SteamSetupDto {
            app_id: None,
            available: false,
            configured: false,
            configuration_state: "unavailable".into(),
            status: "This game has no Steam app ID match".into(),
            launch_options: None,
        });
    };
    let steam_id = redunar_daemon::SteamAppId::new(app_id).ok_or("Invalid Steam app identifier")?;
    match crate::backend::service().steam_bridge_setup_status(steam_id) {
        redunar_daemon::SteamBridgeSetupStatus::Available(setup) => Ok(SteamSetupDto {
            app_id: Some(app_id),
            available: true,
            configured: setup.configuration_status().is_configured(),
            configuration_state: steam_configuration_state(setup.configuration_status()).into(),
            status: setup.configuration_status().to_string(),
            launch_options: Some(setup.launch_options().to_owned()),
        }),
        redunar_daemon::SteamBridgeSetupStatus::Unavailable(reason) => Ok(SteamSetupDto {
            app_id: Some(app_id),
            available: false,
            configured: false,
            configuration_state: "unavailable".into(),
            status: reason,
            launch_options: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reset_inheritance_preserves_other_settings() {
        let current = PerGameProfile {
            overlay_visible: Inheritable::Custom(false),
            instant_replay: Inheritable::Custom(false),
            ..Default::default()
        };
        let result =
            profile_patch(current, &serde_json::json!({"overlay":null,"replay":false})).unwrap();
        assert_eq!(result.overlay_visible, Inheritable::InheritGlobal);
        assert_eq!(result.instant_replay, current.instant_replay);
    }
    #[test]
    fn rejects_unimplemented_or_malformed_overrides() {
        for values in [
            serde_json::json!({"quality":"High"}),
            serde_json::json!({"overlay":"false"}),
            serde_json::json!({"retiredFeature":true}),
        ] {
            assert!(profile_patch(Default::default(), &values).is_err());
        }
    }

    #[test]
    fn active_catalog_surface_omits_retired_overrides() {
        let profile = PerGameProfile {
            overlay_visible: Inheritable::Custom(true),
            instant_replay: Inheritable::Custom(false),
            ..Default::default()
        };
        let overrides = active_profile_overrides(profile);
        assert_eq!(overrides.get("overlay"), Some(&true));
        assert!(!overrides.contains_key("replay"));
    }

    #[test]
    fn steam_configuration_state_distinguishes_actionable_and_blocked_setup() {
        use redunar_daemon::{SteamLaunchOptionsReason, SteamLaunchOptionsStatus};

        assert_eq!(
            steam_configuration_state(&SteamLaunchOptionsStatus::Configured),
            "configured"
        );
        assert_eq!(
            steam_configuration_state(&SteamLaunchOptionsStatus::NotConfigured),
            "not-configured"
        );
        assert_eq!(
            steam_configuration_state(&SteamLaunchOptionsStatus::Ambiguous(
                SteamLaunchOptionsReason::SteamRunning
            )),
            "needs-attention"
        );
        assert_eq!(
            steam_configuration_state(&SteamLaunchOptionsStatus::Unavailable(
                SteamLaunchOptionsReason::NativeSteamNotFound
            )),
            "unavailable"
        );
    }
}
