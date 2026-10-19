//! Matched rendering comparison: real Tauri view, generated native-command data.
//! No service, game monitor, shortcut owner, updater or production state starts.
#[path = "../src/display_backend.rs"]
mod display_backend;
#[path = "../src/webview_settings.rs"]
mod webview_settings;

use gtk::prelude::*;
use std::{
    fs::OpenOptions,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::Manager;

#[derive(Default)]
struct Clock {
    enabled: bool,
    previous: Option<i64>,
    intervals: Vec<f64>,
}
#[derive(Clone, Default)]
struct Paints(Arc<Mutex<Clock>>);

#[tauri::command]
fn render_clock(
    app: tauri::AppHandle,
    state: tauri::State<'_, Paints>,
    enabled: bool,
) -> serde_json::Value {
    let mut clock = state.0.lock().unwrap();
    clock.enabled = enabled;
    clock.previous = None;
    serde_json::json!({
        "paint_intervals_ms": std::mem::take(&mut clock.intervals),
        "display_backend": app.get_webview_window("main").unwrap().gtk_window().unwrap().display().type_().name(),
        "native_focused": app.get_webview_window("main").unwrap().is_focused().unwrap_or(false)
    })
}

#[tauri::command]
fn render_report(app: tauri::AppHandle, data: serde_json::Value) {
    let directory = fixture_directory();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("result.json"))
        .unwrap();
    writeln!(file, "{}", serde_json::to_string_pretty(&data).unwrap()).unwrap();
    app.exit(if data.get("error").is_some() { 1 } else { 0 });
}

fn fixture_directory() -> PathBuf {
    assert_eq!(std::env::var("REDUNAR_RENDER_FIXTURE").as_deref(), Ok("1"));
    let directory = PathBuf::from(std::env::var_os("REDUNAR_RENDER_FIXTURE_DIR").unwrap());
    assert!(directory.is_absolute());
    let metadata = std::fs::symlink_metadata(&directory).unwrap();
    assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    assert_eq!(metadata.permissions().mode() & 0o077, 0);
    assert_eq!(
        std::fs::read(directory.join("fixture-v1")).unwrap(),
        b"redunar-render-fixture-v1\n"
    );
    directory
}

fn main() {
    display_backend::configure();
    let directory = fixture_directory();
    // Match production policy exactly; an explicit caller override still wins.
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    let initialization = std::fs::read_to_string(directory.join("init.js")).unwrap();
    let responses: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&std::fs::read(directory.join("responses.json")).unwrap()).unwrap();
    let mut context = tauri::generate_context!();
    context.config_mut().identifier = "com.redunar.RenderingProbe".into();
    context.config_mut().app.windows.clear();
    let paints = Paints::default();
    let observed = paints.clone();
    // A broken load/IPC path must not leave a hidden fixture process running.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(60));
        eprintln!("Native rendering fixture timed out");
        std::process::exit(2);
    });
    let reporting: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool =
        tauri::generate_handler![render_clock, render_report];
    let app = tauri::Builder::default()
        .manage(paints)
        .invoke_handler(move |invoke| {
            if matches!(invoke.message.command(), "render_clock" | "render_report") {
                return reporting(invoke);
            }
            // This executable registers no real service commands. Keep Tauri's
            // IPC transport intact, resolving only labelled fixture responses.
            let response = responses
                .get(invoke.message.command())
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            invoke.resolver.resolve(response);
            true
        })
        .setup(move |app| {
            let window = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html#replay".into()),
            )
            .title("Redunar rendering comparison — generated fixture")
            .inner_size(1280.0, 800.0)
            .incognito(true)
            .data_directory(directory.join("webview"))
            .initialization_script(initialization)
            .build()?;
            webview_settings::configure(&window)?;
            display_backend::record_selected();
            let observed = observed.clone();
            gtk::glib::timeout_add_local_once(Duration::from_millis(100), move || {
                let native = window.gtk_window().unwrap();
                let frame_clock = native
                    .frame_clock()
                    .expect("mapped fixture has a frame clock");
                frame_clock.connect_after_paint(move |frame| {
                    let mut clock = observed.0.lock().unwrap();
                    if clock.enabled {
                        let now = frame.frame_time();
                        if let Some(previous) = clock.previous {
                            if now > previous && clock.intervals.len() < 8192 {
                                clock.intervals.push((now - previous) as f64 / 1000.0);
                            }
                        }
                        clock.previous = Some(now);
                    }
                });
            });
            Ok(())
        })
        .build(context)
        .unwrap();
    app.run_return(|_, _| {});
}
