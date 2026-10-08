use crate::tray_native::{Action, Icon, NativeTray};
use std::sync::{Arc, Mutex};
use tauri::Manager;

#[derive(Default)]
struct TrayState(Mutex<Option<NativeTray>>);

pub fn show_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    app.manage(TrayState::default());
    if crate::backend::service()
        .app_preferences()
        .is_ok_and(|preferences| preferences.close_to_tray)
    {
        if let Err(error) = set_enabled(app.handle(), true) {
            eprintln!("Redunar tray unavailable: {error}");
            show_window(app.handle());
        }
    }
    Ok(())
}

/// Native ownership controls both registration and the close behavior.
pub fn set_enabled(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let state = app.state::<TrayState>();
    let mut tray = state
        .0
        .lock()
        .map_err(|_| "The tray state is unavailable")?;
    if !enabled {
        // Recover the user's way back before removing the connection/icon.
        // Retain the icon and preference if native window recovery fails.
        if let Some(window) = app.get_webview_window("main") {
            if !window.is_visible().map_err(|error| error.to_string())? {
                window.show().map_err(|error| error.to_string())?;
                window.set_focus().map_err(|error| error.to_string())?;
            }
        }
        tray.take();
        return Ok(());
    }
    if tray.as_ref().is_some_and(NativeTray::is_running) {
        return Ok(());
    }
    tray.take();
    let image = app
        .default_window_icon()
        .ok_or("The tray icon is unavailable")?;
    let icon = Icon::from_rgba(image.width(), image.height(), image.rgba())?;
    let app = app.clone();
    let action = Arc::new(move |action| {
        let handle = app.clone();
        // Never wait on GTK from the bus worker: disabling/shutdown joins it.
        let _ = app.run_on_main_thread(move || match action {
            Action::Open | Action::Recover => show_window(&handle),
            Action::Quit => handle.exit(0),
        });
    });
    *tray = Some(NativeTray::start(icon, action)?);
    Ok(())
}

pub fn close_to_tray(app: &tauri::AppHandle) -> bool {
    let enabled = crate::backend::service()
        .app_preferences()
        .is_ok_and(|preferences| preferences.close_to_tray);
    should_hide(enabled, enabled && has_item(app) && registered(app))
}

fn should_hide(enabled: bool, registered: bool) -> bool {
    enabled && registered
}

pub(crate) fn registered(app: &tauri::AppHandle) -> bool {
    app.try_state::<TrayState>().is_some_and(|state| {
        state
            .0
            .lock()
            .is_ok_and(|tray| tray.as_ref().is_some_and(NativeTray::registered))
    })
}

pub(crate) fn shutdown(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<TrayState>() {
        if let Ok(mut tray) = state.0.lock() {
            tray.take();
        }
    }
}

pub(crate) fn has_item(app: &tauri::AppHandle) -> bool {
    app.try_state::<TrayState>().is_some_and(|state| {
        state
            .0
            .lock()
            .is_ok_and(|tray| tray.as_ref().is_some_and(NativeTray::is_running))
    })
}

pub(crate) fn set_tooltip(app: &tauri::AppHandle, text: &str) {
    if let Some(state) = app.try_state::<TrayState>() {
        if let Ok(tray) = state.0.lock() {
            if let Some(tray) = tray.as_ref() {
                tray.set_tooltip(text);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_icon_is_square_rgba_with_one_byte_per_channel() {
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        let icon = context.default_window_icon().expect("app icon");
        assert_eq!((icon.width(), icon.height()), (256, 256));
        assert_eq!(icon.rgba().len(), 256 * 256 * 4);
        assert!(Icon::from_rgba(icon.width(), icon.height(), icon.rgba()).is_ok());
    }

    #[test]
    fn close_keeps_a_reachable_window_without_preference_and_registration() {
        assert!(should_hide(true, true));
        assert!(!should_hide(false, true));
        assert!(!should_hide(true, false));
        assert!(!should_hide(false, false));
    }
}
