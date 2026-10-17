//! Runs the production webview configuration without starting Redunar services.
#[path = "../src/webview_settings.rs"]
mod webview_settings;

use std::{
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::Manager;
use webkit2gtk::{SettingsExt, WebViewExt};

static PASSED: AtomicBool = AtomicBool::new(false);

struct PrivateState(PathBuf);
impl Drop for PrivateState {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn main() {
    assert_eq!(
        std::env::var("REDUNAR_PRIVATE_DESKTOP_FIXTURE").as_deref(),
        Ok("1"),
        "Run only in the documented private D-Bus/Xvfb fixture"
    );
    let path = std::env::temp_dir().join(format!(
        "redunar-webview-settings-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .unwrap();
    // Only own cleanup after creation succeeded; never remove a preexisting path.
    let root = PrivateState(path);
    for (variable, directory) in [
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_RUNTIME_DIR", "runtime"),
    ] {
        let path = root.0.join(directory);
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        std::env::set_var(variable, path);
    }
    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    let mut context = tauri::generate_context!();
    context.config_mut().identifier = "com.redunar.WebviewSettingsProbe".into();
    for window in &mut context.config_mut().app.windows {
        window.visible = false;
        window.incognito = true;
        window.data_directory = Some(root.0.join("webview"));
        window.url = tauri::WebviewUrl::External("about:blank".parse().unwrap());
    }
    let timeout_directory = root.0.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(15));
        if !PASSED.load(Ordering::Acquire) {
            eprintln!("FAIL: native settings probe did not complete");
            let _ = std::fs::remove_dir_all(timeout_directory);
            std::process::exit(1);
        }
    });
    let app = tauri::Builder::default()
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            window.with_webview(|webview| {
                webview
                    .inner()
                    .settings()
                    .unwrap()
                    .set_enable_smooth_scrolling(false);
            })?;
            webview_settings::configure(&window)?;
            webview_settings::configure(&window)?;
            let handle = app.handle().clone();
            window.with_webview(move |webview| {
                let view = webview.inner();
                let settings = view.settings().expect("native settings getter available");
                assert!(
                    settings.enables_smooth_scrolling(),
                    "production setup restores native easing"
                );
                let independent = webkit2gtk::WebView::new();
                let other_settings = independent.settings().unwrap();
                other_settings.set_enable_smooth_scrolling(false);
                assert!(!other_settings.enables_smooth_scrolling());
                assert!(
                    settings.enables_smooth_scrolling(),
                    "settings are scoped to each view"
                );
                PASSED.store(true, Ordering::Release);
                handle.exit(0);
            })?;
            Ok(())
        })
        .build(context)
        .unwrap();
    app.run_return(|_, _| {});
    assert!(
        PASSED.load(Ordering::Acquire),
        "native configuration was not exercised"
    );
    println!("PASS: production Tauri settings getter, repeated setup, native easing and independent-view isolation");
}
