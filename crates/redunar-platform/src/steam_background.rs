//! Start the host app outside Steam's game reaper, using only the user manager.
use crate::steam_session_bridge::STEAM_BACKGROUND_ARGUMENT;
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    io,
    path::Path,
    process::{Child, Command, Stdio},
};

pub(crate) const USER_SERVICE_RUNNER: &str = "/usr/bin/systemd-run";
const GAME_ENVIRONMENT: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "VK_LAYER_PATH",
    "VK_ADD_LAYER_PATH",
    "VK_INSTANCE_LAYERS",
    "SteamAppId",
    "SteamGameId",
    "SteamOverlayGameId",
    "STEAM_COMPAT_APP_ID",
];

pub(crate) fn startup_command(
    runner: &Path,
    executable: &Path,
    inherited: BTreeMap<OsString, OsString>,
) -> Command {
    let mut command = Command::new(runner);
    command
        .args([
            "--user",
            "--collect",
            "--quiet",
            "--no-ask-password",
            "--service-type=exec",
            "--property=StandardInput=null",
            "--property=StandardOutput=null",
            "--property=StandardError=null",
        ])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut removed = GAME_ENVIRONMENT
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    for (name, value) in inherited {
        if game_environment(&name) {
            if let Some(name) = name.to_str().filter(|name| valid_environment_name(name))
                && !removed.iter().any(|existing| existing == name)
            {
                removed.push(name.to_owned());
            }
            continue;
        }
        if !desktop_environment(&name) {
            continue;
        }
        // A transient service doesn't inherit its caller's desktop environment.
        // Forward only desktop/audio settings literally. Forwarding all caller
        // variables as --setenv argv would expose unrelated secrets to /proc.
        let mut argument = OsString::from("--setenv=");
        argument.push(&name);
        argument.push("=");
        argument.push(&value);
        command.arg(argument).env(name, value);
    }
    command.arg(format!("--property=UnsetEnvironment={}", removed.join(" ")));
    // This must be a service, never --scope or a direct child fallback: Steam's
    // subreaper adopts orphan descendants even after process_group(0)/setsid.
    // The user manager is the app's parent; only the short launcher belongs to
    // Steam. Existing backend-owner arbitration handles concurrent starters.
    command
        .arg("--")
        .arg(executable)
        .arg(STEAM_BACKGROUND_ARGUMENT);
    command
}

fn game_environment(name: &OsStr) -> bool {
    GAME_ENVIRONMENT.iter().any(|key| name == OsStr::new(key))
        || name.to_str().is_some_and(|key| key.starts_with("REDUNAR_"))
}

fn valid_environment_name(name: &str) -> bool {
    name.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn desktop_environment(name: &OsStr) -> bool {
    const KEYS: &[&str] = &[
        "HOME",
        "USER",
        "LOGNAME",
        "PATH",
        "LANG",
        "LANGUAGE",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_RUNTIME_DIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
        "XDG_DATA_DIRS",
        "XDG_CONFIG_DIRS",
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_TYPE",
        "XDG_SESSION_DESKTOP",
        "GTK_THEME",
        "GDK_BACKEND",
        "GDK_SCALE",
        "GDK_DPI_SCALE",
        "PULSE_SERVER",
        "PULSE_COOKIE",
        "PIPEWIRE_REMOTE",
        "PIPEWIRE_RUNTIME_DIR",
    ];
    KEYS.iter().any(|key| name == OsStr::new(key))
        || name
            .to_str()
            .is_some_and(|key| key.starts_with("LC_") && valid_environment_name(key))
}

/// Own only the short service-manager client, never the persistent host app.
pub(crate) struct BackgroundStarter(pub(crate) Child);

impl BackgroundStarter {
    pub(crate) fn check(&mut self) -> io::Result<()> {
        if let Some(status) = self.0.try_wait()?
            && !status.success()
        {
            return Err(io::Error::other("background user service startup failed"));
        }
        Ok(())
    }
}

impl Drop for BackgroundStarter {
    fn drop(&mut self) {
        // Bound startup, error and cancellation cleanup. This PID is the client,
        // not the user-manager-owned app or an existing backend owner.
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[cfg(test)]
#[path = "steam_background_tests.rs"]
mod tests;
