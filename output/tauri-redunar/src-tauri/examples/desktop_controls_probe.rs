//! Native regressions in an isolated D-Bus/Xvfb session. Uses fixture preferences
//! and a pipe-only helper; never opens input devices or changes host settings.
#![allow(dead_code)]
#[path = "../src/background_start.rs"]
mod background_start;
#[path = "../src/hotkeys.rs"]
mod hotkeys;
#[path = "../src/tray.rs"]
mod tray;
#[path = "../src/tray_native/mod.rs"]
mod tray_native;

use std::{
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::Manager;

mod backend {
    pub fn service() -> &'static redunar_daemon::RedunarService {
        static SERVICE: std::sync::OnceLock<redunar_daemon::RedunarService> =
            std::sync::OnceLock::new();
        SERVICE.get_or_init(|| {
            redunar_daemon::RedunarService::with_state_directory(
                std::env::temp_dir().join(format!("redunar-controls-probe-{}", std::process::id())),
            )
        })
    }
    pub fn ensure_write_access() -> Result<(), String> {
        Ok(())
    }
}

static PASSED: AtomicBool = AtomicBool::new(false);

fn main() {
    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    let directory =
        std::env::temp_dir().join(format!("redunar-controls-probe-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let helper = directory.join("pipe-helper.py");
    std::fs::write(&helper, "#!/usr/bin/python3\nimport sys\nfor line in sys.stdin:\n if line.strip() == 'START': break\nprint('READY fixture', flush=True)\nsys.stdin.read()\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::env::set_var("REDUNAR_HOTKEY_HELPER", &helper);
    backend::service().set_close_to_tray_enabled(false).unwrap();
    let mut context = tauri::generate_context!();
    for window in &mut context.config_mut().app.windows {
        window.visible = false;
        window.url = tauri::WebviewUrl::External("about:blank".parse().unwrap());
    }
    let app = tauri::Builder::default()
        .manage(hotkeys::ShortcutMonitor::default())
        .setup(|app| {
            tray::setup(app)?;
            assert!(
                !tray::has_item(app.handle()),
                "disabled startup has no icon"
            );
            let monitor = app.state::<hotkeys::ShortcutMonitor>();
            monitor.set_app_handle(app.handle().clone());
            monitor.activate().unwrap();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                // Snapshot never drains events. No webview or status command is
                // running; READY must arrive through the native wake callback.
                wait_for(|| {
                    handle.state::<hotkeys::ShortcutMonitor>().snapshot().state == "Active"
                });
                let main = handle.get_webview_window("main").unwrap();
                assert!(!main.is_visible().unwrap());
                for _ in 0..3 {
                    tray::set_enabled(&handle, true).unwrap();
                    tray::set_enabled(&handle, true).unwrap();
                    assert!(tray::has_item(&handle));
                    main.hide().unwrap();
                    wait_for(|| !main.is_visible().unwrap());
                    tray::set_enabled(&handle, false).unwrap();
                    assert!(
                        !tray::has_item(&handle),
                        "disabled icon must be unregistered"
                    );
                    wait_for(|| main.is_visible().unwrap());
                }
                // The in-game menu is owned by the helper and the Vulkan layer;
                // the app only mirrors its open/close reports. No desktop menu
                // window may exist for any reason anymore.
                assert!(handle.get_webview_window("replay-menu").is_none());
                backend::service()
                    .set_replay_hotkeys(String::new(), Vec::new())
                    .unwrap();
                let monitor = handle.state::<hotkeys::ShortcutMonitor>();
                assert_eq!(monitor.activate().unwrap().state, "Inactive");
                monitor.shutdown();
                PASSED.store(true, Ordering::Release);
                handle.exit(0);
            });
            Ok(())
        })
        .build(context)
        .unwrap();
    app.run_return(|_, _| {});
    assert!(
        PASSED.load(Ordering::Acquire),
        "native probe did not complete"
    );
    std::fs::remove_dir_all(directory).unwrap();
    println!("PASS: hidden native shortcut dispatch, no legacy menu window, empty shortcuts, tray startup and three native register/unregister cycles");
}

fn wait_for(mut condition: impl FnMut() -> bool) {
    for _ in 0..100 {
        if condition() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("native condition did not settle");
}
