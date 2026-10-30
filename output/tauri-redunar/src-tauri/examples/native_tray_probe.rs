//! Explicit, temporary desktop tray check. No preferences, input devices or games.
#![allow(dead_code)]
#[path = "../src/tray_native/mod.rs"]
mod tray_native;

use std::{sync::Arc, time::Duration};

fn main() {
    assert_eq!(
        std::env::args().skip(1).collect::<Vec<_>>(),
        ["--desktop"],
        "Run deliberately with --desktop; this briefly adds an icon to the session tray"
    );
    let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    let image = context.default_window_icon().expect("bundled app icon");
    let icon = tray_native::Icon::from_rgba(image.width(), image.height(), image.rgba()).unwrap();
    let tray = tray_native::NativeTray::start(icon, Arc::new(|_| {})).unwrap();
    let mut registered = false;
    for _ in 0..40 {
        if tray.registered() {
            registered = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(registered, "The desktop did not register this probe's item");
    tray.set_tooltip("Redunar temporary tray check");
    std::thread::sleep(Duration::from_secs(3));
    drop(tray);
    println!("PASS: native icon registered with the desktop host and its connection closed");
}
