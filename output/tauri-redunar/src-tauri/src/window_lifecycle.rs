//! Closing a window must not tear down a live game's native session.
use tauri::Manager;

// The native session owner decides whether a game finished or needs attention.
// The window layer only manages reachability; no process or cleanup authority.
type SettleGame = fn(&tauri::AppHandle) -> bool;

pub(crate) fn close_for_game(app: &tauri::AppHandle, settle: SettleGame) {
    let handle = app.clone();
    if app
        .run_on_main_thread(move || {
            // Tauri runs this inline when already on GTK's thread. Defer until
            // the close button's own input observer has finished retaining it.
            #[cfg(target_os = "linux")]
            gtk::glib::idle_add_local_once(move || begin_close(&handle, settle));
            #[cfg(not(target_os = "linux"))]
            begin_close(&handle, settle);
        })
        .is_err()
    {
        crate::background_start::retain_window(app);
        redunar_daemon::log_op!("Window close could not reach the desktop; game session retained");
    }
}

fn begin_close(app: &tauri::AppHandle, settle: SettleGame) {
    // A later Open/input cancels this ticket, as does a newer close request.
    let ticket = app.state::<crate::background_start::State>().begin_close();
    if settle(app) {
        return;
    }
    if let Err(error) = crate::tray::set_enabled(app, true) {
        show_failure(
            app,
            &format!("Keep Redunar open while the game is running. Tray unavailable: {error}"),
        );
        return;
    }
    if crate::tray::registered(app) {
        finish_close(app, ticket, settle);
    } else {
        #[cfg(target_os = "linux")]
        {
            let app = app.clone();
            gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(1), move || {
                finish_close(&app, ticket, settle);
            });
        }
        #[cfg(not(target_os = "linux"))]
        show_failure(
            app,
            "Keep Redunar open while the game is running. The tray is unavailable.",
        );
    }
}

fn finish_close(app: &tauri::AppHandle, ticket: u64, settle: SettleGame) {
    let state = app.state::<crate::background_start::State>();
    if !state.close_is_current(ticket) || settle(app) {
        return;
    }
    if !crate::tray::registered(app) {
        show_failure(
            app,
            "Keep Redunar open while the game is running. The tray is unavailable.",
        );
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        if window.hide().is_ok() {
            state.finish_close(ticket);
        } else {
            show_failure(
                app,
                "Redunar could not move to the tray. Keep it open while the game is running.",
            );
        }
    }
}

pub(crate) fn show_failure(app: &tauri::AppHandle, message: &str) {
    crate::tray::show_window(app);
    if let Some(window) = app.get_webview_window("main") {
        let message = serde_json::to_string(message).unwrap_or_default();
        let _ = window.eval(format!(
            "window.dispatchEvent(new CustomEvent('native-error',{{detail:{message}}}))"
        ));
    }
}
