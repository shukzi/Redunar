//! Native regressions in an isolated D-Bus/Xvfb session. Uses fixture preferences
//! and a pipe-only helper; never opens input devices or changes host settings.
#![allow(dead_code)]
#[path = "../src/hotkeys.rs"]
mod hotkeys;
#[path = "../src/replay_menu_window.rs"]
mod replay_menu_window;
#[path = "../src/tray.rs"]
mod tray;

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
                app.tray_by_id("redunar").is_none(),
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
                    assert_eq!(indicator_status(), "Active");
                    main.hide().unwrap();
                    wait_for(|| !main.is_visible().unwrap());
                    tray::set_enabled(&handle, false).unwrap();
                    assert_eq!(indicator_status(), "Passive", "native icon must be hidden");
                    wait_for(|| main.is_visible().unwrap());
                }
                hotkeys::apply_menu_response(&handle, "OK RELEASE\n").unwrap();
                assert!(handle.get_webview_window("replay-menu").is_none());
                hotkeys::apply_menu_response(&handle, "OK GRAB\n").unwrap();
                let menu = handle.get_webview_window("replay-menu").unwrap();
                wait_for(|| menu.is_visible().unwrap());
                hotkeys::apply_menu_response(&handle, "OK RELEASE\n").unwrap();
                wait_for(|| !menu.is_visible().unwrap());
                assert!(hotkeys::apply_menu_response(&handle, "ERR fixture").is_err());
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
    println!("PASS: hidden native shortcut dispatch, menu open/close, empty shortcuts, tray startup and three native Active/Passive cycles");
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

/// Read the actual Linux indicator property on a worker so the UI thread can
/// answer the D-Bus request. A retained handle alone is not visibility evidence.
fn indicator_status() -> String {
    use gtk::glib::variant::ToVariant;
    let bus =
        gtk::gio::bus_get_sync(gtk::gio::BusType::Session, gtk::gio::Cancellable::NONE).unwrap();
    let reply = bus
        .call_sync(
            bus.unique_name().as_deref(),
            "/org/ayatana/NotificationItem/tray_icon_tray_app_redunar",
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&("org.kde.StatusNotifierItem", "Status").to_variant()),
            None,
            gtk::gio::DBusCallFlags::NONE,
            1000,
            gtk::gio::Cancellable::NONE,
        )
        .unwrap();
    reply
        .get::<(gtk::glib::Variant,)>()
        .unwrap()
        .0
        .get::<String>()
        .unwrap()
}
