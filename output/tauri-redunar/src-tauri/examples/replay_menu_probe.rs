//! Load the production replay document using the production native window
//! builder and isolated command fixtures. Does not initialize the daemon.
#[path = "../src/replay_menu_window.rs"]
mod replay_menu_window;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Report(Arc<Mutex<Option<serde_json::Value>>>);
#[tauri::command]
fn replay_runtime_status() -> serde_json::Value {
    serde_json::json!({"phase":"Buffering","can_save":true,"buffered_seconds":30})
}
#[tauri::command]
fn replay_preferences() -> serde_json::Value {
    serde_json::json!({"close_overlay_on_outside_click":true})
}
#[tauri::command]
fn probe_report(app: tauri::AppHandle, report: tauri::State<'_, Report>, value: serde_json::Value) {
    *report.0.lock().unwrap() = Some(value);
    app.exit(0);
}
fn main() {
    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    let report = Report::default();
    let result = report.clone();
    let mut context = tauri::generate_context!();
    // Keep a hidden fixture window alive while Tauri creates the secondary
    // native window. Neither document starts the desktop or the daemon.
    for window in &mut context.config_mut().app.windows {
        window.visible = false;
        window.url = tauri::WebviewUrl::App("replay-menu.html".into());
    }
    let app = tauri::Builder::default()
        .manage(report)
        .invoke_handler(tauri::generate_handler![
            replay_runtime_status,
            replay_preferences,
            probe_report
        ])
        .setup(|app| {
            replay_menu_window::show(app.handle()).map_err(std::io::Error::other)?;
            Ok(())
        })
        .on_page_load(|window, payload| {
            if window.label() == "replay-menu"
                && payload.event() == tauri::webview::PageLoadEvent::Finished
            {
                window
                    .eval(include_str!("../../tests/replay-menu-native-probe.js"))
                    .unwrap();
            }
        })
        .build(context)
        .unwrap();
    app.run_return(|_, _| {});
    let guard = result.0.lock().unwrap();
    let value = guard
        .as_ref()
        .expect("Native menu probe returned no report");
    println!("{value}");
    assert_eq!(
        value["passed"], true,
        "Native replay window failed: {value}"
    );
}
