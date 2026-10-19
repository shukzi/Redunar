use super::*;

#[test]
fn persistent_app_is_started_by_the_user_manager_with_literal_desktop_environment() {
    let executable = Path::new("/private/a space/$literal/redunar-tauri");
    let inherited = BTreeMap::from([
        ("DISPLAY".into(), ":1".into()),
        ("WAYLAND_DISPLAY".into(), "wayland-1".into()),
        ("XDG_SESSION_TYPE".into(), "wayland".into()),
        ("XDG_RUNTIME_DIR".into(), "/private/runtime".into()),
        (
            "PULSE_SERVER".into(),
            "unix:/private/audio with space".into(),
        ),
        (
            "GTK_THEME".into(),
            "$(touch /tmp/must-not-exist) ${HOME}".into(),
        ),
        ("LD_PRELOAD".into(), "/game/overlay.so".into()),
        ("PRIVATE_TOKEN".into(), "must-not-enter-argv".into()),
        ("LD_LIBRARY_PATH".into(), "/game/libraries".into()),
        ("VK_ADD_LAYER_PATH".into(), "/game/layers".into()),
        ("SteamAppId".into(), "42".into()),
        ("REDUNAR_CAPTURE_SESSION".into(), "game-owned".into()),
    ]);
    let command = startup_command(Path::new(USER_SERVICE_RUNNER), executable, inherited);
    assert_eq!(command.get_program(), USER_SERVICE_RUNNER);
    let arguments = command.get_args().collect::<Vec<_>>();
    assert!(arguments.contains(&OsStr::new("--user")));
    assert!(arguments.contains(&OsStr::new("--collect")));
    assert!(arguments.contains(&OsStr::new("--service-type=exec")));
    assert!(!arguments.contains(&OsStr::new("--scope")));
    assert!(!arguments.contains(&OsStr::new("--wait")));
    assert_eq!(
        &arguments[arguments.len() - 3..],
        [
            OsStr::new("--"),
            executable.as_os_str(),
            OsStr::new(STEAM_BACKGROUND_ARGUMENT)
        ]
    );
    assert!(arguments.contains(&OsStr::new(
        "--setenv=GTK_THEME=$(touch /tmp/must-not-exist) ${HOME}"
    )));
    let environment = command.get_envs().collect::<BTreeMap<_, _>>();
    for (key, value) in [
        ("WAYLAND_DISPLAY", "wayland-1"),
        ("XDG_SESSION_TYPE", "wayland"),
    ] {
        assert_eq!(
            environment.get(OsStr::new(key)),
            Some(&Some(OsStr::new(value)))
        );
        assert!(
            arguments
                .iter()
                .any(|argument| *argument == OsStr::new(&format!("--setenv={key}={value}")))
        );
    }
    assert_eq!(
        environment.get(OsStr::new("DISPLAY")),
        Some(&Some(OsStr::new(":1")))
    );
    assert_eq!(
        environment.get(OsStr::new("PULSE_SERVER")),
        Some(&Some(OsStr::new("unix:/private/audio with space")))
    );
    for key in [
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "VK_ADD_LAYER_PATH",
        "SteamAppId",
        "REDUNAR_CAPTURE_SESSION",
        "PRIVATE_TOKEN",
    ] {
        assert!(!environment.contains_key(OsStr::new(key)));
        assert!(!arguments.iter().any(|arg| {
            arg.to_string_lossy()
                .starts_with(&format!("--setenv={key}="))
        }));
    }
    assert!(arguments.iter().any(|arg| {
        arg.to_string_lossy()
            .starts_with("--property=UnsetEnvironment=")
            && arg.to_string_lossy().contains("REDUNAR_CAPTURE_SESSION")
    }));
}

#[test]
fn steam_startup_preserves_an_explicit_backend_override_without_inventing_one() {
    for value in [None, Some("x11"), Some("wayland,x11")] {
        let mut inherited = BTreeMap::from([
            (
                OsString::from("XDG_SESSION_TYPE"),
                OsString::from("wayland"),
            ),
            (
                OsString::from("WAYLAND_DISPLAY"),
                OsString::from("wayland-1"),
            ),
        ]);
        if let Some(value) = value {
            inherited.insert("GDK_BACKEND".into(), value.into());
        }
        let command = startup_command(
            Path::new(USER_SERVICE_RUNNER),
            Path::new("/fixture/redunar-tauri"),
            inherited,
        );
        let environment = command.get_envs().collect::<BTreeMap<_, _>>();
        assert_eq!(
            environment
                .get(OsStr::new("GDK_BACKEND"))
                .copied()
                .flatten(),
            value.map(OsStr::new)
        );
        let backend_arguments = command
            .get_args()
            .filter(|arg| arg.to_string_lossy().starts_with("--setenv=GDK_BACKEND="))
            .collect::<Vec<_>>();
        assert_eq!(backend_arguments.len(), usize::from(value.is_some()));
    }
}

#[test]
fn missing_user_manager_never_falls_back_to_a_persistent_steam_child() {
    let mut command = startup_command(
        Path::new("/private/missing-user-service-runner"),
        Path::new("/bin/sleep"),
        BTreeMap::new(),
    );
    assert!(command.spawn().is_err());
}
