// The replay menu owns a separate document so the desktop shell can never
// appear in its transparent native window, including during startup.
const REPLAY_MENU_WIDTH: f64 = 760.0;
const REPLAY_MENU_HEIGHT: f64 = 520.0;

pub(crate) fn show(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    if let Some(window) = app.get_webview_window("replay-menu") {
        window.center().map_err(|error| error.to_string())?;
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }
    WebviewWindowBuilder::new(
        app,
        "replay-menu",
        WebviewUrl::App("replay-menu.html".into()),
    )
    .title("Redunar Replay")
    .inner_size(REPLAY_MENU_WIDTH, REPLAY_MENU_HEIGHT)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .transparent(true)
    // Keep both the native surface and WebKit canvas clear outside the menu.
    .background_color(tauri::window::Color(0, 0, 0, 0))
    .shadow(false)
    .center()
    .focused(true)
    .visible(true)
    .build()
    .map(|_| ())
    .map_err(|error| format!("Replay menu could not open: {error}"))
}
