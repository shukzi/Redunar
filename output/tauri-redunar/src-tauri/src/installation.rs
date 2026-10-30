//! Read-only installation evidence, separate from saved profiles and runtime ownership.
use redunar_core::{GameMatchRule, GameRecord};
use redunar_platform::{GameDiscoverySource, SteamGameDiscovery};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationStatus {
    Installed,
    NotInstalled,
    Unknown,
}

impl InstallationStatus {
    pub fn require_installed(self) -> Result<(), String> {
        match self {
            Self::Installed => Ok(()),
            Self::NotInstalled => {
                Err("This game is not installed. Reinstall it before launching.".into())
            }
            Self::Unknown => Err(
                "Installation status could not be checked. Reload the library and try again."
                    .into(),
            ),
        }
    }
}

pub fn steam_id(game: &GameRecord) -> Option<u32> {
    game.match_rules.iter().find_map(|rule| match rule {
        GameMatchRule::SteamAppId(id) => Some(*id),
        _ => None,
    })
}

fn installed_steam_ids(home: Option<&Path>) -> Option<HashSet<u32>> {
    let games = SteamGameDiscovery::new(home?).discover().ok()?;
    Some(
        games
            .into_iter()
            .filter_map(|game| match game.source {
                GameDiscoverySource::Steam { app_id } => Some(app_id),
                _ => None,
            })
            .collect(),
    )
}

fn status(
    executable: &Path,
    app_id: Option<u32>,
    steam: Option<&HashSet<u32>>,
) -> InstallationStatus {
    if let Some(id) = app_id {
        return match steam {
            Some(ids) if ids.contains(&id) => InstallationStatus::Installed,
            Some(_) => InstallationStatus::NotInstalled,
            None => InstallationStatus::Unknown,
        };
    }
    match executable.metadata() {
        Ok(metadata) if metadata.is_file() => InstallationStatus::Installed,
        Ok(_) => InstallationStatus::NotInstalled,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            InstallationStatus::NotInstalled
        }
        Err(_) => InstallationStatus::Unknown,
    }
}

pub fn for_game(game: &GameRecord) -> InstallationStatus {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let id = steam_id(game);
    let steam = id.and_then(|_| installed_steam_ids(home.as_deref()));
    status(&game.launch.executable, id, steam.as_ref())
}

#[tauri::command]
pub async fn game_installation_statuses() -> Result<HashMap<String, InstallationStatus>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let catalog = crate::backend::service()
            .load_game_catalog()
            .map_err(|e| e.to_string())?;
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        // One bounded local scan per explicit refresh, never per monitoring tick.
        let steam = catalog
            .games
            .iter()
            .any(|game| steam_id(game).is_some())
            .then(|| installed_steam_ids(home.as_deref()))
            .flatten();
        Ok(catalog
            .games
            .iter()
            .map(|game| {
                (
                    game.id.get().to_string(),
                    status(&game.launch.executable, steam_id(game), steam.as_ref()),
                )
            })
            .collect())
    })
    .await
    .map_err(|_| "Installation worker unavailable".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_games_are_blocked_without_confusing_launchers_with_games() {
        let ids = HashSet::from([42]);
        assert_eq!(
            status(Path::new("/bin/sh"), Some(43), Some(&ids)),
            InstallationStatus::NotInstalled
        );
        assert_eq!(
            status(Path::new("/missing/steam"), Some(42), Some(&ids)),
            InstallationStatus::Installed
        );
        assert_eq!(
            status(Path::new("/bin/sh"), Some(42), None),
            InstallationStatus::Unknown
        );
        assert!(InstallationStatus::NotInstalled
            .require_installed()
            .is_err());
        assert!(InstallationStatus::Unknown.require_installed().is_err());
        assert!(InstallationStatus::Installed.require_installed().is_ok());
    }

    #[test]
    fn native_removal_and_reinstallation_are_observed() {
        let root =
            std::env::temp_dir().join(format!("redunar-install-native-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let exe = root.join("game");
        std::fs::write(&exe, b"fixture").unwrap();
        assert_eq!(status(&exe, None, None), InstallationStatus::Installed);
        std::fs::remove_file(&exe).unwrap();
        assert_eq!(status(&exe, None, None), InstallationStatus::NotInstalled);
        std::fs::write(&exe, b"reinstalled").unwrap();
        assert_eq!(status(&exe, None, None), InstallationStatus::Installed);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn steam_native_flatpak_and_extra_libraries_survive_uninstall_reinstall() {
        let home =
            std::env::temp_dir().join(format!("redunar-install-steam-{}", std::process::id()));
        for (folder, id) in [
            (".local/share/Steam", 42),
            (".var/app/com.valvesoftware.Steam/.local/share/Steam", 43),
            ("external", 44),
        ] {
            let root = home.join(folder);
            std::fs::create_dir_all(root.join("steamapps/common/Game")).unwrap();
            std::fs::write(root.join(format!("steamapps/appmanifest_{id}.acf")), format!(r#""AppState" {{ "appid" "{id}" "name" "Game" "StateFlags" "4" "installdir" "Game" }}"#)).unwrap();
        }
        std::fs::write(
            home.join(".local/share/Steam/steamapps/libraryfolders.vdf"),
            format!(
                r#""libraryfolders" {{ "1" {{ "path" "{}" }} }}"#,
                home.join("external").display()
            ),
        )
        .unwrap();
        assert_eq!(
            installed_steam_ids(Some(&home)).unwrap(),
            HashSet::from([42, 43, 44])
        );
        let manifest = home.join(".local/share/Steam/steamapps/appmanifest_42.acf");
        let saved = std::fs::read(&manifest).unwrap();
        std::fs::remove_file(&manifest).unwrap();
        assert_eq!(
            installed_steam_ids(Some(&home)).unwrap(),
            HashSet::from([43, 44])
        );
        std::fs::write(manifest, saved).unwrap();
        assert_eq!(
            installed_steam_ids(Some(&home)).unwrap(),
            HashSet::from([42, 43, 44])
        );
        assert!(installed_steam_ids(Some(Path::new("relative"))).is_none());
        std::fs::remove_dir_all(home).unwrap();
    }
}
