//! Local, isolated native bridge/WebKit playback check. Optional argv[1] is a
//! read-only source clip; otherwise generate a synthetic Matroska recording.
#[allow(dead_code)]
#[path = "../src/clip_metadata.rs"]
mod clip_metadata;
#[allow(dead_code)]
#[path = "../src/media.rs"]
mod media;
#[allow(dead_code)]
#[path = "../src/media_tools.rs"]
mod media_tools;
#[allow(dead_code)]
#[path = "../src/playback.rs"]
mod playback;
mod backend {
    pub static SERVICE: std::sync::OnceLock<redunar_daemon::RedunarService> =
        std::sync::OnceLock::new();
    pub fn service() -> redunar_daemon::RedunarService {
        SERVICE.get().unwrap().clone()
    }
}
use std::{
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};
use tauri::Manager;
#[derive(Clone, Default)]
struct Report(Arc<Mutex<Option<serde_json::Value>>>);
#[tauri::command]
fn probe_report(
    app: tauri::AppHandle,
    report: tauri::State<'_, Report>,
    states: serde_json::Value,
) {
    *report.0.lock().unwrap() = Some(states);
    app.exit(0);
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn main() {
    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    let directory =
        std::env::temp_dir().join(format!("redunar-native-media-probe-{}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    // Register cleanup only after creating the directory ourselves.
    let root = Temporary(directory);
    let service = redunar_daemon::RedunarService::with_state_directory(root.0.join("state"));
    service
        .set_replay_save_parent(Some(root.0.clone()))
        .unwrap();
    let directory = service.replay_save_directory().unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(".redunar-replay-store-v1"),
        b"redunar-replay-store-v1\n",
    )
    .unwrap();
    let source = directory.join("redunar-replay-1-1-1.mkv");
    if let Some(input) = std::env::args_os().nth(1) {
        std::fs::copy(input, &source).unwrap();
    } else {
        assert!(Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=1280x720:r=30",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000",
                "-t",
                "8",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-b:v",
                "12M",
                "-c:a",
                "libopus",
                "-f",
                "matroska"
            ])
            .arg(&source)
            .stdin(Stdio::null())
            .status()
            .unwrap()
            .success());
    }
    assert!(backend::SERVICE.set(service).is_ok());
    let report = Report::default();
    let result = report.clone();
    let mut context = tauri::generate_context!();
    for window in &mut context.config_mut().app.windows {
        window.incognito = true;
        window.data_directory = Some(root.0.join("webview"));
    }
    let app = tauri::Builder::default()
        .manage(report)
        .manage(playback::Playback::default())
        .invoke_handler(tauri::generate_handler![
            media::replay_clips,
            media::clip_playback_path,
            media::clip_thumbnail,
            clip_metadata::clip_metadata,
            probe_report
        ])
        .on_page_load(|window, payload| {
            if payload.event() != tauri::webview::PageLoadEvent::Finished {
                return;
            }
            window
                .eval(include_str!("../../tests/replay-native-probe.js"))
                .unwrap();
        })
        .build(context)
        .unwrap();
    app.run_return(|app, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            app.state::<playback::Playback>().shutdown();
        }
    });
    let report = result.0.lock().unwrap();
    let states = report
        .as_ref()
        .and_then(|value| value.as_array())
        .expect("Native probe returned no report");
    let first_ready = states
        .iter()
        .find(|state| state["ready"].as_u64().unwrap_or(0) >= 2);
    println!(
        "{}",
        serde_json::json!({"first_ready_ms": first_ready.map(|state| &state["elapsedMs"]), "final": states.last()})
    );
    assert!(first_ready.is_some(), "Native playback never became ready");
    let final_state = states.last().unwrap();
    assert!(
        final_state["metadata"]["duration_seconds"]
            .as_f64()
            .is_some_and(|value| value > 0.0),
        "Native metadata inspection failed"
    );
    assert_eq!(
        final_state["invalidMetadataRejected"], true,
        "Clip metadata bypassed inventory validation"
    );
    assert_eq!(
        final_state["preload"], "auto",
        "Captured clips require normal video preloading"
    );
    assert_eq!(
        final_state["frames"], "8",
        "Native filmstrip did not finish"
    );
    assert_eq!(
        final_state["position"], 0,
        "Filmstrip changed the player position"
    );
    assert!(
        !states
            .iter()
            .any(|state| state.get("csp").is_some() || state.get("jsError").is_some()),
        "Native webview reported a script or policy error"
    );
}
