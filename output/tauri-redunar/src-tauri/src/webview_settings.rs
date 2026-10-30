use webkit2gtk::{SettingsExt, WebViewExt};

pub(crate) fn configure(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    window.with_webview(|webview| {
        // Restore WebKit's original native smooth scrolling. The UI delegates
        // wheel input to the engine, without a second animation or interception.
        // This settings object belongs only to this view; GTK/system preferences
        // and the renderer workaround remain unchanged.
        // WebKitWebView's "settings" property is not readable through GObject.
        // Its supported native getter returns a borrowed/ref-counted settings
        // object safely, including when called again during window setup.
        if let Some(settings) = webview.inner().settings() {
            settings.set_enable_smooth_scrolling(true);
        } else {
            redunar_daemon::log_op!("Window scrolling settings unavailable");
        }
    })
}
