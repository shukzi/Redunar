//! Real-wrapper preload decisions with private activation brokers and fake
//! commands. All library paths are absent fixture files; no GPU/game is opened.
use redunar_capture::CaptureSessionId;
use redunar_platform::{
    SteamActivationBroker, SteamAppId, SteamCaptureEnvironment, VULKAN_CAPTURE_LAYER_NAME,
};
use std::{
    ffi::{OsStr, OsString},
    fs,
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::symlink,
    },
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

struct Fixture {
    runtime: PathBuf,
    private: PathBuf,
    library: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let runtime = std::env::temp_dir().join(format!("rdstgl-{}-{name}", std::process::id()));
        fs::create_dir(&runtime).unwrap();
        let private = runtime.join("redunar");
        fs::create_dir(&private).unwrap();
        let library = private.join("libredunar_capture_opengl.so");
        Self {
            runtime,
            private,
            library,
        }
    }

    fn executable(&self, relative: impl AsRef<Path>) -> PathBuf {
        let path = self.runtime.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        symlink("/usr/bin/printenv", &path).unwrap();
        path
    }

    fn run(
        &self,
        executable: &Path,
        arguments: &[OsString],
        inherited: Option<&OsStr>,
        app_id: &str,
    ) -> Vec<u8> {
        let activate = app_id == "42";
        let session = CaptureSessionId::new([7; 16]).unwrap();
        let capture = SteamCaptureEnvironment::new(
            self.private.join("layer"),
            self.private.join("capture.sock"),
            self.private.join("reply.sock"),
            session,
        )
        .unwrap()
        .with_opengl_library(&self.library)
        .unwrap();
        let mut broker = SteamActivationBroker::bind_in_runtime(
            &self.private,
            SteamAppId::new(42).unwrap(),
            capture,
            Duration::from_secs(10),
        )
        .unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_redunar-steam-launch"));
        command
            .env_clear()
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .args(["--app-id", app_id, "--"])
            .arg(executable)
            .args(arguments)
            .args([
                "LD_PRELOAD",
                "VK_INSTANCE_LAYERS",
                "REDUNAR_CAPTURE_SESSION",
            ]);
        if let Some(inherited) = inherited {
            command.env("LD_PRELOAD", inherited);
        }
        let result = command.output().unwrap();
        assert_eq!(broker.claimed(), activate);
        broker.shutdown();
        // printenv fails for the missing capture fields on denied activation.
        assert_eq!(result.status.success(), activate);
        if activate {
            let fields = result
                .stdout
                .split(|byte| *byte == b'\n')
                .collect::<Vec<_>>();
            assert_eq!(fields[1], VULKAN_CAPTURE_LAYER_NAME.as_bytes());
            assert_eq!(fields[2], session.to_hex().as_bytes());
        }
        result.stdout
    }

    fn assert_preload(output: &[u8], expected: &OsStr) {
        assert_eq!(
            output.split(|byte| *byte == b'\n').next().unwrap(),
            expected.as_bytes()
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.runtime).unwrap();
    }
}

#[test]
fn native_steam_runtime_commands_keep_capture() {
    let fixture = Fixture::new("native");
    for relative in [
        "native-game",
        "proton/native-game",
        "steam-runtime/run.sh",
        "pressure-vessel/bin/pv-bwrap",
    ] {
        let executable = fixture.executable(relative);
        let output = fixture.run(&executable, &[], None, "42");
        Fixture::assert_preload(&output, fixture.library.as_os_str());
    }
}

#[test]
fn proton_preloads_are_filtered_across_exec_without_losing_vulkan_or_foreign_entries() {
    let fixture = Fixture::new("proton");
    let proton = fixture.executable("proton");
    let non_utf8_proton = fixture.executable(OsString::from_vec(b"disk-\xff/proton".to_vec()));
    let inherited = OsString::from_vec(
        b"/foreign-\xfe.so /old/libredunar_capture_opengl.so:/other.so".to_vec(),
    );
    let expected = OsString::from_vec(b"/foreign-\xfe.so:/other.so".to_vec());
    for executable in [&proton, &non_utf8_proton] {
        let output = fixture.run(executable, &[], Some(&inherited), "42");
        Fixture::assert_preload(&output, &expected);
    }
    let output = fixture.run(
        Path::new("/usr/bin/env"),
        &[non_utf8_proton.into_os_string()],
        Some(&inherited),
        "42",
    );
    Fixture::assert_preload(&output, &expected);
    for inherited in [None, Some(OsStr::new("/old/libredunar_capture_opengl.so"))] {
        let output = fixture.run(&proton, &[], inherited, "42");
        Fixture::assert_preload(&output, OsStr::new(""));
    }
}

#[test]
fn unavailable_or_malformed_activation_keeps_inherited_proton_preload() {
    let fixture = Fixture::new("denied");
    let proton = fixture.executable("proton");
    let inherited = OsStr::new("/old/libredunar_capture_opengl.so:/foreign.so");
    for app_id in ["invalid", "43"] {
        let output = fixture.run(&proton, &[], Some(inherited), app_id);
        Fixture::assert_preload(&output, inherited);
    }
}
