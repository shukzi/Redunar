//! Exercise the real packaged wrapper with private sockets and literal argv.
//! No graphics library, Steam process, game or host profile is opened.
use redunar_capture::CaptureSessionId;
use redunar_platform::{
    SteamActivationBroker, SteamAppId, SteamCaptureEnvironment, SteamSessionBroker,
    steam_session_socket_path,
};
use std::{
    fs,
    process::Command,
    sync::{Arc, Mutex},
    time::Duration,
};

#[test]
fn steam_play_requests_capture_before_exec_and_fail_open_preserves_literal_argv() {
    let runtime = std::env::temp_dir().join(format!("rdstplay-{}", std::process::id()));
    fs::create_dir(&runtime).unwrap();
    let private = runtime.join("redunar");
    fs::create_dir(&private).unwrap();
    let activation = Arc::new(Mutex::new(None));
    let owner = activation.clone();
    let capture_runtime = private.clone();
    let app = SteamAppId::new(42).unwrap();
    let session_id = CaptureSessionId::new([7; 16]).unwrap();
    let mut broker =
        SteamSessionBroker::bind(steam_session_socket_path(&runtime), move |id, _pid| {
            if id != app {
                return false;
            }
            let environment = SteamCaptureEnvironment::new(
                capture_runtime.join("layer"),
                capture_runtime.join("capture.sock"),
                capture_runtime.join("reply.sock"),
                session_id,
            )
            .unwrap();
            *owner.lock().unwrap() = Some(
                SteamActivationBroker::bind_in_runtime(
                    &capture_runtime,
                    id,
                    environment,
                    Duration::from_secs(10),
                )
                .unwrap(),
            );
            true
        })
        .unwrap();
    let wrapper = env!("CARGO_BIN_EXE_redunar-steam-launch");
    let result = Command::new(wrapper)
        .args([
            "--app-id",
            "42",
            "--",
            "/usr/bin/printenv",
            "REDUNAR_CAPTURE_SESSION",
        ])
        .env("XDG_RUNTIME_DIR", &runtime)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!result.stdout.is_empty());
    assert!(activation.lock().unwrap().as_ref().unwrap().claimed());

    let marker = runtime.join("must-not-exist");
    let literal = format!("$(touch {})", marker.display());
    let result = Command::new(wrapper)
        .args(["--app-id", "43", "--", "/usr/bin/printf", "%s", &literal])
        .env("XDG_RUNTIME_DIR", &runtime)
        .output()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, literal.as_bytes());
    assert!(!marker.exists());
    broker.shutdown();
    activation.lock().unwrap().take();
    fs::remove_dir_all(runtime).unwrap();
}
