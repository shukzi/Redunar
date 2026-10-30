use redunar_platform::{
    SteamAppId, ensure_steam_session, resolve_steam_wrapper_environment, steam_broker_socket_path,
};
use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let Some(invocation) = Invocation::parse(env::args_os().skip(1)) else {
        eprintln!("usage: redunar-steam-launch --app-id N -- command [arguments...]");
        return ExitCode::from(64);
    };

    let mut managed = BTreeMap::new();
    if let (Some(app_id), Some(socket_path)) = (invocation.app_id, broker_socket_path()) {
        let inherited = env::vars_os().collect::<BTreeMap<OsString, OsString>>();
        managed = resolve_steam_wrapper_environment(app_id, &socket_path, &inherited);
        if managed.is_empty()
            && let Some((runtime, app)) = background_app_paths()
        {
            match ensure_steam_session(app_id, &runtime, &app) {
                Ok(true) => {
                    managed = resolve_steam_wrapper_environment(app_id, &socket_path, &inherited);
                }
                Ok(false) => {
                    eprintln!(
                        "redunar-steam-launch: session unavailable or another game is active"
                    );
                }
                Err(_) => eprintln!(
                    "redunar-steam-launch: background session unavailable; continuing game launch"
                ),
            }
        }
    }
    if is_proton_invocation(&invocation) {
        remove_opengl_preload(&mut managed);
    }
    let managed_value = |name: &str| managed.get(&OsString::from(name)).cloned();
    let use_gamescope = managed_value("REDUNAR_GAMESCOPE")
        .or_else(|| env::var_os("REDUNAR_GAMESCOPE"))
        .and_then(|value| value.to_str().map(|value| value == "1"))
        .unwrap_or(false)
        && std::process::Command::new("gamescope")
            .arg("--help")
            .output()
            .is_ok();
    let use_gamemode = managed_value("REDUNAR_GAMEMODE")
        .or_else(|| env::var_os("REDUNAR_GAMEMODE"))
        .is_some_and(|value| value == "1")
        && std::process::Command::new("gamemoderun")
            .arg("--version")
            .output()
            .is_ok();
    let mut command = if use_gamescope {
        let mut command = Command::new("gamescope");
        command.arg("-f");
        command
            .arg("--")
            .arg(&invocation.command)
            .args(&invocation.arguments);
        command
    } else if use_gamemode {
        let mut command = Command::new("gamemoderun");
        command.arg(&invocation.command).args(&invocation.arguments);
        command
    } else {
        let mut command = Command::new(&invocation.command);
        command.args(&invocation.arguments);
        command
    };

    for (name, value) in managed {
        command.env(name, value);
    }
    let error = command.exec();
    eprintln!("redunar-steam-launch could not start the original game command: {error}");
    ExitCode::from(if error.kind() == std::io::ErrorKind::NotFound {
        127
    } else {
        126
    })
}

fn is_proton_invocation(invocation: &Invocation) -> bool {
    // Steam Runtime and pressure-vessel also launch native games. Only a
    // Proton interpreter in the literal argv disables the GL interposer.
    std::iter::once(&invocation.command)
        .chain(invocation.arguments.iter())
        .any(|value| {
            Path::new(value)
                .file_name()
                .is_some_and(|name| name.as_bytes().eq_ignore_ascii_case(b"proton"))
        })
}

fn remove_opengl_preload(managed: &mut BTreeMap<OsString, OsString>) {
    let Some(preload) = managed.get(OsStr::new("LD_PRELOAD")).cloned() else {
        return;
    };
    let mut retained = OsString::new();
    // The Linux loader accepts both spaces and colons, including in inherited
    // entries. Keep path bytes intact and remove only Redunar's GL library.
    for entry in preload
        .as_bytes()
        .split(|byte| *byte == b':' || *byte == b' ')
        .filter(|entry| !entry.is_empty())
    {
        let entry = OsStr::from_bytes(entry);
        if Path::new(entry).file_name() == Some(OsStr::new("libredunar_capture_opengl.so")) {
            continue;
        }
        if !retained.is_empty() {
            retained.push(":");
        }
        retained.push(entry);
    }
    // An empty override is intentional: dropping the update would restore
    // the child's inherited preload, including a different Redunar copy.
    managed.insert(OsString::from("LD_PRELOAD"), retained);
}

fn broker_socket_path() -> Option<PathBuf> {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|path| steam_broker_socket_path(&path))
}

fn background_app_paths() -> Option<(PathBuf, PathBuf)> {
    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())?;
    let executable = env::current_exe().ok()?;
    Some((runtime, executable.parent()?.join("redunar-tauri")))
}

struct Invocation {
    app_id: Option<SteamAppId>,
    command: OsString,
    arguments: Vec<OsString>,
}

impl Invocation {
    fn parse(arguments: impl IntoIterator<Item = OsString>) -> Option<Self> {
        let arguments = arguments.into_iter().collect::<Vec<_>>();
        let separator = arguments
            .iter()
            .position(|argument| argument == OsStr::new("--"))?;
        let command = arguments.get(separator.checked_add(1)?)?.clone();
        let command_arguments = arguments.get(separator + 2..)?.to_vec();

        // A malformed Redunar prefix must not reinterpret or shell-parse the
        // Steam-provided command. It simply disables activation and execs the
        // argv following `--` unchanged.
        let app_id = match arguments.get(..separator) {
            Some([option, value]) if option == OsStr::new("--app-id") => value
                .to_str()
                .and_then(|value| value.parse::<u32>().ok())
                .and_then(SteamAppId::new),
            _ => None,
        };

        Some(Self {
            app_id,
            command,
            arguments: command_arguments,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    #[test]
    fn parser_preserves_non_utf8_steam_argv_without_shell_processing() {
        let command = OsString::from_vec(b"/games/non-utf8-\xff".to_vec());
        let literal = OsString::from("$(touch /tmp/redunar-wrapper-must-not-run)");
        let non_utf8 = OsString::from_vec(vec![b'-', b'-', 0xfe]);
        let invocation = Invocation::parse([
            OsString::from("--app-id"),
            OsString::from("1808500"),
            OsString::from("--"),
            command.clone(),
            literal.clone(),
            non_utf8.clone(),
        ])
        .expect("valid wrapper invocation");

        assert_eq!(invocation.app_id, SteamAppId::new(1_808_500));
        assert_eq!(invocation.command.as_bytes(), command.as_bytes());
        assert_eq!(invocation.arguments, [literal, non_utf8]);
    }

    #[test]
    fn malformed_prefix_fails_open_to_the_original_argv() {
        let invocation = Invocation::parse([
            OsString::from("--app-id"),
            OsString::from("not-a-number"),
            OsString::from("--"),
            OsString::from("/games/example"),
            OsString::from("--literal"),
        ])
        .expect("original command remains available");

        assert_eq!(invocation.app_id, None);
        assert_eq!(invocation.command, OsStr::new("/games/example"));
        assert_eq!(invocation.arguments, [OsString::from("--literal")]);
    }

    #[test]
    fn missing_original_command_is_rejected() {
        assert!(
            Invocation::parse([
                OsString::from("--app-id"),
                OsString::from("12"),
                OsString::from("--"),
            ])
            .is_none()
        );
    }

    #[test]
    fn proton_commands_disable_only_redunar_opengl_preload() {
        let invocation = Invocation {
            app_id: None,
            command: OsString::from("/steam/compatibilitytools.d/Proton/proton"),
            arguments: Vec::new(),
        };
        assert!(is_proton_invocation(&invocation));
        let mut managed = [(
            OsString::from("LD_PRELOAD"),
            OsString::from("/run/redunar/libredunar_capture_opengl.so:/usr/lib/game.so"),
        )]
        .into_iter()
        .collect();
        remove_opengl_preload(&mut managed);
        assert_eq!(
            managed.get(OsStr::new("LD_PRELOAD")),
            Some(&OsString::from("/usr/lib/game.so"))
        );
    }

    #[test]
    fn native_commands_keep_redunar_opengl_preload() {
        let invocation = Invocation {
            app_id: None,
            command: OsString::from("/games/native-game"),
            arguments: Vec::new(),
        };
        assert!(!is_proton_invocation(&invocation));
    }

    #[test]
    fn native_runtime_commands_are_not_proton() {
        for command in [
            "/steam/ubuntu12_32/steam-runtime/run.sh",
            "/steam/SteamLinuxRuntime_sniper/pressure-vessel/bin/pv-bwrap",
        ] {
            let invocation = Invocation {
                app_id: None,
                command: OsString::from(command),
                arguments: vec![OsString::from("/games/native-game")],
            };
            assert!(!is_proton_invocation(&invocation));
        }
    }

    #[test]
    fn nested_non_utf8_proton_paths_are_recognized() {
        let proton = OsString::from_vec(b"/steam/disk-\xff/Proton/proton".to_vec());
        for invocation in [
            Invocation {
                app_id: None,
                command: proton.clone(),
                arguments: Vec::new(),
            },
            Invocation {
                app_id: None,
                command: OsString::from("/steam/steam-launch-wrapper"),
                arguments: vec![OsString::from("--"), proton],
            },
        ] {
            assert!(is_proton_invocation(&invocation));
        }
    }

    #[test]
    fn preload_filter_preserves_foreign_bytes_and_clears_redunar_copies() {
        let mut managed = [(
            OsString::from("LD_PRELOAD"),
            OsString::from_vec(
                b"/new/libredunar_capture_opengl.so:/game-\xff.so /old/libredunar_capture_opengl.so:/other.so".to_vec(),
            ),
        )]
        .into_iter()
        .collect();
        remove_opengl_preload(&mut managed);
        assert_eq!(
            managed.get(OsStr::new("LD_PRELOAD")),
            Some(&OsString::from_vec(b"/game-\xff.so:/other.so".to_vec()))
        );
        managed.insert(
            OsString::from("LD_PRELOAD"),
            OsString::from("/new/libredunar_capture_opengl.so /old/libredunar_capture_opengl.so"),
        );
        remove_opengl_preload(&mut managed);
        assert_eq!(
            managed.get(OsStr::new("LD_PRELOAD")),
            Some(&OsString::new())
        );
    }
}
