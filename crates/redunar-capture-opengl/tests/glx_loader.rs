//! Real preload/ELF lookups with fake GLX providers; no display or GPU access.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::{fs, path::PathBuf, process::Command};

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn glx_handle_lookups_preserve_the_exact_provider_and_loader_errors() {
    let fixture =
        Fixture(std::env::temp_dir().join(format!("redunar-glx-loader-{}", std::process::id())));
    fs::create_dir(&fixture.0).unwrap();
    let provider_source = fixture.0.join("provider.c");
    let caller_source = fixture.0.join("caller.c");
    fs::write(&provider_source, include_str!("fixtures/glx_provider.c")).unwrap();
    fs::write(&caller_source, include_str!("fixtures/glx_caller.c")).unwrap();
    let provider = fixture.0.join("provider.so");
    let other_provider = fixture.0.join("other-provider.so");
    let caller = fixture.0.join("caller");
    for output in [&provider, &other_provider] {
        let result = Command::new("cc")
            .args(["-shared", "-fPIC"])
            .arg(&provider_source)
            .arg("-o")
            .arg(output)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let result = Command::new("cc")
        .arg(&caller_source)
        .args(["-ldl", "-o"])
        .arg(&caller)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let test_binary = std::env::current_exe().unwrap();
    let sidecar = test_binary
        .parent()
        .unwrap()
        .join("libredunar_capture_opengl.so");
    assert!(
        sidecar.is_file(),
        "Cargo must build the actual preload sidecar"
    );
    for mode in [
        "local",
        "global",
        "other-provider",
        "promoted",
        "cached-miss",
    ] {
        let result = Command::new("timeout")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .args(["--kill-after=2s", "10s", "env"])
            .arg(format!("LD_PRELOAD={}", sidecar.display()))
            .arg(&caller)
            .arg(mode)
            .arg(&provider)
            .arg(&other_provider)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{mode}: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
